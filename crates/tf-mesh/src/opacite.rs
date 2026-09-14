//! La carte d'opacité du voisinage, calculée UNE fois — et lue par RANGÉES.
//!
//! ## Pourquoi elle existe
//!
//! La passe gloutonne demandait l'opacité d'une case et celle de son voisin,
//! pour chacune des six faces : **49 152 appels virtuels par section**, quel
//! que soit son contenu. Mesuré sur un build, elle prenait 68 % du temps de
//! maillage en ne produisant que 10 % des quads.
//!
//! ## Pourquoi elle est en bits
//!
//! Le défaut de fond était plus profond qu'un coût d'appel : le glouton
//! travaillait en O(volume) là où sa sortie est en O(surface visible). Mesuré :
//! une section **entièrement pleine** — six quads en sortie — coûtait 94 µs,
//! parce que ses 24 576 cases de masque étaient toutes examinées pour découvrir
//! qu'elles sont cachées. Le même défaut que le `warmup(extent)`
//! d'`ExeWorldEdit` : payer l'emprise au lieu du contenu.
//!
//! Une rangée de 18 cases tient dans un `u32`, et « cette case est opaque et
//! son voisin ne l'est pas » devient `rangee & !(rangee << 1)` : **une
//! opération pour seize cases**. Il ne reste ensuite qu'à visiter les bits
//! posés, qui sont exactement les faces à dessiner.

use crate::forme::Formes;
use crate::voisinage::{Voisinage, COTE, COTE_PAD, VOL_PAD};

const MOTS: usize = VOL_PAD.div_ceil(64);
/// Masque des seize cases INTÉRIEURES d'une rangée paddée : bits 1 à 16.
pub const DEDANS: u32 = 0x1_FFFE;

pub struct Opacite {
    bits: [u64; MOTS],
    /// Une rangée de 18 cases par `(y, z)` du voisinage paddé. Redondant avec
    /// `bits`, et c'est voulu : c'est la forme que la passe gloutonne lit, et
    /// la recomposer par rangée coûterait plus que la stocker.
    rangees: [u32; COTE_PAD * COTE_PAD],
    /// Aucune case opaque du tout.
    pub vide: bool,
    /// Toutes les cases INTÉRIEURES sont opaques, et la peau aussi. Rien ne
    /// peut être visible.
    pub bouchee: bool,
}

impl Opacite {
    pub fn relever<F: Formes + ?Sized>(v: &Voisinage, f: &F) -> Self {
        let mut bits = [0u64; MOTS];
        let mut rangees = [0u32; COTE_PAD * COTE_PAD];
        let mut vide = true;
        let mut bouchee = true;
        let ids = v.ids();
        for (r, sortie) in rangees.iter_mut().enumerate() {
            let base = r * COTE_PAD;
            let mut rangee = 0u32;
            for k in 0..COTE_PAD {
                if f.opaque(ids[base + k]) {
                    rangee |= 1 << k;
                    let i = base + k;
                    bits[i >> 6] |= 1u64 << (i & 63);
                }
            }
            *sortie = rangee;
            vide &= rangee == 0;
            bouchee &= rangee == (1 << COTE_PAD) - 1;
        }
        Opacite {
            bits,
            rangees,
            vide,
            bouchee,
        }
    }

    /// Coordonnées locales de `-1` à `16`, comme `Voisinage`.
    #[inline]
    pub fn est(&self, x: i32, y: i32, z: i32) -> bool {
        let i = crate::voisinage::index_pad(x, y, z);
        self.bits[i >> 6] & (1u64 << (i & 63)) != 0
    }

    /// La rangée d'opacité sur X, pour `y` et `z` locaux (`-1` à `16`).
    ///
    /// Le bit `k` vaut la case `x = k - 1`. Les seize cases intérieures sont
    /// donc les bits 1 à 16, d'où `DEDANS`.
    #[inline]
    pub fn rangee(&self, y: i32, z: i32) -> u32 {
        self.rangees[((y + 1) as usize) * COTE_PAD + (z + 1) as usize]
    }

    /// Les faces visibles d'une rangée, pour la direction `positif` sur X.
    ///
    /// Le bit `k` posé veut dire : la case `x = k - 1` est opaque et son voisin
    /// de ce côté ne l'est pas. Une opération pour seize cases, contre seize
    /// paires de lectures.
    #[inline]
    pub fn visibles_x(rangee: u32, positif: bool) -> u32 {
        let voisin = if positif { rangee >> 1 } else { rangee << 1 };
        rangee & !voisin & DEDANS
    }

    /// Les faces visibles entre deux rangées PARALLÈLES — pour Y et Z, où le
    /// voisin est dans une autre rangée.
    #[inline]
    pub fn visibles_entre(rangee: u32, voisine: u32) -> u32 {
        rangee & !voisine & DEDANS
    }

    /// Vrai si la section intérieure ne porte aucune case opaque.
    pub fn interieur_vide(&self) -> bool {
        (0..COTE as i32).all(|y| (0..COTE as i32).all(|z| self.rangee(y, z) & DEDANS == 0))
    }
}
