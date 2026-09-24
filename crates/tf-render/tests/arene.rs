//! **Remplacer une section sans toucher aux autres.**
//!
//! Les deux arènes rangent chaque section à une PLACE stable, et un
//! remplacement ne réécrit que les places visées. Mesuré en vol sur du bâti
//! (`tf-app --example vol`), l'ancienne recopie coûtait **35 ms par image**
//! et grandissait avec la scène.
//!
//! Ce qui se vérifie ici, et nulle part ailleurs :
//!
//! 1. **Ce qui est DESSINÉ est ce qu'une construction complète dessinerait.**
//!    Pas la même disposition — les trous, les emplacements réemployés et
//!    l'ordre des places diffèrent, et c'est voulu — mais le même ensemble de
//!    quads et de faces, chacun à la même ORIGINE. La passe de modèles se
//!    vérifie en rejouant la dichotomie du shader sur le processeur
//!    (`AreneModeles::dessinees`) : un contrôle qui relirait les places
//!    relirait le raisonnement qui les a produites.
//! 2. **Un remplacement paie ce qu'il change**, pas la scène. Les deux arènes
//!    COMPTENT ce qu'elles écrivent ; un chronomètre dirait la même chose en
//!    dépendant de la machine.
//! 3. **Aucune suite d'arrivées, de départs et d'éditions ne corrompt rien** —
//!    des milliers d'opérations tirées d'une graine, croisées à chaque pas.

use std::collections::HashMap;

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_mesh::{Adresse, Grille, TableFormes};
use tf_render::{Arene, AreneModeles, FaceModele};

const AIR: StateId = 0;
const CUBE: StateId = 1;
const MODELE: StateId = 2;
const AUTRE: StateId = 3;

/// Air, un cube plein, et deux modèles : une dalle et une autre dalle, pour
/// voir un modèle changer sans changer de nombre de poses.
fn table() -> TableFormes {
    let dalle = |haut: f32| tf_mesh::forme::Cuboide {
        min: [0.0, 0.0, 0.0],
        max: [16.0, haut, 16.0],
        faces: 0x3F,
        cull: 0x3F,
    };
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t.pousser(false, false, vec![dalle(8.0)]);
    t.pousser(false, false, vec![dalle(4.0), dalle(12.0)]);
    t
}

/// Une section dont chaque case est choisie par `f(x, y, z)`.
fn section(y: i8, f: impl Fn(usize, usize, usize) -> StateId) -> Section {
    let palette: Vec<StateId> = vec![AIR, CUBE, MODELE, AUTRE];
    let mut idx = vec![0u16; 4096];
    for yy in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let id = f(x, yy, z);
                idx[yy * 256 + z * 16 + x] = palette.iter().position(|p| *p == id).unwrap() as u16;
            }
        }
    }
    let bits = bits_for(palette.len());
    let data = pack(&idx, bits.into(), Packing::NoStraddle);
    Section {
        y,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing: Packing::NoStraddle,
    }
}

fn pleine(y: i8, id: StateId) -> Section {
    section(y, |_, _, _| id)
}

fn uni(id: StateId, _: tf_mesh::forme::Face, _: StateId) -> (u32, [f32; 3]) {
    (id, [1.0; 3])
}

/// Six faces par cuboïde, distinctes par état : une géométrie qui changerait
/// de bloc se verrait.
fn faces(id: StateId, _: StateId) -> Vec<FaceModele> {
    let n = if id == AUTRE { 2 } else { 1 };
    (0..n * 6)
        .map(|f| FaceModele {
            min: [id as f32, f as f32, 0.0, 0.0],
            max: [16.0, 8.0, 16.0, 0.0],
            uv: [0.0, 0.0, 16.0, 16.0],
            face: f % 6,
            couche: id,
            teinte: 0,
            cullable: 0,
        })
        .collect()
}

/// Ce que la passe des quads DESSINE : chaque instance visible, avec
/// l'origine RÉELLE de sa section au lieu de son numéro d'emplacement.
fn quads_dessines(a: &Arene) -> Vec<([u32; 4], u32, u32, u32)> {
    let o = a.origines();
    let mut v: Vec<_> = a
        .visibles()
        .map(|i| {
            (
                o[i.section as usize].position.map(f32::to_bits),
                i.geo,
                i.couche,
                i.teinte,
            )
        })
        .collect();
    v.sort_unstable();
    v
}

