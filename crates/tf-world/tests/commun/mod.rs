//! Ce que plusieurs fichiers de test partagent.

#![allow(dead_code)]

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use tf_anvil::{deflate_level, inflate, read, write, Compression, RawChunk, Region};

/// **Une VRAIE région** : un chunk, dont la charge de `taille` octets
/// commence par `marque`.
///
/// Pas des octets quelconques : la copie de travail juge une région « à
/// jour » à son CONTENU, chunk par chunk — et des octets quelconques se lisent
/// comme une région VIDE. Deux fixtures « différentes » y étaient identiques.
///
/// La charge est INCOMPRESSIBLE : la taille du fichier suit donc `taille`,
/// et un test qui compare des tailles de région compare encore quelque chose.
pub fn region_marquee(marque: u8, taille: usize) -> Vec<u8> {
    region_marquee_niveau(marque, taille, 6)
}

/// [`region_marquee`], compressée à un niveau choisi : même CONTENU, autres
/// octets — ce que fait une annulation qui recompresse un chunk.
pub fn region_marquee_niveau(marque: u8, taille: usize, niveau: u32) -> Vec<u8> {
    let mut x = (marque as u64) << 32 | taille as u64;
    let mut charge: Vec<u8> = (0..taille.max(1))
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (x >> 56) as u8
        })
        .collect();
    charge[0] = marque;
    let mut r = Region::vide(0, 0);
    r.slots[0] = Some(RawChunk {
        index: 0,
        timestamp: 0,
        compression: Compression::Zlib,
        payload: Cow::Owned(deflate_level(&charge, Compression::Zlib, niveau).unwrap()),
        external: false,
    });
    write(&r).unwrap().region
}

/// **Une région dont le chunk (5, 5) est DÉPORTÉ** dans `c.5.5.mcc` : rend la
/// région et le contenu du `.mcc`. Une charge déportée ne compte dans le
/// contenu d'une région que si un chunk y RENVOIE — une charge orpheline n'y
/// change rien, et un test qui n'en a que d'orphelines ne teste rien.
pub fn region_deportee(graine: u64) -> (Vec<u8>, Vec<u8>) {
    let mut x = graine;
    let grande: Vec<u8> = (0..1_100_000)
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (x >> 56) as u8
        })
        .collect();
    let mut r = Region::vide(0, 0);
    r.slots[5 + 5 * 32] = Some(RawChunk {
        index: 5 + 5 * 32,
        timestamp: 0,
        compression: Compression::Zlib,
        payload: Cow::Owned(deflate_level(&grande, Compression::Zlib, 1).unwrap()),
        external: false,
    });
    let out = write(&r).unwrap();
    assert_eq!(out.external.len(), 1, "le chunk doit partir en `.mcc`");
    assert_eq!(out.external[0].name, "c.5.5.mcc");
    (out.region, out.external[0].bytes.clone())
}

/// La marque d'une région de [`region_marquee`].
pub fn marque(octets: &[u8]) -> u8 {
    let r = read(octets, 0, 0).unwrap();
    let c = r.slots[0].as_ref().expect("la fixture porte un chunk");
    inflate(&c.payload, c.compression).unwrap()[0]
}

/// Un dossier temporaire, sans dépendance, effacé en partant.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(etiquette: &str) -> Self {
        // Identifiant unique sans dépendance : l'horloge et l'identifiant du
        // fil. Deux tests parallèles ne doivent pas se marcher dessus.
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "tf-{etiquette}-{n}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Un sous-dossier, créé.
    pub fn sous(&self, nom: &str) -> PathBuf {
        let p = self.0.join(nom);
        fs::create_dir_all(&p).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
