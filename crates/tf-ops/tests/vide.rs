//! **Un monde VIDE** : plat, toutes ses couches d'air — le préréglage « The
//! Void » du jeu. Le seul monde où une opération peut CRÉER les chunks qui
//! manquent, parce que le chunk qu'elle écrit est celui que le jeu y aurait
//! généré, à ses blocs près.
//!
//! Partout ailleurs une opération n'engendre pas de chunk, et les tests
//! d'`edition.rs` le tiennent ; ici on vérifie l'autre moitié de la règle —
//! et qu'elle ne déborde pas sur la première.

use tf_anvil::{decode_section, inflate, read, scan, Interner};
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, deplacer, peut_creer, rejouer, Sens, CREATION_MAX};
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif, Pas};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::niveau::MondeVide;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource, SourceError};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn vide() -> MondeVide {
    MondeVide {
        data_version: 2975,
        biome: "minecraft:the_void".into(),
    }
}

/// Un monde vide sans AUCUNE région : ce qu'on obtient en le créant.
fn monde_vide() -> Staging<MemorySource, MemorySource> {
    Staging::new(MemorySource::new(), MemorySource::new()).avec_monde_vide(Some(vide()))
}

fn boite(a: [i32; 3], b: [i32; 3]) -> BBox {
    BBox::new(
        BlockPos::new(a[0], a[1], a[2]),
        BlockPos::new(b[0], b[1], b[2]),
    )
}

fn remplir(i: &mut Interner, bloc: &str) -> Plan {
    Plan::nouveau(Masque::Tout, Motif::Bloc(i.intern(bloc))).en_comptant()
}

/// Les cases non-air d'une région, par nom — relues par le MOTEUR, chunk par
/// chunk, section par section.
fn pleines(st: &Staging<MemorySource, MemorySource>, pos: RegionPos) -> Vec<([i32; 3], String)> {
    let octets = match st.read_region(&SURFACE, DOSSIER, pos) {
        Ok(o) => o,
        Err(SourceError::NotFound) => return Vec::new(),
        Err(e) => panic!("{e:?}"),
    };
    let r = read(&octets, pos.x, pos.z).unwrap();
    let mut i = Interner::new();
    let mut out = Vec::new();
    for c in r.iter() {
        let nbt = inflate(&c.payload, c.compression).unwrap();
        let sc = scan(&nbt).unwrap();
        let (cx, cz) = (sc.x_pos.unwrap(), sc.z_pos.unwrap());
        for s in &sc.sections {
            let Some(sec) = decode_section(&nbt, &sc, s, &mut i).unwrap() else {
                continue;
            };
            for (k, &id) in sec.unpack().iter().enumerate() {
                let nom = i.resolve(sec.palette[id as usize]).unwrap().to_string();
                if nom != "minecraft:air" {
                    let (x, y, z) = ((k % 16) as i32, (k / 256) as i32, ((k / 16) % 16) as i32);
                    out.push(([cx * 16 + x, s.y as i32 * 16 + y, cz * 16 + z], nom));
                }
            }
        }
    }
    out.sort();
    out
}

/// **Dans un monde vide, remplir CRÉE les chunks qui manquent** — et ce qu'on
/// y relit est exactement la sélection, rien de plus : trois chunks, chacun
/// FINI, du biome du monde.
#[test]
fn dans_un_monde_vide_remplir_cree_les_chunks() {
    let st = monde_vide();
    let mut i = Interner::new();
    let sel = boite([0, 60, 0], [47, 62, 15]);
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &remplir(&mut i, "minecraft:stone"),
        &i,
    )
    .unwrap();
    assert_eq!(r.chunks_crees, 3);
    assert_eq!(
        r.chunks_absents, 0,
        "rien n'est laissé de côté dans un monde vide"
    );
    assert_eq!(r.patches.len(), 3);
    assert_eq!(r.blocs, Some(48 * 3 * 16));

    let blocs = pleines(&st, ZERO);
    assert_eq!(blocs.len(), 48 * 3 * 16, "la sélection, et rien d'autre");
    assert!(blocs
        .iter()
        .all(|(p, n)| n == "minecraft:stone" && sel.contains(BlockPos::new(p[0], p[1], p[2]))));

    let octets = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let r = read(&octets, 0, 0).unwrap();
    for c in r.iter() {
        let sc = scan(&inflate(&c.payload, c.compression).unwrap()).unwrap();
        assert!(!sc.incomplet, "un chunk créé est FINI");
        assert_eq!(sc.data_version, 2975);
    }
}

