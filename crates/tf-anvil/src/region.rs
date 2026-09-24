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
/// Au-delà, sa charge part dans un fichier `c.X.Z.mcc` à côté de la région.
pub const MAX_SECTORS: usize = 255;

/// L'offset d'un chunk tient sur 3 octets, en secteurs.
pub const MAX_SECTOR_OFFSET: u32 = 0x00FF_FFFF;

/// Bit de l'octet de compression qui signale une charge DÉPORTÉE.
///
/// Minecraft range alors la charge dans `c.<chunkX>.<chunkZ>.mcc`, et ne
/// laisse dans le `.mca` qu'un talon : longueur 1, c'est-à-dire l'octet de
/// compression et rien derrière.
pub const EXTERNAL_FLAG: u8 = 0x80;

/// Nom du fichier de charge déportée d'un chunk, en coordonnées MONDE.
pub fn external_file_name(chunk_x: i32, chunk_z: i32) -> String {
    format!("c.{chunk_x}.{chunk_z}.mcc")
}

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
    /// Vraie si la charge vit dans un `.mcc` à côté du fichier de région.
    ///
    /// À la LECTURE, `payload` est alors VIDE : ce crate ne touche pas au
    /// disque, c'est à l'appelant d'aller chercher le fichier et de la
    /// remplir. Un chunk déporté dont on oublierait de résoudre la charge
    /// serait vu comme un chunk vide — d'où `needs_external`, qui le dit.
    pub external: bool,
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

    /// Vraie pour un chunk déporté dont la charge n'a pas encore été fournie.
    /// Le décoder en l'état donnerait un chunk vide, sans erreur.
    pub fn needs_external(&self) -> bool {
        self.external && self.payload.is_empty()
    }

    /// Fournit la charge d'un chunk déporté, lue depuis son `.mcc`.
    pub fn resolve_external(&mut self, bytes: Vec<u8>) {
        self.payload = Cow::Owned(bytes);
    }
}

