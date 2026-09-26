//! `.litematic` — le format de Litematica, versions 4 à 7, tel que
//! `LitematicaSchematic` le lit et l'écrit.
//!
//! ## Ce qu'il a de particulier
//!
//! - **Plusieurs régions**, chacune avec sa position dans le repère du
//!   fichier et une taille qui peut être NÉGATIVE (la région est décrite par
//!   ses deux coins, dans l'ordre où on les a posés). On les fusionne dans la
//!   boîte qui les englobe ; ce qui n'appartient à aucune est de l'air.
//! - **Des indices à cheval sur deux longs** : `max(2, ⌈log₂ n⌉)` bits par
//!   case, sans bourrage — là où le jeu, depuis 1.16, n'en met jamais à cheval.
//!   Confondre les deux décale tout le build d'une case à la première frontière.
//! - **L'air est toujours l'indice 0** : Litematica l'y met en créant sa
//!   palette, et son lecteur de structures le remet à 0 s'il n'y est pas.
//! - **Trois repères** : les block entities sont relatives au coin MINIMAL de
//!   leur région, les entités à sa `Position` (le premier coin posé, pas
//!   forcément le minimal), et le reste de ce qui situe une entité — la case
//!   d'un cadre, le lit d'un villageois — reste dans le repère du MONDE d'où
//!   elle vient.

use tf_anvil::entites::Entite;
use tf_anvil::{Interner, StateId};
use tf_nbt::{tag, Writer};
use tf_ops::Presse;

use crate::commun::{champ_xyz, cle_depuis_compound, ecrire_etat, entete_liste, Compound};
use crate::entites::{self, Repere};
use crate::{
    est_air, ranger_block_entities, verifier_taille, Bilan, Erreur, Lecture, Lu, Meta, Palette,
    Remarque,
};

/// La version qu'écrit Litematica pour Minecraft 1.13 à 1.20.4.
const VERSION: i32 = 6;
/// Et celle des objets à COMPOSANTS (1.20.5 et suivants).
const VERSION_COMPOSANTS: i32 = 7;
/// Le premier `DataVersion` à composants : 1.20.5.
const DV_COMPOSANTS: i32 = 3837;

/// Combien de cases la boîte englobante peut compter au-delà de ce que ses
/// régions décrivent : deux régions minuscules posées à vingt mille blocs
/// l'une de l'autre englobent des centaines de millions de cases d'air, que
/// le presse-papiers devrait allouer — quelques octets de fichier pour des
/// gigaoctets de mémoire. On l'accepte jusqu'à ce facteur, ou jusqu'à 2²⁴
/// cases pour de petits fichiers.
const ENGLOBANTE_PAR_DECRITE: u64 = 64;
const ENGLOBANTE_MIN: u64 = 1 << 24;

/// Combien de bits par indice pour une palette de `n` états.
pub(crate) fn bits_pour(n: usize) -> u32 {
    let log = usize::BITS - n.saturating_sub(1).leading_zeros();
    log.max(2)
}

/// Combien de longs pour `n` indices de `bits` bits — au moins un, comme
/// `LitematicaBitArray`.
pub(crate) fn longs_pour(n: usize, bits: u32) -> usize {
    ((n as u64 * bits as u64).div_ceil(64) as usize).max(1)
}

/// Range des indices À CHEVAL sur les longs, poids faibles d'abord.
pub(crate) fn empaqueter(valeurs: impl Iterator<Item = u32>, bits: u32, n: usize) -> Vec<u64> {
    let mut longs = vec![0u64; longs_pour(n, bits)];
    for (i, v) in valeurs.enumerate() {
        let debut = i as u64 * bits as u64;
        let (k, o) = ((debut >> 6) as usize, (debut & 63) as u32);
        longs[k] |= (v as u64) << o;
        if o + bits > 64 {
            longs[k + 1] |= (v as u64) >> (64 - o);
        }
    }
    longs
}

/// L'indice `i`, lu à cheval. L'appelant a vérifié que les longs suffisent.
#[inline]
pub(crate) fn valeur(longs: &[u64], i: usize, bits: u32) -> u32 {
    let debut = i as u64 * bits as u64;
    let (k, o) = ((debut >> 6) as usize, (debut & 63) as u32);
    let mut v = longs[k] >> o;
    if o + bits > 64 {
        v |= longs[k + 1] << (64 - o);
    }
    (v & ((1u64 << bits) - 1)) as u32
}

