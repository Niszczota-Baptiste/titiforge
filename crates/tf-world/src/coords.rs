//! Adressage du monde.
//!
//! C'est la couche où une erreur de signe ne plante pas : elle charge la
//! mauvaise moitié du monde, ou écrit des blocs à côté. `we-engine` a payé ce
//! piège une fois — « le bloc −1 est dans la région −1, pas la région 0 » —
//! et toute conversion passe donc ici, jamais par un `/` écrit sur place.
//!
//! Repère Minecraft : **+X = Est, +Z = Sud, +Y = Haut**.
//!
//! | Unité | Côté | Contient |
//! |---|---|---|
//! | bloc | 1 | — |
//! | section | 16 | 4 096 blocs |
//! | chunk | 16 × H × 16 | une colonne de sections |
//! | région | 32 × 32 chunks | jusqu'à 1 024 chunks |

/// Division PLANCHER, la seule correcte pour des coordonnées signées.
///
/// `-1 / 16` vaut 0 en Rust comme en C : la troncature ramène vers zéro. Or le
/// bloc −1 est dans le chunk −1. Une division naïve charge donc la mauvaise
/// moitié du monde, et ne le signale pas — quinze colonnes sur seize sont
/// fausses, jamais celle qu'on vérifie à la main en premier.
#[inline]
pub const fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// Reste PLANCHER, toujours dans `[0, b)` même pour un `a` négatif.
#[inline]
pub const fn floor_mod(a: i32, b: i32) -> i32 {
    a.rem_euclid(b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Une section 16³, en unités de section. `y` est le `Y` du format Anvil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Une colonne de sections, en unités de chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

/// Un fichier `.mca`, en unités de région.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        BlockPos { x, y, z }
    }

    #[inline]
    pub const fn section(self) -> SectionPos {
        SectionPos {
            x: floor_div(self.x, 16),
            y: floor_div(self.y, 16),
            z: floor_div(self.z, 16),
        }
    }

    #[inline]
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: floor_div(self.x, 16),
            z: floor_div(self.z, 16),
        }
    }

    /// Position DANS sa section, toujours dans `[0, 15]`.
    #[inline]
    pub const fn local(self) -> (usize, usize, usize) {
        (
            floor_mod(self.x, 16) as usize,
            floor_mod(self.y, 16) as usize,
            floor_mod(self.z, 16) as usize,
        )
    }

    /// Index YZX dans sa section — `i = y*256 + z*16 + x`.
    #[inline]
    pub const fn local_index(self) -> usize {
        let (x, y, z) = self.local();
        (y << 8) | (z << 4) | x
    }
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        SectionPos { x, y, z }
    }

    #[inline]
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: self.x,
            z: self.z,
        }
    }

    #[inline]
    pub const fn region(self) -> RegionPos {
        RegionPos {
            x: floor_div(self.x, 32),
            z: floor_div(self.z, 32),
        }
    }

    /// Coin de plus petites coordonnées, en blocs.
    #[inline]
    pub const fn min_block(self) -> BlockPos {
        BlockPos::new(self.x * 16, self.y * 16, self.z * 16)
    }

    /// Coin de plus grandes coordonnées, INCLUS.
    #[inline]
    pub const fn max_block(self) -> BlockPos {
        BlockPos::new(self.x * 16 + 15, self.y * 16 + 15, self.z * 16 + 15)
    }
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        ChunkPos { x, z }
    }

    #[inline]
    pub const fn region(self) -> RegionPos {
        RegionPos {
            x: floor_div(self.x, 32),
            z: floor_div(self.z, 32),
        }
    }

    /// Index du chunk DANS son fichier de région : `localX + localZ * 32`.
    #[inline]
    pub const fn index_in_region(self) -> usize {
        (floor_mod(self.x, 32) + floor_mod(self.z, 32) * 32) as usize
    }

    #[inline]
    pub const fn section(self, y: i32) -> SectionPos {
        SectionPos::new(self.x, y, self.z)
    }
}

impl RegionPos {
    pub const fn new(x: i32, z: i32) -> Self {
        RegionPos { x, z }
    }

    /// Nom de fichier conventionnel.
    pub fn file_name(self) -> String {
        format!("r.{}.{}.mca", self.x, self.z)
    }

    /// Chunk de plus petites coordonnées de cette région.
    #[inline]
    pub const fn min_chunk(self) -> ChunkPos {
        ChunkPos::new(self.x * 32, self.z * 32)
    }
}

