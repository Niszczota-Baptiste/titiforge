//! `.schem` — la spécification Sponge, versions 1 à 3, telle que WorldEdit la
//! lit et l'écrit (`SpongeSchematicV2Writer`, `V3Reader`…).
//!
//! ## Où est l'origine
//!
//! Les versions ne s'accordent pas, et les outils non plus :
//!
//! | | coin de la boîte | origine du collage | position des entités |
//! |---|---|---|---|
//! | v1, v2 | `Offset` | `Offset − Metadata.WEOffset` | ABSOLUE (WorldEdit) |
//! | v3 | `Offset + WorldEdit.Origin` | `WorldEdit.Origin` | relative au coin |
//!
//! En v2, WorldEdit écrit la position des entités dans le repère de son
//! presse-papiers — le monde d'où l'on a copié — là où Litematica, qui lit
//! aussi le v2, la prend relative au coin. **On écrit donc le coin à
//! l'origine** (`Offset` nul) : les deux lectures coïncident, et l'ancre voyage
//! dans `WEOffset`. En v3, `Offset` vaut −ancre et `Origin` +ancre, pour la
//! même raison : le coin tombe à zéro.
//!
//! À la lecture, un v2 dont les entités sont RELATIVES (écrit par un autre
//! outil) se reconnaît à ce qu'elles tombent dans la boîte sans décalage et
//! hors d'elle avec.

use tf_anvil::{Interner, StateId};
use tf_nbt::{tag, Writer};
use tf_ops::Presse;

use crate::commun::{
    chaine_depuis_cle, champ_position, champ_triplet, cle_depuis_chaine, ecrire_varint,
    entete_liste, lire_varint, Compound,
};
use crate::entites::{self, block_entity, decomposer, decomposer_mobile, Repere};
use crate::{
    ranger_block_entities, verifier_taille, Bilan, Erreur, Lecture, Lu, Meta, Palette, Remarque,
};

/// Le plus grand côté qu'un `.schem` décrit : un short non signé.
const MAX_COTE: u32 = u16::MAX as u32;

// ── lire ────────────────────────────────────────────────────────────────────

