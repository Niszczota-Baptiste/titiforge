//! Quelle TUILE et quelle TEINTE, pour chaque face d'un bloc.
//!
//! `table_formes` dit au mailleur ce qu'un bloc OPPOSE ; ici on dit au rendu à
//! quoi il RESSEMBLE. Les deux tables sont séparées parce que le mailleur n'a
//! aucun besoin de savoir qu'un atlas existe.
//!
//! Deux défauts que cette table corrige, tous deux relevés sur une vraie save :
//!
//! - **Une seule tuile pour les six faces.** Prendre la texture du dessus et
//!   la poser partout habille les côtés d'un bloc d'herbe avec de l'herbe. Un
//!   modèle déclare une texture PAR FACE, et c'est gratuit de les lire toutes.
//! - **Les textures teintées du jeu sont GRISES.** `grass_block_top.png` vaut
//!   (147, 147, 147) : c'est le jeu qui les multiplie par une couleur de
//!   biome, ce que la face signale avec `tintindex`. En ignorant l'indication,
//!   le sol de tout terrain sort BLANCHÂTRE — la texture s'affiche, simplement
//!   pas de la bonne couleur, ce qui se lit « les blocs ont la mauvaise
//!   couleur » et ne désigne pas la cause.
//!
//! Cette table ne concerne que les **cubes pleins** : eux seuls passent par la
//! passe gloutonne, qui est la seule à produire des quads. Un bloc-modèle
//! porte sa géométrie, et ses faces se textureront avec elle.

use tf_mesh::forme::{Face, FACES};

use crate::atlas::Atlas;
use crate::catalogue::{decouper, Catalogue};
use crate::modele::Element;

/// Ce que le rendu doit savoir d'une face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Apparence {
    /// La couche d'atlas.
    pub couche: u32,
    /// Ce par quoi multiplier le texel. `[1, 1, 1]` pour une face non teintée.
    pub teinte: [f32; 3],
    /// Les uv de la face, en seizièmes — DÉDUITES du cuboïde quand le modèle
    /// n'en déclare pas, ce qui est le cas de 26 % des faces du pack.
    ///
    /// Elles ne suivent pas encore la rotation de la variante (`uvlock`) : une
    /// dalle tournée montrera la bonne portion de texture, pas forcément dans
    /// le bon sens. Une texture de travers se voit et se corrige ; une face
    /// absente ne se voit pas du tout, et c'est l'ordre dans lequel on les
    /// traite.
    pub uv: [f32; 4],
}

impl Default for Apparence {
    fn default() -> Self {
        Apparence {
            couche: 0,
            teinte: [1.0; 3],
            uv: [0.0, 0.0, 16.0, 16.0],
        }
    }
}

/// Les couleurs de teinte, **en attendant de lire les biomes**.
///
/// C'est un RÉGLAGE, pas une mesure, et il se nomme comme tel. Dans le jeu la
/// couleur vient du biome du bloc — une donnée qui vit dans le chunk, par
/// section, et qu'on ne décode pas encore. Les valeurs ci-dessous sont celles
/// des plaines ; un désert ou une taïga en ont d'autres.
///
/// `tintindex` dit qu'une face EST teintée ; il ne dit pas par quoi. C'est le
/// bloc qui décide — dans le jeu, un gestionnaire par bloc — et c'est pour ça
/// que la correspondance est ici et pas dans le modèle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teintes {
    pub herbe: [u8; 3],
    pub feuillage: [u8; 3],
    pub eau: [u8; 3],
}

impl Default for Teintes {
    fn default() -> Self {
        Teintes {
            herbe: [0x91, 0xBD, 0x59],
            feuillage: [0x77, 0xAB, 0x2F],
            eau: [0x3F, 0x76, 0xE4],
        }
    }
}

impl Teintes {
    /// La couleur qu'un bloc applique à ses faces teintées, s'il en a une.
    ///
    /// Sur les noms plutôt que sur une liste exhaustive : le catalogue
    /// Minefield en compte 1 678, et une liste écrite à la main en raterait la
    /// moitié le jour de la prochaine mise à jour du serveur.
    pub fn pour(&self, nom: &str) -> [u8; 3] {
        let feuille = nom.rsplit(':').next().unwrap_or(nom);
        if feuille.contains("water") || feuille.contains("cauldron") {
            self.eau
        } else if feuille.contains("leaves") || feuille.contains("vine") {
            self.feuillage
        } else {
            self.herbe
        }
    }
}

