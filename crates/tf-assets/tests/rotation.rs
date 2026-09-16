//! La rotation d'une variante, ancrée sur un bloc qui ne laisse aucune
//! ambiguïté.
//!
//! Le sens d'une rotation ne se lit pas dans un commentaire — le dépôt s'est
//! déjà fait avoir par un ordre d'indices annoncé à l'envers. Il se MESURE
//! contre un cas dont le format lui-même dit où le résultat doit atterrir.

use tf_assets::rotation::{axes, est_identite, tourner, tourner_cuboide, tourner_face};
use tf_mesh::forme::{Cuboide, Face, FACES};

/// Le modèle de `minecraft:mushroom_stem` : un PLAN sur la face nord.
///
/// `from [0,0,0] to [16,16,0]`, une seule face déclarée — `north` — et elle
/// porte `cullface: north`.
fn plan_nord() -> Cuboide {
    Cuboide {
        min: [0.0, 0.0, 0.0],
        max: [16.0, 16.0, 0.0],
        faces: Face::MoinsZ.bit(),
        cull: Face::MoinsZ.bit(),
    }
}

/// Le tableau que `minecraft:mushroom_stem` déclare : une rotation par face
/// exposée, et le format NOMME la face attendue dans sa condition.
///
/// Six rotations, six faces distinctes. C'est ce qui force le sens : il n'y a
/// pas de convention à choisir, seulement une à retrouver.
const CHAMPIGNON: [(&str, u16, u16, Face); 6] = [
    ("north=true", 0, 0, Face::MoinsZ),
    ("east=true", 0, 90, Face::PlusX),
    ("south=true", 0, 180, Face::PlusZ),
    ("west=true", 0, 270, Face::MoinsX),
    ("up=true", 270, 0, Face::PlusY),
    ("down=true", 90, 0, Face::MoinsY),
];

#[test]
fn le_champignon_pose_ses_six_plans_sur_ses_six_faces() {
    let mut vues = Vec::new();
    for (quand, x, y, attendue) in CHAMPIGNON {
        let c = tourner_cuboide(&plan_nord(), axes(x, y));

        // La face déclarée suit la rotation.
        assert_eq!(
            c.faces,
            attendue.bit(),
            "{quand} (x:{x} y:{y}) : la face déclarée devrait être {attendue:?}"
        );
        // Le `cullface` aussi — c'est le même masque, transformé pareil.
        assert_eq!(c.cull, attendue.bit(), "{quand} : cullface devrait suivre");

        // Et le plan doit être COLLÉ à cette face, pas quelque part ailleurs :
        // épaisseur nulle sur l'axe de la face, et à ras du bon bord.
        let axe = attendue.axe();
        assert_eq!(
            c.min[axe], c.max[axe],
            "{quand} : le plan doit rester d'épaisseur nulle"
        );
        let attendu = if attendue.positif() { 16.0 } else { 0.0 };
        assert_eq!(
            c.min[axe], attendu,
            "{quand} : le plan devrait être à ras de {attendue:?}, il est à {}",
            c.min[axe]
        );
        // Les deux autres axes couvrent tout le bloc.
        for k in 0..3 {
            if k == axe {
                continue;
            }
            assert_eq!((c.min[k], c.max[k]), (0.0, 16.0), "{quand} : axe {k}");
        }
        assert!(c.au_bord(attendue), "{quand} : le plan doit être au bord");
        vues.push(attendue);
    }
    vues.sort();
    vues.dedup();
    assert_eq!(vues.len(), 6, "les six rotations doivent viser six faces");
}

/// Un quart de tour quatre fois est l'identité — sur les deux axes, et sur les
/// faces comme sur les coins.
#[test]
fn quatre_quarts_de_tour_ne_font_rien() {
    for (x, y) in [(90u16, 0u16), (0, 90)] {
        // On tourne un cuboïde ASYMÉTRIQUE pour de vrai : une composition
        // d'axes juste sur le papier peut être fausse à l'usage.
        let depart = escalier();
        let mut c = depart;
        for _ in 0..4 {
            c = tourner_cuboide(&c, axes(x, y));
        }
        assert_eq!(c, depart, "quatre × (x:{x} y:{y}) doit rendre l'original");
    }
}

