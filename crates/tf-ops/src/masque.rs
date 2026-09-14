//! Ce qu'une opération LIT pour décider : un masque.
//!
//! ## La contrainte qui fait tout marcher
//!
//! **Un masque est un prédicat sur l'ÉTAT seul, jamais sur la position.** Ça
//! peut sembler une limite ; c'est la condition de l'étage palette. Un masque
//! qui regarderait la position devrait être évalué 4 096 fois par section ; un
//! masque qui ne regarde que l'état s'évalue une fois par ENTRÉE de palette —
//! dix fois au lieu de quatre mille, et le résultat vaut pour tous les blocs
//! qui la référencent.
//!
//! Ce qui dépend de la position n'est pas un masque, c'est une SÉLECTION, et ça
//! se décrit autrement (une `BBox`, une forme, un prédicat de colonne). Les
//! mélanger ferait retomber toute opération à l'étage bloc — × 95 perdus,
//! mesuré dans le prototype.
//!
//! ## Et donc la table
//!
//! Même à l'étage bloc, on n'évalue jamais le masque par bloc : on le
//! précalcule en une table indexée par la palette. C'est le piège de `count_of`
//! sous une autre forme — une recherche linéaire par bloc est une boucle
//! quadratique, mesurée à 340 µs pour une seule section à grosse palette.

use tf_anvil::StateId;

/// Quels états une opération accepte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Masque {
    /// Tout, y compris l'air.
    Tout,
    /// Exactement cet état.
    Etat(StateId),
    /// L'un de ceux-là. Gardé TRIÉ pour une recherche binaire.
    Parmi(Vec<StateId>),
    Non(Box<Masque>),
    Et(Vec<Masque>),
    Ou(Vec<Masque>),
}

impl Masque {
    /// Construit un `Parmi` trié et dédoublonné.
    pub fn parmi(mut ids: Vec<StateId>) -> Masque {
        ids.sort_unstable();
        ids.dedup();
        match ids.len() {
            0 => Masque::Ou(Vec::new()), // n'accepte rien
            1 => Masque::Etat(ids[0]),
            _ => Masque::Parmi(ids),
        }
    }

    /// Ce masque accepte-t-il cet état ?
    ///
    /// À n'appeler que pour construire une table : jamais dans une boucle de
    /// blocs.
    pub fn accepte(&self, id: StateId) -> bool {
        match self {
            Masque::Tout => true,
            Masque::Etat(e) => *e == id,
            Masque::Parmi(v) => v.binary_search(&id).is_ok(),
            Masque::Non(m) => !m.accepte(id),
            Masque::Et(v) => v.iter().all(|m| m.accepte(id)),
            Masque::Ou(v) => v.iter().any(|m| m.accepte(id)),
        }
    }

    /// La table du masque pour UNE palette : un booléen par entrée.
    ///
    /// O(palette), calculée une fois, consultée par bloc en O(1). C'est la
    /// seule forme sous laquelle un masque entre dans une boucle chaude.
    pub fn table(&self, palette: &[StateId]) -> Vec<bool> {
        palette.iter().map(|&id| self.accepte(id)).collect()
    }

    /// Ce masque accepte-t-il TOUTE la palette ?
    ///
    /// C'est ce qui fait passer une section à l'étage O(1) même quand le masque
    /// n'est pas `Tout` : `//replace pierre,terre roche` sur une section qui ne
    /// contient que de la pierre et de la terre donne un résultat UNIFORME, et
    /// il n'y a aucune raison de le payer plus cher qu'un `//set`.
    pub fn accepte_toute(&self, palette: &[StateId]) -> bool {
        !palette.is_empty() && palette.iter().all(|&id| self.accepte(id))
    }

    /// Ce masque n'accepte-t-il AUCUNE entrée de la palette ?
    ///
    /// La section est alors à sauter entièrement — et c'est le cas le plus
    /// fréquent sur un vrai monde : un `//replace` visant un bloc rare ne
    /// concerne qu'une section sur mille.
    pub fn n_accepte_rien(&self, palette: &[StateId]) -> bool {
        palette.iter().all(|&id| !self.accepte(id))
    }
}
