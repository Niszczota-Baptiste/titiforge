//! **Dans quel SENS une texture se pose sur une face** — au pixel, contre la
//! règle du jeu, par les deux passes et à travers les rotations de variante.
//!
//! Le jeu (`FaceBakery`, `FaceInfo`) attache chaque coin d'uv à un sommet
//! précis de la face : vu de dehors, un côté montre sa texture debout et non
//! retournée, le dessus avec le nord en haut. Une variante tournée emporte
//! sa texture avec elle — une bûche couchée garde ses fibres le long de son
//! axe — sauf avec `uvlock`, où la texture reste alignée sur le monde.
//!
//! Rien de tout ça ne se voit sur une texture unie ou symétrique, c'est-à-dire
//! sur presque tout ce qu'un test pose d'habitude. La tuile est donc faite de
//! quatre QUADRANTS de couleurs différentes, et chaque point de chaque face
//! est comparé à la couleur que la règle lui donne. La référence dit quelle
//! couleur doit se trouver en un point du MONDE ; la caméra ne sert qu'à le
//! regarder.
//!
//! **C'est une jonction** : `tf_assets::uv` calcule le sens de chaque face,
//! les deux shaders le dessinent. Avant elle, 73 points sur 96 sortaient
//! faux sur un cube NON tourné — le nord retourné d'un demi-tour, le sud et
//! le dessous à l'envers, l'est et l'ouest couchés.

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_assets::rotation::{axes, tourner_point, Axes};
use tf_assets::uv::{poser, uv_par_defaut};
use tf_mesh::forme::{Cuboide, Face, FACES};
use tf_mesh::{Formes, Grille, TableFormes};
use tf_render::{
    Appareil, Arene, AreneModeles, AtlasGpu, Camera, Cible, HabillageFaces, Scene, Sens,
};

const COTE: u32 = 200;

fn app() -> Option<Appareil> {
    match Appareil::ouvrir() {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("pas d'adaptateur graphique ici ({e}) : test sauté");
            None
        }
    }
}

/// Les quatre quadrants de la tuile, dans l'ordre de l'image :
/// haut-gauche, haut-droite, bas-gauche, bas-droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quadrant {
    Rouge,
    Vert,
    Bleu,
    Blanc,
}

const RGBA: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
];

/// Une tuile 4 × 4 : la moitié HAUTE de l'image en rouge et vert, la moitié
/// basse en bleu et blanc.
fn atlas(app: &Appareil) -> AtlasGpu {
    let mut px = Vec::new();
    for ligne in 0..4 {
        for col in 0..4 {
            let q = (ligne / 2) * 2 + col / 2;
            px.extend(RGBA[q]);
        }
    }
    AtlasGpu::nouveau(app, 4, 1, &px)
}

/// Le quadrant qu'un texel `(u, v)` — en seizièmes, `v` vers le BAS de
/// l'image — désigne.
fn quadrant([u, v]: [f32; 2]) -> Quadrant {
    match (v < 8.0, u < 8.0) {
        (true, true) => Quadrant::Rouge,
        (true, false) => Quadrant::Vert,
        (false, true) => Quadrant::Bleu,
        (false, false) => Quadrant::Blanc,
    }
}

/// **La texture posée à plat sur le monde**, face par face — ce qu'on voit
/// d'un cube NON tourné, écrit depuis ce qu'on VOIT de dehors : un côté
/// debout et non retourné, le dessus avec le nord en haut, le dessous avec
/// le sud en haut. Pour un point `(x, y, z)` en seizièmes du bloc.
fn a_plat(face: Face, [x, y, z]: [f32; 3]) -> [f32; 2] {
    match face {
        Face::MoinsY => [x, 16.0 - z],
        Face::PlusY => [x, z],
        Face::MoinsZ => [16.0 - x, 16.0 - y],
        Face::PlusZ => [x, 16.0 - y],
        Face::MoinsX => [z, 16.0 - y],
        Face::PlusX => [16.0 - z, 16.0 - y],
    }
}

