//! La peau qui traverse les chunks, et les sections qu'on ne maille pas.

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_mesh::forme::Cuboide;
use tf_mesh::{Face, Grille, TableFormes, Voisinage, COTE};

const AIR: StateId = 0;
const PIERRE: StateId = 1;
const DALLE: StateId = 2;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    );
    t
}

/// Une section dont chaque case est décidée par `f`.
fn section(y: i8, mut f: impl FnMut(i32, i32, i32) -> StateId) -> Section {
    let mut palette: Vec<StateId> = Vec::new();
    let mut idx = vec![0u16; 4096];
    for by in 0..16i32 {
        for bz in 0..16i32 {
            for bx in 0..16i32 {
                let id = f(bx, by, bz);
                let k = match palette.iter().position(|p| *p == id) {
                    Some(k) => k,
                    None => {
                        palette.push(id);
                        palette.len() - 1
                    }
                };
                idx[(by * 256 + bz * 16 + bx) as usize] = k as u16;
            }
        }
    }
    let bits = bits_for(palette.len());
    let data = if palette.len() == 1 {
        Vec::new()
    } else {
        pack(&idx, bits as usize, Packing::NoStraddle)
    };
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

#[test]
fn la_peau_traverse_les_chunks() {
    // Deux chunks côte à côte, tous deux pleins de pierre. La frontière ne doit
    // produire AUCUNE face : sans peau vraie, on dessinerait un mur de faces
    // fantômes le long de chaque bord de chunk.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    g.poser(1, 0, pleine(0, PIERRE));

    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    assert_eq!(
        v.get(COTE as i32, 0, 0),
        PIERRE,
        "la case juste après le bord appartient au chunk voisin"
    );

    let c = g.mailler(&t);
    // Les quads sont LOCAUX à leur section : c'est le lot qui dit où ils sont.
    let lot = c.lots.iter().find(|l| l.adresse == (0, 0, 0)).unwrap();
    assert!(
        !lot.quads.quads.iter().any(|q| q.face == Face::PlusX),
        "la frontière entre deux chunks pleins ne montre rien"
    );
    let voisin = c.lots.iter().find(|l| l.adresse == (1, 0, 0)).unwrap();
    assert!(
        voisin.quads.quads.iter().any(|q| q.face == Face::PlusX),
        "mais la face EXTÉRIEURE du chunk voisin, oui"
    );
    assert_eq!(lot.origine(), [0, 0, 0]);
    assert_eq!(voisin.origine(), [16, 0, 0]);
}

#[test]
fn la_peau_traverse_aussi_les_sections_empilees() {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    g.poser(0, 0, pleine(1, PIERRE));

    let c = g.mailler(&t);
    let bas = c.lots.iter().find(|l| l.adresse == (0, 0, 0)).unwrap();
    assert!(
        !bas.quads.quads.iter().any(|q| q.face == Face::PlusY),
        "le plafond de la section du bas est le plancher de celle du haut"
    );
    let haut = c.lots.iter().find(|l| l.adresse == (0, 0, 1)).unwrap();
    assert!(
        !haut.quads.quads.iter().any(|q| q.face == Face::MoinsY),
        "et réciproquement"
    );
    assert_eq!(haut.origine(), [0, 16, 0]);
}

#[test]
fn ce_qui_manque_vaut_de_l_air_et_non_de_la_pierre() {
    // Au bord d'une zone chargée, supposer opaque effacerait des faces
    // RÉELLES. Entre deux erreurs, on prend celle qui se voit.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));

    let c = g.mailler(&t);
    assert_eq!(
        c.quads(),
        6,
        "une section isolée montre ses six faces, pas zéro"
    );
}

#[test]
fn une_section_d_air_n_est_pas_maillee() {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, AIR));
    g.poser(0, 0, pleine(1, PIERRE));

    let c = g.mailler(&t);
    assert_eq!(c.sautees, 1, "la section d'air se saute sur sa PALETTE");
    assert_eq!(c.maillees(), 1);
    // Et la pierre voit quand même son voisin d'air : ses faces sortent.
    let pierre = c.lots.iter().find(|l| l.adresse == (0, 0, 1)).unwrap();
    assert!(
        pierre.quads.quads.iter().any(|q| q.face == Face::MoinsY),
        "sauter une section d'air ne doit pas masquer les faces de ses voisins"
    );
}