// ── lire ────────────────────────────────────────────────────────────────────

struct Region<'a> {
    c: Compound<'a>,
    /// `Position` : le premier coin, dans le repère du fichier.
    position: [i64; 3],
    /// Le coin MINIMAL, dans le même repère.
    min: [i64; 3],
    taille: [u32; 3],
}

pub(crate) fn lire(racine: &Compound, interner: &mut Interner) -> Result<Lu, Erreur> {
    let version = racine.entier("Version").ok_or(Erreur::Manque("Version"))?;
    if version < 4 {
        return Err(Erreur::Ancien("un .litematic d'avant Minecraft 1.13"));
    }
    if version > VERSION_COMPOSANTS as i64 {
        return Err(Erreur::Version {
            format: ".litematic",
            version,
        });
    }
    let data_version = racine
        .entier("MinecraftDataVersion")
        .and_then(|v| i32::try_from(v).ok());
    let meta = racine.compound("Metadata")?;
    let conteneur = racine
        .compound("Regions")?
        .ok_or(Erreur::Manque("Regions"))?;
    let mut bilan = Bilan::default();

    let mut regions = Vec::new();
    for ch in conteneur.champs.iter().filter(|c| c.t == tag::COMPOUND) {
        let c = Compound::lire(conteneur.buf, ch.charge.start)?;
        let pos = c.triplet("Position")?.ok_or(Erreur::Manque("Position"))?;
        let size = c.triplet("Size")?.ok_or(Erreur::Manque("Size"))?;
        if size.contains(&0) {
            return Err(Erreur::Incoherent(format!("région de taille {size:?}")));
        }
        let taille = verifier_taille(size.map(|t| i64::from(t).abs()))?;
        let position = pos.map(i64::from);
        let min = std::array::from_fn(|a| {
            // Une taille négative va du premier coin VERS les petites
            // coordonnées : le coin minimal est `pos + taille + 1`.
            position[a]
                + if size[a] < 0 {
                    i64::from(size[a]) + 1
                } else {
                    0
                }
        });
        regions.push(Region {
            c,
            position,
            min,
            taille,
        });
    }
    if regions.is_empty() {
        return Err(Erreur::Incoherent("aucune région".into()));
    }

    // ── la boîte qui les englobe
    let emin: [i64; 3] = std::array::from_fn(|a| regions.iter().map(|r| r.min[a]).min().unwrap());
    let emax: [i64; 3] = std::array::from_fn(|a| {
        regions
            .iter()
            .map(|r| r.min[a] + r.taille[a] as i64)
            .max()
            .unwrap()
    });
    let taille = verifier_taille(std::array::from_fn(|a| emax[a] - emin[a]))?;
    let decrites: u64 = regions
        .iter()
        .map(|r| r.taille.iter().map(|&t| t as u64).product::<u64>())
        .sum();
    let englobante = Presse::volume(taille) as u64;
    if englobante > (decrites * ENGLOBANTE_PAR_DECRITE).max(ENGLOBANTE_MIN) {
        return Err(Erreur::Incoherent(format!(
            "régions trop éloignées : une boîte de {englobante} cases pour {decrites} décrites"
        )));
    }
    let air = interner.intern("minecraft:air");
    let mut presse = Presse::uniforme(taille, air);
    // L'origine du fichier — là où Litematica pose son curseur — est l'ancre.
    presse.ancre = emin.map(|v| v.wrapping_neg() as i32);

    let plusieurs = regions.len() > 1;
    let mut vues = if plusieurs {
        vec![0u64; presse.blocs.len().div_ceil(64)]
    } else {
        Vec::new()
    };
    let mut recouvertes = 0u64;
    let mut lues = Vec::new();
    let mut ticks = 0usize;
    let [px, _, pz] = taille.map(|t| t as usize);

    for r in &regions {
        // ── la palette
        let pal =
            r.c.liste("BlockStatePalette")?
                .ok_or(Erreur::Manque("BlockStatePalette"))?
                .compounds()?;
        if pal.is_empty() {
            return Err(Erreur::Incoherent("palette vide".into()));
        }
        let mut table: Vec<StateId> = Vec::with_capacity(pal.len());
        for e in &pal {
            let cle = match cle_depuis_compound(e)? {
                Some(k) => k,
                None => {
                    bilan
                        .etats
                        .insert(e.chaine("Name").unwrap_or("?").to_string());
                    "minecraft:air".to_string()
                }
            };
            table.push(interner.intern(&cle));
        }

        // ── les indices
        let bits = bits_pour(table.len());
        let [sx, sy, sz] = r.taille.map(|t| t as usize);
        let volume = sx * sy * sz;
        let longs =
            r.c.longs("BlockStates")
                .ok_or(Erreur::Manque("BlockStates"))?;
        if longs.len() < longs_pour(volume, bits) {
            return Err(Erreur::Incoherent(format!(
                "{} longs pour {volume} cases de {bits} bits",
                longs.len()
            )));
        }
        let o: [usize; 3] = std::array::from_fn(|a| (r.min[a] - emin[a]) as usize);
        let mut i = 0usize;
        for y in 0..sy {
            for z in 0..sz {
                let base = ((o[1] + y) * pz + (o[2] + z)) * px + o[0];
                for x in 0..sx {
                    let v = valeur(&longs, i, bits);
                    i += 1;
                    let id = *table.get(v as usize).ok_or_else(|| {
                        Erreur::Incoherent(format!(
                            "indice {v} pour une palette de {}",
                            table.len()
                        ))
                    })?;
                    let d = base + x;
                    if plusieurs {
                        let (k, b) = (d / 64, d % 64);
                        if vues[k] >> b & 1 == 1 {
                            recouvertes += 1;
                        }
                        vues[k] |= 1 << b;
                    }
                    presse.blocs[d] = id;
                }
            }
        }

        // ── les block entities : relatives au coin MINIMAL de la région
        if let Some(l) = r.c.liste("TileEntities")? {
            for c in l.compounds()? {
                match Entite::depuis_compound(c.tout().to_vec()) {
                    Ok(Some(mut e)) => {
                        e.case = std::array::from_fn(|a| e.case[a].wrapping_add(o[a] as i32));
                        lues.push(e);
                    }
                    _ => bilan.be_ignorees += 1,
                }
            }
        }

        // ── les entités : relatives à la POSITION de la région
        if let Some(l) = r.c.liste("Entities")? {
            let base: [f64; 3] = std::array::from_fn(|a| (r.position[a] - emin[a]) as f64);
            for c in l.compounds()? {
                let Some(p) = c.position("Pos")? else {
                    bilan.entites_ignorees += 1;
                    continue;
                };
                if c.chaine("id").is_none() {
                    bilan.entites_ignorees += 1;
                    continue;
                }
                let local = std::array::from_fn(|a| base[a] + p[a]);
                match entites::importer(
                    c.tout().to_vec(),
                    local,
                    Repere::Inconnu,
                    taille,
                    data_version,
                ) {
                    Ok(i) => {
                        let m = bilan.prendre(i);
                        presse.mobiles.push(m);
                    }
                    Err(_) => bilan.entites_ignorees += 1,
                }
            }
        }

        for nom in ["PendingBlockTicks", "PendingFluidTicks"] {
            if let Some(l) = r.c.liste(nom)? {
                ticks += l.n;
            }
        }
    }
    ranger_block_entities(&mut presse, lues, &mut bilan);
    if plusieurs {
        bilan.remarques.push(Remarque::RegionsFusionnees {
            regions: regions.len(),
            recouvertes,
        });
    }
    if ticks > 0 {
        bilan.remarques.push(Remarque::TicksIgnores(ticks));
    }
    let texte = |k| meta.as_ref().and_then(|m| m.chaine(k)).map(str::to_string);
    Ok(Lu {
        presse,
        lecture: Lecture::Litematic {
            version: version as i32,
            regions: regions.len(),
        },
        data_version,
        nom: texte("Name"),
        auteur: texte("Author"),
        remarques: bilan.fin(),
    })
}

