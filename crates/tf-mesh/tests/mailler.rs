//! Le maillage d'une section.
//!
//! Ce qu'il faut prouver : les faces cachées disparaissent, les visibles
//! restent, la fusion gloutonne ne perd ni n'invente de surface, et les deux
//! passes ne se marchent pas dessus.

use tf_mesh::forme::{Cuboide, Face, FACES};
use tf_mesh::{mailler, Formes, Maillage, Quad, TableFormes, Voisinage, COTE};

const AIR: u32 = 0;
const PIERRE: u32 = 1;
const AUTRE: u32 = 2;
const DALLE: u32 = 3;
const FLEUR: u32 = 4;

/// Air, deux cubes pleins distincts, une dalle basse, une fleur en croix.
fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new()); // AIR
    t.pousser(false, true, Vec::new()); // PIERRE
    t.pousser(false, true, Vec::new()); // AUTRE
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: Face::MoinsY.bit(), // seule la face du bas porte cullface
        }],
    ); // DALLE
    t.pousser(
        false,
        false,
        vec![
            // Deux quads croisés, comme une plante. Plats sur un axe, donc
            // deux de leurs faces sont d'aire nulle.
            Cuboide {
                min: [0.0, 0.0, 8.0],
                max: [16.0, 16.0, 8.0],
                faces: Face::MoinsZ.bit() | Face::PlusZ.bit(),
                cull: 0,
            },
            Cuboide {
                min: [8.0, 0.0, 0.0],
                max: [8.0, 16.0, 16.0],
                faces: Face::MoinsX.bit() | Face::PlusX.bit(),
                cull: 0,
            },
        ],
    ); // FLEUR
    t
}

fn vide() -> Voisinage {
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| AIR);
    v
}

fn quads_de(v: &Voisinage, t: &dyn Formes) -> Maillage {
    mailler(v, t)
}

// ── l'ordre des faces, MESURÉ ───────────────────────────────────────────────

#[test]
fn l_ordre_des_faces_est_celui_que_le_mailleur_produit() {
    // Un ordre d'indices se MESURE contre le code qui le produit, il ne se lit
    // pas dans un commentaire. Dans `we-engine`, une table d'ombrage annonçait
    // « −X +X +Y −Y » pour un mailleur qui produit « −X +X −Y +Y » : le dessus
    // des blocs était assombri et le dessous éclairé à plein, invisible sur un
    // build gris, depuis toujours.
    let t = table();
    let mut v = vide();
    v.set(5, 5, 5, PIERRE);
    let m = quads_de(&v, &t);

    assert_eq!(m.quads.len(), 6, "un cube isolé montre ses six faces");

    // Chaque face doit être du BON côté du bloc.
    let attendu: [(Face, [f32; 3]); 6] = [
        (Face::MoinsX, [5.0 * 16.0, 5.0 * 16.0, 5.0 * 16.0]),
        (Face::PlusX, [6.0 * 16.0, 5.0 * 16.0, 5.0 * 16.0]),
        (Face::MoinsY, [5.0 * 16.0, 5.0 * 16.0, 5.0 * 16.0]),
        (Face::PlusY, [5.0 * 16.0, 6.0 * 16.0, 5.0 * 16.0]),
        (Face::MoinsZ, [5.0 * 16.0, 5.0 * 16.0, 5.0 * 16.0]),
        (Face::PlusZ, [5.0 * 16.0, 5.0 * 16.0, 6.0 * 16.0]),
    ];
    for (face, min) in attendu {
        let q = m
            .quads
            .iter()
            .find(|q| q.face == face)
            .unwrap_or_else(|| panic!("{face:?} absente"));
        assert_eq!(q.min, min, "{face:?} du mauvais côté du bloc");
        assert_eq!(q.taille, [16.0, 16.0], "{face:?}");
    }

    // Et l'ordre déclaré est bien celui de production.
    let ordre: Vec<Face> = m.quads.iter().map(|q| q.face).collect();
    assert_eq!(
        ordre,
        FACES.to_vec(),
        "la face NÉGATIVE d'abord sur chaque axe : axe * 2 + (positif ? 1 : 0)"
    );
}

