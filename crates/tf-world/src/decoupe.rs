//! **Le découpage du monde, vu et suivi.**
//!
//! Un monde Minecraft n'est pas une étendue lisse : il est découpé en chunks
//! de 16 × 16, eux-mêmes rangés par 32 × 32 dans des fichiers `r.X.Z.mca`.
//! Ce découpage n'est pas un détail d'implémentation qu'on cacherait à
//! l'utilisateur — c'est l'unité dans laquelle il travaille :
//!
//! - un build qui déborde d'un `.mca` s'échange en DEUX fichiers ;
//! - une sélection alignée sur les chunks se traite à l'étage palette, une
//!   sélection décalée d'un bloc tombe à l'étage bloc — mesuré, **× 21** ;
//! - Minecraft lui-même montre les bords de chunk (F3+G), parce que tout
//!   constructeur en a besoin pour aligner une ferme ou un mur.
//!
//! Ce que Minecraft NE montre pas, c'est le découpage en `.mca`. Or c'est
//! celui qui décide ce qu'on exporte, ce qu'on envoie à quelqu'un, et ce
//! qu'une opération va réécrire. D'où les deux niveaux ici, du même code.
//!
//! ## Deux sens du mot « découper »
//!
//! **Voir** le découpage — `cellules_autour` rend les cellules visibles avec
//! leur emprise et le `.mca` dont elles relèvent. **S'y aligner** —
//! `BBox::aligner` étend une sélection aux cellules entières, ce qu'un
//! `//chunk` fait dans WorldEdit.

use crate::coords::{floor_div, BBox, BlockPos, ChunkPos, RegionPos};

/// L'unité de découpage regardée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Niveau {
    /// 16 × 16 blocs — ce que Minecraft montre avec F3+G.
    Chunk,
    /// 512 × 512 blocs, un fichier `r.X.Z.mca`.
    Region,
}

impl Niveau {
    /// Le côté d'une cellule, en blocs.
    pub const fn cote(self) -> i32 {
        match self {
            Niveau::Chunk => 16,
            Niveau::Region => 512,
        }
    }

    /// La cellule qui contient cette abscisse.
    ///
    /// **Division PLANCHER.** Le bloc −1 est dans le chunk −1, pas le chunk 0 :
    /// une division qui tronque vers zéro fait sauter la grille d'une cellule
    /// au passage de l'origine, et seulement du côté négatif. Un défaut qui ne
    /// se voit que dans un quart du monde est un défaut qu'on met longtemps à
    /// reproduire.
    #[inline]
    pub const fn cellule_axe(self, bloc: i32) -> i32 {
        floor_div(bloc, self.cote())
    }

    /// La cellule qui contient ce bloc, en (x, z).
    #[inline]
    pub const fn cellule_de(self, p: BlockPos) -> (i32, i32) {
        (self.cellule_axe(p.x), self.cellule_axe(p.z))
    }
}

/// Une cellule du découpage, avec de quoi la montrer ET la nommer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cellule {
    pub niveau: Niveau,
    /// Coordonnées de la cellule — de chunk, ou de région.
    pub x: i32,
    pub z: i32,
    /// Son emprise MONDE, bornée en hauteur par ce qu'on a demandé.
    pub boite: BBox,
    /// Le fichier dont elle relève.
    ///
    /// **C'est la réponse à « montrer visuellement les différents `.mca` ».**
    /// Une cellule de chunk la porte aussi : c'est ce qui permet de teinter
    /// les chunks par leur région, donc de VOIR la frontière de fichier sans
    /// avoir à dessiner un second quadrillage par-dessus le premier.
    pub region: RegionPos,
}

impl Cellule {
    /// Le nom du fichier : `r.X.Z.mca`.
    pub fn fichier(&self) -> String {
        self.region.file_name()
    }