// ── boîte englobante ────────────────────────────────────────────────────────

/// Boîte de blocs, bornes **incluses** des deux côtés.
///
/// Inclusives parce que c'est ainsi qu'un utilisateur désigne une sélection :
/// « de −10 à 10 » fait 21 blocs, pas 20. Une convention exclusive d'un côté
/// donne un décalage d'un bloc à chaque conversion, et personne ne le voit
/// avant qu'un mur sorte trop court.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl BBox {
    /// Boîte normalisée : les coins peuvent être donnés dans n'importe quel
    /// ordre, comme deux clics de souris.
    pub fn new(a: BlockPos, b: BlockPos) -> Self {
        BBox {
            min: BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            max: BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
        }
    }

    pub fn single(p: BlockPos) -> Self {
        BBox { min: p, max: p }
    }

    #[inline]
    pub fn contains(&self, p: BlockPos) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    /// Volume en blocs.
    ///
    /// En `u128`, et ce n'est pas de la coquetterie. Le monde jouable tient
    /// en `u64` — 1,38 × 10¹⁸, treize fois sous la borne — mais **rien dans le
    /// type ne borne une boîte au monde jouable** : `BlockPos` porte des
    /// `i32`, et une sélection fabriquée à partir de nombres saisis ou dérivée
    /// d'un fichier abîmé peut couvrir tout le domaine. Là, le volume vaut
    /// 7,9 × 10²⁸, neuf ordres de grandeur au-dessus d'un `u64`. Le
    /// débordement rendrait un volume minuscule, donc une opération annoncée
    /// « à faible coût » qui ne finit jamais.
    pub fn volume(&self) -> u128 {
        let d = |a: i32, b: i32| (b as i64 - a as i64 + 1).max(0) as u128;
        d(self.min.x, self.max.x) * d(self.min.y, self.max.y) * d(self.min.z, self.max.z)
    }

    pub fn size(&self) -> (u32, u32, u32) {
        let d = |a: i32, b: i32| (b as i64 - a as i64 + 1).clamp(0, u32::MAX as i64) as u32;
        (
            d(self.min.x, self.max.x),
            d(self.min.y, self.max.y),
            d(self.min.z, self.max.z),
        )
    }

    pub fn intersects(&self, o: &BBox) -> bool {
        self.min.x <= o.max.x
            && self.max.x >= o.min.x
            && self.min.y <= o.max.y
            && self.max.y >= o.min.y
            && self.min.z <= o.max.z
            && self.max.z >= o.min.z
    }

    pub fn intersection(&self, o: &BBox) -> Option<BBox> {
        if !self.intersects(o) {
            return None;
        }
        Some(BBox {
            min: BlockPos::new(
                self.min.x.max(o.min.x),
                self.min.y.max(o.min.y),
                self.min.z.max(o.min.z),
            ),
            max: BlockPos::new(
                self.max.x.min(o.max.x),
                self.max.y.min(o.max.y),
                self.max.z.min(o.max.z),
            ),
        })
    }

    /// Étend la boîte pour contenir `p`.
    pub fn extend(&mut self, p: BlockPos) {
        self.min = BlockPos::new(
            self.min.x.min(p.x),
            self.min.y.min(p.y),
            self.min.z.min(p.z),
        );
        self.max = BlockPos::new(
            self.max.x.max(p.x),
            self.max.y.max(p.y),
            self.max.z.max(p.z),
        );
    }

    /// Toutes les sections que la boîte touche, même partiellement.
    ///
    /// C'est la conversion qui décide de ce qu'une opération va CHARGER. Trop
    /// étroite, elle lit de l'air là où il y a de la pierre ; trop large, elle
    /// décode des chunks jamais lus — mesuré dans `we-engine` : 47 % du temps
    /// d'une sphère de 62 blocs partait à chauffer un build entier.
    pub fn sections(&self) -> impl Iterator<Item = SectionPos> + '_ {
        let a = self.min.section();
        let b = self.max.section();
        (a.y..=b.y).flat_map(move |y| {
            (a.z..=b.z).flat_map(move |z| (a.x..=b.x).map(move |x| SectionPos::new(x, y, z)))
        })
    }

    pub fn chunks(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        let a = self.min.chunk();
        let b = self.max.chunk();
        (a.z..=b.z).flat_map(move |z| (a.x..=b.x).map(move |x| ChunkPos::new(x, z)))
    }

    pub fn regions(&self) -> impl Iterator<Item = RegionPos> + '_ {
        let (a, b) = self.region_bounds();
        (a.z..=b.z).flat_map(move |z| (a.x..=b.x).map(move |x| RegionPos::new(x, z)))
    }

    /// Les coins de la BOÎTE de régions, sans l'énumérer.
    ///
    /// Une sélection démesurée en couvre des milliards : qui veut savoir
    /// COMBIEN doit pouvoir le demander sans les parcourir, sinon la question
    /// coûte déjà la réponse qu'on voulait éviter.
    pub fn region_bounds(&self) -> (RegionPos, RegionPos) {
        (self.min.chunk().region(), self.max.chunk().region())
    }

    /// Part de `section` réellement couverte, en coordonnées LOCALES incluses.
    /// `None` si la section n'est pas touchée.
    pub fn clip_to_section(&self, s: SectionPos) -> Option<LocalBox> {
        let sb = BBox {
            min: s.min_block(),
            max: s.max_block(),
        };
        let i = self.intersection(&sb)?;
        Some(LocalBox {
            x0: (i.min.x - sb.min.x) as usize,
            y0: (i.min.y - sb.min.y) as usize,
            z0: (i.min.z - sb.min.z) as usize,
            x1: (i.max.x - sb.min.x) as usize,
            y1: (i.max.y - sb.min.y) as usize,
            z1: (i.max.z - sb.min.z) as usize,
        })
    }

    /// Vraie si la boîte contient ENTIÈREMENT la section.
    ///
    /// C'est le test qui envoie une opération à l'étage palette ou section
    /// plutôt qu'à l'étage bloc. Le rendre trop permissif écrirait hors de la
    /// sélection.
    pub fn covers_section(&self, s: SectionPos) -> bool {
        self.contains(s.min_block()) && self.contains(s.max_block())
    }
}

