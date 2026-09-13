//! Décodeur INDÉPENDANT — à garder GELÉ.
//!
//! Il relit un `.mca` produit par `tf_anvil` et dit ce qu'il contient. Son
//! seul rôle est de prouver la non-destruction, et il ne peut le faire qu'à
//! une condition : **ne jamais partager de code avec `src/`**. S'il appelait
//! `tf_anvil`, le test dirait seulement « le code est d'accord avec lui-même ».
//!
//! Sa méthode est délibérément l'OPPOSÉE de celle du crate : là où `tf-anvil`
//! marche dans les octets et n'ouvre que `block_states`, celui-ci construit un
//! arbre NBT complet et naïf. Deux erreurs indépendantes ont peu de chances de
//! coïncider ; deux implémentations de la même idée, beaucoup.
//!
//! Cette différence de méthode est le test. Ne pas l'« optimiser ».

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Read;

// ── arbre NBT ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    Str(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&Vec<Tag>> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Tag::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_i8(&self) -> Option<i8> {
        match self {
            Tag::Byte(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_longs(&self) -> Option<&Vec<i64>> {
        match self {
            Tag::LongArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&Vec<u8>> {
        match self {
            Tag::ByteArray(v) => Some(v),
            _ => None,
        }
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.b[self.i];
        self.i += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_be_bytes(self.b[self.i..self.i + 2].try_into().unwrap());
        self.i += 2;
        v
    }
    fn i32(&mut self) -> i32 {
        let v = i32::from_be_bytes(self.b[self.i..self.i + 4].try_into().unwrap());
        self.i += 4;
        v
    }
    fn i64(&mut self) -> i64 {
        let v = i64::from_be_bytes(self.b[self.i..self.i + 8].try_into().unwrap());
        self.i += 8;
        v
    }
    fn name(&mut self) -> String {
        let n = self.u16() as usize;
        let s = String::from_utf8_lossy(&self.b[self.i..self.i + n]).into_owned();
        self.i += n;
        s
    }
    fn payload(&mut self, t: u8) -> Tag {
        match t {
            0 => Tag::End,
            1 => Tag::Byte(self.u8() as i8),
            2 => {
                let v = i16::from_be_bytes(self.b[self.i..self.i + 2].try_into().unwrap());
                self.i += 2;
                Tag::Short(v)
            }
            3 => Tag::Int(self.i32()),
            4 => Tag::Long(self.i64()),
            5 => {
                let v = f32::from_be_bytes(self.b[self.i..self.i + 4].try_into().unwrap());
                self.i += 4;
                Tag::Float(v)
            }
            6 => {
                let v = f64::from_be_bytes(self.b[self.i..self.i + 8].try_into().unwrap());
                self.i += 8;
                Tag::Double(v)
            }
            7 => {
                let n = self.i32().max(0) as usize;
                let v = self.b[self.i..self.i + n].to_vec();
                self.i += n;
                Tag::ByteArray(v)
            }
            8 => Tag::Str(self.name()),
            9 => {
                let et = self.u8();
                let n = self.i32().max(0) as usize;
                if et == 0 {
                    return Tag::List(Vec::new());
                }
                Tag::List((0..n).map(|_| self.payload(et)).collect())
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let t = self.u8();
                    if t == 0 {
                        break;
                    }
                    let k = self.name();
                    v.push((k, self.payload(t)));
                }
                Tag::Compound(v)
            }
            11 => {
                let n = self.i32().max(0) as usize;
                Tag::IntArray((0..n).map(|_| self.i32()).collect())
            }
            12 => {
                let n = self.i32().max(0) as usize;
                Tag::LongArray((0..n).map(|_| self.i64()).collect())
            }
            other => panic!("décodeur gelé : type de tag {other} inconnu"),
        }
    }
}

pub fn parse_nbt(bytes: &[u8]) -> (String, Tag) {
    let mut p = P { b: bytes, i: 0 };
    let t = p.u8();
    assert_eq!(t, 10, "la racine NBT doit être un compound");
    let name = p.name();
    (name, p.payload(10))
}

// ── lecture du .mca ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FrozenChunk {
    pub local_x: u32,
    pub local_z: u32,
    pub timestamp: u32,
    pub root: Tag,
}

/// Décode entièrement une région. Les chunks sont indexés par `(lx, lz)`.
pub fn decode_region(buf: &[u8]) -> BTreeMap<(u32, u32), FrozenChunk> {
    let mut out = BTreeMap::new();
    assert!(buf.len() >= 8192, "fichier plus court que l'en-tête");
    for i in 0..1024usize {
        let loc = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        let off = (loc >> 8) as usize * 4096;
        let cnt = (loc & 0xff) as usize;
        if off == 0 || cnt == 0 {
            continue;
        }
        let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        let comp = buf[off + 4];
        // Bit 0x80 : la charge vit dans un `c.X.Z.mcc` à côté. Ce décodeur ne
        // voit que le `.mca`, donc il ne peut rien en dire — il saute, plutôt
        // que d'inflater un talon vide et de faire croire à un chunk cassé.
        if comp & 0x80 != 0 {
            continue;
        }
        let payload = &buf[off + 5..off + 4 + len];
        let mut inflated = Vec::new();
        match comp {
            1 => flate2::read::GzDecoder::new(payload)
                .read_to_end(&mut inflated)
                .unwrap(),
            2 => flate2::read::ZlibDecoder::new(payload)
                .read_to_end(&mut inflated)
                .unwrap(),
            3 => {
                inflated = payload.to_vec();
                0
            }
            other => panic!("décodeur gelé : compression {other} inconnue"),
        };
        let ts = u32::from_be_bytes(buf[4096 + i * 4..4096 + i * 4 + 4].try_into().unwrap());
        let (_, root) = parse_nbt(&inflated);
        out.insert(
            ((i % 32) as u32, (i / 32) as u32),
            FrozenChunk {
                local_x: (i % 32) as u32,
                local_z: (i / 32) as u32,
                timestamp: ts,
                root,
            },
        );
    }
    out
}

/// Clé canonique d'un état de bloc.
///
/// Réimplémentée ici exprès, même règle mais autre code : `nom` seul, ou
/// `nom|k=v,k=v` trié.
fn key_of(entry: &Tag) -> String {
    let name = entry
        .get("Name")
        .and_then(|t| t.as_str())
        .unwrap_or("?")
        .to_string();
    let mut props: Vec<(String, String)> = match entry.get("Properties") {
        Some(Tag::Compound(v)) => v
            .iter()
            .filter_map(|(k, t)| t.as_str().map(|s| (k.clone(), s.to_string())))
            .collect(),
        _ => Vec::new(),
    };
    if props.is_empty() {
        return name;
    }
    props.sort();
    let body: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{name}|{}", body.join(","))
}

/// Dépacke des indices, en déduisant le packing de la LONGUEUR du tableau.
///
/// Réimplémenté ici depuis la spec, sans regarder `format.rs`. Les deux
/// dispositions ne donnent la même longueur que lorsque `bits` divise 64 —
/// et là elles produisent les mêmes octets, donc l'ambiguïté est sans effet.
fn depack(data: &[i64], bits: usize, count: usize) -> Vec<usize> {
    let par_long = 64 / bits;
    let sans = count.div_ceil(par_long);
    let avec = (count * bits).div_ceil(64);
    let mask = (1u64 << bits) - 1;
    let mut out = Vec::with_capacity(count);

    if data.len() == sans {
        // sans chevauchement
        'a: for &w in data {
            let w = w as u64;
            for k in 0..par_long {
                if out.len() >= count {
                    break 'a;
                }
                out.push(((w >> (k * bits)) & mask) as usize);
            }
        }
    } else if data.len() == avec {
        // avec chevauchement
        for n in 0..count {
            let off = n * bits;
            let li = off / 64;
            let b = off % 64;
            let Some(&low) = data.get(li) else { break };
            let low = low as u64;
            let v = if b + bits <= 64 {
                (low >> b) & mask
            } else {
                let high = data.get(li + 1).map(|&h| h as u64).unwrap_or(0);
                ((low >> b) | (high << (64 - b))) & mask
            };
            out.push(v as usize);
        }
    } else {
        panic!(
            "décodeur gelé : {} longs ne correspond ni à {sans} (sans chevauchement) \
             ni à {avec} (avec) pour {bits} bits",
            data.len()
        );
    }
    out.resize(count, 0);
    out
}