    /// La parité de la cellule, pour un damier.
    ///
    /// Deux cellules voisines ne partagent jamais leur parité : c'est le
    /// minimum qu'il faut pour qu'un œil SÉPARE deux `.mca` adjacents. Un
    /// quadrillage d'une seule couleur montre où sont les bords, pas à quel
    /// fichier appartient ce qu'il y a entre eux.
    ///
    /// Sur `x + z` en euclidien, jamais `%` : `(-1) % 2` vaut `-1` en Rust, ce
    /// qui donnerait trois parités au lieu de deux et un damier qui se casse à
    /// l'origine.
    pub const fn parite(&self) -> u8 {
        (self.x.rem_euclid(2) ^ self.z.rem_euclid(2)) as u8
    }
}

/// Le rayon maximal accepté, en CELLULES.
///
/// 64 fait 129 × 129 = 16 641 cellules. Au-delà, on ne montre plus un
/// découpage : on dessine un quadrillage illisible et on paie une géométrie
/// que personne ne regarde. Ce n'est pas un plafond de mémoire — c'est le
/// point où la fonctionnalité cesse d'en être une.
pub const RAYON_MAX: u32 = 64;

/// Les cellules autour d'un point, à ce niveau.
///
/// Le rayon est en CELLULES et pas en blocs, parce que c'est ainsi qu'on
/// demande la chose : « mon chunk et ses voisins » est un rayon de 1, quel que
/// soit le niveau. Le compte est donc borné par construction — `(2r + 1)²` —
/// et c'est ce qui dispense d'un plafond de mémoire : il n'y a pas d'entrée
/// qui puisse faire exploser la sortie.
///
/// `y` borne la hauteur des boîtes rendues. **Elle n'est PAS alignée** : un
/// chunk fait 16 × 16 en horizontal et toute la hauteur du monde en vertical.
/// Aligner y sur 16 alignerait sur les SECTIONS, qui sont un autre découpage —
/// et un utilisateur qui demande « mon chunk » ne demande pas la tranche de
/// seize blocs où il se trouve.
pub fn cellules_autour(
    centre: BlockPos,
    rayon: u32,
    niveau: Niveau,
    y: (i32, i32),
) -> Vec<Cellule> {
    let rayon = rayon.min(RAYON_MAX) as i32;
    let (cx, cz) = niveau.cellule_de(centre);
    let cote = niveau.cote();
    let (y0, y1) = (y.0.min(y.1), y.0.max(y.1));
    let mut out = Vec::with_capacity(((2 * rayon + 1) * (2 * rayon + 1)) as usize);
    // Ordre fixe — z puis x, comme tout le dépôt. Une liste dont l'ordre
    // dépendrait d'une table de hachage ferait deux images différentes de la
    // même scène, et un test impossible à écrire.
    for dz in -rayon..=rayon {
        for dx in -rayon..=rayon {
            // En `i64` : `cx` peut valoir `i32::MIN / 16` et le rayon s'y
            // ajoute. Un débordement rendrait une cellule à l'autre bout du
            // monde, ce qu'aucune image ne montrerait comme une erreur.
            let x = (cx as i64 + dx as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let z = (cz as i64 + dz as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let bx = (x as i64 * cote as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            let bz = (z as i64 * cote as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            out.push(Cellule {
                niveau,
                x,
                z,
                boite: BBox::new(
                    BlockPos::new(bx, y0, bz),
                    BlockPos::new(bx.saturating_add(cote - 1), y1, bz.saturating_add(cote - 1)),
                ),
                region: match niveau {
                    Niveau::Chunk => ChunkPos::new(x, z).region(),
                    Niveau::Region => RegionPos::new(x, z),
                },
            });
        }
    }
    out
}

impl BBox {
    /// Étend la boîte aux cellules ENTIÈRES de ce niveau — le `//chunk` de
    /// WorldEdit.
    ///
    /// **Elle étend, elle ne rétrécit jamais.** Rétrécir ferait perdre en
    /// silence des blocs que l'utilisateur avait sélectionnés, et il ne le
    /// verrait qu'après l'opération. Entre deux erreurs on prend celle qui se
    /// voit : une sélection trop large se montre avant qu'on applique quoi que
    /// ce soit.
    ///
    /// **La hauteur n'est pas touchée.** Un chunk n'est découpé qu'en
    /// horizontal ; aligner y sur 16 alignerait sur les sections, qui sont un
    /// autre découpage et ne sont pas ce qu'on demande.
    ///
    /// L'intérêt n'est pas cosmétique : une sélection alignée sur les chunks
    /// couvre des sections ENTIÈRES, donc passe par l'étage palette. Décalée
    /// d'un seul bloc, elle fait tomber 2 892 sections sur 24 576 à l'étage
    /// bloc — mesuré, 1,35 ms deviennent 28,9.
    pub fn aligner(&self, niveau: Niveau) -> BBox {
        let cote = niveau.cote() as i64;
        let bas = |b: i32| (niveau.cellule_axe(b) as i64 * cote).max(i32::MIN as i64) as i32;
        let haut =
            |b: i32| ((niveau.cellule_axe(b) as i64 + 1) * cote - 1).min(i32::MAX as i64) as i32;
        BBox {
            min: BlockPos::new(bas(self.min.x), self.min.y, bas(self.min.z)),
            max: BlockPos::new(haut(self.max.x), self.max.y, haut(self.max.z)),
        }
    }

    /// Étend la HAUTEUR aux sections entières — les tranches de 16 blocs.
    ///
    /// **`aligner` ne suffit pas à gagner l'étage palette, et c'est ce que je
    /// viens de découvrir en le mesurant.** Une sélection alignée sur les
    /// chunks en x et z, mais qui va de y = −40 à −20, ne couvre AUCUNE
    /// section entière : les sections vont de −48 à −33 et de −32 à −17. Douze
    /// sections passaient encore par l'étage bloc, et le message de l'outil
    /// annonçait pourtant « alignée ».
    ///
    /// Les deux sont donc nécessaires, et ce sont deux gestes distincts :
    /// `//chunk` ne touche pas à la hauteur parce que c'est la convention de
    /// WorldEdit, et parce qu'étendre verticalement sans le dire ferait
    /// remplir de la pierre du sol au ciel.
    pub fn aligner_sections(&self) -> BBox {
        let bas = floor_div(self.min.y, 16) as i64 * 16;
        let haut = (floor_div(self.max.y, 16) as i64 + 1) * 16 - 1;
        BBox {
            min: BlockPos::new(self.min.x, bas.max(i32::MIN as i64) as i32, self.min.z),
            max: BlockPos::new(self.max.x, haut.min(i32::MAX as i64) as i32, self.max.z),
        }
    }

    /// Combien de sections la boîte couvre ENTIÈREMENT, sur combien elle en
    /// touche.
    ///
    /// **C'est la seule mesure qui dise ce que l'opération va coûter**, et
    /// elle se compte plutôt qu'elle ne se déduit : « alignée sur les chunks »
    /// est vrai et ne prouve rien tant que la hauteur ne tombe pas sur des
    /// tranches de seize.
    pub fn sections_entieres(&self) -> (usize, usize) {
        let mut entieres = 0;
        let mut total = 0;
        for s in self.sections() {
            total += 1;
            if self.covers_section(s) {
                entieres += 1;
            }
        }
        (entieres, total)
    }

    /// Vrai si la boîte est DÉJÀ alignée sur ce niveau.
    ///
    /// Sert à le DIRE : une interface qui propose « aligner » sur une
    /// sélection déjà alignée propose un non-geste, et l'utilisateur ne sait
    /// pas si le bouton a marché.
    pub fn est_alignee(&self, niveau: Niveau) -> bool {
        *self == self.aligner(niveau)
    }
}