/// Une part de section, en coordonnées locales **incluses** des deux côtés.
///
/// Un vrai type et non un tuple de tuples : les opérations parcourront ça en
/// boucle serrée, et `(( 0,0,0 ), ( 15,15,15 ))` ne dit pas lequel des six
/// nombres est lequel. Un axe interverti au site d'appel produirait une
/// sélection tournée d'un quart de tour — silencieuse sur une boîte cubique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBox {
    pub x0: usize,
    pub y0: usize,
    pub z0: usize,
    pub x1: usize,
    pub y1: usize,
    pub z1: usize,
}

impl LocalBox {
    /// Toute la section.
    pub const PLEINE: LocalBox = LocalBox {
        x0: 0,
        y0: 0,
        z0: 0,
        x1: 15,
        y1: 15,
        z1: 15,
    };

    pub const fn is_full(&self) -> bool {
        self.x0 == 0
            && self.y0 == 0
            && self.z0 == 0
            && self.x1 == 15
            && self.y1 == 15
            && self.z1 == 15
    }

    pub const fn count(&self) -> usize {
        (self.x1 - self.x0 + 1) * (self.y1 - self.y0 + 1) * (self.z1 - self.z0 + 1)
    }

    /// Les index YZX des cases couvertes, dans l'ordre de parcours du format.
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        (self.y0..=self.y1).flat_map(move |y| {
            (self.z0..=self.z1)
                .flat_map(move |z| (self.x0..=self.x1).map(move |x| (y << 8) | (z << 4) | x))
        })
    }
}

// ── hauteur du monde ────────────────────────────────────────────────────────

/// Bornes verticales d'un monde, en blocs.
///
/// Pas une préférence : c'est la hauteur que le jeu accepte. Les rendre
/// réglables laisserait écrire hors du monde et produirait des régions
/// qu'aucun Minecraft ne relit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Height {
    pub min_y: i32,
    pub max_y: i32,
}

impl Height {
    /// 1.18 et après : y ∈ [−64, 319], soit 24 sections.
    pub const MODERNE: Height = Height {
        min_y: -64,
        max_y: 319,
    };
    /// 1.13 – 1.17 : y ∈ [0, 255], soit 16 sections.
    pub const ANCIENNE: Height = Height {
        min_y: 0,
        max_y: 255,
    };

    pub const fn sections(&self) -> i32 {
        (self.max_y - self.min_y + 1) / 16
    }

    pub const fn min_section_y(&self) -> i32 {
        floor_div(self.min_y, 16)
    }

    pub const fn max_section_y(&self) -> i32 {
        floor_div(self.max_y, 16)
    }

    pub const fn contains(&self, y: i32) -> bool {
        y >= self.min_y && y <= self.max_y
    }
}