#[test]
fn une_face_et_son_opposee_ne_sortent_pas_au_meme_endroit() {
    let t = table();
    let mut v = vide();
    v.set(0, 0, 0, PIERRE);
    let m = quads_de(&v, &t);
    for axe in 0..3 {
        let neg = m.quads.iter().find(|q| q.face == FACES[axe * 2]).unwrap();
        let pos = m
            .quads
            .iter()
            .find(|q| q.face == FACES[axe * 2 + 1])
            .unwrap();
        assert_ne!(
            neg.min[axe], pos.min[axe],
            "axe {axe} : les deux faces d'un même bloc sortiraient superposées"
        );
        assert_eq!(pos.min[axe] - neg.min[axe], 16.0);
    }
}

// ── ce que la passe gloutonne fait gagner ───────────────────────────────────

#[test]
fn un_mur_plein_sort_en_un_quad_par_face() {
    let t = table();
    let mut v = vide();
    for y in 0..COTE as i32 {
        for x in 0..COTE as i32 {
            v.set(x, y, 8, PIERRE);
        }
    }
    let m = quads_de(&v, &t);

    let par_face: Vec<usize> = FACES
        .iter()
        .map(|f| m.quads.iter().filter(|q| q.face == *f).count())
        .collect();
    assert_eq!(
        par_face,
        vec![1, 1, 1, 1, 1, 1],
        "256 blocs, 1 536 faces, SIX quads : les deux grandes faces du mur, et \
         ses quatre tranches — chacune fusionnée sur toute sa longueur"
    );

    let grand = m.quads.iter().find(|q| q.face == Face::PlusZ).unwrap();
    assert_eq!(
        grand.taille,
        [16.0 * 16.0, 16.0 * 16.0],
        "16 × 16 blocs en un quad"
    );
    // Une tranche fait 16 blocs de long sur UN d'épaisseur. Lire la taille
    // dans l'autre ordre donnerait un quad de 1 × 16 — invisible sur un mur
    // carré, faux partout ailleurs.
    let tranche = m.quads.iter().find(|q| q.face == Face::PlusY).unwrap();
    assert_eq!(
        tranche.taille,
        [16.0 * 16.0, 16.0],
        "le plan de ±Y est (X, Z) dans cet ordre : 16 blocs en X, 1 en Z"
    );
    let flanc = m.quads.iter().find(|q| q.face == Face::MoinsX).unwrap();
    assert_eq!(
        flanc.taille,
        [16.0 * 16.0, 16.0],
        "le plan de ±X est (Y, Z) : 16 blocs en Y, 1 en Z"
    );
}

#[test]
fn deux_etats_distincts_ne_fusionnent_pas() {
    let t = table();
    let mut v = vide();
    for x in 0..COTE as i32 {
        v.set(x, 0, 0, if x < 8 { PIERRE } else { AUTRE });
    }
    let m = quads_de(&v, &t);
    let dessus: Vec<&Quad> = m.quads.iter().filter(|q| q.face == Face::PlusY).collect();
    assert_eq!(
        dessus.len(),
        2,
        "fusionner deux états donnerait un quad qui porte la texture d'un seul"
    );
    assert_eq!(dessus[0].taille, [8.0 * 16.0, 16.0]);
    assert_eq!(dessus[1].taille, [8.0 * 16.0, 16.0]);
}

#[test]
fn un_cube_plein_de_section_ne_montre_que_sa_peau() {
    let t = table();
    let mut v = vide();
    for y in 0..COTE as i32 {
        for z in 0..COTE as i32 {
            for x in 0..COTE as i32 {
                v.set(x, y, z, PIERRE);
            }
        }
    }
    let m = quads_de(&v, &t);
    assert_eq!(
        m.quads.len(),
        6,
        "4 096 blocs, 24 576 faces, 6 quads : c'est tout l'intérêt du glouton"
    );
    assert!(m
        .quads
        .iter()
        .all(|q| q.taille == [16.0 * 16.0, 16.0 * 16.0]));
}