/// L'inverse d'une permutation signée.
fn inverse(a: Axes) -> Axes {
    let mut inv = a;
    for (i, &(j, s)) in a.iter().enumerate() {
        inv[j] = (i, s);
    }
    inv
}

/// Ce que la règle attend en un point `p` (seizièmes du bloc) de la face
/// `face` du MONDE, pour un cube plein aux uv par défaut tourné par `a`.
fn attendu(face: Face, p: [f32; 3], a: Axes, uvlock: bool) -> Quadrant {
    if uvlock {
        // Alignée sur le monde : la face montre ce qu'un cube non tourné
        // montrerait à cet endroit.
        quadrant(a_plat(face, p))
    } else {
        // La texture suit la géométrie : ce point montre ce que le cube non
        // tourné montrait à son ANTÉCÉDENT.
        let r = inverse(a);
        let avant = tf_assets::rotation::tourner_face(face, r);
        quadrant(a_plat(avant, tourner_point(p, r)))
    }
}

/// Le quadrant d'un pixel rendu, d'après sa couleur DOMINANTE : l'ombrage
/// assombrit, il ne change pas la teinte.
fn lire(image: &[u8], (x, y): (u32, u32)) -> Option<Quadrant> {
    let i = ((y * COTE + x) * 4) as usize;
    let [r, g, b] = [image[i] as i32, image[i + 1] as i32, image[i + 2] as i32];
    if r > 60 && g > 60 && b > 60 && (r - g).abs() < 40 && (r - b).abs() < 40 {
        Some(Quadrant::Blanc)
    } else if r > g + 60 && r > b + 60 {
        Some(Quadrant::Rouge)
    } else if g > r + 60 && g > b + 60 {
        Some(Quadrant::Vert)
    } else if b > r + 60 && b > g + 60 {
        Some(Quadrant::Bleu)
    } else {
        None
    }
}

fn projeter(cam: &Camera, p: [f32; 3]) -> (u32, u32) {
    let m = cam.gpu(1.0).vue_projection;
    let v = [p[0], p[1], p[2], 1.0];
    let clip: [f32; 4] = std::array::from_fn(|r| (0..4).map(|c| m[c][r] * v[c]).sum());
    let (x, y) = (clip[0] / clip[3], clip[1] / clip[3]);
    (
        ((x + 1.0) * 0.5 * COTE as f32) as u32,
        ((1.0 - y) * 0.5 * COTE as f32) as u32,
    )
}

/// Une caméra qui regarde la face de DEHORS, un peu de biais pour que le
/// dessus et le dessous ne soient jamais vus à la verticale pile.
fn camera_pour(face: Face) -> Camera {
    let centre = [8.5, 8.5, 8.5];
    let n = face.pas();
    let oeil = [
        centre[0] + 3.0 * n[0] as f32 + 0.3,
        centre[1] + 3.0 * n[1] as f32 + 0.2,
        centre[2] + 3.0 * n[2] as f32 + 0.25,
    ];
    Camera {
        oeil,
        cible: centre,
        fov: 0.9,
        proche: 0.05,
        loin: 100.0,
    }
}

/// Seize points de la face, en seizièmes du bloc, loin des bords des
/// quadrants.
fn points(face: Face) -> Vec<[f32; 3]> {
    let axe = face.axe();
    let bord = if face.pas()[axe] > 0 { 16.0 } else { 0.0 };
    let (a, b) = match axe {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut out = Vec::new();
    for s in [2.0, 6.0, 10.0, 14.0] {
        for t in [2.0, 6.0, 10.0, 14.0] {
            let mut p = [0.0; 3];
            p[axe] = bord;
            p[a] = s;
            p[b] = t;
            out.push(p);
        }
    }
    out
}

fn section(f: impl Fn(i32, i32, i32) -> StateId) -> Section {
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
        y: 0,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing: Packing::NoStraddle,
    }
}

