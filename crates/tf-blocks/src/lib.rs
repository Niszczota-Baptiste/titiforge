//! Les états de blocs, et ce que les transformations leur font.
//!
//! Le cœur du crate est la **dérivation** : les règles de rotation ne sont pas
//! écrites, elles se lisent dans le pack. Voir `regles`.

#![forbid(unsafe_code)]

pub mod geometrie;
pub mod regles;
pub mod transfo;

pub use regles::{Manque, Permutation, ReglesBloc, Table};
pub use transfo::{Transfo, TOUTES};