#[test]
fn une_section_pleine_entouree_de_pierre_ne_montre_rien() {
    // La peau sert à ça : sans elle, on dessinerait six murs de faces
    // fantômes le long des frontières de chunk.
    let t = table();
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| PIERRE);
    let m = quads_de(&v, &t);
    assert!(
        m.est_vide(),
        "{} quads émis à l'intérieur de la pierre",
        m.quads.len()
    );
}

#[test]
fn la_fusion_ne_perd_ni_n_invente_de_surface() {
    // Le glouton doit rendre exactement la même AIRE qu'un maillage naïf.
    let t = table();
    let mut v = vide();
    let mut n = 0u32;
    v.remplir(|x, y, z| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if !Voisinage::dedans(x, y, z) {
            return AIR;
        }
        match n % 3 {
            0 => PIERRE,
            1 => AUTRE,
            _ => AIR,
        }
    });

    let m = quads_de(&v, &t);
    let naif = aire_naive(&v, &t);
    assert_eq!(
        m.aire(),
        naif,
        "la fusion change le NOMBRE de quads, jamais la surface couverte"
    );
}

/// Compte la surface face par face, sans aucune fusion.
fn aire_naive(v: &Voisinage, t: &dyn Formes) -> f64 {
    let n = COTE as i32;
    let mut aire = 0f64;
    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                if !t.opaque(v.get(x, y, z)) {
                    continue;
                }
                for f in FACES {
                    let p = f.pas();
                    if !t.opaque(v.get(x + p[0], y + p[1], z + p[2])) {
                        aire += 16.0 * 16.0;
                    }
                }
            }
        }
    }
    aire
}

// ── la passe de modèles ─────────────────────────────────────────────────────

#[test]
fn une_dalle_montre_ses_faces_et_pas_celles_d_un_cube() {
    let t = table();
    let mut v = vide();
    v.set(3, 3, 3, DALLE);
    let m = quads_de(&v, &t);

    assert_eq!(m.quads_glouton, 0, "une dalle n'est pas un cube plein");
    assert_eq!(m.quads_modele, 6);

    let dessus = m.quads.iter().find(|q| q.face == Face::PlusY).unwrap();
    assert_eq!(
        dessus.min[1],
        3.0 * 16.0 + 8.0,
        "le dessus d'une dalle basse est à mi-hauteur, pas au sommet du bloc"
    );
    let cote = m.quads.iter().find(|q| q.face == Face::PlusX).unwrap();
    assert_eq!(
        cote.taille,
        [8.0, 16.0],
        "haute de 8 seizièmes, large de 16"
    );
}

#[test]
fn un_bloc_modele_n_efface_pas_les_faces_de_son_voisin() {
    // Marqué opaque, un escalier creuserait un trou dans le mur qu'il touche.
    let t = table();
    let mut v = vide();
    v.set(5, 5, 5, PIERRE);
    v.set(6, 5, 5, DALLE);
    let m = quads_de(&v, &t);

    assert!(
        m.quads
            .iter()
            .any(|q| q.face == Face::PlusX && q.id == PIERRE),
        "la face de la pierre CONTRE la dalle doit rester dessinée"
    );
}

#[test]
fn le_cullface_ne_joue_qu_au_bord_du_bloc_et_contre_un_opaque() {
    let t = table();

    // Dalle seule : sa face du bas est à ras et déclarée cullable, mais il n'y
    // a rien dessous.
    let mut v = vide();
    v.set(5, 5, 5, DALLE);
    let seule = quads_de(&v, &t).quads_modele;

    // Dalle posée sur de la pierre : la face du bas disparaît.
    let mut v2 = vide();
    v2.set(5, 5, 5, DALLE);
    v2.set(5, 4, 5, PIERRE);
    let m2 = quads_de(&v2, &t);
    assert_eq!(
        m2.quads_modele,
        seule - 1,
        "la face du bas d'une dalle posée sur un bloc plein ne se voit pas"
    );
    assert!(!m2
        .quads
        .iter()
        .any(|q| q.face == Face::MoinsY && q.id == DALLE));

    // La face du DESSUS est à ras elle aussi, mais ne porte pas `cullface`
    // dans ce modèle : un bloc au-dessus ne doit pas la faire disparaître.
    let mut v3 = vide();
    v3.set(5, 5, 5, DALLE);
    v3.set(5, 6, 5, PIERRE);
    let m3 = quads_de(&v3, &t);
    assert!(
        m3.quads
            .iter()
            .any(|q| q.face == Face::PlusY && q.id == DALLE),
        "sans `cullface`, une face reste dessinée quoi qu'il y ait à côté"
    );
}