/// Un cuboïde qui n'a aucune symétrie, pour que « ça n'a pas bougé » ne puisse
/// pas passer pour « ça a bien tourné ».
fn escalier() -> Cuboide {
    Cuboide {
        min: [8.0, 0.5, 2.0],
        max: [16.0, 8.0, 14.0],
        faces: Face::PlusX.bit() | Face::MoinsY.bit() | Face::PlusZ.bit(),
        cull: Face::PlusX.bit(),
    }
}

#[test]
fn zero_degre_ne_recopie_rien() {
    assert!(est_identite(axes(0, 0)));
    assert!(est_identite(axes(360, 720)));
    let v = vec![escalier()];
    assert_eq!(tourner(v.clone(), 0, 0), v);
}

/// Deux rotations différentes doivent donner deux résultats différents. Sinon
/// un escalier `facing=east` et un `facing=west` seraient dessinés pareil —
/// exactement le défaut que ce module corrige.
#[test]
fn les_seize_rotations_sont_distinctes_sur_une_forme_sans_symetrie() {
    let mut vus: Vec<Cuboide> = Vec::new();
    for x in [0u16, 90, 180, 270] {
        for y in [0u16, 90, 180, 270] {
            let c = tourner_cuboide(&escalier(), axes(x, y));
            assert!(
                !vus.contains(&c),
                "x:{x} y:{y} rend la même chose qu'une autre rotation"
            );
            vus.push(c);
        }
    }
    assert_eq!(vus.len(), 16);
}

/// Une rotation ne crée ni ne détruit de face, et n'en confond jamais deux.
#[test]
fn une_rotation_permute_les_faces_sans_en_perdre() {
    for x in [0u16, 90, 180, 270] {
        for y in [0u16, 90, 180, 270] {
            let a = axes(x, y);
            let mut arrivees: Vec<Face> = FACES.iter().map(|&f| tourner_face(f, a)).collect();
            arrivees.sort();
            arrivees.dedup();
            assert_eq!(arrivees.len(), 6, "x:{x} y:{y} perd une face");
            // Deux faces opposées le restent : une rotation ne retourne pas
            // un bloc à l'envers sur lui-même.
            for f in FACES {
                assert_eq!(
                    tourner_face(f.opposee(), a),
                    tourner_face(f, a).opposee(),
                    "x:{x} y:{y} : {f:?} et son opposée divergent"
                );
            }
        }
    }
}

/// Le volume est un invariant : une rotation déplace, elle ne redimensionne
/// pas. Et les coins ressortent dans l'ordre — une boîte de volume négatif
/// serait lue de travers par `remplit()` sans se plaindre.
#[test]
fn une_rotation_conserve_le_volume_et_l_ordre_des_coins() {
    let v = |c: &Cuboide| (c.max[0] - c.min[0]) * (c.max[1] - c.min[1]) * (c.max[2] - c.min[2]);
    for x in [0u16, 90, 180, 270] {
        for y in [0u16, 90, 180, 270] {
            let c = tourner_cuboide(&escalier(), axes(x, y));
            assert!((v(&c) - v(&escalier())).abs() < 1e-3, "x:{x} y:{y}");
            for k in 0..3 {
                assert!(c.min[k] <= c.max[k], "x:{x} y:{y} : coins croisés");
            }
        }
    }
}

/// Le cube plein est invariant : s'il ne l'était pas, tourner un bloc de
/// pierre le rendrait non opaque et le mailleur rouvrirait les faces de tous
/// ses voisins.
#[test]
fn le_cube_plein_reste_plein_et_opaque() {
    for x in [0u16, 90, 180, 270] {
        for y in [0u16, 90, 180, 270] {
            let c = tourner_cuboide(&Cuboide::PLEIN, axes(x, y));
            assert_eq!(c, Cuboide::PLEIN, "x:{x} y:{y}");
            assert!(c.remplit());
        }
    }
}