/// Ce que la passe de modèles DESSINE, par le shader rejoué : chaque face,
/// à l'origine réelle de sa pose, par son CONTENU — l'ordre de la table de
/// faces dépend de l'ordre de remplissage, donc son rang ne se compare pas.
fn faces_dessinees(m: &AreneModeles, q: &Arene) -> Vec<([u32; 4], u32, [u32; 16])> {
    let o = q.origines();
    let mut v: Vec<_> = m
        .dessinees()
        .into_iter()
        .map(|(slot, local, f)| {
            (
                o[slot as usize].position.map(f32::to_bits),
                local,
                // Le contenu de la face, par ses seize mots : aucune
                // allocation par face, sinon le croisement coûte plus que ce
                // qu'il vérifie.
                bytemuck::cast::<FaceModele, [u32; 16]>(m.faces[f as usize]),
            )
        })
        .collect();
    v.sort_unstable();
    v
}

/// La scène tenue à jour par remplacements, et sa grille.
struct Scene {
    g: Grille,
    t: TableFormes,
    q: Arene,
    m: AreneModeles,
}

impl Scene {
    fn neuve(g: Grille) -> Scene {
        let t = table();
        let c = g.mailler(&t);
        Scene {
            q: Arene::depuis(&c, &uni),
            m: AreneModeles::depuis(&c, &faces),
            g,
            t,
        }
    }

    /// Remaille les sections visées et remplace — le chemin de l'application.
    fn remplacer(&mut self, visees: &[Adresse]) {
        let neufs = self.g.mailler_ces(&self.t, visees);
        self.q.remplacer(visees, &neufs.lots, &uni);
        self.m
            .remplacer(self.q.emplacements(), visees, &neufs.lots, &faces);
    }

    /// Le témoin : tout reconstruire depuis la grille.
    fn verifier(&self, quoi: &str) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.m.verifier()));
        assert!(
            r.is_ok(),
            "{quoi} : une règle de la passe de modèles est rompue"
        );
        let c = self.g.mailler(&self.t);
        let q = Arene::depuis(&c, &uni);
        let m = AreneModeles::depuis(&c, &faces);
        // **Un emplacement par lot, et pas un de plus.** Une section partie
        // qui garderait le sien dessinerait toujours juste — aucune instance
        // ne le désigne plus — mais la table grandirait d'une entrée par
        // section JAMAIS vue : sur un vol de huit cents régions, des millions.
        // Mesuré par mutation : sans ce compte, l'oubli passait tout le reste.
        assert_eq!(
            self.q.emplacements().len(),
            c.lots.len(),
            "{quoi} : {} emplacements tenus pour {} lots",
            self.q.emplacements().len(),
            c.lots.len()
        );
        let (a, b) = (quads_dessines(&self.q), quads_dessines(&q));
        assert_eq!(
            a.len(),
            b.len(),
            "{quoi} : {} quads contre {} rebâtis",
            a.len(),
            b.len()
        );
        assert!(
            a == b,
            "{quoi} : les quads dessinés diffèrent d'une arène rebâtie"
        );
        let (a, b) = (faces_dessinees(&self.m, &self.q), faces_dessinees(&m, &q));
        assert_eq!(
            a.len(),
            b.len(),
            "{quoi} : {} faces contre {} rebâties",
            a.len(),
            b.len()
        );
        assert!(
            a == b,
            "{quoi} : les faces dessinées diffèrent d'une arène rebâtie"
        );
    }
}

/// **Une section qui APPARAÎT ne déplace rien** — c'était le cas qui obligeait
/// à tout recopier, quand le champ `section` était l'indice du lot.
#[test]
fn une_section_qui_apparait_ne_deplace_rien() {
    let mut g = Grille::new();
    g.poser(0, 0, pleine(1, CUBE));
    g.poser(1, 0, pleine(1, MODELE));
    g.poser(1, 0, pleine(2, CUBE));
    let mut s = Scene::neuve(g);

    s.g.poser(0, 0, pleine(0, CUBE));
    s.remplacer(&Grille::sections_autour([0, 0, 0], [15, 15, 15]));
    s.verifier("après une section neuve");
}

/// **Une section qui DISPARAÎT** : sa place devient un trou, qui ne dessine
/// rien. C'est ce qu'une éviction produit à chaque pas de caméra.
#[test]
fn une_section_qui_disparait_laisse_un_trou_qui_ne_dessine_rien() {
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, CUBE));
    g.poser(0, 0, pleine(1, MODELE));
    g.poser(1, 0, pleine(1, CUBE));
    g.poser(2, 0, pleine(1, MODELE));
    let mut s = Scene::neuve(g);

    assert!(s.g.retirer((0, 0, 1)), "la section devait être là");
    s.remplacer(&Grille::sections_autour([0, 16, 0], [15, 31, 15]));
    assert!(
        s.m.trous() > 0 || s.m.faces_a_dessiner as usize == s.m.dessinees().len(),
        "la prémisse : la place d'une section retirée devient un trou, ou le \
         tableau raccourcit"
    );
    s.verifier("après un retrait");
}

