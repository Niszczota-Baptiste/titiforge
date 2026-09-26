//! Ce que le mailleur produit.

use tf_anvil::StateId;

use crate::forme::Face;

/// Un quad, en **seizièmes de bloc** dans le repère de la section.
///
/// La même unité que les modèles, et en flottants pour la même raison : 17 %
/// des coordonnées du pack Minefield ne sont pas entières. Les quads de la
/// passe gloutonne, eux, tombent toujours sur des bords de bloc — donc sur des
/// multiples de 16, exacts en flottant, et comparables sans tolérance. Un quad
/// de 3 × 5 blocs sort à 48 × 80.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Coin de plus petites coordonnées, dans le repère de la section.
    pub min: [f32; 3],
    /// Étendue dans les deux axes du plan de la face, dans l'ordre croissant
    /// des axes restants. Pour `±Y` : `[x, z]`.
    pub taille: [f32; 2],
    pub face: Face,
    /// L'état qui a produit ce quad — c'est lui qui désigne la tuile d'atlas.
    pub id: StateId,
    /// Le biome de la case, pour les états qui en prennent la couleur.
    ///
    /// Zéro — « on ne sait pas » — pour tous les autres, et c'est voulu :
    /// mettre le biome partout casserait la fusion gloutonne à chaque
    /// frontière, sur des blocs dont la couleur n'en dépend pas.
    pub biome: StateId,
}

impl Quad {
    /// Surface en seizièmes carrés. Sert aux tests de conservation : la somme
    /// des surfaces d'un maillage glouton doit égaler celle du maillage naïf.
    pub fn aire(&self) -> f64 {
        self.taille[0] as f64 * self.taille[1] as f64
    }
}

#[derive(Debug, Default, Clone)]
pub struct Maillage {
    pub quads: Vec<Quad>,
    /// Combien de quads viennent de la passe gloutonne.
    pub quads_glouton: usize,
    /// Combien viennent de la passe de modèles.
    pub quads_modele: usize,
}

impl Maillage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn est_vide(&self) -> bool {
        self.quads.is_empty()
    }

    pub fn len(&self) -> usize {
        self.quads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    pub fn aire(&self) -> f64 {
        self.quads.iter().map(Quad::aire).sum()
    }
}

/// Un bloc-modèle **posé**, sans sa géométrie.
///
/// C'est la réponse à la mesure de la passe de modèles : sur un build
/// Minefield, 349 k blocs-modèles produisaient **5,8 M de quads**, neuf
/// dixièmes du maillage. Or ces quads sont la MÊME géométrie répétée — deux
/// dalles de chêne posées côte à côte n'ont pas deux modèles, elles ont deux
/// positions.
///
/// On n'émet donc plus la géométrie mais la POSE : douze octets par bloc au
/// lieu d'un quad par face de chaque cuboïde. Le modèle vit une fois, dans un
/// tampon indexé par l'état, et le GPU le répète.
///
/// Contrepartie assumée : le masquage des faces ne peut plus être fait ici,
/// puisqu'on n'émet plus de faces. D'où `voisins_opaques`, que le shader
/// consulte — ça déplace le travail, ça ne le supprime pas. Mais il devient
/// proportionnel au nombre de BLOCS et non au nombre de faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    /// Position locale dans la section, `0..16` par axe.
    pub pos: [u8; 3],
    /// Un bit par face (ordre de `Face`) : le voisin de ce côté est opaque.
    pub voisins_opaques: u8,
    pub id: StateId,
    /// Le biome de la case — **zéro pour un état qui n'en prend pas la
    /// couleur**, exactement comme dans la clé de fusion de la passe
    /// gloutonne.
    ///
    /// C'est ce zéro qui fait tout le travail en aval : la table de géométrie
    /// du rendu est mémoïsée sur `(état, biome)`, donc un catalogue non teinté
    /// — la quasi-totalité — garde UNE table par état, comme avant que les
    /// biomes existent. Le mettre partout ferait autant de copies de la
    /// géométrie d'un escalier qu'il y a de biomes dans la scène, pour une
    /// couleur que l'escalier ne prend pas.
    pub biome: StateId,
}

/// Quelle texture une face de fluide porte — celle que le jeu choisit
/// (`LiquidBlockRenderer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TextureFluide {
    /// `*_still` : une surface qui ne court pas, et tout dessous.
    Immobile = 0,
    /// `*_flow` : les côtés, et une surface qui court — tournée selon le
    /// courant (`FaceFluide::angle`).
    Courant = 1,
    /// `water_overlay` : le côté de l'eau contre un bloc translucide qui
    /// remplit sa case — verre, feuilles. Le jeu la montre pour qu'on voie
    /// l'eau à travers la vitre sans voir la vitre à travers l'eau.
    Voile = 2,
}

/// **Une face de FLUIDE** — éventuellement fusionnée, en blocs dans le repère
/// de la section.
///
/// Pas un `Quad` : une surface d'eau n'est pas plate. Ses quatre coins ont
/// chacun leur hauteur — la moyenne pondérée des colonnes qui les entourent,
/// la règle même du jeu — et c'est ce qui fait descendre un courant en pente
/// au lieu de marches. Une étendue immobile, elle, a ses quatre coins égaux :
/// ce sont ses faces-là qui se fusionnent, et un océan sort en un quad par
/// section au lieu de 256.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceFluide {
    /// La CASE qui porte la face — pas le plan de la face : la hauteur d'un
    /// coin se compte depuis le bas de la case, et un côté est RENTRÉ d'un
    /// millième dans sa case, comme dans le jeu.
    pub pos: [u8; 3],
    /// Étendue dans les deux axes du plan, dans l'ordre croissant des axes
    /// (celui de `Quad::taille`), en blocs, de 1 à 16.
    pub taille: [u8; 2],
    pub face: Face,
    pub genre: crate::forme::GenreFluide,
    pub texture: TextureFluide,
    /// Les hauteurs du HAUT de la face, en 255e de bloc depuis le bas de la
    /// dernière rangée de cases.
    ///
    /// Dessus : les quatre coins `[NO, SO, SE, NE]` — `(x0, z0)`, `(x0, z1)`,
    /// `(x1, z1)`, `(x1, z0)` — l'ordre où le jeu émet ses sommets. Côtés :
    /// `[bout bas, bout haut]` le long de l'axe horizontal du plan, puis
    /// rien. Dessous : rien, il est plat.
    pub hauteurs: [u8; 4],
    /// Le sens du courant d'une surface `Courant`, en 65 536e de tour :
    /// `atan2(dz, dx) − π/2`, comme le jeu tourne sa texture.
    pub angle: u16,
    /// Le biome de la case pour l'EAU, qui en prend la couleur ; zéro pour la
    /// lave — la même règle que la clé de fusion gloutonne.
    pub biome: StateId,
}

/// Ce qu'une face de fluide pèse au GPU : cinq mots de 32 bits.
pub const OCTETS_FACE_FLUIDE: usize = 20;

/// Ce que produit la passe de modèles en mode instances.
#[derive(Debug, Default, Clone)]
pub struct Instances {
    pub poses: Vec<Instance>,
}

impl Instances {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.poses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.poses.is_empty()
    }

    /// Octets que ça pèse sur le GPU.
    pub fn octets(&self) -> usize {
        self.poses.len() * std::mem::size_of::<Instance>()
    }
}
