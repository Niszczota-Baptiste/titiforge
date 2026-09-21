//! La teinte de biome traverse-t-elle vraiment jusqu'au quad ?
//!
//! Le défaut qu'on ferme ici ne se voit pas : tout compile, tous les tests
//! passent, et le sol reste uniformément « plaines ». C'est la forme
//! « déclaré, branché, testé — et inatteignable » que le dépôt a déjà payée
//! trois fois, et la seule parade est d'EXIGER la présence plutôt que de la
//! constater d'un coup d'œil.
//!
//! Le compromis central se teste ici aussi : la fusion gloutonne ne doit
//! casser un quad sur une frontière de biome que pour les états TEINTÉS. Le
//! faire partout couperait les quads d'une muraille de pierre à chaque
//! frontière, pour une couleur que la pierre ne prend pas.

use tf_anvil::{Section, StateId};
use tf_mesh::forme::{Cuboide, TableFormes};
use tf_mesh::{Chantier, Grille};

const AIR: StateId = 0;
const PIERRE: StateId = 1;
const HERBE: StateId = 2;
const PLAINES: StateId = 10;
const DESERT: StateId = 11;

/// Une table où seule l'HERBE prend la couleur de son biome.
fn table() -> TableFormes {
    let mut t = TableFormes::new();
    let cube = vec![Cuboide::PLEIN];
    t.pousser(true, false, Vec::new()); // 0 : air
    t.pousser(false, true, cube.clone()); // 1 : pierre
    let h = t.pousser(false, true, cube); // 2 : herbe
    t.marquer_teinte(h);
    t
}

/// Une section pleine du bloc donné, avec de l'air au-dessus pour que la face
/// du dessus soit visible.
fn section(y: i8, id: StateId) -> Section {
    let mut s = Section::uniform(y, AIR);
    let mut idx = vec![0u16; 4096];
    s.palette = vec![AIR, id];
    for (i, c) in idx.iter_mut().enumerate() {
        // Les huit couches du bas seulement : le dessus reste de l'air.
        if i < 8 * 256 {
            *c = 1;
        }
    }
    s.repack(&idx);
    s
}

/// Les 64 cellules : la moitié en `a`, l'autre en `b`, coupées en X.
fn deux_biomes(a: StateId, b: StateId) -> Vec<StateId> {
    let mut v = vec![a; 64];
    for y in 0..4 {
        for z in 0..4 {
            for x in 2..4 {
                v[(y << 4) | (z << 2) | x] = b;
            }
        }
    }
    v
}

fn mailler(id: StateId, biomes: Option<Vec<StateId>>) -> Chantier {
    let mut g = Grille::new();
    g.poser(0, 0, section(0, id));
    if let Some(b) = biomes {
        assert!(g.poser_biomes(0, 0, 0, b), "64 cellules attendues");
    }
    g.mailler(&table())
}

// ── le biome arrive jusqu'au quad ───────────────────────────────────────────

#[test]
fn un_quad_d_herbe_porte_le_biome_de_sa_case() {
    let c = mailler(HERBE, Some(vec![DESERT; 64]));
    let quads: Vec<_> = c.lots.iter().flat_map(|l| l.quads.quads.iter()).collect();
    assert!(!quads.is_empty(), "il doit y avoir des quads");
    for q in &quads {
        assert_eq!(
            q.biome, DESERT,
            "la teinte de biome doit traverser jusqu'au quad"
        );
    }
}

#[test]
fn un_quad_de_pierre_ne_porte_aucun_biome() {
    // Zéro veut dire « on ne sait pas », et c'est le bon défaut : la pierre
    // ne prend pas la couleur de son biome, donc lui en donner un n'aurait
    // aucun effet sinon de casser sa fusion.
    let c = mailler(PIERRE, Some(vec![DESERT; 64]));
    for l in &c.lots {
        for q in &l.quads.quads {
            assert_eq!(q.biome, 0);
        }
    }
}

#[test]
fn sans_biomes_poses_le_quad_n_en_invente_pas() {
    let c = mailler(HERBE, None);
    for l in &c.lots {
        for q in &l.quads.quads {
            assert_eq!(q.biome, 0, "pas de biome connu, donc pas de biome");
        }
    }
}

#[test]
fn une_longueur_de_cellules_inattendue_est_refusee() {
    // Un biome décalé donne un sol de la mauvaise couleur, et rien ne le
    // dirait. On refuse plutôt que de tronquer.
    let mut g = Grille::new();
    g.poser(0, 0, section(0, HERBE));
    assert!(!g.poser_biomes(0, 0, 0, vec![DESERT; 63]));
    assert!(!g.poser_biomes(0, 0, 0, vec![DESERT; 65]));
    let c = g.mailler(&table());
    for l in &c.lots {
        for q in &l.quads.quads {
            assert_eq!(q.biome, 0);
        }
    }
}

// ── le compromis : la fusion ne casse QUE pour les teintés ──────────────────

#[test]
fn une_frontiere_de_biome_coupe_les_quads_d_herbe() {
    let un = mailler(HERBE, Some(vec![PLAINES; 64]));
    let deux = mailler(HERBE, Some(deux_biomes(PLAINES, DESERT)));
    assert!(
        deux.quads() > un.quads(),
        "une frontière doit couper : {} contre {}",
        deux.quads(),
        un.quads()
    );
    // Et les deux biomes sont présents dans la sortie.
    let vus: std::collections::BTreeSet<StateId> = deux
        .lots
        .iter()
        .flat_map(|l| l.quads.quads.iter().map(|q| q.biome))
        .collect();
    assert_eq!(
        vus,
        [PLAINES, DESERT].into_iter().collect(),
        "les deux biomes doivent sortir, pas un seul"
    );
}

#[test]
fn une_frontiere_de_biome_ne_coupe_pas_les_quads_de_pierre() {
    // Le cœur du compromis. Casser la fusion partout couperait les quads
    // d'une muraille à chaque frontière, pour une couleur que la pierre ne
    // prend pas.
    let un = mailler(PIERRE, Some(vec![PLAINES; 64]));
    let deux = mailler(PIERRE, Some(deux_biomes(PLAINES, DESERT)));
    assert_eq!(
        deux.quads(),
        un.quads(),
        "la pierre n'est pas teintée : sa fusion doit être intacte"
    );
}

/// Ce que la coupure COÛTE, chiffré plutôt que supposé.
///
/// Une section pleine d'herbe coupée en deux biomes ne doit pas exploser en
/// quads : la coupure suit la grille des cellules, qui fait quatre blocs, pas
/// un. Le pire cas — un damier de biomes — est mesuré ici pour qu'on sache où
/// est le plafond.
#[test]
fn le_cout_de_la_coupure_est_borne() {
    let un = mailler(HERBE, Some(vec![PLAINES; 64]));
    let deux = mailler(HERBE, Some(deux_biomes(PLAINES, DESERT)));
    let damier: Vec<StateId> = (0..64)
        .map(|i: usize| {
            let (x, z, y) = (i & 3, (i >> 2) & 3, i >> 4);
            if (x + z + y) % 2 == 0 {
                PLAINES
            } else {
                DESERT
            }
        })
        .collect();
    let pire = mailler(HERBE, Some(damier));
    println!(
        "quads : un biome {} · deux {} · damier {}",
        un.quads(),
        deux.quads(),
        pire.quads()
    );
    assert!(
        pire.quads() <= un.quads() * 16,
        "même en damier, la coupure suit la grille de 4 blocs : {} contre {}",
        pire.quads(),
        un.quads()
    );
}