#[test]
fn une_section_de_plantes_est_maillee_elle() {
    // Air et plantes sont indiscernables du point de vue de l'opacité. C'est
    // la palette qui tranche, et elle ne se trompe pas.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, DALLE));

    let c = g.mailler(&t);
    assert_eq!(c.sautees, 0);
    assert_eq!(c.poses(), 4096, "4 096 dalles, 4 096 poses");
}

#[test]
fn une_section_absente_se_saute_sans_erreur() {
    let t = table();
    let g = Grille::new();
    let c = g.mailler(&t);
    assert_eq!(c.maillees(), 0);
    assert_eq!(c.quads(), 0);
}

#[test]
fn le_chantier_est_deterministe() {
    let t = table();
    let mut g = Grille::new();
    let mut n = 11u32;
    for (cx, cz) in [(0, 0), (1, 0), (0, 1)] {
        g.poser(
            cx,
            cz,
            section(0, |_, _, _| {
                n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match n % 4 {
                    0 => PIERRE,
                    1 => DALLE,
                    _ => AIR,
                }
            }),
        );
    }
    let mut a = g.mailler(&t);
    let mut b = g.mailler(&t);
    a.trier();
    b.trier();
    for (x, y) in a.lots.iter().zip(b.lots.iter()) {
        assert_eq!(x.adresse, y.adresse);
        assert_eq!(x.quads.quads, y.quads.quads);
        assert_eq!(x.poses.poses, y.poses.poses);
    }
}

#[test]
fn les_coordonnees_negatives_tombent_dans_le_bon_chunk() {
    // Division PLANCHER : le bloc −1 est dans le chunk −1, pas le chunk 0.
    // Une division entière naïve chargerait la mauvaise moitié du monde.
    let t = table();
    let mut g = Grille::new();
    g.poser(-1, -1, pleine(-1, PIERRE));
    assert_eq!(g.bloc(-1, -1, -1), PIERRE);
    assert_eq!(g.bloc(-16, -16, -16), PIERRE);
    assert_eq!(g.bloc(0, 0, 0), AIR, "le chunk 0 n'a rien");

    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    assert_eq!(
        v.get(-1, -1, -1),
        PIERRE,
        "la peau du chunk 0 touche le chunk −1"
    );
    let _ = t;
}

#[cfg(feature = "parallele")]
#[test]
fn le_chantier_parallele_rend_exactement_le_meme_resultat() {
    // Une optimisation non vérifiée est une corruption silencieuse. Le chemin
    // rapide se compare au chemin lent sur la même entrée, et le compte doit
    // être exact — pas « du même ordre ».
    let t = table();
    let mut g = Grille::new();
    let mut n = 99u32;
    for cz in 0..4i32 {
        for cx in 0..4i32 {
            for sy in 0..3i8 {
                g.poser(
                    cx,
                    cz,
                    section(sy, |_, _, _| {
                        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match n % 5 {
                            0 => PIERRE,
                            1 => DALLE,
                            _ => AIR,
                        }
                    }),
                );
            }
        }
    }

    let mut sequentiel = g.mailler(&t);
    let mut parallele = g.mailler_parallele(&t);
    sequentiel.trier();
    parallele.trier();

    assert_eq!(sequentiel.sautees, parallele.sautees);
    assert_eq!(sequentiel.maillees(), parallele.maillees());
    assert_eq!(sequentiel.lots.len(), parallele.lots.len());
    for (a, b) in sequentiel.lots.iter().zip(parallele.lots.iter()) {
        assert_eq!(a.adresse, b.adresse);
        assert_eq!(a.quads.quads, b.quads.quads, "section {:?}", a.adresse);
        assert_eq!(a.poses.poses, b.poses.poses, "section {:?}", a.adresse);
    }
}

