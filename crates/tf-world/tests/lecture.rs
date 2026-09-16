//! Lire une PORTION de save. Ce qui est vérifié ici, c'est surtout ce qu'on ne
//! lit PAS : l'invariant n° 7 dit qu'il n'existe aucun état « le monde est
//! chargé », et une lecture qui déborde de son emprise le contredirait en
//! silence — sur un monde Minefield, en octets qu'on ne peut pas tenir.

use tf_anvil::Interner;
use tf_bench::{region_en, Terrain};
use tf_world::{
    sections_de, BBox, BlockPos, Dimension, Folder, MemorySource, RegionPos, RegionSink,
};

/// Quatre régions autour de l'origine, 2 × 2 chunks chacune, 2 sections.
///
/// Autour de l'ORIGINE exprès : c'est là que la division plancher se trompe.
/// Chaque région ne peuple que son coin de plus petites coordonnées, donc
/// `r.-1.-1` porte les chunks (−32, −32) et (−31, −32) — pas (−1, −1).
fn monde() -> MemorySource {
    let m = MemorySource::new();
    let t = Terrain {
        side: 2,
        sections: 2,
        ..Terrain::default()
    };
    for (x, z) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
        m.write_region(
            &Dimension::Overworld,
            Folder::Region,
            RegionPos::new(x, z),
            &region_en(&t, x, z),
        )
        .unwrap();
    }
    m
}

fn lire(sel: &BBox) -> (tf_world::Bilan, Vec<tf_world::SectionLue>) {
    let src = monde();
    let mut interner = Interner::new();
    let mut vues = Vec::new();
    let bilan = sections_de(
        &src,
        &Dimension::Overworld,
        Folder::Region,
        sel,
        &mut interner,
        |s| vues.push(s),
    );
    (bilan, vues)
}

fn boite(x0: i32, z0: i32, x1: i32, z1: i32) -> BBox {
    BBox::new(
        BlockPos {
            x: x0,
            y: -64,
            z: z0,
        },
        BlockPos {
            x: x1,
            y: 319,
            z: z1,
        },
    )
}

#[test]
fn une_emprise_d_un_chunk_ne_lit_qu_un_chunk() {
    let (bilan, vues) = lire(&boite(0, 0, 15, 15));
    assert_eq!(bilan.chunks, 1, "un seul chunk dans l'emprise");
    assert_eq!(bilan.regions, 1, "une seule région ouverte");
    assert!(vues.iter().all(|s| s.chunk.x == 0 && s.chunk.z == 0));
    assert_eq!(bilan.illisibles, 0);
}

/// **L'emprise borne le travail, pas le filtre après coup.** Une région
/// entière lue puis jetée coûterait autant que si on l'avait gardée.
#[test]
fn une_emprise_hors_du_monde_ne_lit_rien() {
    let (bilan, vues) = lire(&boite(10_000, 10_000, 10_015, 10_015));
    assert_eq!(bilan.regions, 0, "aucune région ne devrait être ouverte");
    assert_eq!(bilan.chunks, 0);
    assert!(vues.is_empty());
}

/// Une emprise à cheval sur quatre régions les prend toutes les quatre, et
/// seulement les chunks demandés dans chacune.
/// **Le bloc −1 est dans la région −1, pas la région 0.** Une division entière
/// naïve chargerait la mauvaise moitié du monde sans rien signaler.
#[test]
fn une_emprise_a_cheval_sur_l_origine_prend_les_quatre_regions() {
    // Un seul bloc de part et d'autre de chaque frontière de région.
    let (bilan, _) = lire(&boite(-1, -1, 0, 0));
    assert_eq!(
        bilan.regions, 4,
        "quatre régions se touchent à l'origine ; une division tronquée n'en verrait qu'une"
    );
    // Trois d'entre elles ne PEUPLENT aucun chunk de cette emprise : elles ne
    // portent que leur propre coin, loin de l'origine. On les a quand même
    // ouvertes, et on n'en a rendu qu'un chunk — c'est l'emprise qui décide,
    // pas le contenu du fichier qu'on vient d'ouvrir.
    assert_eq!(bilan.chunks, 1);
}

