//! Le maillage d'une section.
//!
//! Deux passes, et il en faut deux :
//!
//! - **gloutonne** (`glouton`) pour les blocs qui bouchent leur case. Elle
//!   fusionne les rectangles de faces identiques : une muraille de 64 × 40
//!   sort en un quad au lieu de 2 560.
//! - **modèles** (`modeles`) pour tout le reste. Sur la cible Minefield, deux
//!   tiers du catalogue et près de la moitié de la géométrie d'un build.
//!
//! Les deux lisent le même `Voisinage` — une section **avec une peau d'une
//! case** — et écrivent dans le même `Maillage`. Aucune ne connaît de fichier,
//! de pack ni de GPU : ce qu'elles savent d'un bloc passe par `Formes`.

#![forbid(unsafe_code)]

pub mod forme;
pub mod glouton;
pub mod maillage;
pub mod modeles;
pub mod voisinage;

pub use forme::{Cuboide, Face, Formes, TableFormes, FACES};
pub use maillage::{Maillage, Quad};
pub use voisinage::{Voisinage, COTE, COTE_PAD, PAD, VOL_PAD};

/// Maille une section : les deux passes, dans le même maillage.
///
/// L'ordre n'est pas indifférent pour la lecture des chiffres — `quads_glouton`
/// et `quads_modele` disent lequel a coûté quoi — mais il l'est pour le
/// résultat : les deux passes traitent des ensembles de blocs DISJOINTS. Un
/// bloc opaque ne passe jamais par les modèles, un bloc-modèle n'entre jamais
/// dans le masque glouton. Sans cette disjonction, tout ce qui est entre les
/// deux serait dessiné deux fois.
pub fn mailler(v: &Voisinage, f: &dyn Formes) -> Maillage {
    let mut out = Maillage::new();
    glouton::mailler(v, f, &mut out);
    modeles::mailler(v, f, &mut out);
    out
}
