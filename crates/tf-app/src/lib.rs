//! **La coque de titiforge.**
//!
//! Elle tient la fenêtre, les outils et l'interface — et elle sait se dessiner
//! dans une TEXTURE, sans serveur graphique. Ce n'est pas un mode dégradé :
//! c'est ce qui permet de la vérifier au pixel, comme le rendu, et de montrer
//! à quoi elle ressemble depuis une machine qui n'a pas d'écran.
//!
//! - `etat` : ce que l'interface montre et modifie. Pur, testable.
//! - `accueil` : ouvrir un monde sans ligne de commande — les saves de la
//!   machine, les récents, un chemin collé. Pur, testable.
//! - `nuancier` : choisir un bloc sans connaître son identifiant — le visé,
//!   les récents, ce que le monde porte, ce que le pack déclare. Pur.
//! - `interface` : le dessin egui. Il LIT l'état, il ne décide rien.
//! - `regles` : les règles de rotation des états, dérivées en fond et
//!   données au fil moteur.
//! - `scene` : le montage du monde vers l'arène GPU.
//! - `chargeur` : le fil qui lit les régions, une réponse par cellule.
//! - `pilote` : la boucle caméra → demande → fil → scène. Dans la
//!   bibliothèque et non dans la fenêtre, pour qu'un test puisse faire VOLER
//!   une caméra et vérifier que le monde arrive.

#![forbid(unsafe_code)]

pub mod accueil;
pub mod chargeur;
pub mod etat;
pub mod interface;
pub mod moteur;
pub mod nuancier;
pub mod pilote;
pub mod regles;
pub mod scene;

pub use etat::{Etat, Quadrillage, ResumeSelection, SousLeReticule};
pub use moteur::{Commande, Moteur, Reponse};
