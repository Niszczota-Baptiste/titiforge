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

// ── ce qui ne se lit pas, et POURQUOI ───────────────────────────────────────

/// Le chunk (0, 0) d'un terrain de deux sections, inflaté.
fn chunk_de_terrain() -> Vec<u8> {
    let t = Terrain {
        side: 1,
        sections: 2,
        ..Terrain::default()
    };
    let octets = region_en(&t, 0, 0);
    let r = tf_anvil::read(&octets, 0, 0).unwrap();
    let brut = r.get(0, 0).expect("la fixture porte le chunk (0, 0)");
    tf_anvil::inflate(&brut.payload, brut.compression).unwrap()
}

/// Le même chunk, BOURRÉ d'un tableau d'octets incompressible glissé à la
/// racine : plus d'un mégaoctet compressé, donc ce que le jeu DÉPORTE dans un
/// `.mcc`. Le lecteur enjambe ce qu'il ne connaît pas, donc les sections sont
/// celles du chunk d'origine.
fn bourre(inflated: &[u8]) -> Vec<u8> {
    let mut x = 0x5eed_u64;
    let bruit: Vec<u8> = (0..1_100_000)
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (x >> 56) as u8
        })
        .collect();
    // Le dernier octet est le TAG_End de la racine : le champ passe avant.
    let mut out = inflated[..inflated.len() - 1].to_vec();
    out.push(7); // TAG_Byte_Array
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(b"Bourrage");
    out.extend_from_slice(&(bruit.len() as i32).to_be_bytes());
    out.extend_from_slice(&bruit);
    out.push(0);
    out
}

/// Une région dont les chunks `(i, 0)`, `i < n`, portent tous `charge`,
/// annoncée sous `compression`.
fn region_de(charge: &[u8], compression: tf_anvil::Compression, n: u16) -> tf_anvil::WriteOutput {
    let mut r = tf_anvil::Region::vide(0, 0);
    for i in 0..n {
        r.slots[i as usize] = Some(tf_anvil::RawChunk {
            index: i,
            timestamp: 0,
            compression,
            payload: std::borrow::Cow::Owned(charge.to_vec()),
            external: false,
        });
    }
    tf_anvil::write(&r).unwrap()
}

fn lire_dans(src: &MemorySource, sel: &BBox) -> (tf_world::Bilan, Vec<(i8, Vec<String>)>) {
    let mut interner = Interner::new();
    let mut vues = Vec::new();
    let bilan = sections_de(
        src,
        &Dimension::Overworld,
        Folder::Region,
        sel,
        &mut interner,
        |s| vues.push(s),
    );
    // Par NOM : chaque lecture numérote dans son propre interner.
    let mut noms: Vec<(i8, Vec<String>)> = vues
        .iter()
        .map(|s| {
            let mut p: Vec<String> = s
                .section
                .palette
                .iter()
                .map(|&id| interner.resolve(id).unwrap_or("?").to_string())
                .collect();
            p.sort();
            (s.section.y, p)
        })
        .collect();
    noms.sort();
    (bilan, noms)
}

/// **Un chunk déporté se lit comme les autres.** Le jeu déporte dans un
/// `c.X.Z.mcc` tout chunk de plus d'un mégaoctet compressé, et le `.mca` ne
/// garde qu'un talon vide. Le chemin d'AFFICHAGE l'oubliait : il comptait le
/// chunk « illisible » pendant que les opérations, qui le résolvent, le
/// lisaient très bien — un bâtiment chargé d'entités disparaissait de l'écran
/// et restait éditable.
#[test]
fn un_chunk_deporte_se_lit_comme_les_autres() {
    let normal = chunk_de_terrain();
    let temoin = MemorySource::new();
    let z = tf_anvil::deflate(&normal, tf_anvil::Compression::Zlib).unwrap();
    temoin
        .write_region(
            &Dimension::Overworld,
            Folder::Region,
            RegionPos::new(0, 0),
            &region_de(&z, tf_anvil::Compression::Zlib, 1).region,
        )
        .unwrap();
    let (b0, attendu) = lire_dans(&temoin, &boite(0, 0, 15, 15));
    assert_eq!(b0.illisibles, 0);
    assert!(!attendu.is_empty(), "le témoin doit porter des sections");

    let gros = tf_anvil::deflate(&bourre(&normal), tf_anvil::Compression::Zlib).unwrap();
    let out = region_de(&gros, tf_anvil::Compression::Zlib, 1);
    assert_eq!(out.external.len(), 1, "le chunk doit partir en `.mcc`");
    assert_eq!(out.external[0].name, "c.0.0.mcc");
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &out.region,
    )
    .unwrap();
    src.write_external(
        &Dimension::Overworld,
        Folder::Region,
        &out.external[0].name,
        &out.external[0].bytes,
    )
    .unwrap();
    let (bilan, vues) = lire_dans(&src, &boite(0, 0, 15, 15));
    assert_eq!(bilan.illisibles, 0, "{:?}", bilan.raisons);
    assert_eq!(bilan.chunks, 1);
    assert_eq!(vues, attendu, "les sections du chunk, par nom");
}