/// Un fichier de charge déportée que l'appelant doit écrire à côté du `.mca`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFile {
    /// `c.<chunkX>.<chunkZ>.mcc`, en coordonnées MONDE.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Ce que produit une écriture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutput {
    pub region: Vec<u8>,
    /// Charges déportées à écrire à côté du `.mca`.
    pub external: Vec<ExternalFile>,
    /// Fichiers `.mcc` devenus inutiles : le chunk tient de nouveau en ligne.
    ///
    /// Les laisser ne casserait rien pour le jeu — il ne lit un `.mcc` que si
    /// le talon le désigne — mais ils occuperaient le disque pour toujours, et
    /// une save qui grossit sans raison finit par être signalée comme un bug.
    pub removed_external: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Region<'a> {
    pub region_x: i32,
    pub region_z: i32,
    /// 1024 emplacements, indexés par `localX + localZ * 32`.
    pub slots: Vec<Option<RawChunk<'a>>>,
    /// Emplacements qui annonçaient un chunk qu'on n'a pas pu repérer —
    /// offset hors du fichier, longueur incohérente. Laissés vides pour
    /// sauver les autres, et COMPTÉS : sans ce nombre, une région corrompue
    /// se lisait exactement comme une région vide.
    pub illisibles: usize,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReadError {
    /// Fichier plus court que l'en-tête de 8 Kio.
    TooShort,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WriteError {
    /// La région dépasse ce qu'un offset de 3 octets peut désigner : 16 777 215
    /// secteurs, soit 64 Gio. Physiquement impossible avec des chunks qui
    /// partent en `.mcc` au-delà de 1 Mio, mais l'en-tête ne peut pas le dire,
    /// et un offset tronqué désignerait un autre chunk.
    RegionTooLarge { sectors: u32 },
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::RegionTooLarge { sectors } => write!(
                f,
                "la région occuperait {sectors} secteurs, au-delà des {MAX_SECTOR_OFFSET} \
                 qu'un offset de 3 octets peut désigner"
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
    let mut illisibles = 0;

    for i in 0..CHUNKS {
        let loc = u32::from_be_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        let sector_off = (loc >> 8) as usize;
        let sector_cnt = (loc & 0xff) as usize;
        if sector_off == 0 || sector_cnt == 0 {
            continue; // chunk absent — le cas normal pour une région de bordure
        }
        let Some(off) = sector_off.checked_mul(SECTOR) else {
            illisibles += 1;
            continue;
        };
        if off + 5 > buf.len() {
            illisibles += 1;
            continue;
        }
        let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
        // `len` compte l'octet de compression : 1 signifie « charge vide »,
        // 0 est incohérent.
        if len == 0 || off + 4 + len > buf.len() {
            illisibles += 1;
            continue;
        }
        let timestamp = u32::from_be_bytes(
            buf[CHUNKS * 4 + i * 4..CHUNKS * 4 + i * 4 + 4]
                .try_into()
                .unwrap(),
        );

        // Le bit de poids fort de l'octet de compression signale une charge
        // DÉPORTÉE. Le masquer est obligatoire : sans ça, `2 | 0x80` = 130 est
        // lu comme une compression inconnue, et le chunk devient illisible.
        let comp_byte = buf[off + 4];
        let external = comp_byte & EXTERNAL_FLAG != 0;
        slots[i] = Some(RawChunk {
            index: i as u16,
            timestamp,
            compression: Compression::from_byte(comp_byte & !EXTERNAL_FLAG),
            // Un talon ne porte rien : la charge est dans le `.mcc`.
            payload: if external {
                Cow::Borrowed(&[])
            } else {
                Cow::Borrowed(&buf[off + 5..off + 4 + len])
            },
            external,
        });
    }

    Ok(Region {
        region_x,
        region_z,
        slots,
        illisibles,
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
pub fn write(region: &Region<'_>) -> Result<WriteOutput, WriteError> {
    let mut body: Vec<u8> = Vec::new();
    let mut locations = vec![0u8; CHUNKS * 4];
    let mut timestamps = vec![0u8; CHUNKS * 4];
    let mut external: Vec<ExternalFile> = Vec::new();
    let mut removed_external: Vec<String> = Vec::new();
    let mut next_sector = (HEADER / SECTOR) as u32; // 2

    for i in 0..CHUNKS {
        let Some(chunk) = region.slots.get(i).and_then(|s| s.as_ref()) else {
            continue;
        };
        let (cx, cz) = region.chunk_coords(chunk);

        let inline_len = chunk.payload.len() + 1; // + l'octet de compression
        let deporte = (4 + inline_len).div_ceil(SECTOR) > MAX_SECTORS;

        let (len, comp_byte) = if deporte {
            // Talon : longueur 1, donc l'octet de compression et rien derrière.
            external.push(ExternalFile {
                name: external_file_name(cx, cz),
                bytes: chunk.payload.to_vec(),
            });
            (1usize, chunk.compression.to_byte() | EXTERNAL_FLAG)
        } else {
            if chunk.external {
                // Il tenait dans un `.mcc` et tient de nouveau en ligne : son
                // ancien fichier n'a plus de raison d'être.
                removed_external.push(external_file_name(cx, cz));
            }
            (inline_len, chunk.compression.to_byte())
        };

        let total = 4 + len;
        let sectors = total.div_ceil(SECTOR);
        body.extend_from_slice(&(len as u32).to_be_bytes());
        body.push(comp_byte);
        if !deporte {
            body.extend_from_slice(&chunk.payload);
        }
        body.resize(body.len() + (sectors * SECTOR - total), 0);

        if next_sector > MAX_SECTOR_OFFSET {
            return Err(WriteError::RegionTooLarge {
                sectors: next_sector,
            });
        }
        let loc = (next_sector << 8) | (sectors as u32);
        locations[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
        timestamps[i * 4..i * 4 + 4].copy_from_slice(&chunk.timestamp.to_be_bytes());
        next_sector += sectors as u32;
    }

    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(&locations);
    out.extend_from_slice(&timestamps);
    out.extend_from_slice(&body);
    Ok(WriteOutput {
        region: out,
        external,
        removed_external,
    })
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