/// **Un modèle qui change de nombre de faces** : c'est le cas propre à la
/// passe de modèles, où la somme préfixe décidait tout le reste.
#[test]
fn un_modele_qui_grossit_ne_decale_rien() {
    let mut g = Grille::new();
    for cx in 0..3 {
        g.poser(cx, 0, pleine(0, MODELE));
    }
    let mut s = Scene::neuve(g);
    // La première colonne passe à un modèle de DEUX cuboïdes : même nombre de
    // poses, deux fois plus de faces. Elle ne tient plus dans sa place.
    s.g.poser(0, 0, pleine(0, AUTRE));
    s.remplacer(&Grille::sections_autour([0, 0, 0], [15, 15, 15]));
    s.verifier("après un modèle plus gros");
    // Et retour : il tient, sa place se réemploie et le reste devient trou.
    s.g.poser(0, 0, pleine(0, MODELE));
    s.remplacer(&Grille::sections_autour([0, 0, 0], [15, 15, 15]));
    s.verifier("après un modèle plus petit");
}

/// **Un remplacement paie ce qu'il change, pas la scène.**
///
/// La propriété qui justifie toute la pièce, et elle se COMPTE : sur une
/// scène de cinquante colonnes, remplacer UNE section écrit les instances de
/// cette section et de sa marge — pas les cinquante colonnes. Avant les
/// places stables, chaque remplacement réécrivait le tableau entier.
#[test]
fn un_remplacement_paie_ce_qu_il_change_pas_la_scene() {
    let mut g = Grille::new();
    for cz in 0..5 {
        for cx in 0..10 {
            g.poser(
                cx,
                cz,
                pleine(0, if (cx + cz) % 2 == 0 { CUBE } else { MODELE }),
            );
        }
    }
    let mut s = Scene::neuve(g);
    let (q0, m0) = (s.q.ecrites(), s.m.ecrites());
    let (scene_q, scene_m) = (s.q.len() as u64, s.m.poses.len() as u64);
    assert!(
        scene_q > 0 && scene_m > 0,
        "la prémisse : les deux passes portent quelque chose"
    );

    // UN bloc qui change au milieu de la colonne (4, 2), qui est du cube : il
    // devient un modèle. Première écriture, je changeais la section ENTIÈRE
    // en ne déclarant qu'un bloc visé — le témoin remaillait les voisines,
    // pas le remplacement, et l'écart accusait l'arène d'une faute de la
    // liste. C'est la leçon des visées qui débordent, une fois de plus.
    s.g.poser(
        4,
        2,
        section(
            0,
            |x, y, z| if (x, y, z) == (8, 8, 8) { MODELE } else { CUBE },
        ),
    );
    let visees = Grille::sections_autour([64 + 8, 8, 32 + 8], [64 + 8, 8, 32 + 8]);
    s.remplacer(&visees);
    s.verifier("après une édition d'un bloc");

    let (dq, dm) = (s.q.ecrites() - q0, s.m.ecrites() - m0);
    assert!(
        dq * 10 < scene_q,
        "{dq} instances écrites pour une édition, sur une arène de {scene_q} : \
         le remplacement paie la scène"
    );
    assert!(
        dm * 10 < scene_m,
        "{dm} poses écrites pour une édition, sur {scene_m} : le remplacement \
         paie la scène"
    );
}

/// **Un trou se réemploie** : une section qui part puis une section de même
/// taille qui arrive ne font pas grandir le tableau.
#[test]
fn un_trou_se_reemploie() {
    let mut g = Grille::new();
    for cx in 0..6 {
        g.poser(cx, 0, pleine(0, MODELE));
    }
    let mut s = Scene::neuve(g);
    let (lq, lm, fm) = (s.q.len(), s.m.poses.len(), s.m.faces_a_dessiner);

    // La colonne 2 part — un trou au MILIEU, pas au bout — puis la colonne
    // 10, de même contenu, arrive ailleurs.
    s.g.retirer((2, 0, 0));
    s.remplacer(&Grille::sections_autour([32, 0, 0], [47, 15, 15]));
    s.g.poser(10, 0, pleine(0, MODELE));
    s.remplacer(&Grille::sections_autour([160, 0, 0], [175, 15, 15]));
    s.verifier("après départ puis arrivée");

    assert!(
        s.m.poses.len() <= lm && s.m.faces_a_dessiner <= fm,
        "le trou aurait dû servir : {} poses et {} faces contre {lm} et {fm}",
        s.m.poses.len(),
        s.m.faces_a_dessiner
    );
    assert!(
        s.q.len() <= lq + 256,
        "l'arène des quads a grandi de {} au lieu de réemployer",
        s.q.len() - lq
    );
}

