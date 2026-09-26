//! Le maillage d'une section.
//!
//! Trois passes, et il en faut trois :
//!
//! - **gloutonne** (`glouton`) pour les blocs qui bouchent leur case. Elle
//!   fusionne les rectangles de faces identiques : une muraille de 64 × 40
//!   sort en un quad au lieu de 2 560.
//! - **modèles** (`modeles`) pour tout le reste. Sur la cible Minefield, deux
//!   tiers du catalogue et près de la moitié de la géométrie d'un build.
//! - **fluides** (`fluides`) pour l'eau et la lave, qui n'ont pas de modèle :
//!   leur surface n'est pas plate, et c'est le code du jeu qui la décide.
//!
//! Toutes lisent le même `Voisinage` — une section **avec une peau d'une
//! case**. Les deux premières écrivent dans le même `Maillage`, la troisième
//! dans sa liste de `FaceFluide`. Aucune ne connaît de fichier, de pack ni de
//! GPU : ce qu'elles savent d'un bloc passe par `Formes`.

#![forbid(unsafe_code)]

pub mod chantier;
pub mod fluides;
pub mod forme;
pub mod glouton;
pub mod maillage;
pub mod modeles;
pub mod opacite;
pub mod voisinage;

pub use chantier::{Adresse, Chantier, Grille, Lot, Maillages, ABSENT};
pub use forme::{Cuboide, Face, Fluide, Formes, GenreFluide, TableFormes, FACES};
pub use maillage::{
    FaceFluide, Instance, Instances, Maillage, Quad, TextureFluide, OCTETS_FACE_FLUIDE,
};
pub use opacite::Opacite;
pub use voisinage::{Voisinage, COTE, COTE_PAD, PAD, VOL_PAD};

/// Maille une section : les deux passes, dans le même maillage.
///
/// L'ordre n'est pas indifférent pour la lecture des chiffres — `quads_glouton`
/// et `quads_modele` disent lequel a coûté quoi — mais il l'est pour le
/// résultat : les deux passes traitent des ensembles de blocs DISJOINTS. Un
/// bloc opaque ne passe jamais par les modèles, un bloc-modèle n'entre jamais
/// dans le masque glouton. Sans cette disjonction, tout ce qui est entre les
/// deux serait dessiné deux fois.
/// Mesuré : une section entièrement d'air coûte quand même **17 µs**, parce
/// que la carte d'opacité et la passe de modèles la parcourent toutes les
/// deux. Sur un monde plein de ciel, c'est le poste principal.
///
/// Le mailleur ne peut pas l'éviter seul : une section d'air n'est pas
/// distinguable d'une section de plantes du point de vue de l'opacité. C'est
/// l'appelant qui le sait gratuitement — sa palette a UNE entrée, et c'est de
/// l'air. **Ne pas appeler le mailleur sur une section-là.**
pub fn mailler<F: Formes + ?Sized>(v: &Voisinage, f: &F) -> Maillage {
    let mut out = Maillage::new();
    // La carte d'opacité est relevée UNE fois pour les deux passes. Chacune la
    // demandait à son compte : 49 152 appels virtuels par section pour la
    // gloutonne seule, quel que soit son contenu.
    let op = opacite::Opacite::relever(v, f);
    glouton::mailler_avec(v, f, &op, &mut out);
    modeles::mailler_avec(v, f, &op, &mut out);
    out
}

/// Maille une section pour le **GPU** : quads gloutons et poses de modèles.
///
/// C'est le chemin du rendu. `mailler` reste celui de tout ce qui a besoin de
/// la géométrie côté processeur — un export, une capture, un test.
pub fn mailler_pour_gpu<F: Formes + ?Sized>(v: &Voisinage, f: &F) -> (Maillage, Instances) {
    let mut quads = Maillage::new();
    let mut poses = Instances::new();
    let op = opacite::Opacite::relever(v, f);
    glouton::mailler_avec(v, f, &op, &mut quads);
    modeles::instancier_avec(v, f, &op, &mut poses);
    (quads, poses)
}