/// **Annuler une création rend l'ABSENCE** — pas une coquille vide que le jeu
/// n'aurait jamais écrite : la région quitte la copie de travail. Refaire la
/// rend.
#[test]
fn annuler_une_creation_rend_l_absence() {
    let st = monde_vide();
    let mut i = Interner::new();
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite([0, 0, 0], [20, 3, 20]),
        &remplir(&mut i, "minecraft:glass"),
        &i,
    )
    .unwrap();
    let apres = pleines(&st, ZERO);
    let mut journal = Journal::new();
    assert!(r
        .journaliser(&mut journal, "Remplir", "poser", Vec::new(), 0)
        .is_some());

    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert!(
        matches!(
            st.read_region(&SURFACE, DOSSIER, ZERO),
            Err(SourceError::NotFound)
        ),
        "la région créée disparaît avec ses chunks"
    );
    assert!(st.is_clean(), "rien ne reste dans la copie");

    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(pleines(&st, ZERO), apres);
}

/// Rien à écrire, rien de créé : un remplacement de pierre dans le vide ne
/// trouve pas de pierre, et ne doit pas semer des chunks vides pour rien.
#[test]
fn rien_a_ecrire_rien_de_cree() {
    let st = monde_vide();
    let mut i = Interner::new();
    let p = Plan::nouveau(
        Masque::Etat(i.intern("minecraft:stone")),
        Motif::Bloc(i.intern("minecraft:dirt")),
    );
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite([0, 0, 0], [63, 10, 63]),
        &p,
        &i,
    )
    .unwrap();
    assert_eq!(r.chunks_crees, 0);
    assert!(r.est_vide());
    assert!(st.is_clean(), "pas une région écrite");
}

/// **Un chunk laissé à mi-génération, dans un monde vide, devient un chunk vide
/// FINI** : le jeu n'y aurait rien mis de plus. Sans ça, une muraille qui
/// traverse la couronne du bord de la zone explorée y aurait des trous. Et
/// annuler rend le chunk D'ORIGINE, pas l'absence.
#[test]
fn dans_un_monde_vide_un_chunk_a_mi_generation_est_remplace() {
    let brut = tf_bench::avec_statut(
        &region(&Terrain::petite()),
        0,
        0,
        &[(1, 0)],
        "minecraft:noise",
    );
    let src = MemorySource::new();
    src.put_region(SURFACE, DOSSIER, ZERO, brut);
    let st = Staging::new(src, MemorySource::new()).avec_monde_vide(Some(vide()));
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let mut i = Interner::new();
    // Toute la hauteur du chunk (1, 0), pour que rien de l'ancien terrain n'y
    // survive s'il n'était pas remplacé.
    let sel = boite([16, -64, 0], [31, 319, 15]);
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &remplir(&mut i, "minecraft:glass"),
        &i,
    )
    .unwrap();
    assert_eq!(r.chunks_crees, 1);
    let dans_le_chunk: Vec<_> = pleines(&st, ZERO)
        .into_iter()
        .filter(|(p, _)| (16..32).contains(&p[0]) && (0..16).contains(&p[2]))
        .collect();
    assert_eq!(dans_le_chunk.len(), 16 * 16 * 384);
    assert!(dans_le_chunk.iter().all(|(_, n)| n == "minecraft:glass"));

    let mut journal = Journal::new();
    r.journaliser(&mut journal, "Remplir", "poser", Vec::new(), 0)
        .unwrap();
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    let rendu = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let charge = |o: &[u8]| {
        let r = read(o, 0, 0).unwrap();
        let c = r.get(1, 0).unwrap();
        inflate(&c.payload, c.compression).unwrap()
    };
    assert_eq!(
        charge(&rendu),
        charge(&avant),
        "le chunk inachevé d'origine"
    );
}