/// Une composante sRGB `0..1` ramenée en LINÉAIRE.
///
/// La courbe exacte du standard, seuil compris — l'approximation en puissance
/// 2,2 se trompe de plusieurs unités dans les tons sombres, qui sont justement
/// ceux d'une teinte.
pub fn en_lineaire(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// La teinte FINALE d'une face teintée : ce par quoi multiplier le texel,
/// **en linéaire**.
///
/// Deux pièges superposés, et il a fallu les deux pour que le sol soit vert.
///
/// **1. La règle est celle du jeu, `texel × teinte`, sans compenser le gris.**
/// La texture porte le GRAIN et la luminosité (147/255 pour l'herbe), la
/// teinte porte la COULEUR. Compenser le gris pour retrouver la couleur de
/// biome pleine est faux et VISIBLE : le facteur vaut 1,735 pour l'herbe, donc
/// le canal vert monte à 1,286 et se fait écrêter à 1 puisqu'une teinte ne
/// peut qu'assombrir, pendant que le rouge passe à 0,987 sans être touché. Le
/// vert perd son avance sur le rouge et le sol sort OLIVE — mesuré :
/// (145, 147, 89) au lieu de (84, 109, 51). Un écrêtage par canal ne conserve
/// pas une teinte, il la déplace. `Atlas::facteur_de_teinte` reste juste pour
/// ce qu'il décrit — une couleur PLATE, sans texture — et le chemin texturé ne
/// doit pas s'en servir.
///
/// **2. Une couleur de biome est en sRGB, le mélange se fait en LINÉAIRE.**
/// L'atlas et la cible sont en `Rgba8UnormSrgb` : le texel est décodé à la
/// lecture et le résultat réencodé à l'écriture. Une teinte passée telle
/// quelle y devient un facteur linéaire alors qu'elle est une valeur sRGB, et
/// le sol sort délavé — mesuré sur une vraie save, (113, 128, 90) au lieu de
/// (84, 109, 51). C'est le piège des couleurs de sommet d'`ExeWorldEdit`, sous
/// une autre forme : l'espace d'une couleur se dit, il ne se devine pas.
/// Convertie, la chaîne rend (82, 108, 47) — la couleur du jeu à deux unités
/// près, l'écart entre multiplier en sRGB et multiplier en linéaire.
///
/// Bornée quand même : une teinte ne peut qu'ASSOMBRIR, c'est une
/// multiplication. Une couleur de biome tient dans un octet, donc la borne ne
/// mord jamais aujourd'hui — elle tient la propriété le jour où la couleur
/// viendra d'ailleurs.
pub fn teinte_finale(couleur: [u8; 3]) -> [f32; 3] {
    let mut out = [1.0f32; 3];
    for k in 0..3 {
        out[k] = en_lineaire((couleur[k] as f32 / 255.0).clamp(0.0, 1.0));
    }
    out
}

/// L'habillage d'un état : son cube, et chacun de ses cuboïdes.
///
/// Les deux ne servent jamais ensemble. Un cube plein opaque passe par la
/// passe gloutonne et n'a pas de géométrie ; tout le reste passe par la passe
/// de modèles et porte la sienne, cuboïde par cuboïde.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Habillage {
    /// Les six faces du cuboïde qui remplit la case — pour la passe gloutonne.
    pub cube: [Apparence; 6],
    /// Les six faces de CHAQUE cuboïde — pour la passe de modèles. Dans
    /// l'ordre exact de `TableFormes::cuboides`, parce que les deux sortent du
    /// même parcours.
    pub cuboides: Vec<[Apparence; 6]>,
}

/// L'habillage des six faces d'un élément de modèle, la rotation de sa
/// variante appliquée.
///
/// La rotation DÉPLACE les faces : la texture du nord d'un modèle tourné de
/// 90° habille l'est du bloc. L'oublier ici referait, sur les textures, le
/// défaut qu'on vient de corriger sur la géométrie.
pub fn habiller(
    e: &Element,
    axes: crate::rotation::Axes,
    atlas: &Atlas,
    couleur: Option<[u8; 3]>,
) -> [Apparence; 6] {
    let mut faces = [Apparence::default(); 6];
    for f in FACES {
        let Some(fd) = e.faces.get(&f) else {
            continue;
        };
        let Some(couche) = atlas.couche(&fd.texture) else {
            continue;
        };
        faces[crate::rotation::tourner_face(f, axes).indice()] = Apparence {
            couche,
            teinte: match (fd.tintindex, couleur) {
                (Some(_), Some(c)) => teinte_finale(c),
                _ => [1.0; 3],
            },
            uv: crate::modele::uv_de(e, f, fd),
        };
    }
    faces
}

/// Les noms de texture des seules faces qu'on va DESSINER.
///
/// Un tableau de textures est plafonné à 2 048 couches et le pack du serveur
/// en cite 2 207 : charger tout le catalogue dépasse la limite ET paie des
/// tuiles qu'aucun bloc de la scène n'emploie.
pub fn textures_des_etats(cat: &Catalogue, cles: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for cle in cles {
        let (nom, etat) = decouper(&cle);
        let Some(bs) = cat.blockstate(nom) else {
            continue;
        };
        for var in bs.pour(&etat) {
            let Some(m) = cat.modele(&var.modele) else {
                continue;
            };
            for e in &m.elements {
                for fd in e.faces.values() {
                    if !fd.texture.starts_with('#') {
                        v.push(fd.texture.clone());
                    }
                }
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// La face d'un bloc, pour indexer une `[Apparence; 6]`.
pub fn indice(f: Face) -> usize {
    f.indice()
}
