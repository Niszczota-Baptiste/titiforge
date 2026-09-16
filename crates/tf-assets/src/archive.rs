//! Lire un `.jar` ou un `.zip` — le format dans lequel Mojang livre ses
//! assets.
//!
//! Le codex extrait du site est un dossier ; l'installation d'un utilisateur,
//! non. Un launcher range ses assets dans `versions/<v>/<v>.jar`, et les packs
//! du serveur dans `resourcepacks/*.zip`. Sans lecteur d'archive, titiforge ne
//! peut lire que ce qu'on lui a préparé — et on ne peut rien préparer chez
//! quelqu'un d'autre.
//!
//! **Un ZIP vient du disque d'un utilisateur**, et il est lu avec la même
//! méfiance que le NBT : toute longueur est vérifiée contre la taille réelle du
//! fichier avant de servir à réserver quoi que ce soit, tout nom d'entrée passe
//! par la même garde que les chemins de modèle, et la décompression est
//! plafonnée — une bombe zip annonce un ratio délirant et fait allouer jusqu'à
//! la mort du processus.
//!
//! On n'implémente que ce que Mojang produit : stocké et dégonflé, sans
//! chiffrement, sans zip64. Le reste est REFUSÉ en le nommant, jamais deviné.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use crate::source::{Source, SourceError};

/// Plafond de décompression d'une entrée. Une texture fait quelques kilo-octets,
/// un `blockstates.json` quelques méga.
const MAX_INFLATE: usize = 64 * 1024 * 1024;

/// Où vit une entrée dans l'archive, et comment elle est rangée.
#[derive(Debug, Clone, Copy)]
struct Entree {
    /// Décalage de l'en-tête LOCAL, pas des données : sa longueur de nom et
    /// son champ supplémentaire peuvent différer de ceux du répertoire
    /// central, et s'y fier décale la lecture de quelques octets — ce qui
    /// donne une charge illisible et aucune explication.
    entete: usize,
    methode: u16,
    compresse: usize,
    decompresse: usize,
}

/// Une archive lue en mémoire.
///
/// Le fichier entier est chargé : un `.jar` de version fait une vingtaine de
/// méga-octets, et le garder évite de rouvrir le fichier pour chacune des
/// 2 207 textures qu'un pack cite.
pub struct Archive {
    octets: Vec<u8>,
    entrees: HashMap<String, Entree>,
    nom: String,
}

fn u16_le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

impl Archive {
    pub fn ouvrir(chemin: impl AsRef<Path>) -> Result<Self, SourceError> {
        let chemin = chemin.as_ref();
        let octets = std::fs::read(chemin).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => SourceError::Absent(chemin.display().to_string()),
            _ => SourceError::Io(e.to_string()),
        })?;
        let nom = chemin
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
            .to_string();
        let entrees = repertoire(&octets)
            .ok_or_else(|| SourceError::Io(format!("{nom} : archive illisible")))?;
        Ok(Archive {
            octets,
            entrees,
            nom,
        })
    }

    pub fn len(&self) -> usize {
        self.entrees.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entrees.is_empty()
    }

    /// Les noms d'entrée, triés. Sert à découvrir ce qu'un pack contient.
    pub fn noms(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entrees.keys().map(|s| s.as_str()).collect();
        v.sort_unstable();
        v
    }
}

