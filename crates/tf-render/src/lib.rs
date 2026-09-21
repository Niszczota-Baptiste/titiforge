//! Le rendu wgpu.
//!
//! La cible par défaut est une TEXTURE, pas une fenêtre : un moteur qui ne sait
//! dessiner que dans une fenêtre ne se teste pas. Une image relue en mémoire se
//! compare au pixel près, et une intégration continue la produit sans écran.

#![forbid(unsafe_code)]

pub mod appareil;
pub mod arene;
pub mod camera;
pub mod controles;
pub mod lignes;
pub mod modeles;
pub mod scene;
pub mod viser;

pub use appareil::{Appareil, AppareilError};
pub use arene::{depaqueter, empaqueter, Arene, InstanceQuad, Tranche};
pub use camera::Camera;
pub use controles::{Mode, Vue};
pub use lignes::{rgba, Lignes, Sommet, SEIZIEMES_PAR_BLOC};
pub use modeles::{faces_de, origines, AreneModeles, FaceModele, HabillageFaces, Origine, Pose};
pub use scene::{AtlasGpu, Cible, Compte, Scene};
pub use viser::{rayon_ecran, viser, Touche};
