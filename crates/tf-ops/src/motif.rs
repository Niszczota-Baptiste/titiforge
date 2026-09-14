//! Ce qu'une opération ÉCRIT : un motif.
//!
//! Comme le masque, un motif est une DONNÉE, pas une fermeture. C'est la
//! couture du greffon : une API qui laisserait décrire un motif par du code
//! appelé par bloc ferait retomber toute opération à l'étage bloc, et le cœur
//! ne pourrait plus rien décider. En données, le cœur LIT le motif et choisit
//! son plan.
//!
//! La ligne de partage est nette : un motif qui rend le même état pour tout le
//! monde (`Bloc`) se pose sur la palette ; un motif qui dépend de la POSITION
//! (`Melange`) doit voir chaque case.

use tf_anvil::StateId;

use crate::hash::hash3;

/// Ce qu'une opération pose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motif {
    /// Toujours le même état.
    Bloc(StateId),
    /// Un tirage pondéré, haché sur la position.
    ///
    /// Les poids sont des entiers : `[(3, pierre), (1, terre)]` donne trois
    /// quarts de pierre. Des flottants rendraient le tirage dépendant de
    /// l'arrondi, donc de la plateforme, donc irrejouable.
    Melange(Vec<(u32, StateId)>),
    /// Laisse en place ce qui est là. Utile comme branche d'un motif composé,
    /// et comme non-opération explicite.
    Garder,
}

impl Motif {
    /// Un mélange, poids nuls retirés.
    ///
    /// Un poids nul n'est pas une erreur — c'est une ligne que l'utilisateur a
    /// mise à zéro dans son formulaire — mais le garder fausserait le total et
    /// donc toutes les proportions.
    pub fn melange(entrees: Vec<(u32, StateId)>) -> Motif {
        let v: Vec<(u32, StateId)> = entrees.into_iter().filter(|(p, _)| *p > 0).collect();
        match v.len() {
            0 => Motif::Garder,
            1 => Motif::Bloc(v[0].1),
            _ => Motif::Melange(v),
        }
    }

    /// L'état que ce motif pose partout, s'il en pose un seul.
    ///
    /// `None` veut dire « ça dépend de la position » — et c'est exactement la
    /// question que le plan pose pour choisir son étage.
    pub fn uniforme(&self) -> Option<StateId> {
        match self {
            Motif::Bloc(id) => Some(*id),
            _ => None,
        }
    }

    /// Ce motif écrit-il quoi que ce soit ?
    pub fn est_muet(&self) -> bool {
        matches!(self, Motif::Garder)
    }

    /// Ce que ce motif pose en (x, y, z), sachant ce qui s'y trouve.
    ///
    /// **Hors boucle chaude** : elle recompile le tirage à chaque appel. C'est
    /// le chemin lisible, celui des tests et du chemin de référence ; la boucle
    /// de blocs, elle, compile une fois et indexe. Une seule règle malgré tout
    /// — deux implémentations d'une même règle finissent par diverger, et ce
    /// dépôt en a déjà payé trois.
    pub fn choisir(&self, x: i32, y: i32, z: i32, seed: u64, actuel: StateId) -> StateId {
        match self.tirage().indice(x, y, z, seed) {
            None => actuel,
            Some(i) => self.etats()[i],
        }
    }

    /// Le tirage COMPILÉ de ce motif.
    ///
    /// À construire une fois, hors de la boucle. La première version calculait
    /// la somme des poids et prenait un modulo **par bloc** : mesuré sur une
    /// région pleine, 18,6 ns par bloc et 1,88 s pour l'opération. Une division
    /// 64 bits ne se pipeline pas, et une somme de trois entiers refaite cent
    /// millions de fois reste cent millions de sommes.
    pub fn tirage(&self) -> Tirage {
        match self {
            Motif::Bloc(_) => Tirage::UNIQUE,
            Motif::Garder => Tirage::MUET,
            Motif::Melange(v) => {
                let mut seuils = Vec::with_capacity(v.len());
                let mut cumul = 0u64;
                for (p, _) in v {
                    cumul += *p as u64;
                    seuils.push(cumul);
                }
                Tirage {
                    seuils,
                    total: cumul,
                }
            }
        }
    }

    /// Tous les états que ce motif peut poser.    /// Tous les états que ce motif peut poser.
    ///
    /// Sert à préparer la palette AVANT d'écrire : ajouter une entrée au milieu
    /// d'une boucle de blocs ferait grandir `bits` en plein parcours.
    pub fn etats(&self) -> Vec<StateId> {
        match self {
            Motif::Bloc(id) => vec![*id],
            Motif::Garder => Vec::new(),
            Motif::Melange(v) => v.iter().map(|(_, id)| *id).collect(),
        }
    }
}

/// Un tirage pondéré, compilé : des seuils cumulés et leur total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tirage {
    /// Les poids CUMULÉS. Le dernier vaut `total`.
    seuils: Vec<u64>,
    total: u64,
}

impl Tirage {
    /// Le motif ne pose rien : l'état reste.
    const MUET: Tirage = Tirage {
        seuils: Vec::new(),
        total: 0,
    };
    /// Le motif pose toujours son unique état.
    const UNIQUE: Tirage = Tirage {
        seuils: Vec::new(),
        total: 1,
    };

    /// Lequel des états du motif tombe en (x, y, z) — par son indice.
    ///
    /// `None` veut dire « ne touche pas à cette case ».
    ///
    /// **Sans division.** `(h × total) >> 64` rend un entier uniforme dans
    /// `[0, total)` pour le prix d'une multiplication large — la méthode de
    /// Lemire. Un `%` coûtait plus cher que le hachage lui-même.
    ///
    /// Le biais de cette méthode vaut `total / 2^64` : pour un mélange de
    /// quelques blocs, il est de l'ordre de 10⁻¹⁸. On ne le mesurera jamais ;
    /// la division, si.
    ///
    /// **Cette formule est FIGÉE.** Un build fait avec une graine doit se
    /// rejouer à l'identique ; changer la façon de tirer changerait tous les
    /// mondes déjà construits, sans que rien ne le signale.
    #[inline]
    pub fn indice(&self, x: i32, y: i32, z: i32, seed: u64) -> Option<usize> {
        match self.total {
            0 => None,
            1 if self.seuils.is_empty() => Some(0),
            total => {
                let h = hash3(x, y, z, seed);
                let tirage = ((h as u128 * total as u128) >> 64) as u64;
                // Les mélanges ont deux à dix entrées : une recherche linéaire
                // y bat une binaire, qui paierait ses branchements.
                Some(
                    self.seuils
                        .iter()
                        .position(|&s| tirage < s)
                        .unwrap_or(self.seuils.len() - 1),
                )
            }
        }
    }
}