// ── écrire ──────────────────────────────────────────────────────────────────

pub(crate) fn ecrire(
    presse: &Presse,
    interner: &Interner,
    meta: &Meta,
    _bilan: &mut Bilan,
) -> Result<Vec<u8>, Erreur> {
    let palette = Palette::de(presse, interner, true)?;
    let bits = bits_pour(palette.cles.len());
    let volume = presse.blocs.len();
    let longs = empaqueter(
        presse.blocs.iter().map(|&id| palette.indice(id)),
        bits,
        volume,
    );
    let air: Vec<bool> = palette.cles.iter().map(|k| est_air(k)).collect();
    let pleins = presse
        .blocs
        .iter()
        .filter(|&&id| !air[palette.indice(id) as usize])
        .count();
    let version = if meta.data_version >= DV_COMPOSANTS {
        VERSION_COMPOSANTS
    } else {
        VERSION
    };
    let [sx, sy, sz] = presse.taille.map(|t| t as i32);
    let nom_region = if meta.nom.is_empty() {
        "titiforge"
    } else {
        meta.nom.as_str()
    };

    let mut w = Writer::with_capacity(longs.len() * 8 + 4096);
    w.raw(&[tag::COMPOUND]).raw_str("");
    w.field(tag::INT, "MinecraftDataVersion")
        .i32_payload(meta.data_version);
    w.field(tag::INT, "Version").i32_payload(version);
    w.field(tag::INT, "SubVersion").i32_payload(1);
    w.field(tag::COMPOUND, "Metadata");
    w.field(tag::STRING, "Name").raw_str(&meta.nom);
    w.field(tag::STRING, "Author").raw_str(&meta.auteur);
    w.field(tag::STRING, "Description")
        .raw_str(&meta.description);
    w.field(tag::INT, "RegionCount").i32_payload(1);
    w.field(tag::INT, "TotalVolume").i32_payload(volume as i32);
    w.field(tag::INT, "TotalBlocks").i32_payload(pleins as i32);
    w.field(tag::LONG, "TimeCreated").i64_payload(meta.date_ms);
    w.field(tag::LONG, "TimeModified").i64_payload(meta.date_ms);
    champ_xyz(&mut w, "EnclosingSize", [sx, sy, sz]);
    w.end();

    w.field(tag::COMPOUND, "Regions");
    w.field(tag::COMPOUND, nom_region);
    w.field(tag::LIST, "BlockStatePalette")
        .list_header(tag::COMPOUND, palette.cles.len());
    for cle in &palette.cles {
        ecrire_etat(&mut w, cle);
        w.end();
    }
    w.field(tag::LONG_ARRAY, "BlockStates")
        .long_array_payload(&longs);
    // Les block entities sont déjà en LOCAL, relatives au coin : c'est
    // exactement le repère de la région.
    entete_liste(&mut w, "TileEntities", presse.entites.len());
    for e in &presse.entites {
        w.raw(&e.octets());
    }
    entete_liste(&mut w, "PendingBlockTicks", 0);
    entete_liste(&mut w, "PendingFluidTicks", 0);
    entete_liste(&mut w, "Entities", presse.mobiles.len());
    for m in &presse.mobiles {
        w.raw(&m.octets());
    }
    // L'origine du fichier est l'ANCRE : la région commence à −ancre. C'est
    // elle que Litematica pose sous son curseur.
    champ_xyz(&mut w, "Position", presse.ancre.map(i32::wrapping_neg));
    champ_xyz(&mut w, "Size", [sx, sy, sz]);
    w.end(); // la région
    w.end(); // Regions
    w.end(); // la racine
    Ok(w.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_largeur_des_indices_est_celle_de_litematica() {
        // `max(2, 32 − numberOfLeadingZeros(n − 1))`.
        let cas = [
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 3),
            (16, 4),
            (17, 5),
            (40, 6),
            (256, 8),
            (257, 9),
        ];
        for (n, bits) in cas {
            assert_eq!(bits_pour(n), bits, "{n} états");
        }
        assert_eq!(longs_pour(210, 6), 20);
        assert_eq!(longs_pour(1, 2), 1);
    }

    /// Ranger puis relire rend chaque valeur, pour chaque largeur de 2 à 32
    /// bits et une longueur qui ne tombe pas juste — les indices À CHEVAL
    /// compris.
    #[test]
    fn ranger_a_cheval_puis_relire_rend_chaque_valeur() {
        let mut graine = 0x2545_f491_4f6c_dd1du64;
        for bits in 2..=32u32 {
            let n = 1_000 + bits as usize;
            let masque = if bits == 32 {
                u32::MAX
            } else {
                (1u32 << bits) - 1
            };
            let valeurs: Vec<u32> = (0..n)
                .map(|_| {
                    graine ^= graine << 13;
                    graine ^= graine >> 7;
                    graine ^= graine << 17;
                    (graine as u32) & masque
                })
                .collect();
            let longs = empaqueter(valeurs.iter().copied(), bits, n);
            assert_eq!(longs.len(), longs_pour(n, bits));
            for (i, &v) in valeurs.iter().enumerate() {
                assert_eq!(valeur(&longs, i, bits), v, "{bits} bits, indice {i}");
            }
        }
    }
}
