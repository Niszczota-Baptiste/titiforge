//! **La coque de titiforge.**
//!
//! Elle tient la fenêtre, les outils et l'interface — et elle sait se dessiner
//! dans une TEXTURE, sans serveur graphique. Ce n'est pas un mode dégradé :
//! c'est ce qui permet de la vérifier au pixel, comme le rendu, et de montrer
//! à quoi elle ressemble depuis une machine qui n'a pas d'écran.
//!
//! - `etat` : ce que l'interface montre et modifie. Pur, testable.
//! - `interface` : le dessin egui. Il LIT l'état, il ne décide rien.
//! - `scene` : le montage du monde vers l'arène GPU.

#![forbid(unsafe_code)]

pub mod etat;
pub mod interface;
pub mod scene;

pub use etat::{Etat, Quadrillage, ResumeSelection, SousLeReticule};
