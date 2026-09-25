//! **Une édition fait relire ses POINTS D'INTÉRÊT par le jeu** — et seulement
//! la sienne.
//!
//! À `Valid` = 1, le jeu fait confiance à `poi/` et ne relit pas les blocs :
//! un lit déplacé restait inconnu à sa nouvelle place. La jonction met
//! `Valid` à 0 sur les chunks dont les BLOCS changent. Relu par le décodeur
//! GELÉ de `tf-anvil`.

#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use std::collections::BTreeMap;

use frozen::Tag;
use tf_anvil::Interner;
use tf_bench::poi::{region_poi, SectionPoi};
use tf_bench::{region, Terrain};
use tf_ops::catalogue::{construire, Params, Valeur};
use tf_ops::edition::{rejouer, Sens};
use tf_ops::executer::{executer, CompteRendu, Options};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn section(y: i8, valid: Option<bool>, lits: Vec<[i32; 3]>) -> SectionPoi {
    SectionPoi { y, valid, lits }
}

fn monde() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    m.put_region(
        SURFACE,
        Folder::Poi,
        ZERO,
        region_poi(
            0,
            0,
            &[
                // Le chunk qu'on éditera : un lit sous terre, une section vide.
                (
                    0,
                    0,
                    vec![
                        section(-2, Some(true), vec![[3, -30, 4]]),
                        section(4, Some(true), vec![]),
                    ],
                ),
                // Un chunk qu'on ne touchera pas.
                (5, 5, vec![section(0, Some(true), vec![[85, 5, 85]])]),
                // Un chunk déjà invalide, et un sans drapeau du tout.
                (
                    1,
                    0,
                    vec![section(-2, Some(false), vec![]), section(0, None, vec![])],
                ),
            ],
        ),
    );
    Staging::new(m, MemorySource::new())
}

/// Chaque section de chaque chunk de `poi/` : `(chunk, clé) → Valid`.
fn drapeaux(
    st: &Staging<MemorySource, MemorySource>,
) -> BTreeMap<((u32, u32), String), Option<i8>> {
    let octets = st.read_region(&SURFACE, Folder::Poi, ZERO).unwrap();
    let mut out = BTreeMap::new();
    for (k, c) in frozen::decode_region(&octets) {
        let Some(Tag::Compound(sections)) = c.root.get("Sections") else {
            panic!("Sections absent");
        };
        for (y, s) in sections {
            out.insert((k, y.clone()), s.get("Valid").and_then(Tag::as_i8));
        }
    }
    out
}

fn contenu(st: &Staging<MemorySource, MemorySource>) -> BTreeMap<(u32, u32), Tag> {
    let octets = st.read_region(&SURFACE, Folder::Poi, ZERO).unwrap();
    frozen::decode_region(&octets)
        .into_iter()
        .map(|(k, c)| (k, c.root))
        .collect()
}

fn lancer(st: &Staging<MemorySource, MemorySource>, op: &str, p: &Params, s: &BBox) -> CompteRendu {
    let mut i = Interner::new();
    let t = construire(op, p, &mut i).unwrap();
    executer(
        &t,
        st,
        &SURFACE,
        Folder::Region,
        s,
        &mut i,
        &Options::default(),
    )
    .unwrap()
}

fn poser(bloc: &str) -> Params {
    let mut p = Params::new();
    p.poser("bloc", Valeur::texte(bloc));
    p
}

/// Une boîte dans le chunk (0, 0), sous terre.
fn dans_zero() -> BBox {
    BBox::new(BlockPos::new(2, -40, 2), BlockPos::new(6, -35, 6))
}

