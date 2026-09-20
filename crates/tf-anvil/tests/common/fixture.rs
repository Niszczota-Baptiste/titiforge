//! Producteur INDÉPENDANT de fichiers de région.
//!
//! Écrit ses octets à la main, depuis la spécification Anvil, **sans utiliser
//! une seule ligne de `tf_anvil`**. C'est la condition pour que les tests
//! prouvent quelque chose : si le producteur et le lecteur partageaient leur
//! code, un bug commun aux deux passerait inaperçu — le test dirait seulement
//! « le code est d'accord avec lui-même ».
//!
//! Pas de fixture binaire dans le dépôt : tout se construit à la volée depuis
//! une graine, donc identique d'une exécution à l'autre, et lisible en revue.

#![allow(dead_code)]

// ── écrivain NBT minimal, indépendant ───────────────────────────────────────

#[derive(Default)]
pub struct Nbt {
    pub b: Vec<u8>,
}

impl Nbt {
    pub fn new() -> Self {
        Self::default()
    }
    fn raw_str(&mut self, s: &str) {
        self.b.extend_from_slice(&(s.len() as u16).to_be_bytes());
        self.b.extend_from_slice(s.as_bytes());
    }
    pub fn field(&mut self, t: u8, name: &str) -> &mut Self {
        self.b.push(t);
        self.raw_str(name);
        self
    }
    pub fn end(&mut self) -> &mut Self {
        self.b.push(0);
        self
    }
    pub fn i8v(&mut self, v: i8) -> &mut Self {
        self.b.push(v as u8);
        self
    }
    pub fn i32v(&mut self, v: i32) -> &mut Self {
        self.b.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn strv(&mut self, v: &str) -> &mut Self {
        self.raw_str(v);
        self
    }
    pub fn list(&mut self, et: u8, n: usize) -> &mut Self {
        self.b.push(et);
        self.i32v(n as i32);
        self
    }
    pub fn longs(&mut self, v: &[u64]) -> &mut Self {
        self.i32v(v.len() as i32);
        for &w in v {
            self.b.extend_from_slice(&w.to_be_bytes());
        }
        self
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.i32v(v.len() as i32);
        self.b.extend_from_slice(v);
        self
    }
}

pub mod t {
    pub const BYTE: u8 = 1;
    pub const INT: u8 = 3;
    pub const BYTE_ARRAY: u8 = 7;
    pub const STRING: u8 = 8;
    pub const LIST: u8 = 9;
    pub const COMPOUND: u8 = 10;
    pub const LONG_ARRAY: u8 = 12;
}

// ── packing d'indices, réimplémenté indépendamment ──────────────────────────

/// Bits par indice — règle Minecraft, plancher à 4.
/// Les bits d'un indice de BIOME. Pas de plancher à 4 : c'est la différence
/// avec les blocs, et elle change la longueur du tableau écrit.
///
/// Réécrit ici plutôt qu'importé de `src/` : le producteur de fixtures doit
/// rester indépendant de ce qu'il sert à tester.
pub fn bits_biome(len: usize) -> usize {
    let mut b = 1usize;
    while (1usize << b) < len {
        b += 1;
    }
    b
}

pub fn bits_for(len: usize) -> usize {
    let mut b = 4usize;
    while (1usize << b) < len {
        b += 1;
    }
    b
}

/// Packe 4096 indices SANS chevauchement (format 1.16+).
pub fn pack(indices: &[u16], bits: usize) -> Vec<u64> {
    let per_long = 64 / bits;
    let mut out = vec![0u64; indices.len().div_ceil(per_long)];
    for (n, &v) in indices.iter().enumerate() {
        out[n / per_long] |= (v as u64) << ((n % per_long) * bits);
    }
    out
}

/// Index YZX.
pub fn li(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

// ── générateur déterministe ─────────────────────────────────────────────────

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

// ── description d'une section, côté test ────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionSpec {
    pub y: i8,
    /// Entrées de palette : `(nom, [(clé, valeur)])`.
    pub palette: Vec<(String, Vec<(String, String)>)>,
    /// 4096 indices dans la palette.
    pub indices: Vec<u16>,
}

impl SectionSpec {
    /// Section homogène : palette d'une entrée, aucun `data` écrit.
    pub fn uniform(y: i8, name: &str) -> Self {
        SectionSpec {
            y,
            palette: vec![(name.to_string(), vec![])],
            indices: vec![0; 4096],
        }
    }