/// Les 4096 états d'une section, dans l'ordre YZX.
///
/// Le dépack est réécrit ici depuis la spec : longs BIG-endian,
/// `bits = max(4, ceil(log2(len)))`, aucun chevauchement entre deux longs.
pub fn section_states(section: &Tag) -> Option<Vec<String>> {
    // Les deux dispositions : 1.18+ met tout dans un compound `block_states`,
    // 1.13–1.17 pose `Palette` et `BlockStates` en champs frères.
    let (palette_tag, data_tag) = match section.get("block_states") {
        Some(bs) => (bs.get("palette")?, bs.get("data")),
        None => (section.get("Palette")?, section.get("BlockStates")),
    };
    let palette: Vec<String> = palette_tag.as_list()?.iter().map(key_of).collect();
    if palette.is_empty() {
        return None;
    }
    if palette.len() == 1 {
        return Some(vec![palette[0].clone(); 4096]);
    }
    let data = match data_tag.and_then(|t| t.as_longs()) {
        Some(d) if !d.is_empty() => d,
        _ => return Some(vec![palette[0].clone(); 4096]),
    };

    let mut bits = 4usize;
    while (1usize << bits) < palette.len() {
        bits += 1;
    }
    Some(
        depack(data, bits, 4096)
            .into_iter()
            .map(|i| palette.get(i).cloned().unwrap_or_else(|| "?".into()))
            .collect(),
    )
}

/// Tous les états d'un chunk, par `Y` de section.
pub fn chunk_states(chunk: &FrozenChunk) -> BTreeMap<i8, Vec<String>> {
    let mut out = BTreeMap::new();
    let liste = chunk
        .root
        .get("sections")
        .and_then(|t| t.as_list())
        .or_else(|| {
            chunk
                .root
                .get("Level")
                .and_then(|l| l.get("Sections"))
                .and_then(|t| t.as_list())
        });
    if let Some(list) = liste {
        for s in list {
            let y = s.get("Y").and_then(|t| t.as_i8()).unwrap_or(0);
            if let Some(states) = section_states(s) {
                out.insert(y, states);
            }
        }
    }
    out
}

/// Compte les occurrences de chaque état dans toute la région.
pub fn census(buf: &[u8]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for chunk in decode_region(buf).values() {
        for states in chunk_states(chunk).values() {
            for s in states {
                *out.entry(s.clone()).or_insert(0) += 1;
            }
        }
    }
    out
}
