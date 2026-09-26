//! `.nbt` de structure — ce qu'écrit un bloc de structure, et ce que lisent
//! `/place template` et les datapacks (`StructureTemplate` du jeu).
//!
//! Une LISTE de blocs plutôt qu'une grille : chaque entrée porte sa case, son
//! indice de palette et, s'il y a lieu, sa block entity — sans ses
//! coordonnées. Une case absente de la liste n'est pas de l'air : c'est un
//! `structure_void`, que le jeu laisse tel quel en posant la structure. On
//! la lit comme de l'air (qu'un collage sans l'air laisse tel quel aussi), et
//! le compte rendu le dit.
//!
//! Le jeu, lui, écrit l'air : une structure posée CREUSE sa place. On fait de
//! même.

use std::collections::BTreeMap;

use tf_anvil::{Interner, StateId};
use tf_nbt::{tag, Writer};
use tf_ops::Presse;

use crate::commun::{
    champ_liste_entiers, champ_position, cle_depuis_compound, ecrire_etat, entete_liste, Compound,
};
use crate::entites::{self, block_entity, decomposer, Repere};
use crate::{
    ranger_block_entities, verifier_taille, Bilan, Erreur, Lecture, Lu, Meta, Palette, Remarque,
};

/// Au-delà de 128³ cases, un `.nbt` ne sert plus à rien qu'à attendre.
///
/// Une LISTE de blocs coûte une trentaine d'octets par case, air compris.
/// Mesuré (`--example mesurer`) sur 12,6 millions de cases : 6,9 s pour
/// écrire, 4,9 s pour relire, 32,6 Mo — là où `.schem` fait le même
/// extrait en 0,3 s et 1 Mo. Un bloc de structure n'en sauve de toute façon
/// pas plus de 48 × 48 × 48 ; pour un build entier, `.schem` et
/// `.litematic` sont faits pour ça. Le plafond vaut dans les deux sens : un
/// fichier plus gros que ce qu'on écrirait n'est pas un fichier qu'on lit.
pub const MAX_CASES_STRUCTURE: u64 = 1 << 21;

// ── lire ────────────────────────────────────────────────────────────────────