#[test]
fn un_indice_hors_palette_ne_tue_pas_le_mailleur() {
    // `bits` se déduit de la longueur de palette : deux entrées se lisent sur
    // quatre bits, donc seize valeurs sont représentables pour deux valides. Un
    // `.mca` corrompu en porte, et un mailleur qui panique dessus fait mourir
    // l'application à l'AFFICHAGE — avant même que l'utilisateur ait touché à
    // quoi que ce soit.
    let mut s = tf_anvil::Section {
        y: 0,
        palette: vec![0, 1],
        bits: tf_anvil::bits_for(2),
        data: Box::new([]),
        packing: tf_anvil::Packing::NoStraddle,
    };
    let mut idx = vec![0u16; tf_anvil::VOL];
    idx[100] = 9;
    idx[2000] = 15;
    s.repack(&idx);

    let mut g = Grille::new();
    g.poser(0, 0, s);
    // Ce qui compte est de ne pas mourir ; le maillage doit rester cohérent.
    let c = g.mailler(&table());
    for lot in &c.lots {
        for q in &lot.quads.quads {
            assert!(q.aire() > 0.0, "un quad d'aire nulle");
        }
    }
}

// ── le remaillage PARTIEL ───────────────────────────────────────────────────

/// Quelques sections pleines côte à côte et empilées : de quoi qu'un
/// remaillage partiel ait des VOISINS à lire, ce qui est tout l'enjeu.
fn petit_monde() -> (Grille, TableFormes) {
    let f = table();
    let mut g = Grille::new();
    for cz in 0..2 {
        for cx in 0..2 {
            for y in 0..2 {
                g.poser(cx, cz, pleine(y, PIERRE));
            }
        }
    }
    (g, f)
}

/// **Un remaillage partiel doit rendre EXACTEMENT les mêmes quads** que le
/// maillage complet des mêmes sections. Une optimisation qui change l'image
/// n'est pas une optimisation, c'est un bug qu'on a choisi.
#[test]
fn mailler_ces_rend_la_meme_chose_que_tout_mailler() {
    let (g, f) = petit_monde();
    let complet = g.mailler(&f);

    // On remaille une section sur deux, et on compare lot par lot.
    let toutes = g.adresses();
    let moitie: Vec<_> = toutes.iter().copied().step_by(2).collect();
    assert!(moitie.len() >= 2, "il faut de quoi comparer");
    let partiel = g.mailler_ces(&f, &moitie);

    for lot in &partiel.lots {
        let attendu = complet
            .lots
            .iter()
            .find(|l| l.adresse == lot.adresse)
            .unwrap_or_else(|| panic!("{:?} absent du maillage complet", lot.adresse));
        // Sur les QUADS eux-mêmes, pas sur un compte : deux maillages du même
        // nombre de quads peuvent décrire deux images différentes.
        assert_eq!(lot.quads.quads, attendu.quads.quads, "{:?}", lot.adresse);
        assert_eq!(
            lot.quads.quads_glouton, attendu.quads.quads_glouton,
            "{:?}",
            lot.adresse
        );
        assert_eq!(lot.poses.poses, attendu.poses.poses, "{:?}", lot.adresse);
    }
}