/// Le répertoire central, la seule table de vérité d'un ZIP.
///
/// Les en-têtes locaux ne suffisent pas : ils peuvent annoncer des tailles
/// nulles et renvoyer à un descripteur placé APRÈS les données, ce qu'on ne
/// saurait pas trouver sans le répertoire.
fn repertoire(b: &[u8]) -> Option<HashMap<String, Entree>> {
    // La fin du répertoire central est à la fin du fichier, derrière un
    // commentaire d'au plus 65 535 octets. On remonte, ce que fait tout
    // lecteur de ZIP — il n'y a pas de position fixe.
    const EOCD: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];
    let debut = b.len().saturating_sub(22 + 65_535);
    let fin = b.len().checked_sub(22)?;
    let mut eocd = None;
    for i in (debut..=fin).rev() {
        if b[i..i + 4] == EOCD {
            eocd = Some(i);
            break;
        }
    }
    let e = eocd?;
    let nombre = u16_le(b, e + 10)? as usize;
    let mut p = u32_le(b, e + 16)? as usize;

    let mut out = HashMap::with_capacity(nombre);
    const CEN: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];
    for _ in 0..nombre {
        if b.get(p..p + 4)? != CEN {
            return None;
        }
        let methode = u16_le(b, p + 10)?;
        let compresse = u32_le(b, p + 20)? as usize;
        let decompresse = u32_le(b, p + 24)? as usize;
        let n = u16_le(b, p + 28)? as usize;
        let extra = u16_le(b, p + 30)? as usize;
        let commentaire = u16_le(b, p + 32)? as usize;
        let entete = u32_le(b, p + 42)? as usize;
        let nom = std::str::from_utf8(b.get(p + 46..p + 46 + n)?).ok()?;
        // Une taille annoncée plus grande que le fichier est une archive
        // forgée ou tronquée. On le voit AVANT de réserver.
        if entete >= b.len() || compresse > b.len() {
            return None;
        }
        // Un dossier n'a pas de contenu, et son nom finit par `/`.
        if !nom.ends_with('/') {
            out.insert(
                nom.to_string(),
                Entree {
                    entete,
                    methode,
                    compresse,
                    decompresse,
                },
            );
        }
        p = p + 46 + n + extra + commentaire;
    }
    Some(out)
}

impl Source for Archive {
    fn lire(&self, chemin: &str) -> Result<Vec<u8>, SourceError> {
        let Some(e) = self.entrees.get(chemin) else {
            return Err(SourceError::Absent(chemin.to_string()));
        };
        let b = &self.octets;
        // L'en-tête LOCAL porte ses propres longueurs de nom et de champ
        // supplémentaire : celles du répertoire central ne valent que pour le
        // répertoire. S'y fier décale la lecture de quelques octets.
        const LOC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
        let illisible = || SourceError::Io(format!("{} : entrée {chemin} illisible", self.nom));
        if b.get(e.entete..e.entete + 4) != Some(&LOC[..]) {
            return Err(illisible());
        }
        let n = u16_le(b, e.entete + 26).ok_or_else(illisible)? as usize;
        let extra = u16_le(b, e.entete + 28).ok_or_else(illisible)? as usize;
        let debut = e.entete + 30 + n + extra;
        let brut = b.get(debut..debut + e.compresse).ok_or_else(illisible)?;

        match e.methode {
            0 => Ok(brut.to_vec()),
            8 => {
                if e.decompresse > MAX_INFLATE {
                    return Err(SourceError::Io(format!(
                        "{} : entrée {chemin} annonce {} octets décompressés, \
                         au-delà du plafond de {MAX_INFLATE}",
                        self.nom, e.decompresse
                    )));
                }
                // Le plafond est appliqué à la LECTURE et pas seulement à la
                // taille annoncée : une bombe zip ment sur les deux.
                let mut out = Vec::with_capacity(e.decompresse.min(1 << 20));
                flate2::read::DeflateDecoder::new(brut)
                    .take(MAX_INFLATE as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|_| illisible())?;
                if out.len() > MAX_INFLATE {
                    return Err(illisible());
                }
                Ok(out)
            }
            // Chiffrement, bzip2, lzma, zstd : on ne devine pas, on nomme.
            m => Err(SourceError::Io(format!(
                "{} : entrée {chemin} compressée en méthode {m}, non prise en charge",
                self.nom
            ))),
        }
    }

    fn nom(&self) -> &str {
        &self.nom
    }

    fn lister(&self, prefixe: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .entrees
            .keys()
            .filter(|n| n.starts_with(prefixe))
            .cloned()
            .collect();
        v.sort();
        v
    }
}
