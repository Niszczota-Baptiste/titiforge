//! NBT — lecture **ciblée et sans copie**, écriture des seuls tags dont Anvil
//! a besoin.
//!
//! Ce n'est pas un parseur NBT généraliste, et c'est délibéré. Le bench de
//! `we-engine` montre que `prismarine-nbt` pèse 35 % du décodage d'une région
//! une fois le dépack de sections corrigé : un parseur généraliste construit
//! un arbre d'objets pour un chunk dont on ne veut que
//! `sections[].block_states`.
//!
//! Ici on MARCHE dans les octets. On saute ce qu'on ne cherche pas, et on note
//! la PLAGE d'octets de ce qu'on garde. C'est cette plage qui rend le
//! round-trip lossless structurel plutôt qu'espéré : pour réécrire une section
//! modifiée, on remplace ses octets sur place et tout le reste du chunk est
//! recopié tel quel — Heightmaps, structures, données de mods inconnues
//! comprises. Ce que le lecteur ne comprend pas ne peut pas être abîmé.

#![forbid(unsafe_code)]

mod reader;
mod writer;

pub use reader::{Cur, Span, TagId, Trunc, R};
pub use writer::Writer;

/// Identifiants de tags NBT.
pub mod tag {
    pub const END: u8 = 0;
    pub const BYTE: u8 = 1;
    pub const SHORT: u8 = 2;
    pub const INT: u8 = 3;
    pub const LONG: u8 = 4;
    pub const FLOAT: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const BYTE_ARRAY: u8 = 7;
    pub const STRING: u8 = 8;
    pub const LIST: u8 = 9;
    pub const COMPOUND: u8 = 10;
    pub const INT_ARRAY: u8 = 11;
    pub const LONG_ARRAY: u8 = 12;

    /// Taille fixe de la charge d'un tag, ou `None` si elle dépend du contenu.
    /// Sert à sauter une liste entière d'un coup.
    pub const fn fixed_size(t: u8) -> Option<usize> {
        match t {
            END => Some(0),
            BYTE => Some(1),
            SHORT => Some(2),
            INT | FLOAT => Some(4),
            LONG | DOUBLE => Some(8),
            _ => None,
        }
    }

    pub const fn is_valid(t: u8) -> bool {
        t <= LONG_ARRAY
    }
}

pub use writer::{
    biomes_payload, block_states_payload, long_array_payload, named_long_array,
    palette_list_payload, string_list_payload, PaletteEntryRef,
};
