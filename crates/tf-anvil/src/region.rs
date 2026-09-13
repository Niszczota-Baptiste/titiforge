//! Lecture et écriture d'un fichier de région Anvil (`.mca`).
//!
//! En-tête de 8 Kio : 1024 entrées « localisation » (3 octets d'offset en
//! secteurs + 1 octet de nombre de secteurs), puis 1024 horodatages. Secteurs
//! de 4096 octets. Chaque chunk : longueur (4 o BE), compression (1 o), puis
//! le NBT compressé.
//!
//! **Principe non destructif** : à la lecture on garde la charge compressée
//! brute de chaque chunk. À l'écriture, un chunk non modifié est réémis
//! **octet pour octet** — on recopie sa charge. Seuls les chunks explicitement
//! remplacés sont recompressés.

use std::borrow::Cow;

pub const SECTOR: usize = 4096;
pub const CHUNKS: usize = 1024;
pub const HEADER: usize = 2 * CHUNKS * 4;

/// Un chunk ne peut occuper que 255 secteurs : le compte tient sur un octet.
pub const MAX_SECTORS: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Zlib,
    None,
    /// Compression inconnue : on garde la charge telle quelle et on refuse de
    /// la décoder, plutôt que de deviner.
    Other(u8),
}

impl Compression {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Compression::Gzip,
            2 => Compression::Zlib,
            3 => Compression::None,
            other => Compression::Other(other),
        }
    }
    pub fn to_byte(self) -> u8 {
        match self {
            Compression::Gzip => 1,
            Compression::Zlib => 2,
            Compression::None => 3,
            Compression::Other(b) => b,
        }
    }
}

/// La charge compressée d'un chunk : empruntée au fichier d'origine tant
/// qu'elle n'a pas changé, possédée dès qu'elle a été réécrite.
///
/// C'est ce type qui porte l'invariant de non-destruction : tant qu'on voit un
/// `Cow::Borrowed`, les octets sont EXACTEMENT ceux du disque.
pub type Payload<'a> = Cow<'a, [u8]>;

#[derive(Debug, Clone)]
pub struct RawChunk<'a> {
    /// `localX + localZ * 32`, avec localX/localZ ∈ [0, 31].
    pub index: u16,
    pub timestamp: u32,
    pub compression: Compression,
    pub payload: Payload<'a>,
}

impl RawChunk<'_> {
    pub fn local_x(&self) -> i32 {
        (self.index % 32) as i32
    }
    pub fn local_z(&self) -> i32 {
        (self.index / 32) as i32
    }
    /// Vraie tant que la charge est celle du disque, octet pour octet.
    pub fn is_pristine(&self) -> bool {
        matches!(self.payload, Cow::Borrowed(_))
    }
}

#[derive(Debug, Clone)]
pub struct Region<'a> {
    pub region_x: i32,
    pub region_z: i32,
    /// 1024 emplacements, indexés par `localX + localZ * 32`.
    pub slots: Vec<Option<RawChunk<'a>>>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReadError {
    /// Fichier plus court que l'en-tête de 8 Kio.
    TooShort,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WriteError {
    /// Un chunk dépasse 255 secteurs (≈ 1 Mio compressé). Minecraft le range
    /// alors dans un fichier externe `c.X.Z.mcc` — pas encore géré. On refuse
    /// plutôt que d'écrire un en-tête tronqué qui rendrait le chunk illisible
    /// pour le jeu ET pour nous.
    ChunkTooLarge { index: u16, sectors: usize },
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::ChunkTooLarge { index, sectors } => write!(
                f,
                "le chunk {index} demande {sectors} secteurs (maximum {MAX_SECTORS}) : \
                 les chunks surdimensionnés (.mcc) ne sont pas encore gérés"
            ),
        }
    }
}

impl std::error::Error for WriteError {}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::TooShort => write!(f, "fichier plus court que l'en-tête de 8 Kio"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Lit un `.mca`. Aucune décompression : on ne fait que repérer les charges.
///
/// Un emplacement illisible (offset hors fichier, longueur incohérente) est
/// laissé vide plutôt que de faire échouer toute la région : une région
/// partiellement corrompue reste en grande partie récupérable, et refuser de
/// l'ouvrir ferait perdre les 1023 autres chunks.
pub fn read<'a>(buf: &'a [u8], region_x: i32, region_z: i32) -> Result<Region<'a>, ReadError> {
    if buf.len() < HEADER {
        return Err(ReadError::TooShort);
    }
    let mut slots: Vec<Option<RawChunk<'a>>> = vec![None; CHUNKS];

    for i in 0..CHUNKS {
        let loc = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        let sector_off = (loc >> 8) as usize;
        let sector_cnt = (loc & 0xff) as usize;
        if sector_off == 0 || sector_cnt == 0 {
            continue; // chunk absent — le cas normal pour une région de bordure
        }
        let Some(off) = sector_off.checked_mul(SECTOR) else {
            continue;
        };
        if off + 5 > buf.len() {
            continue;
        }
        let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        // `len` compte l'octet de compression : 1 signifie « charge vide »,
        // 0 est incohérent.
        if len == 0 || off + 4 + len > buf.len() {
            continue;
        }
        let timestamp = u32::from_be_bytes(
            buf[CHUNKS * 4 + i * 4..CHUNKS * 4 + i * 4 + 4]
                .try_into()
                .unwrap(),
        );

        slots[i] = Some(RawChunk {
            index: i as u16,
            timestamp,
            compression: Compression::from_byte(buf[off + 4]),
            payload: Cow::Borrowed(&buf[off + 5..off + 4 + len]),
        });
    }

    Ok(Region {
        region_x,
        region_z,
        slots,
    })
}