/// **La marge d'UNE case n'est pas une précaution.** Le mailleur travaille
/// avec une couche de padding : poser un bloc au bord d'une section change
/// les faces visibles de la section d'à côté. Sans la marge, un trait au bord
/// laisse un mur de faces fantômes le long de la frontière.
#[test]
fn les_sections_a_remailler_debordent_d_une_case() {
    use tf_mesh::Grille;

    // **La marge est d'un BLOC, pas d'une section** — et c'est exactement ce
    // qu'il faut : le padding de la section S couvre les blocs
    // `S·16 − 1 .. S·16 + 16`, donc un bloc B ne concerne que les sections
    // dont le padding le contient. Déborder d'une SECTION entière remaillerait
    // vingt-six voisines pour un bloc posé au milieu — un facteur 26 pour rien.
    //
    // Au coin bas (bloc 0), les deux sections de chaque axe : celle du bloc, et
    // celle d'avant, dont le padding touche le bloc 0.
    let a = Grille::sections_autour([0, 0, 0], [0, 0, 0]);
    assert!(a.contains(&(0, 0, 0)), "la sienne");
    assert!(a.contains(&(-1, 0, 0)), "la voisine en −X");
    assert!(a.contains(&(0, -1, 0)), "la voisine en −Z");
    assert!(a.contains(&(0, 0, -1)), "la voisine en −Y");
    assert!(a.contains(&(-1, -1, -1)), "et leur coin commun");
    assert_eq!(a.len(), 8, "2 × 2 × 2 au coin BAS d'une section : {a:?}");

    // Au coin HAUT (bloc 15), c'est l'autre côté : la section suivante.
    let h = Grille::sections_autour([15, 15, 15], [15, 15, 15]);
    assert!(h.contains(&(0, 0, 0)) && h.contains(&(1, 1, 1)), "{h:?}");
    assert_eq!(h.len(), 8);

    // Un bloc au MILIEU ne touche aucune voisine : son padding ne sort pas.
    let b = Grille::sections_autour([8, 8, 8], [8, 8, 8]);
    assert_eq!(b, vec![(0, 0, 0)], "{b:?}");
}

/// Un Y hors de l'intervalle d'un `i8` n'a pas de section : on ne l'invente
/// pas. Le monde va de −64 à 320, mais un `//expand` peut sortir de tout.
#[test]
fn un_y_demesure_ne_donne_pas_de_section() {
    use tf_mesh::Grille;
    let a = Grille::sections_autour([0, i32::MAX - 1, 0], [0, i32::MAX, 0]);
    assert!(a.is_empty(), "{a:?}");
    // Et près de i32::MIN, la marge ne doit pas s'enrouler.
    let b = Grille::sections_autour([i32::MIN, 0, i32::MIN], [i32::MIN, 0, i32::MIN]);
    assert!(b
        .iter()
        .all(|(x, z, _)| *x <= i32::MIN / 16 + 1 && *z <= i32::MIN / 16 + 1));
}

/// Une adresse qui n'a pas de contenu est SAUTÉE, pas maillée à vide — mais
/// elle reste dans la liste des VISÉES, parce que c'est ce qui permet de
/// retirer le maillage d'une section qui vient de se vider.
#[test]
fn une_section_absente_est_sautee_sans_disparaitre_de_la_liste() {
    let (g, f) = petit_monde();
    let nulle_part = (9999, 9999, 0);
    assert!(g.section(nulle_part).is_none());

    let c = g.mailler_ces(&f, &[nulle_part]);
    assert!(c.lots.is_empty());
    assert_eq!(c.sautees, 1);

    // Et `sections_autour` la rend quand même : c'est une liste de CIBLES.
    let vise =
        tf_mesh::Grille::sections_autour([9999 * 16, 0, 9999 * 16], [9999 * 16, 0, 9999 * 16]);
    assert!(vise.contains(&nulle_part));
}

/// **Remplacer par un remaillage partiel doit donner le MÊME chantier qu'un
/// maillage complet.** C'est la seule propriété qui compte : si les deux
/// divergent, l'image dépend de l'historique des opérations, et personne ne
/// sait plus ce qu'il regarde.
#[test]
fn remplacer_par_un_remaillage_partiel_rend_le_chantier_complet() {
    let (mut g, f) = petit_monde();
    let mut courant = g.mailler(&f);

    // On change une section — de la pierre pleine à de l'air.
    let vise = tf_mesh::Grille::sections_autour([16, 0, 0], [31, 15, 15]);
    g.poser(1, 0, pleine(0, AIR));
    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    let complet = g.mailler(&f);
    assert_eq!(courant.lots.len(), complet.lots.len());
    for (a, b) in courant.lots.iter().zip(complet.lots.iter()) {
        assert_eq!(a.adresse, b.adresse, "l'ordre doit rester trié");
        assert_eq!(a.quads.quads, b.quads.quads, "{:?}", a.adresse);
        assert_eq!(a.poses.poses, b.poses.poses, "{:?}", a.adresse);
    }
}