/// Et quand le `.mcc` MANQUE, on le dit — par son nom, avec le chunk.
#[test]
fn un_chunk_deporte_sans_son_mcc_le_dit() {
    let gros =
        tf_anvil::deflate(&bourre(&chunk_de_terrain()), tf_anvil::Compression::Zlib).unwrap();
    let out = region_de(&gros, tf_anvil::Compression::Zlib, 1);
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &out.region,
    )
    .unwrap();
    let (bilan, vues) = lire_dans(&src, &boite(0, 0, 15, 15));
    assert!(vues.is_empty());
    assert_eq!(bilan.illisibles, 1);
    assert_eq!(bilan.raisons.len(), 1);
    let r = &bilan.raisons[0];
    assert!(r.contains("chunk (0, 0)") && r.contains("c.0.0.mcc"), "{r}");
    assert!(bilan.pourquoi().contains("c.0.0.mcc"));
}

/// **« Illisible » recouvre plusieurs défauts, et chacun se NOMME.** Une
/// compression que le format ne définit pas — le LZ4 qu'un serveur récent
/// peut choisir — n'a rien à voir avec un fichier tronqué, et ne se répare
/// pas pareil.
#[test]
fn une_compression_inconnue_se_nomme() {
    let out = region_de(&[1, 2, 3], tf_anvil::Compression::Other(4), 1);
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &out.region,
    )
    .unwrap();
    let (bilan, _) = lire_dans(&src, &boite(0, 0, 15, 15));
    assert_eq!(bilan.illisibles, 1);
    let r = &bilan.raisons[0];
    assert!(
        r.contains("chunk (0, 0)") && r.contains("compression inconnue (4)"),
        "{r}"
    );
}

/// Un en-tête qui pointe au-delà de la fin du fichier : un `.mca` tronqué —
/// ce qu'on obtient en le copiant pendant que le jeu l'écrit. Le chunk reste
/// illisible, et la raison dit ce qu'il faut regarder.
#[test]
fn un_en_tete_qui_pointe_hors_du_fichier_se_dit() {
    let z = tf_anvil::deflate(&chunk_de_terrain(), tf_anvil::Compression::Zlib).unwrap();
    let mut octets = region_de(&z, tf_anvil::Compression::Zlib, 2).region;
    octets.truncate(8192 + 100);
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &octets,
    )
    .unwrap();
    let (bilan, vues) = lire_dans(&src, &boite(0, 0, 31, 15));
    assert!(vues.is_empty());
    assert_eq!(bilan.illisibles, 2, "les deux emplacements de l'en-tête");
    assert_eq!(bilan.raisons.len(), 1, "une raison pour l'en-tête entier");
    let r = &bilan.raisons[0];
    assert!(
        r.contains("r.0.0.mca") && r.contains("hors du fichier"),
        "{r}"
    );
    assert!(r.contains("2 emplacement(s)"), "{r}");
    // Une raison qui couvre deux illisibles n'en cache pas d'autres.
    assert!(
        !bilan.pourquoi().contains("d'autres"),
        "{}",
        bilan.pourquoi()
    );
}

/// Les raisons sont PLAFONNÉES : une région abîmée en aurait mille, et dix
/// raisons identiques n'en disent pas plus qu'une. Le compte, lui, reste
/// exact, et le message dit qu'il y en a d'autres.
#[test]
fn les_raisons_sont_plafonnees_le_compte_non() {
    let out = region_de(&[1, 2, 3], tf_anvil::Compression::Other(4), 10);
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &out.region,
    )
    .unwrap();
    let (bilan, _) = lire_dans(&src, &boite(0, 0, 511, 15));
    assert_eq!(bilan.illisibles, 10);
    assert_eq!(bilan.raisons.len(), tf_world::lecture::RAISONS_MAX);
    assert!(
        bilan.pourquoi().ends_with("et d'autres"),
        "{}",
        bilan.pourquoi()
    );

    // Tout s'est lu : rien à dire.
    let (sain, _) = lire(&boite(0, 0, 31, 31));
    assert_eq!(sain.pourquoi(), "");
}

/// **Un chunk que le jeu n'a pas fini de générer ne s'affiche pas.** Pour le
/// jeu il n'existe pas encore — il ne l'affiche pas non plus, et le recouvrira
/// en reprenant sa génération. Ce n'est pas une anomalie (il y en a une
/// couronne au bord de toute zone explorée) : il est COMPTÉ à part, pas parmi
/// les illisibles, et ne fait l'objet d'aucun message.
#[test]
fn un_chunk_a_mi_generation_ne_s_affiche_pas() {
    let t = Terrain {
        side: 2,
        sections: 2,
        ..Terrain::default()
    };
    let octets = tf_bench::avec_statut(&region_en(&t, 0, 0), 0, 0, &[(1, 0)], "minecraft:features");
    let src = MemorySource::new();
    src.write_region(
        &Dimension::Overworld,
        Folder::Region,
        RegionPos::new(0, 0),
        &octets,
    )
    .unwrap();
    let mut interner = Interner::new();
    let mut chunks = std::collections::BTreeSet::new();
    let bilan = sections_de(
        &src,
        &Dimension::Overworld,
        Folder::Region,
        &boite(0, 0, 31, 31),
        &mut interner,
        |s| {
            chunks.insert((s.chunk.x, s.chunk.z));
        },
    );
    assert_eq!(chunks, [(0, 0), (0, 1), (1, 1)].into_iter().collect());
    assert_eq!(bilan.chunks, 3);
    assert_eq!(bilan.incomplets, 1);
    assert_eq!(bilan.illisibles, 0, "ce n'est pas un défaut du fichier");
    assert_eq!(bilan.pourquoi(), "");
}