    /// Section dont la palette compte `n` entrées — sert à balayer toutes les
    /// largeurs de bits, de 4 à 12.
    pub fn with_palette_size(y: i8, n: usize, rng: &mut Rng) -> Self {
        let palette: Vec<(String, Vec<(String, String)>)> = (0..n)
            .map(|i| {
                if i % 7 == 3 {
                    (
                        format!("minecraft:bloc_{i}"),
                        vec![
                            ("facing".to_string(), "north".to_string()),
                            ("half".to_string(), "top".to_string()),
                        ],
                    )
                } else {
                    (format!("minecraft:bloc_{i}"), vec![])
                }
            })
            .collect();
        let indices: Vec<u16> = (0..4096).map(|_| rng.below(n) as u16).collect();
        SectionSpec {
            y,
            palette,
            indices,
        }
    }
}

fn write_section(nbt: &mut Nbt, s: &SectionSpec) {
    nbt.field(t::BYTE, "Y").i8v(s.y);

    nbt.field(t::COMPOUND, "block_states");
    nbt.field(t::LIST, "palette")
        .list(t::COMPOUND, s.palette.len());
    for (name, props) in &s.palette {
        nbt.field(t::STRING, "Name").strv(name);
        if !props.is_empty() {
            nbt.field(t::COMPOUND, "Properties");
            for (k, v) in props {
                nbt.field(t::STRING, k).strv(v);
            }
            nbt.end();
        }
        nbt.end();
    }
    if s.palette.len() > 1 {
        let bits = bits_for(s.palette.len());
        nbt.field(t::LONG_ARRAY, "data")
            .longs(&pack(&s.indices, bits));
    }
    nbt.end(); // block_states

    // Les biomes : 64 cellules de 4 × 4 × 4, palette de CHAÎNES, et **pas de
    // plancher à 4 bits**. Une section sur trois est monobiome — le cas qui
    // n'écrit aucun `data`, et celui qu'on oublie de tester.
    let biomes: &[&str] = if s.y % 3 == 0 {
        &["minecraft:plains"]
    } else {
        &[
            "minecraft:plains",
            "minecraft:forest",
            "minecraft:river",
            "minecraft:desert",
            "minecraft:swamp",
        ]
    };
    nbt.field(t::COMPOUND, "biomes");
    nbt.field(t::LIST, "palette").list(t::STRING, biomes.len());
    for b in biomes {
        nbt.strv(b);
    }
    if biomes.len() > 1 {
        // Cinq entrées → 3 bits, soit 3 longs pour 64 cellules. Un plancher
        // à 4 bits en écrirait 4, et le chunk ne se chargerait plus.
        let bits = bits_biome(biomes.len());
        let cells: Vec<u16> = (0..64).map(|i| (i % biomes.len()) as u16).collect();
        nbt.field(t::LONG_ARRAY, "data").longs(&pack(&cells, bits));
    }
    nbt.end(); // biomes

    // Un champ que le lecteur ne comprend pas, DANS la section : le splice doit
    // le laisser intact même quand il réécrit block_states juste à côté.
    nbt.field(t::BYTE_ARRAY, "modtest:donnees_de_section")
        .bytes(&[0xDE, 0xAD, 0xBE, 0xEF]);
    nbt.end(); // la section
}

/// Un chunk complet, avec des champs que le lecteur n'interprète pas.
///
/// Ces champs sont l'enjeu du test de non-destruction : `we-engine` ré-encode
/// l'arbre NBT dès qu'un chunk est modifié, donc tout ce que son parseur a mal
/// compris est perdu. Le splice, lui, ne peut pas les toucher.
pub fn chunk_nbt(chunk_x: i32, chunk_z: i32, sections: &[SectionSpec]) -> Vec<u8> {
    let mut n = Nbt::new();
    n.field(t::COMPOUND, ""); // racine

    n.field(t::INT, "DataVersion").i32v(3465); // 1.20.1
    n.field(t::INT, "xPos").i32v(chunk_x);
    n.field(t::INT, "yPos").i32v(-4);
    n.field(t::INT, "zPos").i32v(chunk_z);
    n.field(t::STRING, "Status").strv("minecraft:full");

    // Un Heightmaps réaliste : gros, et sans le moindre intérêt pour nous.
    n.field(t::COMPOUND, "Heightmaps");
    n.field(t::LONG_ARRAY, "MOTION_BLOCKING")
        .longs(&vec![0x0123_4567_89AB_CDEF; 37]);
    n.field(t::LONG_ARRAY, "WORLD_SURFACE")
        .longs(&vec![0xFEDC_BA98_7654_3210; 37]);
    n.end();

    n.field(t::LIST, "sections")
        .list(t::COMPOUND, sections.len());
    for s in sections {
        write_section(&mut n, s);
    }

    n.field(t::LIST, "block_entities").list(t::COMPOUND, 1);
    n.field(t::STRING, "id").strv("minecraft:chest");
    n.field(t::INT, "x").i32v(chunk_x * 16 + 3);
    n.field(t::INT, "y").i32v(64);
    n.field(t::INT, "z").i32v(chunk_z * 16 + 5);
    n.end();

    // Le témoin : de la donnée de mod, dans un champ dont le lecteur ignore
    // tout. Si elle survit à une modification de blocs, la non-destruction est
    // prouvée sur le cas qui compte.
    n.field(t::COMPOUND, "neoforge:attachments");
    n.field(t::BYTE_ARRAY, "create:kinetic")
        .bytes(&(0u8..=255).collect::<Vec<u8>>());
    n.field(t::STRING, "ae2:channel")
        .strv("témoin — ne doit jamais bouger 한국어");
    n.end();

    n.end(); // racine
    n.b
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// Assemble un `.mca` à partir de chunks `(localX, localZ, nbt)`.
///
/// `gap_every` laisse un secteur vide tous les N chunks : un fichier venu du
/// jeu est FRAGMENTÉ, et un lecteur qui suppose une disposition compacte le
/// lirait de travers sans rien signaler.
pub fn region_file(chunks: &[(u32, u32, Vec<u8>)], gap_every: usize) -> Vec<u8> {
    const SECTOR: usize = 4096;
    let mut locations = vec![0u8; 4096];
    let mut timestamps = vec![0u8; 4096];
    let mut body: Vec<u8> = Vec::new();
    let mut next = 2u32;

    for (k, (lx, lz, nbt)) in chunks.iter().enumerate() {
        let payload = zlib(nbt);
        let len = payload.len() + 1;
        let total = 4 + len;
        let sectors = total.div_ceil(SECTOR);

        body.extend_from_slice(&(len as u32).to_be_bytes());
        body.push(2); // zlib
        body.extend_from_slice(&payload);
        body.resize(body.len() + (sectors * SECTOR - total), 0);

        let i = (lx + lz * 32) as usize;
        let loc = (next << 8) | sectors as u32;
        locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
        timestamps[i * 4..i * 4 + 4].copy_from_slice(&(1_700_000_000u32 + k as u32).to_be_bytes());
        next += sectors as u32;

        if gap_every > 0 && (k + 1) % gap_every == 0 {
            body.resize(body.len() + SECTOR, 0); // trou de fragmentation
            next += 1;
        }
    }

    let mut out = Vec::with_capacity(8192 + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    out
}

/// Une région de terrain plausible : strates par hauteur et veines, donc des
/// palettes d'une dizaine d'entrées et des sections entièrement homogènes.
/// Une région de blocs aléatoires mesurerait un cas qui n'existe pas.
pub fn terrain_region(side: u32, sections_per_chunk: usize, seed: u32) -> Vec<u8> {
    const STRATA: [&str; 8] = [
        "minecraft:deepslate",
        "minecraft:stone",
        "minecraft:andesite",
        "minecraft:diorite",
        "minecraft:granite",
        "minecraft:gravel",
        "minecraft:dirt",
        "minecraft:coarse_dirt",
    ];
    let mut rng = Rng::new(seed);
    let mut chunks = Vec::new();
    for cz in 0..side {
        for cx in 0..side {
            let mut secs = Vec::new();
            for k in 0..sections_per_chunk {
                let y = -4i8 + k as i8;
                let world_y = y as i32 * 16;
                if world_y > 64 {
                    secs.push(SectionSpec::uniform(y, "minecraft:air"));
                    continue;
                }
                let base = if world_y < 0 {
                    "minecraft:deepslate"
                } else {
                    "minecraft:stone"
                };
                let mut palette: Vec<(String, Vec<(String, String)>)> =
                    vec![(base.to_string(), vec![])];
                for s in STRATA {
                    if s != base {
                        palette.push((s.to_string(), vec![]));
                    }
                }
                let n = palette.len();
                let indices: Vec<u16> = (0..4096)
                    .map(|_| {
                        if rng.below(100) < 6 {
                            rng.below(n) as u16
                        } else {
                            0
                        }
                    })
                    .collect();
                secs.push(SectionSpec {
                    y,
                    palette,
                    indices,
                });
            }
            chunks.push((cx, cz, chunk_nbt(cx as i32, cz as i32, &secs)));
        }
    }
    region_file(&chunks, 7)
}

// ── format ancien : 1.13 – 1.17 ─────────────────────────────────────────────

/// Packe AVEC chevauchement — les indices sont collés bout à bout, un indice
/// peut donc être à cheval sur deux longs. C'est la disposition de 1.13 à 1.15,
/// et celle de Litematica aujourd'hui encore.
pub fn pack_straddle(indices: &[u16], bits: usize) -> Vec<u64> {
    let total = (indices.len() * bits).div_ceil(64);
    let mut out = vec![0u64; total];
    for (n, &v) in indices.iter().enumerate() {
        let off = n * bits;
        let li = off / 64;
        let b = off % 64;
        out[li] |= (v as u64) << b;
        if b + bits > 64 {
            out[li + 1] |= (v as u64) >> (64 - b);
        }
    }
    out
}

/// Un chunk 1.13 – 1.17 : tout sous `Level`, et la section porte deux champs
/// FRÈRES, `Palette` et `BlockStates`.
///
/// `straddle` choisit le packing : `true` pour 1.13 – 1.15, `false` pour
/// 1.16 – 1.17 (le changement date de 20w17a).
pub fn legacy_chunk_nbt(
    chunk_x: i32,
    chunk_z: i32,
    sections: &[SectionSpec],
    straddle: bool,
) -> Vec<u8> {
    let mut n = Nbt::new();
    n.field(t::COMPOUND, ""); // racine
    n.field(t::INT, "DataVersion")
        .i32v(if straddle { 2230 } else { 2724 });

    n.field(t::COMPOUND, "Level");
    n.field(t::INT, "xPos").i32v(chunk_x);
    n.field(t::INT, "zPos").i32v(chunk_z);
    n.field(t::STRING, "Status").strv("full");

    // Un champ que le lecteur n'interprète pas, DANS Level.
    n.field(t::BYTE_ARRAY, "modtest:legacy")
        .bytes(&[0xC0, 0xFF, 0xEE]);

    n.field(t::LIST, "Sections")
        .list(t::COMPOUND, sections.len());
    for s in sections {
        n.field(t::BYTE, "Y").i8v(s.y);
        n.field(t::LIST, "Palette")
            .list(t::COMPOUND, s.palette.len());
        for (name, props) in &s.palette {
            n.field(t::STRING, "Name").strv(name);
            if !props.is_empty() {
                n.field(t::COMPOUND, "Properties");
                for (k, v) in props {
                    n.field(t::STRING, k).strv(v);
                }
                n.end();
            }
            n.end();
        }
        // Une palette d'une entrée n'écrit PAS de BlockStates : c'est le cas
        // qui oblige à INSÉRER le champ si la section cesse d'être homogène.
        if s.palette.len() > 1 {
            let bits = bits_for(s.palette.len());
            let longs = if straddle {
                pack_straddle(&s.indices, bits)
            } else {
                pack(&s.indices, bits)
            };
            n.field(t::LONG_ARRAY, "BlockStates").longs(&longs);
        }
        // Et un champ inconnu voisin, comme dans le format moderne.
        n.field(t::BYTE_ARRAY, "SkyLight")
            .bytes(&vec![0x77u8; 2048]);
        n.end(); // la section
    }

    n.end(); // Level
    n.end(); // racine
    n.b
}

// ── charges déportées : les chunks surdimensionnés ──────────────────────────

/// Un `.mca` contenant un TALON de chunk déporté à `(lx, lz)`.
///
/// Le talon est ce que Minecraft écrit quand la charge part dans un `.mcc` :
/// longueur 1 — l'octet de compression et rien derrière — et le bit 0x80 posé
/// sur cet octet.
pub fn region_file_with_stub(lx: u32, lz: u32, compression: u8) -> Vec<u8> {
    const SECTOR: usize = 4096;
    let mut locations = vec![0u8; 4096];
    let mut timestamps = vec![0u8; 4096];
    let mut body = vec![0u8; SECTOR];

    body[0..4].copy_from_slice(&1u32.to_be_bytes()); // longueur = 1
    body[4] = compression | 0x80; // le drapeau « déporté »

    let i = (lx + lz * 32) as usize;
    let loc = (2u32 << 8) | 1;
    locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
    timestamps[i * 4..i * 4 + 4].copy_from_slice(&42u32.to_be_bytes());

    let mut out = Vec::with_capacity(8192 + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    out
}

/// Compresse en zlib — le format qu'un `.mcc` porte, comme un chunk en ligne.
pub fn zlib_bytes(bytes: &[u8]) -> Vec<u8> {
    zlib(bytes)
}