pub(crate) fn lire(s: &Compound, interner: &mut Interner) -> Result<Lu, Erreur> {
    let version = s.entier("Version").ok_or(Erreur::Manque("Version"))?;
    if !(1..=3).contains(&version) {
        return Err(Erreur::Version {
            format: ".schem (Sponge)",
            version,
        });
    }
    let dim = |n: &'static str| s.dimension(n).ok_or(Erreur::Manque(n));
    let taille = verifier_taille([dim("Width")?, dim("Height")?, dim("Length")?])?;
    let data_version = s.entier("DataVersion").and_then(|v| i32::try_from(v).ok());
    let offset = s.triplet("Offset")?.unwrap_or([0; 3]);
    let meta = s.compound("Metadata")?;
    let mut bilan = Bilan::default();

    // ── où est l'origine, et dans quel repère sont les entités
    let (ancre, coin_monde) = if version <= 2 {
        let we = meta.as_ref().and_then(|m| {
            let lire = |k| m.entier(k).and_then(|v| i32::try_from(v).ok());
            Some([lire("WEOffsetX")?, lire("WEOffsetY")?, lire("WEOffsetZ")?])
        });
        let ancre = we.map_or([0; 3], |w| w.map(i32::wrapping_neg));
        (ancre, Some(offset))
    } else {
        let origine = match &meta {
            Some(m) => match m.compound("WorldEdit")? {
                Some(w) => w.triplet("Origin")?,
                None => None,
            },
            None => None,
        };
        (
            offset.map(i32::wrapping_neg),
            origine.map(|o| std::array::from_fn(|a| o[a].wrapping_add(offset[a]))),
        )
    };

    // ── les blocs
    let (palette, donnees, conteneur) = if version == 3 {
        let blocs = s.compound("Blocks")?.ok_or(Erreur::Manque("Blocks"))?;
        let p = blocs
            .compound("Palette")?
            .ok_or(Erreur::Manque("Palette"))?;
        let d = blocs.octets("Data").ok_or(Erreur::Manque("Data"))?;
        (p, d, blocs)
    } else {
        let p = s.compound("Palette")?.ok_or(Erreur::Manque("Palette"))?;
        let d = s.octets("BlockData").ok_or(Erreur::Manque("BlockData"))?;
        (p, d, s.clone())
    };
    let table = table_palette(&palette, interner, &mut bilan)?;
    let volume = Presse::volume(taille);
    // Un varint pèse au moins un octet : on le vérifie AVANT de réserver la
    // grille, sans quoi quelques octets qui annoncent une boîte géante font
    // allouer des gigaoctets.
    if donnees.len() < volume {
        return Err(Erreur::Incoherent(format!(
            "{} octets de blocs pour {volume} cases",
            donnees.len()
        )));
    }
    let mut blocs = Vec::with_capacity(volume);
    let mut i = 0usize;
    for _ in 0..volume {
        let v = lire_varint(donnees, &mut i)
            .ok_or_else(|| Erreur::Incoherent("données de blocs tronquées".into()))?;
        let id = table.get(v as usize).ok_or_else(|| {
            Erreur::Incoherent(format!("indice {v} pour une palette de {}", table.len()))
        })?;
        blocs.push(*id);
    }
    if i != donnees.len() {
        return Err(Erreur::Incoherent(format!(
            "{} octets en trop après les blocs",
            donnees.len() - i
        )));
    }
    let mut presse = Presse {
        taille,
        blocs,
        ancre,
        entites: Vec::new(),
        mobiles: Vec::new(),
    };

    // ── les block entities
    let liste = match conteneur.liste("BlockEntities")? {
        Some(l) => Some(l),
        None => conteneur.liste("TileEntities")?,
    };
    let mut lues = Vec::new();
    for c in liste
        .map(|l| l.compounds())
        .transpose()?
        .unwrap_or_default()
    {
        let (Some(p), Some(id)) = (c.triplet("Pos")?, c.chaine("Id").or(c.chaine("id"))) else {
            bilan.be_ignorees += 1;
            continue;
        };
        let champs = if version == 3 {
            match c.compound("Data")? {
                Some(d) => d.champs_sauf(&["id", "x", "y", "z"]),
                None => Vec::new(),
            }
        } else {
            c.champs_sauf(&["Id", "Pos", "id", "x", "y", "z", "ContentVersion"])
        };
        lues.push(block_entity(&champs, Some(id), p));
    }
    ranger_block_entities(&mut presse, lues, &mut bilan);

    // ── les entités
    if let Some(l) = s.liste("Entities")? {
        let entrees = l.compounds()?;
        let positions: Vec<Option<[f64; 3]>> = entrees
            .iter()
            .map(|c| c.position("Pos"))
            .collect::<Result<_, _>>()?;
        let (decalage, repere) = if version == 3 {
            ([0.0; 3], coin_monde.map_or(Repere::Inconnu, Repere::Connu))
        } else if absolues(&positions, offset, taille) {
            (offset.map(f64::from), Repere::Connu(offset))
        } else {
            ([0.0; 3], Repere::Inconnu)
        };
        for (c, p) in entrees.iter().zip(positions) {
            let (Some(p), Some(id)) = (p, c.chaine("Id").or(c.chaine("id"))) else {
                bilan.entites_ignorees += 1;
                continue;
            };
            let champs = if version == 3 {
                match c.compound("Data")? {
                    Some(d) => d.champs_sauf(&["id"]),
                    None => Vec::new(),
                }
            } else {
                c.champs_sauf(&["Id", "id"])
            };
            let mut w = Writer::with_capacity(champs.len() + id.len() + 8);
            w.raw(&champs).field(tag::STRING, "id").raw_str(id).end();
            let local = std::array::from_fn(|a| p[a] - decalage[a]);
            match entites::importer(w.into_bytes(), local, repere, taille, data_version) {
                Ok(i) => {
                    let m = bilan.prendre(i);
                    presse.mobiles.push(m);
                }
                Err(_) => bilan.entites_ignorees += 1,
            }
        }
    }

    if s.champ("BiomeData").is_some() || s.champ("Biomes").is_some() {
        bilan.remarques.push(Remarque::BiomesIgnores);
    }
    let texte = |k| meta.as_ref().and_then(|m| m.chaine(k)).map(str::to_string);
    Ok(Lu {
        presse,
        lecture: Lecture::Sponge {
            version: version as u8,
        },
        data_version,
        nom: texte("Name"),
        auteur: texte("Author"),
        remarques: bilan.fin(),
    })
}

