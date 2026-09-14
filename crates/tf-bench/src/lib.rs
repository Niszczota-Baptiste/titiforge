//! Fixtures de mesure.
//!
//! Aucun fichier binaire dans le dépôt : les régions se construisent à la
//! volée depuis une graine, donc identiques d'une exécution à l'autre et
//! lisibles en revue. Les benchs n'ont ainsi besoin ni de Node, ni d'une save
//! réelle, ni de la moindre ressource extérieure.
//!
//! Le contenu ressemble volontairement à du **terrain Minecraft** plutôt qu'à
//! du bruit : palettes d'une dizaine d'entrées, beaucoup de pierre, des
//! sections entièrement pleines et d'autres entièrement vides. Une région de
//! blocs aléatoires mesurerait un cas qui n'existe pas — palette saturée,
//! compression inefficace, aucune section homogène — et orienterait les
//! optimisations vers le mauvais endroit.

#![forbid(unsafe_code)]

pub mod build;
pub mod catalogue;

pub use build::Build;
pub use catalogue::{Forme, BLOCS};

use std::io::Write;

use tf_anvil::{Packing, SECTOR};
use tf_nbt::{tag, Writer};

/// Générateur déterministe, sans dépendance.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }
    /// Nommé `next_u32` et non `next` : un `next` sur un type qui n'implémente
    /// pas `Iterator` se lit comme s'il en était un, et une boucle `for` sur ce
    /// générateur ne compilerait pas pour une raison illisible.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
}

/// Les blocs d'un sous-sol plausible.
///
/// Vingt et une entrées, et c'est un choix mesuré : une palette de 21 demande
/// **5 bits** par indice. Les largeurs de 4 et 8 bits sont les seules où les
/// deux packings du format produisent les mêmes octets — un bench qui n'aurait
/// que des palettes de 16 entrées ne ferait jamais travailler le chemin
/// intéressant, et ne verrait aucune régression dessus.
const STRATA: [&str; 21] = [
    "minecraft:deepslate",
    "minecraft:stone",
    "minecraft:andesite",
    "minecraft:diorite",
    "minecraft:granite",
    "minecraft:gravel",
    "minecraft:dirt",
    "minecraft:coarse_dirt",
    "minecraft:tuff",
    "minecraft:calcite",
    "minecraft:iron_ore",
    "minecraft:copper_ore",
    "minecraft:coal_ore",
    "minecraft:gold_ore",
    "minecraft:redstone_ore",
    "minecraft:lapis_ore",
    "minecraft:diamond_ore",
    "minecraft:water",
    "minecraft:lava",
    "minecraft:smooth_basalt",
    "minecraft:amethyst_block",
];

#[derive(Debug, Clone, Copy)]
pub struct Terrain {
    /// Côté en chunks. 32 = une région pleine.
    pub side: u32,
    /// Sections par chunk. 24 = la hauteur complète 1.18+ (y ∈ [−64, 319]).
    pub sections: usize,
    pub seed: u32,
    pub packing: Packing,
}

impl Default for Terrain {
    fn default() -> Self {
        Terrain {
            side: 32,
            sections: 24,
            seed: 7,
            packing: Packing::NoStraddle,
        }
    }
}

impl Terrain {
    /// Une région pleine : 1024 chunks, 24 576 sections, 100 663 296 blocs.
    pub fn region_pleine() -> Self {
        Terrain::default()
    }

    /// Un quart de région — pour les benchs qu'on veut voir tourner vite.
    pub fn petite() -> Self {
        Terrain {
            side: 16,
            sections: 12,
            ..Terrain::default()
        }
    }

    pub fn blocs(&self) -> usize {
        (self.side as usize) * (self.side as usize) * self.sections * 4096
    }

    pub fn sections_total(&self) -> usize {
        (self.side as usize) * (self.side as usize) * self.sections
    }
}

