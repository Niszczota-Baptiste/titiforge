//! Lire un pack de ressources — **sans jamais rien redistribuer**.
//!
//! Embarquer les assets de Mojang dans un installeur exposerait celui qui le
//! diffuse : l'EULA l'interdit. On lit l'installation de l'utilisateur, comme
//! WorldPainter, Amulet et Litematica. Et le résultat est MEILLEUR qu'un pack
//! embarqué : le dossier d'un launcher contient aussi les packs du SERVEUR,
//! donc les blocs `minefield:*` arrivent avec leurs textures sans rien
//! demander.
//!
//! La chaîne est `blockstates → models (parent) → textures`, et on ne peut pas
//! la raccourcir : `grass_block` n'a aucune texture qui porte son nom, et
//! surtout un bloc peut n'être pas un cube.
//!
//! Ce crate produit ce que `tf-mesh` demande — une `TableFormes` — et rien de
//! plus. Le mailleur continue d'ignorer qu'un pack existe.

#![forbid(unsafe_code)]

pub mod apparence;
pub mod atlas;
pub mod blockstates;
pub mod catalogue;
pub mod modele;
pub mod rotation;
pub mod source;
pub mod texture;

pub use apparence::{textures_des_etats, Apparence, Habillage, Teintes};
pub use atlas::Atlas;
pub use blockstates::{Blockstate, Variante};
pub use catalogue::{table_rendu, Catalogue, Classement};
pub use modele::{cuboides, resoudre, Element, Modele, ModeleError, ModeleResolu};
pub use rotation::{axes, tourner, Axes};
pub use source::{Dossier, Id, Pile, Source, SourceError};
pub use texture::{TextureError, Tuile};
