//! Les formes qu'un chunk a prises au fil des versions.
//!
//! Deux axes indépendants, et **aucun des deux ne se déduit du `DataVersion`**
//! dans ce crate — ils se MESURENT sur le contenu. Une table de versions est
//! une liste de nombres qu'on recopie de mémoire ou d'un wiki ; elle se trompe
//! en silence sur une capture instantanée, sur un monde converti par un outil
//! tiers, ou sur un chunk dont le `DataVersion` est absent. La structure, elle,
//! ne ment pas.
//!
//! `DataVersion` reste lu et conservé — il sert à écrire les schematics et à
//! renseigner l'utilisateur — mais il ne décide de rien.

/// Disposition du chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// 1.18+ : `sections` à la racine, chaque section portant un compound
    /// `block_states { palette, data }` et un `biomes`.
    Flat,
    /// 1.13 – 1.17 : tout sous `Level`, et la section porte deux champs
    /// FRÈRES, `Palette` (liste) et `BlockStates` (tableau de longs).
    Legacy,
}

/// Comment les indices de palette sont rangés dans les longs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// 1.16+ : un indice ne chevauche jamais deux longs. Les bits de poids
    /// fort inutilisés de chaque long sont à zéro.
    NoStraddle,
    /// 1.13 – 1.15 : les indices sont collés bout à bout, un indice peut donc
    /// être à cheval sur deux longs. C'est aussi le packing de Litematica,
    /// aujourd'hui encore.
    Straddle,
}

/// Nombre de longs qu'occupe un volume d'indices.
pub fn longs_for(count: usize, bits: usize, packing: Packing) -> usize {
    match packing {
        Packing::NoStraddle => {
            let per_long = 64 / bits;
            count.div_ceil(per_long)
        }
        Packing::Straddle => (count * bits).div_ceil(64),
    }
}

/// Déduit le packing de la LONGUEUR du tableau.
///
/// Les deux dispositions ne donnent la même longueur que lorsque `bits` divise
/// 64 — c'est-à-dire 4 et 8 — et dans ces cas-là elles produisent exactement
/// les mêmes octets. L'ambiguïté est donc sans conséquence : elle n'existe que
/// là où les deux formats coïncident.
///
/// Mesuré sur les neuf largeurs du format :
///
/// ```text
///   bits :   4    5    6    7    8    9   10   11   12
///   sans : 256  342  410  456  512  586  683  820  820
///   avec : 256  320  384  448  512  576  640  704  768
/// ```
///
/// Rend `None` si la longueur ne correspond à aucune des deux : le tableau est
/// tronqué ou vient d'un format qu'on ne connaît pas. Deviner écrirait des
/// blocs faux dans la save de quelqu'un.
pub fn detect_packing(count: usize, bits: usize, data_len: usize) -> Option<Packing> {
    let sans = longs_for(count, bits, Packing::NoStraddle);
    let avec = longs_for(count, bits, Packing::Straddle);
    // On teste « sans chevauchement » d'abord : c'est la forme moderne, et
    // quand les deux coïncident elles sont interchangeables.
    if data_len == sans {
        Some(Packing::NoStraddle)
    } else if data_len == avec {
        Some(Packing::Straddle)
    } else {
        None
    }
}

/// Dépacke `count` indices dans `out`.
pub fn unpack_into(data: &[u64], count: usize, bits: usize, packing: Packing, out: &mut [u16]) {
    debug_assert!(out.len() >= count);
    debug_assert!((1..=16).contains(&bits), "bits hors du domaine d'un u16");
    out[..count].fill(0);
    let mask = (1u64 << bits) - 1;

    match packing {
        Packing::NoStraddle => {
            let per_long = 64 / bits;
            let mut n = 0usize;
            for &w in data.iter() {
                if n >= count {
                    break;
                }
                let up_to = per_long.min(count - n);
                for k in 0..up_to {
                    out[n] = ((w >> (k * bits)) & mask) as u16;
                    n += 1;
                }
            }
        }
        Packing::Straddle => {
            for (n, slot) in out[..count].iter_mut().enumerate() {
                let off = n * bits;
                let li = off / 64;
                let b = off % 64;
                let Some(&low) = data.get(li) else { break };
                let v = if b + bits <= 64 {
                    (low >> b) & mask
                } else {
                    // L'indice est à cheval : la fin est dans le long suivant.
                    // `<< (64 - b)` avec b = 0 serait un décalage de 64, qui
                    // est un comportement indéfini — mais b = 0 implique
                    // `b + bits <= 64`, donc on n'arrive jamais ici avec b = 0.
                    let high = data.get(li + 1).copied().unwrap_or(0);
                    ((low >> b) | (high << (64 - b))) & mask
                };
                *slot = v as u16;
            }
        }
    }
}

