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
//! - `chargeur` : le fil qui lit les régions, une réponse par cellule.
//! - `pilote` : la boucle caméra → demande → fil → scène. Dans la
//!   bibliothèque et non dans la fenêtre, pour qu'un test puisse faire VOLER
//!   une caméra et vérifier que le monde arrive.

#![forbid(unsafe_code)]

pub mod chargeur;
pub mod etat;
pub mod interface;
pub mod moteur;
pub mod pilote;
pub mod scene;

pub use etat::{Etat, Quadrillage, ResumeSelection, SousLeReticule};
pub use moteur::{Commande, Moteur, Reponse};