pub(crate) fn lire(racine: &Compound, interner: &mut Interner) -> Result<Lu, Erreur> {
    let size = racine.triplet("size")?.ok_or(Erreur::Manque("size"))?;
    let taille = verifier_taille(size.map(i64::from))?;
    // Une structure peut laisser des cases vides : sa taille n'est donc pas
    // bornée par sa liste de blocs, et quelques octets pourraient annoncer une
    // boîte de deux gigaoctets. Le plafond est celui de l'écriture.
    if Presse::volume(taille) as u64 > MAX_CASES_STRUCTURE {
        return Err(Erreur::TropDeCases {
            format: ".nbt de structure",
            cases: Presse::volume(taille) as u64,
            max: MAX_CASES_STRUCTURE,
        });
    }
    let data_version = racine
        .entier("DataVersion")
        .and_then(|v| i32::try_from(v).ok());
    let mut bilan = Bilan::default();

    // ── la palette : une seule, ou des VARIANTES (une épave)
    let liste = match racine.liste("palette")? {
        Some(l) => l,
        None => {
            let variantes = racine
                .liste("palettes")?
                .ok_or(Erreur::Manque("palette"))?
                .listes()?;
            if variantes.len() > 1 {
                bilan
                    .remarques
                    .push(Remarque::VariantesIgnorees(variantes.len()));
            }
            *variantes
                .first()
                .ok_or_else(|| Erreur::Incoherent("aucune palette".into()))?
        }
    };
    let mut table: Vec<StateId> = Vec::new();
    for e in liste.compounds()? {
        let cle = match cle_depuis_compound(&e)? {
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

    // ── les blocs
    let air = interner.intern("minecraft:air");
    let mut presse = Presse::uniforme(taille, air);
    let mut vues = vec![0u64; presse.blocs.len().div_ceil(64)];
    let mut lues = Vec::new();
    let blocs = racine
        .liste("blocks")?
        .ok_or(Erreur::Manque("blocks"))?
        .compounds()?;
    for b in &blocs {
        let (Some(p), Some(s)) = (b.triplet("pos")?, b.entier("state")) else {
            return Err(Erreur::Incoherent("un bloc sans case ou sans état".into()));
        };
        let i = (0..3)
            .all(|a| p[a] >= 0 && (p[a] as u32) < taille[a])
            .then(|| presse.index(p[0] as u32, p[1] as u32, p[2] as u32))
            .flatten()
            .ok_or_else(|| Erreur::Incoherent(format!("bloc hors de la boîte : {p:?}")))?;
        let id = usize::try_from(s)
            .ok()
            .and_then(|s| table.get(s))
            .ok_or_else(|| {
                Erreur::Incoherent(format!("état {s} pour une palette de {}", table.len()))
            })?;
        presse.blocs[i] = *id;
        vues[i / 64] |= 1 << (i % 64);
        if let Some(n) = b.compound("nbt")? {
            match n.chaine("id") {
                Some(id) => lues.push(block_entity(
                    &n.champs_sauf(&["id", "x", "y", "z"]),
                    Some(id),
                    p,
                )),
                None => bilan.be_ignorees += 1,
            }
        }
    }
    let decrites: u64 = vues.iter().map(|m| m.count_ones() as u64).sum();
    let vides = presse.blocs.len() as u64 - decrites;
    ranger_block_entities(&mut presse, lues, &mut bilan);

    // ── les entités : `pos` relative, le reste dans le repère du monde d'où
    // la structure a été sauvée
    if let Some(l) = racine.liste("entities")? {
        for e in l.compounds()? {
            let (Some(p), Some(n)) = (e.position("pos")?, e.compound("nbt")?) else {
                bilan.entites_ignorees += 1;
                continue;
            };
            if n.chaine("id").is_none() {
                bilan.entites_ignorees += 1;
                continue;
            }
            match entites::importer(n.tout().to_vec(), p, Repere::Inconnu, taille, data_version) {
                Ok(i) => {
                    let m = bilan.prendre(i);
                    presse.mobiles.push(m);
                }
                Err(_) => bilan.entites_ignorees += 1,
            }
        }
    }
    if vides > 0 {
        bilan.remarques.push(Remarque::CasesVides(vides));
    }
    Ok(Lu {
        presse,
        lecture: Lecture::Structure,
        data_version,
        nom: None,
        auteur: racine.chaine("author").map(str::to_string),
        remarques: bilan.fin(),
    })
}

// ── écrire ──────────────────────────────────────────────────────────────────

pub(crate) fn ecrire(
    presse: &Presse,
    interner: &Interner,
    meta: &Meta,
    bilan: &mut Bilan,
) -> Result<Vec<u8>, Erreur> {
    let volume = presse.blocs.len() as u64;
    if volume > MAX_CASES_STRUCTURE {
        return Err(Erreur::TropDeCases {
            format: ".nbt de structure",
            cases: volume,
            max: MAX_CASES_STRUCTURE,
        });
    }
    let palette = Palette::de(presse, interner, false)?;
    let mut bes = BTreeMap::new();
    for e in &presse.entites {
        match decomposer(e)? {
            (Some(id), champs) => {
                bes.insert(e.case, (id, champs));
            }
            (None, _) => bilan.be_sans_id += 1,
        }
    }

    let mut w = Writer::with_capacity(presse.blocs.len() * 32 + 1024);
    w.raw(&[tag::COMPOUND]).raw_str("");
    entete_liste(&mut w, "blocks", presse.blocs.len());
    let [sx, sy, sz] = presse.taille;
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                let i = presse.index(x, y, z).expect("dans la boîte");
                let case = [x as i32, y as i32, z as i32];
                champ_liste_entiers(&mut w, "pos", case);
                w.field(tag::INT, "state")
                    .i32_payload(palette.indice(presse.blocs[i]) as i32);
                if let Some((id, champs)) = bes.get(&case) {
                    w.field(tag::COMPOUND, "nbt").raw(champs);
                    w.field(tag::STRING, "id").raw_str(id);
                    w.end();
                }
                w.end();
            }
        }
    }
    w.field(tag::LIST, "palette")
        .list_header(tag::COMPOUND, palette.cles.len());
    for cle in &palette.cles {
        ecrire_etat(&mut w, cle);
        w.end();
    }
    let ents: Vec<_> = presse
        .mobiles
        .iter()
        .filter_map(|m| {
            let p = m.pos();
            if p.is_none() {
                bilan.entites_ignorees += 1;
            }
            p.map(|p| (m, p))
        })
        .collect();
    entete_liste(&mut w, "entities", ents.len());
    for (m, p) in ents {
        champ_position(&mut w, "pos", p);
        // La case d'un tableau est sa case d'ACCROCHE — `Painting.getPos` dans
        // le jeu ; celle des autres, la case où ils sont.
        let case = m
            .corps
            .first()
            .and_then(|k| k.tuile.as_ref().map(|t| t.v))
            .unwrap_or(p.map(|v| v.floor() as i32));
        champ_liste_entiers(&mut w, "blockPos", case);
        w.field(tag::COMPOUND, "nbt").raw(&m.octets());
        w.end();
    }
    champ_liste_entiers(&mut w, "size", [sx as i32, sy as i32, sz as i32]);
    w.field(tag::INT, "DataVersion")
        .i32_payload(meta.data_version);
    w.end();
    if presse.ancre != [0; 3] {
        bilan.remarques.push(Remarque::AncrePerdue);
    }
    Ok(w.into_bytes())
}