/// **Au-delà du plafond, on ne crée rien** : « tout sélectionner » ne doit pas
/// écrire des gigaoctets de vide en un clic. L'opération écrit ce qui existe,
/// et compte ce qu'elle a laissé.
#[test]
fn au_dela_du_plafond_on_ne_cree_rien() {
    let st = monde_vide();
    let mut i = Interner::new();
    // 129 × 128 colonnes de chunks : juste au-dessus de 16 384.
    let sel = boite([0, 0, 0], [129 * 16 - 1, 0, 128 * 16 - 1]);
    assert!(tf_ops::edition::chunks_dans(&sel) > CREATION_MAX);
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &remplir(&mut i, "minecraft:stone"),
        &i,
    )
    .unwrap();
    assert_eq!(r.chunks_crees, 0);
    assert_eq!(r.chunks_absents, tf_ops::edition::chunks_dans(&sel));
    assert!(st.is_clean());

    // Et juste AU plafond, la création est permise — vérifiée sur la règle,
    // sans écrire seize mille chunks. Les BLOCS seulement, la SURFACE
    // seulement, et depuis 1.18 seulement.
    let au_plafond = boite([0, 0, 0], [128 * 16 - 1, 0, 128 * 16 - 1]);
    assert_eq!(tf_ops::edition::chunks_dans(&au_plafond), CREATION_MAX);
    assert!(peut_creer(&st, &SURFACE, DOSSIER, &au_plafond).is_some());
    assert!(peut_creer(&st, &SURFACE, Folder::Entities, &au_plafond).is_none());
    assert!(peut_creer(&st, &Dimension::Nether, DOSSIER, &au_plafond).is_none());
    let ancien =
        Staging::new(MemorySource::new(), MemorySource::new()).avec_monde_vide(Some(MondeVide {
            data_version: tf_anvil::DV_1_18 - 1,
            biome: "minecraft:the_void".into(),
        }));
    assert!(peut_creer(&ancien, &SURFACE, DOSSIER, &au_plafond).is_none());
}

/// **Un monde normal ne crée JAMAIS** : même opération, même sélection, sans
/// monde vide — rien n'est écrit, et le compte rendu dit pourquoi.
#[test]
fn un_monde_normal_ne_cree_jamais() {
    let st = Staging::new(MemorySource::new(), MemorySource::new());
    let mut i = Interner::new();
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite([0, 60, 0], [47, 62, 15]),
        &remplir(&mut i, "minecraft:stone"),
        &i,
    )
    .unwrap();
    assert_eq!((r.chunks_crees, r.chunks_absents), (0, 3));
    assert!(st.is_clean());
}

/// **Un `//move` vers le vide d'un monde vide passe** : le collage créera les
/// chunks d'arrivée. Ailleurs, la garde le refuse (voir `deplacer.rs`).
#[test]
fn un_deplacement_vers_le_vide_d_un_monde_vide_passe() {
    let st = monde_vide();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let sel = boite([0, 60, 0], [3, 62, 3]);
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &remplir(&mut i, "minecraft:stone"),
        &i,
    )
    .unwrap();
    let r = deplacer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        Pas {
            d: [640, 0, 0],
            avec_air: false,
            air,
            compter: false,
        },
        air,
        &mut i,
    )
    .expect("dans un monde vide, l'arrivée se crée");
    assert!(r.chunks_crees >= 1);
    let arrivee = pleines(&st, RegionPos { x: 1, z: 0 });
    assert_eq!(arrivee.len(), 4 * 3 * 4);
    assert!(pleines(&st, ZERO).is_empty(), "la source est effacée");
}
