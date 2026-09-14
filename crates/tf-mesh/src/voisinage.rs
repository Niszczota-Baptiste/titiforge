//! Ce que le mailleur lit : une section **avec sa peau**.
//!
//! Un mailleur qui ne verrait que ses 16³ cases ne saurait pas si la face de
//! bord doit être dessinée : elle dépend du bloc d'à côté, qui est dans le
//! chunk voisin. D'où une couche de padding d'UNE case sur les six faces.
//!
//! C'est aussi ce qui rend le remaillage local juste : poser un bloc au bord
//! d'un chunk change les faces visibles du chunk d'à côté. Sans la marge, un
//! trait au bord laisse un mur de faces fantômes le long de la frontière, et il
//! faut tout remailler pour le faire disparaître.

use tf_anvil::StateId;

pub const COTE: usize = 16;
pub const PAD: usize = 1;
pub const COTE_PAD: usize = COTE + 2 * PAD;
pub const VOL_PAD: usize = COTE_PAD * COTE_PAD * COTE_PAD;

/// Une section et sa peau, à plat. 18³ × 4 octets = 23 Ko : ça tient au chaud
/// dans le cache, ce qui est tout l'intérêt de la copier plutôt que d'aller
/// chercher chaque voisin dans sa propre section.
#[derive(Clone)]
pub struct Voisinage {
    ids: Box<[StateId; VOL_PAD]>,
}

impl Default for Voisinage {
    fn default() -> Self {
        Voisinage {
            ids: Box::new([0; VOL_PAD]),
        }
    }
}

/// Index dans le tampon paddé, pour des coordonnées locales de `-1` à `16`.
///
/// Ordre YZX, le même que le format : une boucle qui parcourt dans cet ordre
/// avance d'une case en mémoire à chaque pas.
#[inline]
pub const fn index_pad(x: i32, y: i32, z: i32) -> usize {
    let x = (x + PAD as i32) as usize;
    let y = (y + PAD as i32) as usize;
    let z = (z + PAD as i32) as usize;
    y * COTE_PAD * COTE_PAD + z * COTE_PAD + x
}

impl Voisinage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Vrai si la case est DANS la section, peau exclue.
    #[inline]
    pub const fn dedans(x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < COTE as i32 && y < COTE as i32 && z < COTE as i32
    }

    /// Vrai si la case est dans le tampon, peau comprise.
    #[inline]
    pub const fn adressable(x: i32, y: i32, z: i32) -> bool {
        let b = COTE as i32;
        x >= -1 && y >= -1 && z >= -1 && x <= b && y <= b && z <= b
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> StateId {
        debug_assert!(
            Self::adressable(x, y, z),
            "lecture hors du voisinage : ({x}, {y}, {z}). Une lecture qui \
             déborde rendrait le bloc d'une autre rangée — un résultat \
             parfaitement plausible, et faux."
        );
        self.ids[index_pad(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, id: StateId) {
        debug_assert!(Self::adressable(x, y, z), "écriture hors du voisinage");
        self.ids[index_pad(x, y, z)] = id;
    }

    /// Remplit depuis une fonction de position, peau comprise.
    pub fn remplir(&mut self, mut f: impl FnMut(i32, i32, i32) -> StateId) {
        let b = COTE as i32;
        for y in -1..=b {
            for z in -1..=b {
                for x in -1..=b {
                    self.ids[index_pad(x, y, z)] = f(x, y, z);
                }
            }
        }
    }

    pub fn ids(&self) -> &[StateId; VOL_PAD] {
        &self.ids
    }
}