#[test]
fn une_edition_fait_relire_les_points_d_interet_de_ses_chunks() {
    let st = monde();
    let avant = drapeaux(&st);
    let cr = lancer(
        &st,
        "poser",
        &poser("minecraft:emerald_block"),
        &dans_zero(),
    );
    assert_eq!(
        cr.rapport.poi, 1,
        "un chunk de blocs modifié, un chunk de poi relu"
    );

    let apres = drapeaux(&st);
    for ((chunk, y), v) in &apres {
        match chunk {
            // TOUTES les sections du chunk édité, pas seulement celle qui a
            // changé : relire une section juste ne coûte au jeu qu'un regard.
            (0, 0) => assert_eq!(*v, Some(0), "section {y} du chunk édité"),
            _ => assert_eq!(*v, avant[&(*chunk, y.clone())], "{chunk:?} {y} intact"),
        }
    }
    // Rien d'autre n'a bougé : l'enregistrement du lit est toujours là, avec
    // son ticket — c'est le jeu qui le gardera ou non en relisant.
    let lit = &contenu(&st)[&(0, 0)];
    let rec = lit
        .get("Sections")
        .and_then(|s| s.get("-2"))
        .and_then(|s| s.get("Records"))
        .and_then(Tag::as_list)
        .unwrap();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0]
            .get("mod:extra")
            .and_then(|m| m.get("Valid"))
            .and_then(Tag::as_i8),
        Some(1),
        "un `Valid` qui n'est pas celui d'une section ne se touche pas"
    );
}

#[test]
fn un_biome_ne_fait_rien_relire() {
    // Un terrain qui PORTE des biomes : sur la fixture ordinaire, `//setbiome`
    // n'écrit rien, et ce test passait sans jamais mettre sa prémisse en jeu
    // — une mutation qui comptait un biome comme un bloc survivait.
    let st = monde();
    st.write_region(
        &SURFACE,
        Folder::Region,
        ZERO,
        &region(&Terrain::avec_biomes()),
    )
    .unwrap();
    let avant = contenu(&st);
    let mut p = Params::new();
    p.poser("biome", Valeur::texte("minecraft:desert"));
    let cr = lancer(&st, "biome", &p, &dans_zero());
    assert!(
        cr.rapport.biomes > 0,
        "le test ne prouve rien si aucun biome ne change"
    );
    assert_eq!(cr.rapport.poi, 0, "un biome ne déplace aucun lit");
    assert_eq!(contenu(&st), avant);
}

#[test]
fn une_edition_qui_ne_change_aucun_bloc_ne_touche_pas_aux_points_d_interet() {
    let st = monde();
    let avant = contenu(&st);
    let mut p = Params::new();
    p.poser("de", Valeur::texte("minecraft:barrier"));
    p.poser("vers", Valeur::texte("minecraft:stone"));
    let cr = lancer(&st, "remplacer", &p, &dans_zero());
    assert!(cr.rapport.patches.is_empty());
    assert_eq!(contenu(&st), avant);
}

#[test]
fn une_section_deja_invalide_ou_sans_drapeau_ne_produit_rien() {
    let st = monde();
    // Le chunk (1, 0) : une section à 0, une sans `Valid`.
    let s = BBox::new(BlockPos::new(18, -40, 2), BlockPos::new(20, -35, 4));
    let cr = lancer(&st, "poser", &poser("minecraft:emerald_block"), &s);
    assert!(!cr.rapport.patches.is_empty(), "les blocs, eux, ont changé");
    assert_eq!(cr.rapport.poi, 0, "rien à invalider");
    assert!(cr
        .rapport
        .patches
        .iter()
        .all(|p| p.cible.folder == Folder::Region));
}

#[test]
fn annuler_rend_la_table_d_origine() {
    let st = monde();
    let avant = contenu(&st);
    let cr = lancer(
        &st,
        "poser",
        &poser("minecraft:emerald_block"),
        &dans_zero(),
    );
    let apres = contenu(&st);
    assert_ne!(apres, avant);
    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Poser", "poser", Vec::new(), 0));
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(contenu(&st), avant);
    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(contenu(&st), apres);
}

#[test]
fn un_monde_sans_dossier_poi_s_edite_comme_avant() {
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    let st = Staging::new(m, MemorySource::new());
    let cr = lancer(
        &st,
        "poser",
        &poser("minecraft:emerald_block"),
        &dans_zero(),
    );
    assert_eq!(cr.rapport.poi, 0);
    assert!(!cr.rapport.patches.is_empty());
}
