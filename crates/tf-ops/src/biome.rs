//! `//setbiome` — poser un biome sur une sélection.
//!
//! Un biome n'est pas un bloc. Il vit dans une SECONDE palette de la section,
//! sur une grille de **4 × 4 × 4 cellules** — une cellule pour 4 × 4 × 4
//! blocs. Une sélection qui ne tombe pas sur un multiple de quatre ne peut
//! donc pas être respectée au bloc près, et c'est une propriété du FORMAT,
//! pas un défaut d'implémentation.
//!
//! On a le choix entre deux mensonges et une vérité :
//!
//! - n'écrire que les cellules ENTIÈREMENT dans la sélection : le bord ne
//!   change pas, et l'utilisateur voit son biome s'arrêter avant sa
//!   sélection ;
//! - écrire toute cellule que la sélection TOUCHE : le biome déborde jusqu'à
//!   trois blocs — ce que fait WorldEdit, et ce qu'on attend d'un `//setbiome`
//!   sur une zone dessinée à la main ;
//! - prétendre au bloc près, ce qui est impossible.
//!
//! On prend la deuxième, et le rapport DIT combien de cellules ont été
//! écrites pour que le débordement ne soit pas une surprise.

use tf_anvil::{Biomes, StateId, VOL_BIOME};
use tf_world::coords::{BBox, SectionPos};

use crate::plan::{Operation, Rapport};

/// Pose un biome sur toutes les cellules que la sélection touche.
#[derive(Debug, Clone)]
pub struct PoserBiome {
    pub biome: StateId,
    pub compter: bool,
}

impl PoserBiome {
    pub fn nouveau(biome: StateId) -> PoserBiome {
        PoserBiome {
            biome,
            compter: false,
        }
    }

    pub fn en_comptant(mut self) -> PoserBiome {
        self.compter = true;
        self
    }
}

impl Operation for PoserBiome {
    /// Les blocs ne bougent pas. Rendre `Rien` n'est pas un aveu : c'est
    /// exact, et c'est ce qui évite de réécrire les sections pour rien.
    fn appliquer(&self, _s: &mut tf_anvil::Section, _sel: &BBox, _p: SectionPos) -> Rapport {
        Rapport::RIEN
    }

    fn compte(&self) -> bool {
        self.compter
    }

    fn touche_biomes(&self) -> bool {
        true
    }

    fn appliquer_biomes(&self, b: &mut Biomes, sel: &BBox, pos: SectionPos) -> bool {
        let coin = pos.min_block();
        // La section est-elle ENTIÈREMENT dans la sélection ? Alors un seul
        // biome suffit, et la palette tombe à une entrée — le chemin O(1),
        // celui qui fait disparaître le tableau d'indices.
        if sel.covers_section(pos) {
            if b.palette.len() == 1 && b.palette[0] == self.biome {
                return false;
            }
            b.set_uniforme(self.biome);
            return true;
        }

        let mut idx = b.unpack().to_vec();
        let mut change = false;
        let mut rang: Option<u16> = b
            .palette
            .iter()
            .position(|&p| p == self.biome)
            .map(|k| k as u16);
        for cy in 0..4usize {
            for cz in 0..4usize {
                for cx in 0..4usize {
                    // La cellule couvre quatre blocs par axe : on la vise si
                    // la sélection en touche UN seul.
                    let bx = coin.x + (cx * 4) as i32;
                    let by = coin.y + (cy * 4) as i32;
                    let bz = coin.z + (cz * 4) as i32;
                    if bx + 3 < sel.min.x
                        || bx > sel.max.x
                        || by + 3 < sel.min.y
                        || by > sel.max.y
                        || bz + 3 < sel.min.z
                        || bz > sel.max.z
                    {
                        continue;
                    }
                    let i = Biomes::index(cx, cy, cz);
                    // **L'invariant n° 4 vaut aussi pour cette palette-là.**
                    // Comparer l'ÉTAT, pas l'indice : une palette qui porte
                    // deux fois le même biome ferait réécrire des cellules
                    // qui n'ont pas à l'être.
                    if b.palette.get(idx[i] as usize) == Some(&self.biome) {
                        continue;
                    }
                    let k = match rang {
                        Some(k) => k,
                        None => {
                            b.palette.push(self.biome);
                            let k = (b.palette.len() - 1) as u16;
                            rang = Some(k);
                            k
                        }
                    };
                    idx[i] = k;
                    change = true;
                }
            }
        }
        if change {
            debug_assert_eq!(idx.len(), VOL_BIOME);
            b.repack(&idx);
        }
        change
    }
}
