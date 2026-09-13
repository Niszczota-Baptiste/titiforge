//! Repérage ciblé dans un chunk inflaté, et réécriture **par splice**.
//!
//! Le round-trip lossless de `we-engine` repose sur une promesse : un chunk
//! non modifié est réémis octet pour octet, parce qu'on garde sa charge
//! compressée brute. Mais dès qu'un chunk est modifié, il ré-encode tout son
//! arbre NBT — donc tout ce que le parseur a mal compris est perdu.
//!
//! Ici, même un chunk MODIFIÉ ne se ré-encode pas. On repère la plage d'octets
//! de chaque `block_states`, on remplace celle des sections qu'on a touchées,
//! et tout le reste du chunk est recopié tel quel. Heightmaps, structures,
//! block entities, données de mods inconnues : le lecteur n'y touche pas,
//! **donc il ne peut pas les abîmer**. C'est une propriété structurelle, pas
//! une précaution.

use tf_nbt::{tag, Cur, Span, Trunc, R};

use crate::section::{bits_for, Section};
use crate::state::{split_key, state_key, Interner, StateId};

/// Ce qu'un balayage a repéré, sans rien matérialiser.
#[derive(Debug, Clone, Default)]
pub struct ChunkScan {
    pub data_version: i32,
    /// Coordonnées MONDE du chunk, telles qu'écrites dans son contenu.
    ///
    /// Un nom de fichier est une métadonnée qui peut mentir — Windows renomme
    /// un téléchargement en double `r.0.0 (16).mca`. Le contenu, lui, ne ment
    /// pas.
    pub x_pos: Option<i32>,
    pub z_pos: Option<i32>,
    pub sections: Vec<ScannedSection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScannedSection {
    pub y: i8,
    /// Plage de la CHARGE du compound `block_states` — sans son octet de type
    /// ni son nom, puisque c'est elle qu'on remplacera.
    ///
    /// `None` pour une section sans `block_states` : elles existent (sections
    /// purement d'éclairage aux bords du monde) et il ne faut ni les décoder
    /// ni les réécrire.
    pub states: Option<Span>,
}

/// Balaye un chunk inflaté. Ne descend que dans `sections[].block_states` ;
/// tout le reste est enjambé sans être matérialisé.
pub fn scan(inflated: &[u8]) -> R<ChunkScan> {
    let mut c = Cur::new(inflated);
    c.enter_root()?;
    let mut out = ChunkScan::default();

    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::INT, "DataVersion") => out.data_version = c.i32()?,
            (tag::INT, "xPos") => out.x_pos = Some(c.i32()?),
            (tag::INT, "zPos") => out.z_pos = Some(c.i32()?),
            (tag::LIST, "sections") => {
                let (et, n) = c.list_header()?;
                if et != tag::COMPOUND {
                    // Une liste vide s'écrit avec un type END : ce n'est pas
                    // une erreur, c'est un chunk sans sections.
                    if et == tag::END {
                        continue;
                    }
                    return Err(Trunc);
                }
                out.sections.reserve(n);
                for _ in 0..n {
                    out.sections.push(scan_section(&mut c)?);
                }
            }
            _ => c.skip_payload(t)?,
        }
    }
    Ok(out)
}

fn scan_section(c: &mut Cur) -> R<ScannedSection> {
    let mut y: i8 = 0;
    let mut states = None;
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::BYTE, "Y") => y = c.i8()?,
            (tag::COMPOUND, "block_states") => states = Some(c.span_of_payload(t)?),
            _ => c.skip_payload(t)?,
        }
    }
    Ok(ScannedSection { y, states })
}