#[test]
fn une_face_au_milieu_du_bloc_ne_se_masque_jamais() {
    // La fleur est un modèle plat au milieu de sa case : aucune de ses faces
    // n'est à ras du bord, donc aucune ne peut être masquée.
    let t = table();
    let mut v = vide();
    v.set(5, 5, 5, FLEUR);
    let seule = quads_de(&v, &t).quads_modele;

    let mut v2 = vide();
    v2.set(5, 5, 5, FLEUR);
    for f in FACES {
        let p = f.pas();
        v2.set(5 + p[0], 5 + p[1], 5 + p[2], PIERRE);
    }
    assert_eq!(
        quads_de(&v2, &t).quads_modele,
        seule,
        "une plante entourée de blocs reste entièrement visible"
    );
}

#[test]
fn un_cuboide_plat_n_emet_pas_de_quad_d_aire_nulle() {
    let t = table();
    let mut v = vide();
    v.set(5, 5, 5, FLEUR);
    let m = quads_de(&v, &t);
    assert!(
        m.quads.iter().all(|q| q.aire() > 0.0),
        "un quad d'aire nulle est invisible et facturé"
    );
    assert_eq!(m.quads_modele, 4, "deux quads croisés, deux faces chacun");
}

// ── les deux passes ne se marchent pas dessus ───────────────────────────────

#[test]
fn aucun_bloc_n_est_maille_deux_fois() {
    let t = table();
    let mut v = vide();
    let mut n = 7u32;
    v.remplir(|x, y, z| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if !Voisinage::dedans(x, y, z) {
            return AIR;
        }
        match n % 5 {
            0 => PIERRE,
            1 => AUTRE,
            2 => DALLE,
            3 => FLEUR,
            _ => AIR,
        }
    });
    let m = quads_de(&v, &t);

    // Les quads gloutons ne portent QUE des états opaques ; ceux des modèles,
    // que des états non opaques. Si un état passait par les deux, il serait
    // dessiné deux fois — et on ne le verrait qu'en transparence.
    let glouton = &m.quads[..m.quads_glouton];
    let modele = &m.quads[m.quads_glouton..];
    assert!(glouton.iter().all(|q| t.opaque(q.id)));
    assert!(modele.iter().all(|q| !t.opaque(q.id)));
    assert_eq!(m.quads_glouton + m.quads_modele, m.quads.len());
}

#[test]
fn l_air_ne_produit_rien() {
    let t = table();
    let m = quads_de(&vide(), &t);
    assert!(m.est_vide());
}

#[test]
fn un_etat_hors_table_n_est_pas_suppose_opaque() {
    // Entre deux erreurs, on prend celle qui se VOIT : un identifiant inconnu
    // supposé opaque effacerait des faces réelles sans rien signaler.
    let t = table();
    let mut v = vide();
    v.set(5, 5, 5, PIERRE);
    v.set(6, 5, 5, 999);
    let m = quads_de(&v, &t);
    assert!(m
        .quads
        .iter()
        .any(|q| q.face == Face::PlusX && q.id == PIERRE));
}

#[test]
fn le_maillage_est_deterministe() {
    let t = table();
    let mut v = vide();
    let mut n = 3u32;
    v.remplir(|x, y, z| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if Voisinage::dedans(x, y, z) && n % 4 == 0 {
            PIERRE
        } else {
            AIR
        }
    });
    assert_eq!(quads_de(&v, &t).quads, quads_de(&v, &t).quads);
}
