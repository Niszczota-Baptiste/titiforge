//! Le rendu wgpu.
//!
//! La cible par défaut est une TEXTURE, pas une fenêtre : un moteur qui ne sait
//! dessiner que dans une fenêtre ne se teste pas. Une image relue en mémoire se
//! compare au pixel près, et une intégration continue la produit sans écran.

#![forbid(unsafe_code)]

pub mod appareil;
pub mod arene;
pub mod camera;
pub mod modeles;
pub mod scene;

pub use appareil::{Appareil, AppareilError};
pub use arene::{Arene, InstanceQuad, Tranche};
pub use camera::Camera;
pub use modeles::{faces_de, AreneModeles, FaceModele, HabillageFaces, Origine, Pose};
pub use scene::{AtlasGpu, Cible, Compte, Scene};