/// **Un chunk qui se VIDE ne figure plus dans la liste des chunks.** Si la
/// fusion se contentait d'insérer ce que le remaillage produit, le maillage
/// d'une section effacée resterait à l'écran — les blocs supprimés resteraient
/// visibles. Ça ne se voit que sur un effacement, jamais sur une pose.
#[test]
fn une_section_videe_perd_son_maillage() {
    let (mut g, f) = petit_monde();
    let mut courant = g.mailler(&f);
    let avant = courant.lots.len();
    assert!(courant.lots.iter().any(|l| l.adresse == (1, 1, 0)));

    // Effacée pour de bon : la section n'est plus dans la grille du tout.
    let vise = tf_mesh::Grille::sections_autour([16, 0, 16], [31, 15, 31]);
    g.poser(1, 1, pleine(0, AIR));
    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    assert!(
        !courant.lots.iter().any(|l| l.adresse == (1, 1, 0)),
        "le maillage d'une section vidée est resté"
    );
    assert!(courant.lots.len() < avant);
}

/// **L'ordre reste trié même quand on ne remaille qu'une section du MILIEU.**
///
/// Le premier test de fusion ne le voyait pas : sa liste visée couvrait tout
/// le monde, donc `retain` vidait la liste et l'ordre se retrouvait trié par
/// accident. Il faut une section BASSE remaillée seule — retirée du début,
/// rajoutée à la fin — pour que l'ordre se dérange. Le chantier est
/// déterministe, et `Arene::origines` est indexée comme ses lots : deux ordres
/// donneraient deux dispositions d'arène, donc deux images qu'on ne peut plus
/// comparer.
#[test]
fn remplacer_une_seule_section_garde_l_ordre_trie() {
    let (g, f) = petit_monde();
    let mut courant = g.mailler(&f);
    assert!(courant.lots.len() > 2);

    // Le bloc (8, 8, 8) est au MILIEU de la section (0, 0, 0) : son padding
    // ne sort pas, donc la liste visée ne contient qu'elle.
    let vise = tf_mesh::Grille::sections_autour([8, 8, 8], [8, 8, 8]);
    assert_eq!(vise, vec![(0, 0, 0)], "la fixture doit isoler UNE section");
    assert_eq!(
        courant.lots[0].adresse,
        (0, 0, 0),
        "et ce doit être la PREMIÈRE, sinon le retrait ne dérange rien"
    );

    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    let ordre: Vec<_> = courant.lots.iter().map(|l| l.adresse).collect();
    let mut trie = ordre.clone();
    trie.sort_unstable();
    assert_eq!(ordre, trie, "l'ordre des lots s'est dérangé");
}

/// `retirer` est le pendant de `poser`, et il manquait. Sans lui, rien ne peut
/// enlever une section d'une grille : ce qui a été lu une fois y reste pour
/// toujours, même quand la save ne le porte plus.
#[test]
fn retirer_enleve_la_section_et_ses_biomes() {
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    assert!(g.poser_biomes(0, 0, 0, vec![7; tf_mesh::voisinage::VOL_BIOME]));
    assert!(g.section((0, 0, 0)).is_some());

    assert!(g.retirer((0, 0, 0)), "il y en avait une");
    assert!(g.section((0, 0, 0)).is_none());
    // Et le biome part avec : le laisser donnerait la couleur d'une section
    // qui n'existe plus à celle qui prendra sa place.
    assert!(g.poser_biomes(0, 0, 0, vec![9; tf_mesh::voisinage::VOL_BIOME]));

    assert!(!g.retirer((0, 0, 0)), "deux fois ne fait rien");
    assert!(!g.retirer((42, 42, 0)), "et ce qui n'existe pas non plus");
}