/// Un cube plein en (8, 8, 8), habillé par `tf_assets::uv` pour la variante
/// `a` — par la passe gloutonne ou par celle des MODÈLES : le même solide,
/// les deux chemins de dessin.
fn scene(app: &Appareil, a: Axes, uvlock: bool, modele: bool) -> Scene {
    let (min, max) = ([0.0; 3], [16.0; 3]);
    let mut hab: HabillageFaces = [(0, [1.0; 3], [0.0; 4], false); 6];
    let mut sens = [Sens::DROIT; 6];
    for f in FACES {
        let posee = poser(f, min, max, uv_par_defaut(f, min, max), 0, a, uvlock);
        hab[posee.face.indice()] = (0, [1.0; 3], posee.uv, posee.echange);
        sens[posee.face.indice()] = Sens::depuis_uv(posee.uv, posee.echange);
    }
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    let id = if modele {
        t.pousser(
            false,
            false,
            vec![Cuboide {
                min,
                max,
                faces: 0x3F,
                cull: 0,
            }],
        )
    } else {
        t.pousser(false, true, Vec::new())
    };
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(|x, y, z| if (x, y, z) == (8, 8, 8) { id } else { 0 }),
    );
    let ch = g.mailler(&t);
    let arene = Arene::depuis(&ch, &|_, face, _| (0, [1.0; 3], sens[face.indice()]));
    let modeles = AreneModeles::depuis(&ch, &|id, _| {
        tf_render::faces_de(t.cuboides(id), &vec![hab; t.cuboides(id).len()])
    });
    Scene::avec_modeles(app, &arene, &modeles, &atlas(app))
}

/// Les points faux d'une variante, sur ses six faces.
fn fautes(app: &Appareil, x: u16, y: u16, uvlock: bool, modele: bool) -> Vec<String> {
    let a = axes(x, y);
    let s = scene(app, a, uvlock, modele);
    let mut out = Vec::new();
    for face in FACES {
        let cam = camera_pour(face);
        let (img, _) = s.rendre(&Cible::nouvelle(app, COTE, COTE), &cam);
        for p in points(face) {
            let monde = [8.0 + p[0] / 16.0, 8.0 + p[1] / 16.0, 8.0 + p[2] / 16.0];
            let voulu = attendu(face, p, a, uvlock);
            let lu = lire(&img, projeter(&cam, monde));
            if lu != Some(voulu) {
                out.push(format!(
                    "x={x} y={y} uvlock={uvlock} {} — {face:?} {p:?} : {lu:?} au lieu de {voulu:?}",
                    if modele { "modèle" } else { "cube" }
                ));
            }
        }
    }
    out
}

/// Les variantes éprouvées : le cube droit, les bûches couchées, un
/// escalier à l'envers, et des tours complets — avec et sans `uvlock`.
const VARIANTES: [(u16, u16, bool); 10] = [
    (0, 0, false),
    (90, 0, false),
    (0, 90, false),
    (90, 90, false),
    (180, 270, false),
    (270, 90, false),
    (0, 90, true),
    (90, 0, true),
    (180, 0, true),
    (90, 270, true),
];

fn verifier(modele: bool) {
    let Some(app) = app() else { return };
    let tout: Vec<String> = VARIANTES
        .iter()
        .flat_map(|&(x, y, uvlock)| fautes(&app, x, y, uvlock, modele))
        .collect();
    assert!(
        tout.is_empty(),
        "{} point(s) faux sur {} :\n{}",
        tout.len(),
        VARIANTES.len() * 96,
        tout.join("\n")
    );
}

#[test]
fn un_cube_glouton_pose_sa_texture_dans_le_sens_du_jeu() {
    verifier(false);
}

#[test]
fn une_face_de_modele_pose_sa_texture_dans_le_sens_du_jeu() {
    verifier(true);
}
