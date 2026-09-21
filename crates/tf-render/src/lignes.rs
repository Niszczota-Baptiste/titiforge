//! **Le quadrillage : voir où sont les chunks et les `.mca`.**
//!
//! Minecraft montre les bords de chunk (F3+G) parce que tout constructeur en
//! a besoin — aligner une ferme, un mur, une porte. Il ne montre PAS le
//! découpage en `.mca`, et c'est pourtant celui qui décide ce qu'on exporte,
//! ce qu'on envoie à quelqu'un, et ce qu'une opération va réécrire.
//!
//! La géométrie des cellules vit dans `tf_world::decoupe` — les coordonnées
//! sont du savoir Minecraft, pas du rendu. Ici il n'y a que des segments et
//! des couleurs.
//!
//! ## Deux décisions qui se voient
//!
//! **Le quadrillage ne passe pas le test de profondeur.** Un repère qui
//! disparaît derrière le mur qu'on est en train d'aligner n'est pas un
//! repère. C'est un CALQUE : il se dessine par-dessus, en dernier. La
//! contrepartie est assumée — une ligne lointaine peut recouvrir ce qui est
//! devant — et elle est tenue par le rayon borné de `cellules_autour`, qui ne
//! montre que le voisinage.
//!
//! **L'épaisseur ne se règle pas.** wgpu ne garantit pas de trait plus large
//! qu'un pixel, sur aucune plateforme. Ce qui SÉPARE un niveau de l'autre est
//! donc la couleur, jamais la largeur — s'appuyer sur l'épaisseur donnerait un
//! rendu juste sur une carte et plat sur une autre, sans erreur nulle part.

use bytemuck::{Pod, Zeroable};

/// **DEUX unités cohabitent dans le rendu, et il faut savoir laquelle on
/// tient.**
///
/// Les tables de géométrie sont en **seizièmes de bloc**, parce que c'est
/// l'unité des modèles du pack : un cuboïde de dalle va de 0 à 8, et 17,2 %
/// des coordonnées du pack ne sont pas entières. Les origines de section
/// valent donc `chunk × 16`, et les quads gloutons tombent sur des multiples
/// de 16.
///
/// Mais l'espace de la CAMÉRA est en **blocs** : les deux shaders divisent par
/// seize juste avant la matrice de vue. Le quadrillage y entre directement,
/// donc il se donne en BLOCS — sans conversion.
///
/// J'ai écrit la conversion, par symétrie avec les tables de géométrie. Elle
/// dessinait la grille SEIZE FOIS TROP GRANDE, et l'image restait
/// parfaitement plausible : un quadrillage large est exactement ce à quoi
/// ressemble un quadrillage. C'est le piège `toHeights` / `applyHeightmap`
/// sous une autre forme — chaque moitié juste, la jonction fausse — et la
/// parade est la même : un test qui TRAVERSE, ici en exigeant que le contour
/// d'une section encadre les pixels de cette section.
pub const SEIZIEMES_PAR_BLOC: f32 = 16.0;

/// Un sommet de ligne : une position monde, une couleur.
///
/// **`[f32; 3]` est correct ICI, contrairement aux tables d'uniformes.** La
/// règle des seize octets d'alignement d'un `vec3<f32>` en WGSL porte sur les
/// tampons d'uniformes et de stockage ; un tampon de SOMMETS déclare ses
/// décalages d'attributs explicitement, donc `position` à 0 et `couleur` à 12
/// est exactement ce que le pipeline lira. Confondre les deux cas fait border
/// les structures de bourrage qui ne sert à rien — ou pire, l'oublier là où
/// elle compte.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Sommet {
    /// En **blocs** : c'est l'unité de la caméra, celle dans laquelle les
    /// deux shaders de géométrie entrent après leur division par seize.
    pub position: [f32; 3],
    /// RGBA empaqueté, un octet par canal, rouge en poids faible.
    pub couleur: u32,
}

/// Une couleur de quadrillage, en RGBA sur huit bits par canal.
pub const fn rgba(r: u8, v: u8, b: u8, a: u8) -> u32 {
    (r as u32) | ((v as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24)
}

/// Les segments à dessiner, en paires de sommets.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Lignes {
    pub sommets: Vec<Sommet>,
}

impl Lignes {
    pub fn new() -> Lignes {
        Lignes::default()
    }

    /// Un segment, en coordonnées de BLOC — l'unité de la caméra.
    pub fn segment(&mut self, a: [f32; 3], b: [f32; 3], couleur: u32) {
        for p in [a, b] {
            self.sommets.push(Sommet {
                position: p,
                couleur,
            });
        }
    }

    /// Les douze arêtes d'une boîte, en coordonnées de BLOC.
    ///
    /// `max` est le coin EXCLUSIF — le bord du dernier bloc, pas son origine.
    /// Une boîte de blocs a des bornes INCLUSES (`BBox`), donc l'appelant
    /// ajoute un : sans ça le quadrillage passe au milieu de la dernière
    /// rangée de blocs au lieu de la border, ce qui se lit « la grille est
    /// décalée » et fait douter du découpage plutôt que du tracé.
    pub fn contour(&mut self, min: [f32; 3], max: [f32; 3], couleur: u32) {
        let c = |x: usize, y: usize, z: usize| {
            [
                if x == 0 { min[0] } else { max[0] },
                if y == 0 { min[1] } else { max[1] },
                if z == 0 { min[2] } else { max[2] },
            ]
        };
        // Les quatre montants, puis les deux anneaux. Écrit ainsi plutôt
        // qu'en table d'indices : une table d'arêtes se relit mal et se
        // vérifie par le COMPTE, qui ne dit rien sur lesquelles.
        for (x, z) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            self.segment(c(x, 0, z), c(x, 1, z), couleur);
        }
        for y in [0, 1] {
            self.segment(c(0, y, 0), c(1, y, 0), couleur);
            self.segment(c(1, y, 0), c(1, y, 1), couleur);
            self.segment(c(1, y, 1), c(0, y, 1), couleur);
            self.segment(c(0, y, 1), c(0, y, 0), couleur);
        }
    }

    pub fn len(&self) -> usize {
        self.sommets.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.sommets.is_empty()
    }

    pub fn octets(&self) -> usize {
        std::mem::size_of_val(&self.sommets[..])
    }
}
