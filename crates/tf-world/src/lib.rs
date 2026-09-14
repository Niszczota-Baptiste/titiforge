//! Le monde résident.
//!
//! Il n'existe aucun état « le monde est chargé ». Une région pleine fait déjà
//! 100 millions de blocs ; un monde Minefield en fait 80 milliards. Ce crate
//! ne connaît donc qu'une **fenêtre de résidence** plafonnée en octets,
//! pilotée par ce qu'on regarde et par ce qu'on édite.

#![forbid(unsafe_code)]

pub mod coords;
pub mod fs_source;
pub mod journal;
pub mod residency;
pub mod source;
pub mod staging;

pub use coords::{
    floor_div, floor_mod, BBox, BlockPos, ChunkPos, Height, LocalBox, RegionPos, SectionPos,
};
pub use fs_source::FsSource;
pub use journal::{
    Chemin, ChunkPatch, Cible, Correction, Entree, Genre, Journal, JournalError, Record,
};
pub use residency::{Editing, Evicted, Residency, State, Weighed};
pub use source::{
    Dimension, Folder, LockProbe, MemorySource, Overview, RegionInfo, RegionSink, RegionSource,
    SourceError,
};
pub use staging::{CommitError, CommitReport, RegionStore, Staging};
