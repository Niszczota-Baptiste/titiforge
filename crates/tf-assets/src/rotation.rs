//! La rotation d'une VARIANTE : `"x": 90, "y": 270` dans un `blockstates`.
//!
//! Un pack ne décrit pas seize escaliers, il en décrit UN et le tourne. La
//! rotation est donc portée par la variante, pas par le modèle, et l'oublier
//! dessine tous les escaliers d'un build dans la même direction — sans la
//! moindre erreur, ce qui se lit « le rendu est bizarre » et ne désigne pas la
//! cause. Relevé sur une vraie save : tous les escaliers, toutes les échelles,
//! toutes les trappes, et 38 % de la passe de modèles pour le seul
//! `mushroom_stem`, dont les six faces se superposaient en une.
//!
//! **Une seule règle dans le dépôt.** Elle vivait en double — ici pour le
//! rendu, dans `tf-blocks` pour la dérivation — et les deux copies avaient
//! divergé : l'une tournait autour de X dans le sens de l'autre. C'est le
//! piège que le projet s'interdit, et il s'était déjà refermé deux fois.
//!
//! ## Le sens, et comment on le sait
//!
//! Le format applique `x` PUIS `y`, et les deux dans le sens NÉGATIF (le
//! chargeur de Minecraft construit son quaternion avec `-x` et `-y`). Plutôt
//! que de croire une lecture de code, on l'ancre sur un bloc qui ne laisse
//! aucune ambiguïté : `minecraft:mushroom_stem`.
//!
//! Son modèle est un PLAN sur la face nord (`from [0,0,0] to [16,16,0]`, une
//! seule face `north`), et son `blockstates` multipart applique une rotation
//! par face exposée :
//!
//! | quand | rotation | où le plan doit atterrir |
//! |---|---|---|
//! | `north=true` | — | nord (−Z) |
//! | `east=true` | `y: 90` | est (+X) |
//! | `south=true` | `y: 180` | sud (+Z) |
//! | `west=true` | `y: 270` | ouest (−X) |
//! | `up=true` | `x: 270` | haut (+Y) |
//! | `down=true` | `x: 90` | bas (−Y) |
//!
//! Six rotations, six faces distinctes, chacune nommée par sa condition : le
//! sens est FORCÉ, il n'y a pas à le choisir. Un test rejoue ce tableau.
//!
//! ## Une permutation signée, pas une matrice
//!
//! Les angles sont des multiples de 90°, donc la rotation se réduit à une
//! permutation des axes avec un signe. C'est plus qu'une optimisation : la
//! même table s'applique à un cuboïde en seizièmes flottants (le rendu) et à
//! une boîte en 256es entiers (la dérivation), ce qui est exactement ce qui
//! permet de n'écrire la règle qu'une fois.

use tf_mesh::forme::{Cuboide, Face, FACES};

/// Le centre du bloc, en seizièmes. Une rotation de bloc tourne AUTOUR de lui.
pub const CENTRE: f32 = 8.0;

/// L'image des trois axes d'entrée : `image[i] = (axe d'arrivée, sens)`.
///
/// Se lit « l'axe *i* part sur l'axe `.0`, dans le sens `.1` ».
pub type Axes = [(usize, i8); 3];

/// L'identité : chaque axe reste chez lui, dans son sens.
pub const IDENTITE: Axes = [(0, 1), (1, 1), (2, 1)];

/// Un quart de tour autour de X : `Y → −Z`, `Z → Y`.
const QUART_X: Axes = [(0, 1), (2, -1), (1, 1)];
/// Un quart de tour autour de Y : `X → Z`, `Z → −X`.
const QUART_Y: Axes = [(2, 1), (1, 1), (0, -1)];

fn composer(image: Axes, pas: Axes) -> Axes {
    let mut out = IDENTITE;
    for (i, e) in image.iter().enumerate() {
        let (j, s) = pas[e.0];
        out[i] = (j, e.1 * s);
    }
    out
}

/// La permutation signée d'une rotation de variante.
///
/// `x` s'applique avant `y` — l'ordre du format. Les angles hors multiples de
/// 90° n'existent pas dans un `blockstates` ; ils sont ramenés au quart de tour
/// inférieur plutôt que rejetés, parce qu'un pack tiers mal écrit ne doit pas
/// faire disparaître un bloc.
pub fn axes(x: u16, y: u16) -> Axes {
    let mut image = IDENTITE;
    for _ in 0..(x / 90) % 4 {
        image = composer(image, QUART_X);
    }
    for _ in 0..(y / 90) % 4 {
        image = composer(image, QUART_Y);
    }
    image
}

/// Vrai si la rotation ne change rien — de quoi sauter le travail.
pub fn est_identite(a: Axes) -> bool {
    a == IDENTITE
}

/// Le point `p`, en seizièmes, tourné autour du centre du bloc.
pub fn tourner_point(p: [f32; 3], a: Axes) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (i, &(j, s)) in a.iter().enumerate() {
        out[j] = CENTRE + (p[i] - CENTRE) * s as f32;
    }
    out
}

/// La face `f` après rotation.
pub fn tourner_face(f: Face, a: Axes) -> Face {
    let (j, s) = a[f.axe()];
    let positif = if s >= 0 { f.positif() } else { !f.positif() };
    FACES[j * 2 + usize::from(positif)]
}

/// Un masque de faces, tourné. Sert aux deux masques d'un cuboïde —
/// `faces` et `cull` — qui se transforment de la même façon.
pub fn tourner_masque(m: u8, a: Axes) -> u8 {
    let mut out = 0u8;
    for f in FACES {
        if m & f.bit() != 0 {
            out |= tourner_face(f, a).bit();
        }
    }
    out
}

/// Un cuboïde tourné : ses deux coins, et ses deux masques de faces.
///
/// Les coins se croisent après rotation — `min` peut passer au-dessus de
/// `max` — d'où la remise en ordre. L'oublier rendrait une boîte de volume
/// négatif, que `remplit()` et `au_bord()` liraient de travers sans se
/// plaindre.
pub fn tourner_cuboide(c: &Cuboide, a: Axes) -> Cuboide {
    let (p, q) = (tourner_point(c.min, a), tourner_point(c.max, a));
    let mut min = [0.0f32; 3];
    let mut max = [0.0f32; 3];
    for k in 0..3 {
        min[k] = p[k].min(q[k]);
        max[k] = p[k].max(q[k]);
    }
    Cuboide {
        min,
        max,
        faces: tourner_masque(c.faces, a),
        cull: tourner_masque(c.cull, a),
    }
}

/// Les cuboïdes d'une variante, tournés. Ne copie rien quand il n'y a rien à
/// tourner : la plupart des variantes sont à zéro degré.
pub fn tourner(cuboides: Vec<Cuboide>, x: u16, y: u16) -> Vec<Cuboide> {
    let a = axes(x, y);
    if est_identite(a) {
        return cuboides;
    }
    cuboides.iter().map(|c| tourner_cuboide(c, a)).collect()
}