/// Réécrit un `.mca`.
///
/// Les chunks sont posés dans l'ordre de leur index, à partir du secteur 2.
/// C'est la disposition la plus simple et elle est **idempotente** : réécrire
/// une région qu'on vient d'écrire rend exactement les mêmes octets. Un
/// fichier venu du jeu peut être fragmenté différemment ; ce qui est garanti
/// dans tous les cas, c'est que la CHARGE de chaque chunk non modifié est
/// recopiée octet pour octet.
pub fn write(region: &Region<'_>) -> Result<Vec<u8>, WriteError> {
    let mut body: Vec<u8> = Vec::new();
    let mut locations = vec![0u8; CHUNKS * 4];
    let mut timestamps = vec![0u8; CHUNKS * 4];
    let mut next_sector = (HEADER / SECTOR) as u32; // 2

    for i in 0..CHUNKS {
        let Some(chunk) = region.slots.get(i).and_then(|s| s.as_ref()) else {
            continue;
        };

        let len = chunk.payload.len() + 1; // + l'octet de compression
        let total = 4 + len;
        let sectors = total.div_ceil(SECTOR);
        if sectors > MAX_SECTORS {
            return Err(WriteError::ChunkTooLarge {
                index: i as u16,
                sectors,
            });
        }

        body.extend_from_slice(&(len as u32).to_be_bytes());
        body.push(chunk.compression.to_byte());
        body.extend_from_slice(&chunk.payload);
        body.resize(body.len() + (sectors * SECTOR - total), 0);

        let loc = (next_sector << 8) | (sectors as u32);
        locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
        timestamps[i * 4..i * 4 + 4].copy_from_slice(&chunk.timestamp.to_be_bytes());
        next_sector += sectors as u32;
    }

    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    Ok(out)
}

impl<'a> Region<'a> {
    pub fn get(&self, local_x: i32, local_z: i32) -> Option<&RawChunk<'a>> {
        if !(0..32).contains(&local_x) || !(0..32).contains(&local_z) {
            return None;
        }
        self.slots[(local_x + local_z * 32) as usize].as_ref()
    }

    pub fn get_mut(&mut self, local_x: i32, local_z: i32) -> Option<&mut RawChunk<'a>> {
        if !(0..32).contains(&local_x) || !(0..32).contains(&local_z) {
            return None;
        }
        self.slots[(local_x + local_z * 32) as usize].as_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = &RawChunk<'a>> {
        self.slots.iter().filter_map(|s| s.as_ref())
    }

    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Coordonnées MONDE d'un chunk, déduites de la région et de l'index.
    pub fn chunk_coords(&self, chunk: &RawChunk<'_>) -> (i32, i32) {
        (
            self.region_x * 32 + chunk.local_x(),
            self.region_z * 32 + chunk.local_z(),
        )
    }
}

/// Nom de fichier conventionnel d'une région.
pub fn region_file_name(region_x: i32, region_z: i32) -> String {
    format!("r.{region_x}.{region_z}.mca")
}

/// Coordonnées de région lues dans un NOM de fichier.
/// Accepte les deux conventions rencontrées : `r.X.Z.mca` et `r_X_Z.mca`.
pub fn region_coords_from_name(name: &str) -> Option<(i32, i32)> {
    let base = name.rsplit(['/', '\\']).next()?;
    let stem = base.strip_suffix(".mca")?;
    let rest = stem
        .strip_prefix("r.")
        .or_else(|| stem.strip_prefix("r_"))?;
    let sep = if stem.starts_with("r.") { '.' } else { '_' };
    let (a, b) = rest.split_once(sep)?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Division PLANCHER. Le bloc −1 est dans la région −1, pas la région 0 :
/// une division entière naïve charge la mauvaise moitié du monde sans rien
/// signaler.
#[inline]
pub fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// Coordonnées de région d'un chunk.
#[inline]
pub fn region_of_chunk(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (floor_div(chunk_x, 32), floor_div(chunk_z, 32))
}

/// Coordonnées de chunk d'un bloc.
#[inline]
pub fn chunk_of_block(x: i32, z: i32) -> (i32, i32) {
    (floor_div(x, 16), floor_div(z, 16))
}