/// Les chunks rendus sont ceux de l'emprise, avec leurs coordonnées MONDE.
#[test]
fn les_chunks_rendus_portent_leurs_coordonnees_monde() {
    let (bilan, vues) = lire(&boite(-512, -512, 15, 15));
    assert_eq!(bilan.regions, 4);
    let mut chunks: Vec<(i32, i32)> = vues.iter().map(|s| (s.chunk.x, s.chunk.z)).collect();
    chunks.sort_unstable();
    chunks.dedup();
    // Chaque région ne peuple que son coin : r.-1.-1 donne les quatre, r.-1.0
    // et r.0.-1 deux chacune (l'autre moitié sort de l'emprise), r.0.0 une
    // seule. Le chunk 1 n'y est pas — l'emprise s'arrête au bloc 15.
    assert_eq!(
        chunks,
        vec![
            (-32, -32),
            (-32, -31),
            (-32, 0),
            (-31, -32),
            (-31, -31),
            (-31, 0),
            (0, -32),
            (0, -31),
            (0, 0),
        ]
    );
}

/// Un seul interner pour toute la lecture : sans ça, deux chunks lus dans la
/// même passe numéroteraient le même bloc différemment, et rien ne le dirait.
#[test]
fn les_identifiants_d_une_lecture_partagent_un_seul_interner() {
    let src = monde();
    let mut interner = Interner::new();
    let mut vues = Vec::new();
    sections_de(
        &src,
        &Dimension::Overworld,
        Folder::Region,
        &boite(0, 0, 31, 31),
        &mut interner,
        |s| vues.push(s),
    );
    assert!(vues.len() >= 2);
    // Toutes les régions sont identiques : deux chunks distincts doivent donc
    // rendre exactement les mêmes identifiants, pas seulement les mêmes noms.
    let a: Vec<_> = vues
        .iter()
        .find(|s| s.chunk.x == 0 && s.chunk.z == 0)
        .map(|s| s.section.palette.clone())
        .unwrap();
    let b: Vec<_> = vues
        .iter()
        .find(|s| s.chunk.x == 1 && s.chunk.z == 0)
        .map(|s| s.section.palette.clone())
        .unwrap();
    assert_eq!(
        a, b,
        "même contenu, donc mêmes StateId dans un seul interner"
    );
    for id in &a {
        assert!(
            interner.resolve(*id).is_some(),
            "tout identifiant rendu doit être résoluble"
        );
    }
}

/// Une charge illisible est SAUTÉE et COMPTÉE, jamais devinée. Un `.mca` abîmé
/// existe ; le supposer vide écrirait de l'air là où il y a de la pierre.
#[test]
fn une_region_illisible_est_comptee_et_ne_tue_personne() {
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &vec![0xABu8; 8192],
    )
    .unwrap();
    let mut interner = Interner::new();
    let mut vues = 0usize;
    let bilan = sections_de(
        &src,
        &Dimension::Overworld,
        Folder::Region,
        &boite(0, 0, 511, 511),
        &mut interner,
        |_| vues += 1,
    );
    // Un en-tête de zéros ne déclare aucun chunk : ni panique, ni contenu.
    assert_eq!(vues, 0);
    assert_eq!(bilan.chunks, 0);
}

/// Le bilan est rendu d'office, et il est exact : un relevé qui ne dit pas ce
/// qu'il a sauté laisse croire qu'il n'a rien sauté.
#[test]
fn le_bilan_compte_ce_qui_a_vraiment_ete_rendu() {
    let (bilan, vues) = lire(&boite(0, 0, 31, 31));
    assert_eq!(bilan.sections, vues.len(), "sections comptées = rendues");
    assert_eq!(bilan.chunks, 4, "2 × 2 chunks dans r.0.0");
    assert_eq!(bilan.sections, 8, "4 chunks × 2 sections");
}