/// Les états d'une palette Sponge, par indice : `{"minecraft:stone": 0, …}`.
///
/// Les indices doivent couvrir `0..n` sans trou ni doublon — la règle de
/// Litematica, plus stricte que celle de WorldEdit, et la seule sous laquelle
/// « l'indice 7 » désigne sans ambiguïté un état.
fn table_palette(
    p: &Compound,
    interner: &mut Interner,
    bilan: &mut Bilan,
) -> Result<Vec<StateId>, Erreur> {
    let n = p.champs.len();
    if n == 0 {
        return Err(Erreur::Incoherent("palette vide".into()));
    }
    let mut table: Vec<Option<StateId>> = vec![None; n];
    for ch in &p.champs {
        let i = match ch.t {
            tag::INT => tf_nbt::Cur::at(p.buf, ch.charge.start)
                .i32()
                .map_err(|_| Erreur::Illisible)?,
            _ => return Err(Erreur::Incoherent(format!("palette : « {} »", ch.nom))),
        };
        let place = usize::try_from(i)
            .ok()
            .and_then(|i| table.get_mut(i))
            .ok_or_else(|| Erreur::Incoherent(format!("indice de palette {i} pour {n} états")))?;
        if place.is_some() {
            return Err(Erreur::Incoherent(format!(
                "indice de palette {i} en double"
            )));
        }
        let cle = cle_depuis_chaine(ch.nom).unwrap_or_else(|| {
            bilan.etats.insert(ch.nom.to_string());
            "minecraft:air".to_string()
        });
        *place = Some(interner.intern(&cle));
    }
    // `n` indices distincts dans `0..n` : il n'y a pas de trou.
    Ok(table.into_iter().flatten().collect())
}

/// Les entités d'un v2 sont-elles ABSOLUES — dans le repère du monde copié,
/// comme les écrit WorldEdit — ou relatives au coin ?
///
/// Tant que `Offset` est nul, c'est la même chose. Sinon, on garde la lecture
/// qui met le plus d'entités DANS la boîte ; à égalité, celle de WorldEdit,
/// qui produit presque tous les `.schem`.
fn absolues(positions: &[Option<[f64; 3]>], offset: [i32; 3], taille: [u32; 3]) -> bool {
    let dans = |d: [f64; 3]| {
        positions
            .iter()
            .flatten()
            .filter(|p| {
                (0..3).all(|a| p[a] - d[a] >= -1.0 && p[a] - d[a] <= taille[a] as f64 + 1.0)
            })
            .count()
    };
    dans(offset.map(f64::from)) >= dans([0.0; 3])
}

// ── écrire ──────────────────────────────────────────────────────────────────

