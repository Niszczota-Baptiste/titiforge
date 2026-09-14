//! Les opérations d'édition : ce qu'on lit, ce qu'on écrit, et à quel coût.
//!
//! Le crate tient en trois idées :
//!
//!  · un **masque** dit quels états une opération accepte — sur l'état SEUL,
//!    jamais sur la position, et c'est ce qui rend l'étage palette possible ;
//!  · un **motif** dit ce qu'elle pose — une donnée, pas une fermeture, parce
//!    qu'un greffon doit pouvoir en décrire un sans que le cœur perde le droit
//!    de choisir son plan ;
//!  · un **plan** croise les deux et choisit son ÉTAGE : rien, O(1),
//!    O(palette), ou O(blocs). Tout le reste du crate existe pour éviter le
//!    dernier.

#![forbid(unsafe_code)]

pub mod edition;
pub mod hash;
pub mod masque;
pub mod motif;
pub mod plan;

pub use hash::hash3;
pub use masque::Masque;
pub use motif::{Motif, Tirage};
pub use plan::{Etage, Plan, Rapport};
