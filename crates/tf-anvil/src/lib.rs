//! Lecture et écriture de régions Anvil, sans perte.
//!
//! Deux invariants tiennent tout le crate :
//!
//! 1. **Un chunk non modifié ressort octet pour octet.** On garde sa charge
//!    compressée brute ; `RawChunk::is_pristine` dit si c'est encore le cas.
//! 2. **Un chunk modifié n'est pas ré-encodé, il est splicé.** Seuls les
//!    octets des `block_states` qu'on a touchés sont remplacés. Ce que le
//!    lecteur ne comprend pas — Heightmaps, structures, données de mods — ne
//!    peut pas être abîmé, structurellement.

#![forbid(unsafe_code)]

pub mod biomes;
pub mod chunk;
pub mod codec;
pub mod entites;
pub mod format;
pub mod region;
pub mod section;
pub mod state;

pub use biomes::{bits_biome, Biomes, VOL_BIOME};
pub use chunk::{
    biome_edits, decode_biomes, decode_section, edition_entites, encode_section, inverse_edits,
    scan, section_edits, splice, BiomeSpans, ChunkScan, Edit, EncodeError, ScannedSection,
    SectionSpans, SpliceError,
};
pub use codec::{deflate, deflate_level, inflate, CodecError};
pub use entites::{Ancrage, Entite, EntiteReperee, ListeEntites};
pub use format::{
    detect_packing, longs_for, pack, packing_de_repli, unpack_into, version_label, Layout, Packing,
    DV_SANS_CHEVAUCHEMENT,
};
pub use region::{
    chunk_of_block, external_file_name, floor_div, read, region_coords_from_name, region_file_name,
    region_of_chunk, write, Compression, ExternalFile, RawChunk, ReadError, Region, WriteError,
    WriteOutput, CHUNKS, EXTERNAL_FLAG, HEADER, MAX_SECTORS, MAX_SECTOR_OFFSET, SECTOR,
};
pub use section::{bits_for, in_section, local_index, Section, MAX_BITS, MAX_PALETTE, VOL};
pub use state::{split_key, state_key, Interner, StateId};