pub(crate) fn ecrire(
    presse: &Presse,
    version: u8,
    interner: &Interner,
    meta: &Meta,
    bilan: &mut Bilan,
) -> Result<Vec<u8>, Erreur> {
    let [sx, sy, sz] = presse.taille;
    if presse.taille.iter().any(|&t| t > MAX_COTE) {
        return Err(Erreur::TropGrandPourLeFormat {
            format: ".schem",
            taille: presse.taille,
            max: MAX_COTE,
        });
    }
    let palette = Palette::de(presse, interner, false)?;
    let mut donnees = Vec::with_capacity(presse.blocs.len());
    for &id in &presse.blocs {
        ecrire_varint(&mut donnees, palette.indice(id));
    }
    let mut bes = Vec::with_capacity(presse.entites.len());
    for e in &presse.entites {
        match decomposer(e)? {
            (Some(id), champs) => bes.push((id, champs, e.case)),
            (None, _) => bilan.be_sans_id += 1,
        }
    }
    let mut ents = Vec::with_capacity(presse.mobiles.len());
    for m in &presse.mobiles {
        match (decomposer_mobile(m)?, m.pos()) {
            ((Some(id), champs), Some(p)) => ents.push((id, champs, p)),
            _ => bilan.entites_ignorees += 1,
        }
    }

    let mut w = Writer::with_capacity(donnees.len() + 4096);
    let court = |t: u32| t as u16 as i16;
    if version == 3 {
        w.raw(&[tag::COMPOUND]).raw_str("");
        w.field(tag::COMPOUND, "Schematic");
    } else {
        w.raw(&[tag::COMPOUND]).raw_str("Schematic");
    }
    w.field(tag::INT, "Version").i32_payload(version as i32);
    w.field(tag::INT, "DataVersion")
        .i32_payload(meta.data_version);
    w.field(tag::COMPOUND, "Metadata");
    if !meta.nom.is_empty() {
        w.field(tag::STRING, "Name").raw_str(&meta.nom);
    }
    if !meta.auteur.is_empty() {
        w.field(tag::STRING, "Author").raw_str(&meta.auteur);
    }
    w.field(tag::LONG, "Date").i64_payload(meta.date_ms);
    if version == 3 {
        w.field(tag::COMPOUND, "WorldEdit");
        champ_triplet(&mut w, "Origin", presse.ancre);
        w.end();
    } else {
        let [x, y, z] = presse.ancre.map(i32::wrapping_neg);
        w.field(tag::INT, "WEOffsetX").i32_payload(x);
        w.field(tag::INT, "WEOffsetY").i32_payload(y);
        w.field(tag::INT, "WEOffsetZ").i32_payload(z);
    }
    w.end();
    w.field(tag::SHORT, "Width").i16_payload(court(sx));
    w.field(tag::SHORT, "Height").i16_payload(court(sy));
    w.field(tag::SHORT, "Length").i16_payload(court(sz));
    if version == 3 {
        champ_triplet(&mut w, "Offset", presse.ancre.map(i32::wrapping_neg));
        w.field(tag::COMPOUND, "Blocks");
        ecrire_palette(&mut w, &palette);
        w.field(tag::BYTE_ARRAY, "Data")
            .byte_array_payload(&donnees);
        entete_liste(&mut w, "BlockEntities", bes.len());
        for (id, champs, case) in &bes {
            w.field(tag::STRING, "Id").raw_str(id);
            champ_triplet(&mut w, "Pos", *case);
            w.field(tag::COMPOUND, "Data").raw(champs).end();
            w.end();
        }
        w.end();
    } else {
        champ_triplet(&mut w, "Offset", [0; 3]);
        w.field(tag::INT, "PaletteMax")
            .i32_payload(palette.cles.len() as i32);
        ecrire_palette(&mut w, &palette);
        w.field(tag::BYTE_ARRAY, "BlockData")
            .byte_array_payload(&donnees);
        entete_liste(&mut w, "BlockEntities", bes.len());
        for (id, champs, case) in &bes {
            w.raw(champs);
            w.field(tag::STRING, "Id").raw_str(id);
            champ_triplet(&mut w, "Pos", *case);
            w.end();
        }
    }
    // Comme WorldEdit : pas de liste d'entités quand il n'y en a pas.
    if !ents.is_empty() {
        entete_liste(&mut w, "Entities", ents.len());
        for (id, champs, p) in &ents {
            if version == 3 {
                w.field(tag::STRING, "Id").raw_str(id);
                champ_position(&mut w, "Pos", *p);
                w.field(tag::COMPOUND, "Data").raw(champs).end();
            } else {
                // À plat : la position est déjà dans les champs, LOCALE — et
                // le coin est à l'origine, donc c'est aussi l'absolue que
                // WorldEdit attend.
                w.raw(champs);
                w.field(tag::STRING, "Id").raw_str(id);
            }
            w.end();
        }
    }
    if version == 3 {
        w.end();
    }
    w.end();
    Ok(w.into_bytes())
}

fn ecrire_palette(w: &mut Writer, palette: &Palette) {
    w.field(tag::COMPOUND, "Palette");
    for (i, cle) in palette.cles.iter().enumerate() {
        w.field(tag::INT, &chaine_depuis_cle(cle))
            .i32_payload(i as i32);
    }
    w.end();
}