/// Un petit générateur, déterministe : une graine, et la suite se rejoue.
struct Tirage(u64);
impl Tirage {
    fn n(&mut self, borne: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % borne
    }
}

/// **Aucune suite d'opérations ne corrompt les arènes.**
///
/// Arrivées, départs, éditions d'une case, sections qui se vident : tirés
/// d'une graine, et croisés à une construction complète après CHAQUE pas. Les
/// places, les trous, leur recollement, leur découpe, les terminaux et le
/// tassement sont tous traversés — et un seul faux rang de la somme préfixe
/// ferait dessiner une face pour une autre.
#[test]
fn aucune_suite_d_operations_ne_corrompt_les_arenes() {
    for graine in [1u64, 7, 42, 2024] {
        let mut r = Tirage(graine);
        let mut s = Scene::neuve(Grille::new());
        let mut presentes: HashMap<Adresse, ()> = HashMap::new();
        for pas in 0..300 {
            let (cx, cz, sy) = (r.n(6) as i32, r.n(3) as i32, r.n(3) as i8);
            let a = (cx, cz, sy);
            let quoi = r.n(10);
            if quoi < 2 && presentes.contains_key(&a) {
                s.g.retirer(a);
                presentes.remove(&a);
            } else {
                // **Des sections CREUSES** : quelques dizaines de blocs au
                // plus, de nombres de poses et de faces tous différents. Ce
                // qui se vérifie ici est la dynamique des places — trous,
                // découpes, recollements, réemplois — et elle ne dépend pas du
                // volume. Des sections pleines faisaient durer ce test sept
                // minutes pour ne rien vérifier de plus.
                let k = r.n(48) as usize;
                let mut cases: HashMap<usize, StateId> = HashMap::new();
                for _ in 0..k {
                    let i = r.n(4096) as usize;
                    cases.insert(i, [CUBE, MODELE, AUTRE][r.n(3) as usize]);
                }
                s.g.poser(
                    cx,
                    cz,
                    section(sy, move |x, y, z| {
                        cases.get(&(y * 256 + z * 16 + x)).copied().unwrap_or(AIR)
                    }),
                );
                presentes.insert(a, ());
            }
            let b = (cx * 16, sy as i32 * 16, cz * 16);
            s.remplacer(&Grille::sections_autour(
                [b.0, b.1, b.2],
                [b.0 + 15, b.1 + 15, b.2 + 15],
            ));
            s.verifier(&format!("graine {graine}, pas {pas}"));
        }
    }
}

/// **Tasser ne change rien à ce qui est dessiné**, et rend la place.
///
/// Le tassement recopie tout — c'est le seul moment où l'arène paie la
/// scène — et il ne se déclenche que quand les trous pèsent plus que ce qu'on
/// dessine. On le provoque en vidant presque tout.
#[test]
fn tasser_rend_la_place_sans_rien_changer_a_l_image() {
    let mut g = Grille::new();
    // Assez pour franchir le seuil de tassement : 70 000 faces de modèles.
    for cz in 0..8 {
        for cx in 0..8 {
            for sy in 0..3 {
                g.poser(
                    cx,
                    cz,
                    section(sy, |x, _, z| if (x + z) % 2 == 0 { MODELE } else { AIR }),
                );
            }
        }
    }
    let mut s = Scene::neuve(g);
    assert_eq!(s.m.tassements(), 0);
    // On retire les colonnes une à une, en gardant la dernière rangée — assez
    // pour laisser des trous partout, pas seulement au bout.
    for cz in 0..7 {
        for cx in 0..8 {
            for sy in 0..3 {
                s.g.retirer((cx, cz, sy));
            }
            let b = (cx * 16, cz * 16);
            s.remplacer(&Grille::sections_autour(
                [b.0, 0, b.1],
                [b.0 + 15, 47, b.1 + 15],
            ));
        }
    }
    s.verifier("après les retraits");
    assert!(
        s.m.tassements() > 0 || s.q.tassements() > 0,
        "la prémisse : les trous devaient finir par peser plus que le reste"
    );
    assert!(
        s.m.trous() * 2 <= s.m.faces_a_dessiner as u64 + tf_render::arene::TASSER_AU_DELA,
        "après tassement, les trous ne dominent plus"
    );
}
