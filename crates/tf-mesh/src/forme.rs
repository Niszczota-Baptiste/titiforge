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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Les deux fluides du jeu. Deux fluides différents ne se fondent jamais :
/// l'eau montre sa face à la lave comme à l'air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum GenreFluide {
    Eau = 1,
    Lave = 2,
}

/// **Le fluide qu'une case porte** — ce que la passe de fluides lit.
///
/// Orthogonal à tout le reste, et c'est le point : une case d'eau est de
/// l'AIR pour les passes de blocs (ni opaque, ni cuboïde), et un escalier
/// inondé est un bloc-modèle ET une source d'eau. Le jeu fait la même
/// séparation — un état de bloc porte un `FluidState` à part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fluide {
    pub genre: GenreFluide,
    /// La propriété `level` du bloc : 0 pour une source, 1 à 7 pour un
    /// courant qui s'amincit, 8 et au-delà pour une chute. Un bloc inondé
    /// (`waterlogged=true`) porte une source : 0.
    pub niveau: u8,
}

impl Fluide {
    /// Une source — ce que porte tout bloc inondé.
    pub const fn source(genre: GenreFluide) -> Fluide {
        Fluide { genre, niveau: 0 }
    }

    /// La QUANTITÉ, comme le jeu la tire du bloc (`LiquidBlock`) : 8 pour une
    /// source ou une chute, `8 − niveau` pour un courant.
    pub const fn quantite(self) -> u8 {
        if self.niveau == 0 || self.niveau >= 8 {
            8
        } else {
            8 - self.niveau
        }
    }

    /// La hauteur PROPRE, en blocs — `FluidState::getOwnHeight` :
    /// `quantité / 9`. Une source monte donc à 8/9 de sa case, pas à la case
    /// entière : c'est le liseré sous la surface de toute étendue d'eau.
    pub fn hauteur(self) -> f32 {
        self.quantite() as f32 / 9.0
    }

    /// Une chute : `level` de 8 et plus.
    pub const fn tombe(self) -> bool {
        self.niveau >= 8
    }
}

/// Ce que le mailleur demande à son hôte.
///
/// Trois questions, et elles suffisent. Tout ce qui touche aux textures, aux
/// teintes ou aux `.jar` vit ailleurs.
pub trait Formes {
    /// Rien à mailler pour les passes de BLOCS : ni géométrie, ni masquage.
    ///
    /// **Une case d'eau en est**, et c'est voulu : elle ne bouche rien, n'a
    /// pas de cuboïde, et le réticule la traverse comme dans le jeu. Ce
    /// qu'elle dessine, c'est `fluide` qui le dit — et c'est pourquoi une
    /// section ne se saute que si elle n'a NI bloc NI fluide
    /// (`Grille::sans_contenu`).
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

    /// Le FLUIDE que porte cette case : l'eau d'une source, d'un courant ou
    /// d'un bloc inondé, la lave. `None` par défaut — un hôte qui ne connaît
    /// pas les fluides maille comme avant.
    fn fluide(&self, id: StateId) -> Option<Fluide> {
        let _ = id;
        None
    }

    /// Cette case ARRÊTE-t-elle un fluide ? C'est le `Material::isSolid` du
    /// jeu, qui décide de la hauteur d'un coin de surface : une case d'air
    /// au bord d'une étendue d'eau la tire vers le bas, une paroi non.
    ///
    /// **Un pack ne dit pas ce qu'est la matière d'un bloc** — elle vit dans
    /// le code du jeu. La règle par défaut est donc une APPROXIMATION, et
    /// elle se nomme : ce qui bouche sa case, ou porte un cuboïde
    /// d'épaisseur non nulle. Une fleur (deux plans en croix) laisse passer,
    /// un escalier arrête. Une torche ou un tapis, que le jeu laisse passer,
    /// arrêtent ici — un coin de surface un peu plus haut contre eux, rien
    /// d'autre.
    fn solide(&self, id: StateId) -> bool {
        self.opaque(id) || self.cuboides(id).iter().any(Cuboide::epais)
    }
}

impl Cuboide {
    /// Vrai si ce cuboïde a un VOLUME : une épaisseur non nulle sur les
    /// trois axes. Les plans d'une fleur en croix n'en ont pas.
    pub fn epais(&self) -> bool {
        (0..3).all(|k| self.max[k] > self.min[k])
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
    fluides: Vec<Option<Fluide>>,
    /// `Formes::solide`, calculé une fois par état : la passe de fluides le
    /// demande pour chaque voisin d'un coin de surface.
    solides: Vec<bool>,
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
        self.solides
            .push(opaque || modele.iter().any(Cuboide::epais));
        self.modeles.push(modele);
        self.teinte.push(false);
        self.fluides.push(None);
        (self.air.len() - 1) as StateId
    }

    /// Déclare le fluide d'un état — l'eau d'une source, d'un courant, d'un
    /// bloc inondé, ou la lave.
    ///
    /// Séparé de `pousser` pour la même raison que la teinte : le fluide se
    /// lit dans l'ÉTAT (`level`, `waterlogged`), pas dans la forme, et un
    /// escalier inondé garde ses cuboïdes.
    pub fn marquer_fluide(&mut self, id: StateId, f: Fluide) {
        if let Some(c) = self.fluides.get_mut(id as usize) {
            *c = Some(f);
        }
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

    #[inline]
    fn fluide(&self, id: StateId) -> Option<Fluide> {
        self.fluides.get(id as usize).copied().flatten()
    }

    #[inline]
    fn solide(&self, id: StateId) -> bool {
        // Hors table : ni opaque ni modèle, donc de l'air — qui laisse passer.
        self.solides.get(id as usize).copied().unwrap_or(false)
    }
}
