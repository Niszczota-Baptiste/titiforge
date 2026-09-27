//! **Dans quel SENS une texture se pose sur une face** — la règle du jeu,
//! transcrite dans son ordre.
//!
//! Le jeu n'associe pas un rectangle d'uv à une face : il attache chaque coin
//! d'uv à un SOMMET précis (`FaceInfo` donne l'ordre des sommets de chaque
//! face, `BlockFaceUV` le coin qui revient à chacun, décalé par la `rotation`
//! de la face). Puis il tourne les sommets avec la variante. Sans `uvlock`,
//! la texture suit donc la géométrie — une bûche couchée garde ses fibres le
//! long de son axe ; avec, les uv sont RECALCULÉES pour que la texture reste
//! alignée sur le monde (`FaceBakery::recomputeUVs`) — la marche d'un
//! escalier tourné garde ses planches dans le sens de la pièce.
//!
//! Le rendu, lui, ne connaît que les deux axes CROISSANTS du plan de la face.
//! Tout se réduit donc à trois choses : les uv du coin `(0, 0)` de ce plan,
//! celles du coin `(1, 1)`, et si `u` court le long du SECOND axe plutôt que
//! du premier (`echange`). Les rotations de variante sont des multiples de
//! 90° : un rectangle reste un rectangle, et ces trois nombres le disent
//! entièrement.
//!
//! **Les quatre faces qu'on posait de travers.** Avant cette table, le rendu
//! posait `u` le long du premier axe et `v` le long du second, sur toutes les
//! faces : le nord sortait tourné d'un demi-tour, le sud et le dessous
//! retournés, l'est et l'ouest couchés. Seul le dessus était juste —
//! invisible sur la pierre et les planches, flagrant sur la frange d'herbe
//! d'un côté de `grass_block`, qui passait EN BAS du bloc. Mesuré au pixel
//! contre la règle du jeu (`tf-render/tests/orientation.rs`) : 73 points
//! faux sur 96.

use tf_mesh::forme::Face;

use crate::rotation::{tourner_face, tourner_point, Axes};

/// Les uv PAR DÉFAUT d'une face, quand le modèle n'en déclare pas : la
/// portion de texture qui correspond à la position de la face dans le bloc
/// (`BlockElement::uvsByFace`).
///
/// Chaque face a SA formule, et les six diffèrent : le dessous compte `v`
/// depuis le sud, le nord et l'est comptent `u` depuis leur bord de plus
/// GRANDE coordonnée. Une formule par paire d'opposées — ce qu'on avait —
/// donne la bonne portion à la mauvaise place dès qu'un élément ne couvre
/// pas toute sa face.
pub fn uv_par_defaut(face: Face, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    match face {
        Face::MoinsY => [from[0], 16.0 - to[2], to[0], 16.0 - from[2]],
        Face::PlusY => [from[0], from[2], to[0], to[2]],
        Face::MoinsZ => [16.0 - to[0], 16.0 - to[1], 16.0 - from[0], 16.0 - from[1]],
        Face::PlusZ => [from[0], 16.0 - to[1], to[0], 16.0 - from[1]],
        Face::MoinsX => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
        Face::PlusX => [16.0 - to[2], 16.0 - to[1], 16.0 - from[2], 16.0 - from[1]],
    }
}

/// Les quatre sommets d'une face, dans l'ordre du jeu (`FaceInfo`).
///
/// C'est cet ORDRE qui porte le sens de la texture : le sommet 0 reçoit
/// `(u0, v0)`, le 1 `(u0, v1)`, le 2 `(u1, v1)`, le 3 `(u1, v0)` — à la
/// rotation de la face près.
pub fn sommets(face: Face, min: [f32; 3], max: [f32; 3]) -> [[f32; 3]; 4] {
    let ([x0, y0, z0], [x1, y1, z1]) = (min, max);
    match face {
        Face::MoinsY => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        Face::PlusY => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Face::MoinsZ => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        Face::PlusZ => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        Face::MoinsX => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        Face::PlusX => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    }
}

/// Les uv du sommet `i`, pour un rectangle `uv` et une rotation de face
/// (`BlockFaceUV::getU` / `getV`).
pub fn uv_du_sommet(uv: [f32; 4], rotation: u16, i: usize) -> [f32; 2] {
    let s = (i + (rotation as usize / 90)) % 4;
    [
        if s < 2 { uv[0] } else { uv[2] },
        if s == 0 || s == 3 { uv[1] } else { uv[3] },
    ]
}