fn section_payload(sy: i8, rng: &mut Rng, packing: Packing) -> Vec<u8> {
    let world_y = sy as i32 * 16;

    // Au-dessus de la surface : de l'air, donc une section homogène. C'est la
    // moitié d'un vrai monde, et c'est le cas que le format optimise.
    if world_y > 64 {
        return tf_nbt::block_states_payload(
            &[tf_nbt::PaletteEntryRef {
                name: "minecraft:air",
                props: &[],
            }],
            &[],
        );
    }

    let base = if world_y < 0 {
        "minecraft:deepslate"
    } else {
        "minecraft:stone"
    };
    let mut noms: Vec<&str> = vec![base];
    for s in STRATA {
        if s != base {
            noms.push(s);
        }
    }
    let n = noms.len();

    // 6 % de veines, comme un vrai sous-sol.
    let idx: Vec<u16> = (0..4096)
        .map(|_| {
            if rng.below(100) < 6 {
                rng.below(n) as u16
            } else {
                0
            }
        })
        .collect();

    let bits = tf_anvil::bits_for(n) as usize;
    let data = tf_anvil::pack(&idx, bits, packing);
    let entries: Vec<tf_nbt::PaletteEntryRef> = noms
        .iter()
        .map(|name| tf_nbt::PaletteEntryRef { name, props: &[] })
        .collect();
    tf_nbt::block_states_payload(&entries, &data)
}

fn chunk_nbt(cx: i32, cz: i32, t: &Terrain, rng: &mut Rng) -> Vec<u8> {
    let mut w = Writer::with_capacity(64 * 1024);
    // `field` écrit DÉJÀ le nom : un `raw_str("")` de plus serait lu comme le
    // type du premier champ, donc comme un TAG_End, et le chunk se terminerait
    // aussitôt — sans erreur, juste vide.
    w.field(tag::COMPOUND, "");

    w.field(tag::INT, "DataVersion").i32_payload(3465);
    w.field(tag::INT, "xPos").i32_payload(cx);
    w.field(tag::INT, "yPos").i32_payload(-4);
    w.field(tag::INT, "zPos").i32_payload(cz);
    w.field(tag::STRING, "Status").raw_str("minecraft:full");

    // Des Heightmaps réalistes : gros, et sans le moindre intérêt pour nous.
    // Ils font partie du coût de lecture d'un vrai chunk, donc ils doivent
    // faire partie du bench — les omettre flatterait les chiffres.
    w.field(tag::COMPOUND, "Heightmaps");
    w.field(tag::LONG_ARRAY, "MOTION_BLOCKING");
    w.long_array_payload(&vec![0x0123_4567_89AB_CDEF; 37]);
    w.field(tag::LONG_ARRAY, "WORLD_SURFACE");
    w.long_array_payload(&vec![0xFEDC_BA98_7654_3210; 37]);
    w.end();

    w.field(tag::LIST, "sections");
    w.list_header(tag::COMPOUND, t.sections);
    for k in 0..t.sections {
        let sy = -4i8 + k as i8;
        w.field(tag::BYTE, "Y").i8_payload(sy);
        w.field(tag::COMPOUND, "block_states");
        w.raw(&section_payload(sy, rng, t.packing));
        w.end();
    }

    w.field(tag::LIST, "block_entities")
        .list_header(tag::COMPOUND, 0);
    w.end(); // racine
    w.into_bytes()
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// Construit un `.mca` complet.
pub fn region(t: &Terrain) -> Vec<u8> {
    let mut rng = Rng::new(t.seed);
    let mut locations = vec![0u8; 4096];
    let mut timestamps = vec![0u8; 4096];
    let mut body: Vec<u8> = Vec::new();
    let mut next = 2u32;

    for cz in 0..t.side {
        for cx in 0..t.side {
            let payload = zlib(&chunk_nbt(cx as i32, cz as i32, t, &mut rng));
            let len = payload.len() + 1;
            let total = 4 + len;
            let sectors = total.div_ceil(SECTOR);

            body.extend_from_slice(&(len as u32).to_be_bytes());
            body.push(2); // zlib
            body.extend_from_slice(&payload);
            body.resize(body.len() + (sectors * SECTOR - total), 0);

            let i = (cx + cz * 32) as usize;
            let loc = (next << 8) | sectors as u32;
            locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
            timestamps[i * 4..i * 4 + 4].copy_from_slice(&1_700_000_000u32.to_be_bytes());
            next += sectors as u32;
        }
    }

    let mut out = Vec::with_capacity(8192 + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    out
}

/// Occupation mémoire résidente du processus, en octets.
///
/// Lue dans `/proc/self/statm` : c'est la seule mesure qui compte pour ce
/// projet — un pic de RSS est ce qui fait échouer une opération chez
/// l'utilisateur, pas la somme des allocations.
pub fn rss_bytes() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<usize>().ok())
        .map(|pages| pages * 4096)
        .unwrap_or(0)
}
