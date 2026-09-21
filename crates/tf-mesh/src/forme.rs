//! Ce que le mailleur sait d'un bloc — et rien d'autre.
//!
//! `tf-mesh` ne sait pas qu'un pack de ressources existe, ni un fichier, ni un
//! `.jar`. Il demande trois choses à son hôte, et l'hôte les tire d'où il veut.
//! C'est la même frontière que le `StorageAdapter` du monde : ce qui a besoin
//! de savoir *où* vivent les données passe par une couture, jamais par un
//! `import`.

use tf_anvil::StateId;

/// Un cuboïde d'un modèle, en **seizièmes de bloc**.
///
/// Minecraft autorise `-16` à `32` — mesuré sur le pack du serveur, les bornes
/// réelles vont de −16 à 29. Cadrer sur `0..16` rognerait le modèle, ce qui se
/// voit tout de suite sur une icône et jamais dans un test qui n'utiliserait
/// que des cubes.
///
/// En **flottants**, et c'est mesuré : sur les 125 382 coordonnées du pack
/// Minefield, **17,2 % ne sont pas entières** — surtout des demis (16 571),
/// mais aussi des dixièmes (`.6`, `.2`, `.4`, `.3`, `.9`) qu'aucune fraction
/// binaire ne représente. Les arrondir au seizième re-quantifierait un
/// cinquième de la géométrie du serveur, et une chaise sortirait de travers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cuboide {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Masque des faces DÉCLARÉES par le modèle, bit par face (voir `Face`).
    /// Un modèle qui ne déclare pas une face ne la dessine pas.
    pub faces: u8,
    /// Masque des faces portant `cullface` : elles disparaissent quand le
    /// voisin dans cette direction est opaque.
    pub cull: u8,
}

impl Cuboide {
    /// Le cube plein, toutes faces déclarées et toutes cullables.
    pub const PLEIN: Cuboide = Cuboide {
        min: [0.0, 0.0, 0.0],
        max: [16.0, 16.0, 16.0],
        faces: 0x3F,
        cull: 0x3F,
    };

    /// Vrai si ce cuboïde remplit la case. C'est le critère de `Cube`, et il
    /// porte sur la GÉOMÉTRIE — « un seul cuboïde » n'est pas le bon critère :
    /// `grass_block` en déclare deux (le cube, puis la couche d'herbe teintée)
    /// et se retrouverait classé « modèle ».
    pub fn remplit(&self) -> bool {
        self.min[0] <= 0.0
            && self.min[1] <= 0.0
            && self.min[2] <= 0.0
            && self.max[0] >= 16.0
            && self.max[1] >= 16.0
            && self.max[2] >= 16.0
    }

    /// Vrai si la face `f` de ce cuboïde est à RAS du bord du bloc.
    ///
    /// Seules celles-là peuvent être masquées par un voisin : une face au
    /// milieu du bloc reste visible quoi qu'il y ait à côté.
    pub fn au_bord(&self, f: Face) -> bool {
        match f {
            Face::MoinsX => self.min[0] <= 0.0,
            Face::PlusX => self.max[0] >= 16.0,
            Face::MoinsY => self.min[1] <= 0.0,
            Face::PlusY => self.max[1] >= 16.0,
            Face::MoinsZ => self.min[2] <= 0.0,
            Face::PlusZ => self.max[2] >= 16.0,
        }
    }
}

/// Les six faces, **dans l'ordre où le mailleur les produit**.
///
/// L'ordre est `axe * 2 + (positif ? 1 : 0)`, donc la face NÉGATIVE d'abord.
/// C'est écrit ici parce que c'est vérifié par un test qui MESURE l'ordre
/// contre le code : dans `we-engine`, une table d'ombrage annonçait
/// « −X +X +Y −Y » pour un mailleur qui produisait « −X +X −Y +Y ». L'ombrage
/// était inversé depuis le début, invisible sur un build gris, et ça n'est
/// sorti qu'en posant des textures dessus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Face {
    MoinsX = 0,
    PlusX = 1,
    MoinsY = 2,
    PlusY = 3,
    MoinsZ = 4,
    PlusZ = 5,
}

pub const FACES: [Face; 6] = [
    Face::MoinsX,
    Face::PlusX,
    Face::MoinsY,
    Face::PlusY,
    Face::MoinsZ,
    Face::PlusZ,
];

impl Face {
    pub const fn indice(self) -> usize {
        self as usize
    }

    pub const fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// L'axe : 0 = X, 1 = Y, 2 = Z.
    pub const fn axe(self) -> usize {
        (self as usize) >> 1
    }