/// Matérialise une section repérée. C'est le seul endroit qui alloue.
pub fn decode_section(
    inflated: &[u8],
    scanned: &ScannedSection,
    interner: &mut Interner,
) -> R<Option<Section>> {
    let Some(span) = scanned.states else {
        return Ok(None);
    };
    let mut c = Cur::at(inflated, span.start);
    let mut palette: Vec<StateId> = Vec::new();
    let mut data: Vec<u64> = Vec::new();

    while let Some((t, key)) = c.next_field()? {
        if c.pos() > span.end {
            return Err(Trunc);
        }
        match (t, key) {
            (tag::LIST, "palette") => {
                let (et, n) = c.list_header()?;
                if et == tag::END {
                    continue;
                }
                if et != tag::COMPOUND {
                    return Err(Trunc);
                }
                palette.reserve(n);
                for _ in 0..n {
                    palette.push(read_palette_entry(&mut c, interner)?);
                }
            }
            (tag::LONG_ARRAY, "data") => data = c.long_array()?,
            _ => c.skip_payload(t)?,
        }
    }

    if palette.is_empty() {
        return Ok(None); // section sans palette : rien à éditer
    }
    let bits = bits_for(palette.len());
    // Une palette d'une entrée n'a pas de `data`, et un `data` présent avec une
    // palette d'une entrée est du bruit qu'on ignore plutôt que de le
    // propager.
    let data = if palette.len() <= 1 { Vec::new() } else { data };
    Ok(Some(Section {
        y: scanned.y,
        palette,
        bits,
        data: data.into_boxed_slice(),
    }))
}

fn read_palette_entry(c: &mut Cur, interner: &mut Interner) -> R<StateId> {
    let mut name = "";
    let mut props: Vec<(String, String)> = Vec::new();
    while let Some((t, key)) = c.next_field()? {
        match (t, key) {
            (tag::STRING, "Name") => name = c.str()?,
            (tag::COMPOUND, "Properties") => {
                while let Some((pt, pk)) = c.next_field()? {
                    if pt == tag::STRING {
                        props.push((pk.to_string(), c.str()?.to_string()));
                    } else {
                        c.skip_payload(pt)?;
                    }
                }
            }
            _ => c.skip_payload(t)?,
        }
    }
    if name.is_empty() {
        return Err(Trunc);
    }
    Ok(interner.intern(&state_key(name, &mut props)))
}

/// Recompose la charge d'un `block_states` depuis une section.
pub fn encode_section(section: &Section, interner: &Interner) -> Option<Vec<u8>> {
    let mut noms: Vec<(String, Vec<(String, String)>)> = Vec::with_capacity(section.palette.len());
    for &id in &section.palette {
        let key = interner.resolve(id)?;
        let (name, props) = split_key(key);
        noms.push((name.to_string(), props));
    }
    let refs: Vec<tf_nbt::PaletteEntryRef> = noms
        .iter()
        .map(|(n, p)| tf_nbt::PaletteEntryRef { name: n, props: p })
        .collect();
    Some(tf_nbt::block_states_payload(&refs, &section.data))
}

/// Remplacement d'une plage d'octets du chunk inflaté.
pub struct Edit {
    pub span: Span,
    pub bytes: Vec<u8>,
}

/// Remplace des plages d'octets et recopie tout le reste **tel quel**.
///
/// Les plages doivent être disjointes. Deux plages qui se chevauchent sont un
/// bug d'appelant, pas une entrée à assainir : on refuse plutôt que d'écrire
/// une sortie plausible mais fausse dans la save de quelqu'un.
pub fn splice(inflated: &[u8], edits: &mut [Edit]) -> Result<Vec<u8>, SpliceError> {
    if edits.is_empty() {
        return Ok(inflated.to_vec());
    }
    edits.sort_by_key(|e| e.span.start);

    let mut prev_end = 0usize;
    for e in edits.iter() {
        if e.span.start < prev_end {
            return Err(SpliceError::Overlap);
        }
        if e.span.end > inflated.len() || e.span.start > e.span.end {
            return Err(SpliceError::OutOfBounds);
        }
        prev_end = e.span.end;
    }

    let grown: usize = edits.iter().map(|e| e.bytes.len()).sum();
    let shrunk: usize = edits.iter().map(|e| e.span.len()).sum();
    let mut out = Vec::with_capacity(inflated.len() + grown - shrunk.min(inflated.len()));

    let mut cursor = 0usize;
    for e in edits.iter() {
        out.extend_from_slice(&inflated[cursor..e.span.start]);
        out.extend_from_slice(&e.bytes);
        cursor = e.span.end;
    }
    out.extend_from_slice(&inflated[cursor..]);
    Ok(out)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpliceError {
    /// Deux plages se chevauchent.
    Overlap,
    /// Une plage sort du tampon.
    OutOfBounds,
}

impl std::fmt::Display for SpliceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpliceError::Overlap => write!(f, "deux plages de réécriture se chevauchent"),
            SpliceError::OutOfBounds => write!(f, "une plage de réécriture sort du chunk"),
        }
    }
}

impl std::error::Error for SpliceError {}
