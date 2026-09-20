//! Les biomes d'une section — 64 cellules de 4 × 4 × 4 blocs.
//!
//! Depuis 1.18, un biome n'est plus une propriété du chunk mais de la
//! SECTION, et il se range exactement comme les blocs : une palette, des
//! indices packés. Trois choses le distinguent, et les trois se paient cher
//! si on les copie du code des blocs :
//!
//! 1. **La palette est une liste de CHAÎNES**, pas de compounds. Un biome n'a
//!    pas d'état — `minecraft:plains`, et rien d'autre. Lui appliquer le
//!    lecteur d'entrées de bloc chercherait un champ `Name` qui n'existe pas.
//! 2. **Il n'y a que 64 cellules**, une pour 4 × 4 × 4 blocs. L'ordre est le
//!    même, YZX, mais sur quatre : `i = y×16 + z×4 + x`.
//! 3. **Le plancher de 4 bits n'existe pas.** Une palette de deux biomes se
//!    lit sur UN bit. Reprendre `bits_for` des blocs produirait un tableau
//!    quatre fois trop long — valide pour personne, et le chunk ne se
//!    charge plus.
//!
//! Comme pour les blocs, une section à biome unique n'a **pas de `data`**, et
//! en laisser un de la mauvaise longueur casse le chargement.

use crate::state::{Interner, StateId};

/// Cellules de biome dans une section : 4 × 4 × 4.
pub const VOL_BIOME: usize = 64;

/// Bits par indice de biome. **Pas de plancher à 4** : c'est la différence
/// avec les blocs, et elle change la longueur du tableau écrit.
#[inline]
pub fn bits_biome(len: usize) -> u8 {
    let n = len.max(2);
    let mut b = 0u32;
    while (1usize << b) < n {
        b += 1;
    }
    // Plafond à 6 : 64 cellules, donc au plus 64 biomes distincts.
    (b as u8).clamp(1, 6)
}

/// Les biomes d'une section, décodés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Biomes {
    pub palette: Vec<StateId>,
    pub bits: u8,
    pub data: Box<[u64]>,
}

impl Biomes {
    pub fn uniforme(id: StateId) -> Biomes {
        Biomes {
            palette: vec![id],
            bits: bits_biome(1),
            data: Box::new([]),
        }
    }

    pub fn est_uniforme(&self) -> bool {
        self.palette.len() <= 1 || self.data.is_empty()
    }

    /// L'index d'une cellule, en YZX sur quatre.
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        debug_assert!(x < 4 && y < 4 && z < 4);
        ((y << 2) | z) << 2 | x
    }

    pub fn unpack(&self) -> Box<[u16]> {
        let mut out = vec![0u16; VOL_BIOME].into_boxed_slice();
        if !self.est_uniforme() {
            crate::format::unpack_into(
                &self.data,
                VOL_BIOME,
                self.bits as usize,
                // Les biomes n'existent que depuis 1.18, donc bien après le
                // changement de packing : un indice ne chevauche jamais deux
                // longs. Le déduire de la longueur comme pour les blocs
                // n'aurait rien à départager.
                crate::format::Packing::NoStraddle,
                &mut out,
            );
        }
        out
    }

    /// La cellule à ces coordonnées de CELLULE (0..4 sur chaque axe).
    pub fn get(&self, x: usize, y: usize, z: usize) -> Option<StateId> {
        if x >= 4 || y >= 4 || z >= 4 {
            return None;
        }
        if self.est_uniforme() {
            return self.palette.first().copied();
        }
        let idx = self.unpack();
        self.palette
            .get(idx[Biomes::index(x, y, z)] as usize)
            .copied()
    }

    /// La cellule qui porte ce bloc, en coordonnées LOCALES à la section.
    #[inline]
    pub fn cellule_de_bloc(lx: usize, ly: usize, lz: usize) -> (usize, usize, usize) {
        (lx >> 2, ly >> 2, lz >> 2)
    }

    pub fn set_uniforme(&mut self, id: StateId) {
        self.palette = vec![id];
        self.bits = bits_biome(1);
        self.data = Box::new([]);
    }

    /// Repacke depuis 64 indices.
    pub fn repack(&mut self, idx: &[u16]) {
        assert_eq!(idx.len(), VOL_BIOME);
        if self.palette.len() <= 1 {
            self.bits = bits_biome(1);
            self.data = Box::new([]);
            return;
        }
        self.bits = bits_biome(self.palette.len());
        self.data =
            crate::format::pack(idx, self.bits as usize, crate::format::Packing::NoStraddle)
                .into_boxed_slice();
    }

    /// Les noms de la palette, pour l'écriture.
    pub fn noms<'a>(&self, interner: &'a Interner) -> Option<Vec<&'a str>> {
        self.palette
            .iter()
            .map(|&id| interner.resolve(id))
            .collect()
    }
}
