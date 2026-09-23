//! Le monde résident.
//!
//! Il n'existe aucun état « le monde est chargé ». Une région pleine fait déjà
//! 100 millions de blocs ; un monde Minefield en fait 80 milliards. Ce crate
//! ne connaît donc qu'une **fenêtre de résidence** plafonnée en octets,
//! pilotée par ce qu'on regarde et par ce qu'on édite.

#![forbid(unsafe_code)]

pub mod coords;
pub mod decoupe;
pub mod demande;
pub mod fs_source;
pub mod inference;
pub mod journal;
pub mod lecture;
pub mod residency;
pub mod selection;
pub mod source;
pub mod staging;

pub use coords::{
    floor_div, floor_mod, BBox, BlockPos, ChunkPos, Height, LocalBox, RegionPos, SectionPos,
};
pub use decoupe::{cellules_autour, Cellule, Niveau, RAYON_MAX};
pub use demande::{par_region, planifier, voulues, Plan, Suivi, Voulue};
pub use fs_source::{sauvegarder, FsSource};
pub use inference::{accrocher, Accroche, Ancre, Raison, Reference, TOLERANCE};
pub use journal::{
    Chemin, ChunkPatch, Cible, Correction, Entree, Genre, Journal, JournalError, Record,
};
pub use lecture::{sections_de, Bilan, SectionLue};
pub use residency::{Editing, Evicted, Residency, State, Weighed};
pub use selection::{Direction, Selection, DIRECTIONS};
pub use source::{
    Dimension, Folder, LockProbe, MemorySource, Overview, RegionInfo, RegionSink, RegionSource,
    SourceError,
};
pub use staging::{CommitError, CommitReport, RegionStore, Staging};
