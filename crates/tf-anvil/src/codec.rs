//! Compression des charges de chunk.

use std::io::Read;

use crate::region::Compression;

/// Plafond de décompression. Sans lui, une charge forgée annonce un ratio
/// délirant et fait allouer jusqu'à la mort du processus — c'est une bombe zip,
/// et un `.mca` vient du disque d'un utilisateur.
pub const MAX_INFLATED: usize = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CodecError {
    /// Charge illisible pour la compression annoncée.
    Corrupt,
    /// Le chunk dépasse le plafond de décompression.
    TooLarge,
    /// Compression que le format ne définit pas : on garde les octets tels
    /// quels plutôt que de deviner.
    Unsupported(u8),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::Corrupt => write!(f, "charge de chunk illisible"),
            CodecError::TooLarge => {
                write!(f, "chunk décompressé au-delà de {MAX_INFLATED} octets")
            }
            CodecError::Unsupported(b) => write!(f, "compression inconnue ({b})"),
        }
    }
}

impl std::error::Error for CodecError {}

pub fn inflate(payload: &[u8], compression: Compression) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let r = match compression {
        Compression::None => return Ok(payload.to_vec()),
        Compression::Other(b) => return Err(CodecError::Unsupported(b)),
        Compression::Gzip => flate2::read::GzDecoder::new(payload)
            .take(MAX_INFLATED as u64 + 1)
            .read_to_end(&mut out),
        Compression::Zlib => flate2::read::ZlibDecoder::new(payload)
            .take(MAX_INFLATED as u64 + 1)
            .read_to_end(&mut out),
    };
    r.map_err(|_| CodecError::Corrupt)?;
    if out.len() > MAX_INFLATED {
        return Err(CodecError::TooLarge);
    }
    Ok(out)
}

/// Recompresse une charge.
///
/// Niveau 6 — le défaut de zlib, et celui que le jeu utilise. Un fichier de
/// région n'est PAS un cache : il part sur le disque de l'utilisateur et y
/// reste. C'est l'inverse du raisonnement qui met l'aperçu en niveau 1.
pub fn deflate(bytes: &[u8], compression: Compression) -> Result<Vec<u8>, CodecError> {
    use std::io::Write;
    match compression {
        Compression::None => Ok(bytes.to_vec()),
        Compression::Other(b) => Err(CodecError::Unsupported(b)),
        Compression::Zlib => {
            let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            e.write_all(bytes).map_err(|_| CodecError::Corrupt)?;
            e.finish().map_err(|_| CodecError::Corrupt)
        }
        Compression::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
            e.write_all(bytes).map_err(|_| CodecError::Corrupt)?;
            e.finish().map_err(|_| CodecError::Corrupt)
        }
    }
}
