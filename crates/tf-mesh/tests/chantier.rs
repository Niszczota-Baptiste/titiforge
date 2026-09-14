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
