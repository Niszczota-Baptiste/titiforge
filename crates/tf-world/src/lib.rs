//! Le monde résident.
//!
//! Il n'existe aucun état « le monde est chargé ». Une région pleine fait déjà
//! 100 millions de blocs ; un monde Minefield en fait 80 milliards. Ce crate
//! ne connaît donc qu'une **fenêtre de résidence** plafonnée en octets,
//! pilotée par ce qu'on regarde et par ce qu'on édite.

#![forbid(unsafe_code)]

pub mod coords;
pub mod residency;

pub use coords::{
    floor_div, floor_mod, BBox, BlockPos, ChunkPos, Height, LocalBox, RegionPos, SectionPos,
};
pub use residency::{Editing, Evicted, Residency, State, Weighed};