/// Une rotation d'angle droit, en matrice entière : `m[ligne][colonne]`,
/// appliquée à un vecteur colonne. Exacte — le jeu calcule en flottants des
/// cosinus de 90°, mais il n'en tire que des signes et des multiples de 90°.
type Mat = [[i32; 3]; 3];

const I: Mat = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];

fn produit(a: Mat, b: Mat) -> Mat {
    std::array::from_fn(|l| std::array::from_fn(|c| (0..3).map(|k| a[l][k] * b[k][c]).sum()))
}

fn transposee(a: Mat) -> Mat {
    std::array::from_fn(|l| std::array::from_fn(|c| a[c][l]))
}

fn appliquer(m: Mat, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|l| (0..3).map(|k| m[l][k] as f32 * v[k]).sum())
}

/// La matrice d'une permutation signée : l'axe `i` part sur `a[i].0`.
fn matrice(a: Axes) -> Mat {
    let mut m = [[0; 3]; 3];
    for (i, &(j, s)) in a.iter().enumerate() {
        m[j][i] = s as i32;
    }
    m
}

/// Rotation d'un angle droit autour de Y (`Vector3f.YP.rotationDegrees`).
fn ry(quarts: i32) -> Mat {
    let (c, s) = cos_sin(quarts);
    [[c, 0, s], [0, 1, 0], [-s, 0, c]]
}

/// Rotation d'un angle droit autour de X (`Vector3f.XP.rotationDegrees`).
fn rx(quarts: i32) -> Mat {
    let (c, s) = cos_sin(quarts);
    [[1, 0, 0], [0, c, -s], [0, s, c]]
}

fn cos_sin(quarts: i32) -> (i32, i32) {
    match quarts.rem_euclid(4) {
        0 => (1, 0),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (0, -1),
    }
}

/// Ce qui amène le repère des uv d'une face dans celui du monde
/// (`BlockMath::VANILLA_UV_TRANSFORM_LOCAL_TO_GLOBAL`).
fn local_vers_global(face: Face) -> Mat {
    match face {
        Face::PlusZ => I,
        Face::PlusX => ry(1),
        Face::MoinsX => ry(-1),
        Face::MoinsZ => ry(2),
        Face::PlusY => rx(-1),
        Face::MoinsY => rx(1),
    }
}

/// **Les uv recalculées pour `uvlock`** (`FaceBakery::recomputeUVs`,
/// `BlockMath::getUVLockTransform`), transcrites ligne à ligne : le rectangle
/// et la rotation de la face, tels que la texture reste alignée sur le monde
/// une fois la variante tournée.
///
/// `face` est la direction de la face dans le repère du MODÈLE, avant
/// rotation — celle que le jeu passe à `recomputeUVs`.
pub fn pour_uvlock(uv: [f32; 4], rotation: u16, face: Face, a: Axes) -> ([f32; 4], u16) {
    let r = matrice(a);
    let arrivee = tourner_face(face, a);
    // G2L(face) · R⁻¹ · L2G(face tournée), puis autour du CENTRE du bloc.
    let m = produit(
        produit(transposee(local_vers_global(face)), transposee(r)),
        local_vers_global(arrivee),
    );
    let autour_du_centre = |u: f32, v: f32| {
        let p = appliquer(m, [u - 8.0, v - 8.0, -8.0]);
        [p[0] + 8.0, p[1] + 8.0]
    };
    // Les deux coins du rectangle : `(u0, v0)` et `(u1, v1)`, quelle que
    // soit la rotation — c'est ce que `getReverseIndex` désigne.
    let [f2, f3] = autour_du_centre(uv[0], uv[1]);
    let [f6, f7] = autour_du_centre(uv[2], uv[3]);
    let meme_sens = |a: f32, b: f32| signe(a) == signe(b);
    let (f8, f9) = if meme_sens(uv[2] - uv[0], f6 - f2) {
        (f2, f6)
    } else {
        (f6, f2)
    };
    let (f10, f11) = if meme_sens(uv[3] - uv[1], f7 - f3) {
        (f3, f7)
    } else {
        (f7, f3)
    };
    // La rotation de la face : son vecteur (cos, sin) passé par la même
    // matrice, et l'angle qui en sort, compté dans l'AUTRE sens. Ce signe
    // est celui du jeu, et il a une conséquence qu'il faut connaître : sans
    // aucune rotation de variante, une face tournée de 90° passe à 270°. Il
    // est juste pour une face non tournée — tous les escaliers, toutes les
    // barrières — et c'est lui qui les garde alignés sur le monde.
    let (c, s) = cos_sin(rotation as i32 / 90);
    let w = [m[0][0] * c + m[0][1] * s, m[1][0] * c + m[1][1] * s];
    let quarts = match w {
        [1, 0] => 0,
        [0, 1] => 1,
        [-1, 0] => 2,
        [0, -1] => 3,
        // Le vecteur sort du plan : n'arrive pas pour une rotation de bloc,
        // qui garde chaque face dans un plan d'axe. On garde la rotation.
        _ => return ([f8, f10, f9, f11], rotation),
    };
    ([f8, f10, f9, f11], ((4 - quarts) % 4 * 90) as u16)
}