/// Packe `idx` selon la disposition demandée.
pub fn pack(idx: &[u16], bits: usize, packing: Packing) -> Vec<u64> {
    debug_assert!((1..=16).contains(&bits));
    let mut out = vec![0u64; longs_for(idx.len(), bits, packing)];
    match packing {
        Packing::NoStraddle => {
            let per_long = 64 / bits;
            for (n, &v) in idx.iter().enumerate() {
                out[n / per_long] |= (v as u64) << ((n % per_long) * bits);
            }
        }
        Packing::Straddle => {
            for (n, &v) in idx.iter().enumerate() {
                let off = n * bits;
                let li = off / 64;
                let b = off % 64;
                out[li] |= (v as u64) << b;
                if b + bits > 64 {
                    out[li + 1] |= (v as u64) >> (64 - b);
                }
            }
        }
    }
    out
}

// ── DataVersion : conservé, jamais décisionnaire ────────────────────────────

/// Quelques `DataVersion` de publication, pour ÉTIQUETER un monde à l'écran.
///
/// Délibérément absents des chemins de décodage : ce crate détecte la forme
/// d'un chunk sur sa structure, pas sur ce nombre. Une valeur fausse ici
/// donnerait au pire un libellé faux, jamais un bloc faux.
pub const DV_1_13: i32 = 1519;
pub const DV_1_14: i32 = 1952;
pub const DV_1_15: i32 = 2225;
pub const DV_1_16: i32 = 2566;
pub const DV_1_17: i32 = 2724;
pub const DV_1_18: i32 = 2860;
pub const DV_1_19: i32 = 3105;
pub const DV_1_20: i32 = 3463;
pub const DV_1_21: i32 = 3953;

/// `DataVersion` de 20w17a, la capture où le packing a cessé de chevaucher
/// les longs.
///
/// C'est la SEULE constante de version dont un chemin de décodage dépend, et
/// seulement en dernier recours — voir `packing_de_repli`.
pub const DV_SANS_CHEVAUCHEMENT: i32 = 2529;

/// Packing d'un chunk dont la structure ne dit RIEN.
///
/// Le packing se mesure normalement sur la longueur du tableau d'indices. Mais
/// un chunk dont toutes les sections sont homogènes n'a aucun tableau : il n'y
/// a littéralement rien à mesurer. C'est le seul cas où le `DataVersion` sert
/// à décider, et il faut voir exactement ce qui est en jeu.
///
/// Portée de l'erreur possible : elle ne se manifeste que si l'utilisateur
/// rend non homogène une section d'un chunk **entièrement** homogène d'un
/// monde 1.13–1.15. Tout chunk portant ne serait-ce qu'une section à plusieurs
/// blocs passe par la mesure. Un `DataVersion` absent (0) donne le packing
/// moderne, qui est celui de tout monde depuis 2020.
///
/// Si ce seuil devait se révéler faux, le symptôme serait visible : la section
/// réécrite sortirait en damier dans le jeu de l'utilisateur, pas en silence.
pub fn packing_de_repli(data_version: i32) -> Packing {
    if data_version == 0 || data_version >= DV_SANS_CHEVAUCHEMENT {
        Packing::NoStraddle
    } else {
        Packing::Straddle
    }
}

/// Libellé approximatif d'un `DataVersion`. Purement informatif.
pub fn version_label(dv: i32) -> &'static str {
    match dv {
        v if v >= DV_1_21 => "1.21+",
        v if v >= DV_1_20 => "1.20",
        v if v >= DV_1_19 => "1.19",
        v if v >= DV_1_18 => "1.18",
        v if v >= DV_1_17 => "1.17",
        v if v >= DV_1_16 => "1.16",
        v if v >= DV_1_15 => "1.15",
        v if v >= DV_1_14 => "1.14",
        v if v >= DV_1_13 => "1.13",
        0 => "inconnue",
        _ => "antérieure à 1.13",
    }
}