    /// Vrai pour la face du côté POSITIF de l'axe.
    pub const fn positif(self) -> bool {
        (self as usize) & 1 == 1
    }

    /// Le pas vers le voisin que cette face regarde.
    pub const fn pas(self) -> [i32; 3] {
        let d = if self.positif() { 1 } else { -1 };
        match self.axe() {
            0 => [d, 0, 0],
            1 => [0, d, 0],
            _ => [0, 0, d],
        }
    }

    pub const fn opposee(self) -> Face {
        FACES[(self as usize) ^ 1]
    }

    pub const fn depuis(i: usize) -> Face {
        FACES[i % 6]
    }
}

/// Ce que le mailleur demande à son hôte.
///
/// Trois questions, et elles suffisent. Tout ce qui touche aux textures, aux
/// teintes ou aux `.jar` vit ailleurs.
pub trait Formes {
    /// Rien à mailler : ni géométrie, ni masquage.
    fn est_air(&self, id: StateId) -> bool;

    /// Ce bloc BOUCHE sa case : il masque les faces de ses voisins et se fond
    /// dans un quad glouton.
    ///
    /// **Un bloc non-cube ne peut jamais l'être.** Marqué opaque, un escalier
    /// creuserait un trou dans le mur qu'il touche.
    fn opaque(&self, id: StateId) -> bool;

    /// Les cuboïdes du modèle. Vide pour un cube plein — celui-là passe par la
    /// passe gloutonne, qui n'a pas besoin de sa géométrie.
    fn cuboides(&self, id: StateId) -> &[Cuboide];

    /// Ce bloc prend-il la couleur de son BIOME ?
    ///
    /// Faux par défaut, et ce défaut est le bon : la grande majorité des blocs
    /// ne sont pas teintés, et c'est cette réponse qui leur laisse la fusion
    /// gloutonne intacte. Seuls les teintés cassent un quad à une frontière de
    /// biome — herbe, feuilles, eau — et seulement là où la frontière passe.
    fn teinte_biome(&self, id: StateId) -> bool {
        let _ = id;
        false
    }
}

/// Une table plate indexée par `StateId`. Ce que fabriquera `tf-assets`, et ce
/// dont les tests et les benchs se contentent.
#[derive(Debug, Default, Clone)]
pub struct TableFormes {
    air: Vec<bool>,
    opaque: Vec<bool>,
    modeles: Vec<Vec<Cuboide>>,
    teinte: Vec<bool>,
}

impl TableFormes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Déclare un état. Les identifiants arrivent dans l'ordre de l'interner,
    /// donc la table se remplit en poussant.
    pub fn pousser(&mut self, air: bool, opaque: bool, modele: Vec<Cuboide>) -> StateId {
        debug_assert!(
            !(opaque && !modele.is_empty() && !modele.iter().any(Cuboide::remplit)),
            "un bloc opaque doit avoir un cuboïde qui REMPLIT la case, sinon il \
             efface les faces de ses voisins sans rien boucher"
        );
        self.air.push(air);
        self.opaque.push(opaque);
        self.modeles.push(modele);
        self.teinte.push(false);
        (self.air.len() - 1) as StateId
    }

    /// Marque un état comme teinté par son biome.
    ///
    /// Séparé de `pousser` exprès : la teinte se DÉCOUVRE dans le pack (une
    /// face qui porte un `tintindex`), pas au moment où l'on déclare la
    /// forme, et les deux parcours n'ont pas la même source.
    pub fn marquer_teinte(&mut self, id: StateId) {
        if let Some(t) = self.teinte.get_mut(id as usize) {
            *t = true;
        }
    }

    pub fn len(&self) -> usize {
        self.air.len()
    }

    pub fn is_empty(&self) -> bool {
        self.air.is_empty()
    }
}

impl Formes for TableFormes {
    #[inline]
    fn est_air(&self, id: StateId) -> bool {
        self.air.get(id as usize).copied().unwrap_or(true)
    }

    #[inline]
    fn opaque(&self, id: StateId) -> bool {
        // Un identifiant hors table n'est PAS opaque. Le supposer opaque
        // effacerait des faces réelles ; le supposer transparent n'en dessine
        // que trop. Entre deux erreurs, on prend celle qui se voit.
        self.opaque.get(id as usize).copied().unwrap_or(false)
    }

    #[inline]
    fn cuboides(&self, id: StateId) -> &[Cuboide] {
        self.modeles.get(id as usize).map(|v| &v[..]).unwrap_or(&[])
    }

    #[inline]
    fn teinte_biome(&self, id: StateId) -> bool {
        self.teinte.get(id as usize).copied().unwrap_or(false)
    }
}