/// `Math.signum` : −1, 0 ou 1 — et `−0` vaut `0`.
fn signe(x: f32) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// **Une face telle que le rendu la dessine**, une fois la variante tournée.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Posee {
    /// La direction de la face dans le MONDE.
    pub face: Face,
    /// Les uv du coin `(0, 0)` du plan de la face, puis de son coin `(1, 1)`
    /// — ses deux axes CROISSANTS, dans l'ordre croissant : `(Y, Z)` pour
    /// `±X`, `(X, Z)` pour `±Y`, `(X, Y)` pour `±Z`.
    pub uv: [f32; 4],
    /// `u` court le long du SECOND axe du plan, `v` le long du premier.
    pub echange: bool,
}

/// Les deux axes croissants du plan d'une face.
pub fn axes_du_plan(face: Face) -> (usize, usize) {
    match face.axe() {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// **Pose une face d'élément** : ses sommets dans l'ordre du jeu, leurs uv,
/// la variante — et ce qui en reste dans le plan de la face tournée.
///
/// `uv` et `rotation` sont ceux de la face dans le modèle (`uv` déjà déduit
/// s'il n'était pas déclaré) ; `min` et `max` les bornes de l'élément.
pub fn poser(
    face: Face,
    min: [f32; 3],
    max: [f32; 3],
    uv: [f32; 4],
    rotation: u16,
    a: Axes,
    uvlock: bool,
) -> Posee {
    let (uv, rotation) = if uvlock {
        pour_uvlock(uv, rotation, face, a)
    } else {
        (uv, rotation)
    };
    let arrivee = tourner_face(face, a);
    let (au, av) = axes_du_plan(arrivee);
    let p: [[f32; 3]; 4] = sommets(face, min, max).map(|s| tourner_point(s, a));
    let lo = |k: usize| p.iter().map(|q| q[k]).fold(f32::INFINITY, f32::min);
    let (lu, lv) = (lo(au), lo(av));
    // Le coin de chaque sommet dans le plan : 0 au bord bas, 1 au bord haut.
    // Les coordonnées tournées sont EXACTES (une permutation signée autour
    // du centre) : deux sommets d'un même bord portent le même nombre.
    let coin = |q: &[f32; 3]| (q[au] > lu, q[av] > lv);
    let uv_du = |cherche: (bool, bool)| {
        (0..4)
            .find(|&i| coin(&p[i]) == cherche)
            .map(|i| uv_du_sommet(uv, rotation, i))
    };
    let (Some(o), Some(d)) = (uv_du((false, false)), uv_du((true, true))) else {
        // Une face sans étendue : rien à dessiner, rien à orienter.
        return Posee {
            face: arrivee,
            uv: [uv[0], uv[1], uv[2], uv[3]],
            echange: false,
        };
    };
    // Le long du PREMIER axe, qu'est-ce qui change : `u` ou `v` ? On
    // regarde celui des deux qui VARIE : un rectangle d'uv plat dans un sens
    // (`v0 == v1`) ne dit rien de ce sens-là.
    let echange = match uv_du((true, false)) {
        Some(q) if o[0] != d[0] => q[0] == o[0],
        Some(q) => q[1] != o[1],
        None => false,
    };
    Posee {
        face: arrivee,
        uv: [o[0], o[1], d[0], d[1]],
        echange,
    }
}

/// Le `(u, v)` que la face posée donne à un point `(s, t)` de son plan, en
/// fraction de ses deux axes croissants — ce que le shader calcule.
pub fn uv_au_point(p: &Posee, s: f32, t: f32) -> [f32; 2] {
    let (a, b) = if p.echange { (t, s) } else { (s, t) };
    [
        p.uv[0] + (p.uv[2] - p.uv[0]) * a,
        p.uv[1] + (p.uv[3] - p.uv[1]) * b,
    ]
}
