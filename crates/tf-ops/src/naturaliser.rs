//! `//naturalize` — refaire une coupe de terrain crédible.
//!
//! Un build creusé à la pioche laisse de la pierre à nu jusqu'en surface. La
//! naturalisation repose la stratigraphie : **la première couche solide d'une
//! colonne devient de l'herbe, les trois suivantes de la terre, le reste de
//! la pierre.**
//!
//! ## Pourquoi ça ne peut pas être une opération de section
//!
//! « La première couche solide » est une propriété de la COLONNE, pas de la
//! section. Un mur de vingt blocs traverse deux sections ; celle du bas ne
//! peut pas savoir si ce qu'elle voit est la surface ou le sous-sol, et le
//! supposer produirait une bande d'herbe au milieu d'une falaise à chaque
//! frontière de section — tous les seize blocs, régulièrement, ce qui est
//! précisément le défaut qu'on voit et qu'on n'explique pas.
//!
//! D'où la portée `Colonne` : `edition.rs` décode le chunk entier et l'opération
//! descend chaque colonne d'un seul mouvement.
//!
//! ## Ce qui compte comme « solide »
//!
//! Le masque le dit, et c'est un paramètre : sur Minefield, ce qui fait
//! surface n'est pas la liste vanilla. Le défaut — tout sauf l'air — est ce
//! que fait WorldEdit, et il a le mérite d'être prévisible.

use tf_anvil::StateId;
use tf_world::coords::{BBox, BlockPos, ChunkPos};

use crate::colonnes::{Colonnes, Portee};
use crate::masque::Masque;
use crate::plan::{Etage, Operation, Rapport};

/// Les couches à poser, de la surface vers le bas.
#[derive(Debug, Clone)]
pub struct Naturaliser {
    /// Ce qui compte comme solide. Tout ce qui ne l'est pas est ignoré — et
    /// n'interrompt PAS la colonne : une grotte ne doit pas faire repousser
    /// de l'herbe sur son plafond.
    pub solide: Masque,
    /// L'état de la première couche solide rencontrée.
    pub surface: StateId,
    /// Celui des `profondeur` couches suivantes.
    pub sous_sol: StateId,
    /// Celui de tout ce qui est encore plus bas.
    pub roche: StateId,
    pub profondeur: u32,
    pub compter: bool,
}

impl Naturaliser {
    /// `air` est passé plutôt que supposé : un `StateId` n'a de sens que
    /// relativement à son interner, et le DÉFAUT en dépend — « solide »
    /// signifie « tout sauf l'air ».
    ///
    /// Le défaut a failli être `Masque::Tout`, ce qui aurait rempli le CIEL de
    /// pierre : la documentation disait déjà « tout sauf l'air », le code
    /// disait autre chose, et aucun test ne les départageait parce qu'ils
    /// comptaient tous des cases sans regarder lesquelles.
    pub fn nouveau(
        surface: StateId,
        sous_sol: StateId,
        roche: StateId,
        air: StateId,
    ) -> Naturaliser {
        Naturaliser {
            solide: Masque::Non(Box::new(Masque::Etat(air))),
            surface,
            sous_sol,
            roche,
            // Un bloc d'herbe et trois de terre : la coupe de Minecraft, et
            // celle que `//naturalize` pose depuis toujours.
            profondeur: 3,
            compter: false,
        }
    }

    pub fn avec_solide(mut self, m: Masque) -> Naturaliser {
        self.solide = m;
        self
    }

    pub fn en_comptant(mut self) -> Naturaliser {
        self.compter = true;
        self
    }

    /// L'état à poser pour la `n`-ième couche solide d'une colonne, comptée
    /// depuis le haut à partir de zéro.
    ///
    /// Pure et publique : c'est la règle, et un test la fige sans avoir à
    /// monter un monde.
    pub fn couche(&self, n: u32) -> StateId {
        if n == 0 {
            self.surface
        } else if n <= self.profondeur {
            self.sous_sol
        } else {
            self.roche
        }
    }
}

impl Operation for Naturaliser {
    fn appliquer(
        &self,
        _s: &mut tf_anvil::Section,
        _sel: &BBox,
        _p: tf_world::coords::SectionPos,
    ) -> Rapport {
        unreachable!("la naturalisation travaille par colonne")
    }

    fn compte(&self) -> bool {
        self.compter
    }

    fn portee(&self) -> Portee {
        Portee::Colonne
    }

    fn appliquer_colonnes(&self, c: &mut Colonnes, sel: &BBox, chunk: ChunkPos) -> Rapport {
        let Some((cy0, cy1)) = c.bornes_y() else {
            return Rapport::RIEN;
        };
        // On ne descend que ce que la sélection couvre ET que le chunk porte.
        let y0 = sel.min.y.max(cy0);
        let y1 = sel.max.y.min(cy1);
        if y0 > y1 {
            return Rapport::RIEN;
        }
        let coin = BlockPos {
            x: chunk.x * 16,
            y: 0,
            z: chunk.z * 16,
        };
        let x0 = sel.min.x.max(coin.x);
        let x1 = sel.max.x.min(coin.x + 15);
        let z0 = sel.min.z.max(coin.z);
        let z1 = sel.max.z.min(coin.z + 15);
        if x0 > x1 || z0 > z1 {
            return Rapport::RIEN;
        }

        let mut ecrits = 0u64;
        let mut bornes: Option<BBox> = None;
        for wz in z0..=z1 {
            for wx in x0..=x1 {
                let (lx, lz) = ((wx - coin.x) as usize, (wz - coin.z) as usize);
                let mut n = 0u32;
                // De haut en bas : c'est le sens qui définit « la surface ».
                for wy in (y0..=y1).rev() {
                    let Some(actuel) = c.get(lx, wy, lz) else {
                        // Pas de section ici : on ne sait pas ce qu'il y a, et
                        // on n'écrit donc rien. Ce n'est pas de l'air.
                        continue;
                    };
                    if !self.solide.accepte(actuel) {
                        // Une grotte ne remet pas le compteur à zéro : sinon
                        // son plafond porterait de l'herbe, à l'envers.
                        continue;
                    }
                    if c.set(lx, wy, lz, self.couche(n)) {
                        ecrits += 1;
                        let p = BlockPos {
                            x: wx,
                            y: wy,
                            z: wz,
                        };
                        match bornes.as_mut() {
                            Some(b) => b.extend(p),
                            None => bornes = Some(BBox::single(p)),
                        }
                    }
                    n += 1;
                }
            }
        }
        Rapport {
            // Une colonne se lit case par case : c'est l'étage bloc, et le
            // déclarer autrement mentirait au rapport qui sert à vérifier.
            etage: if ecrits == 0 {
                Etage::Rien
            } else {
                Etage::Bloc
            },
            blocs: self.compter.then_some(ecrits),
            bornes,
        }
    }
}
