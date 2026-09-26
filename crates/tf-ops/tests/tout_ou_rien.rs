//! **Une opération qui échoue en route ne laisse rien.**
//!
//! Une opération écrit au fil de l'eau : une région après l'autre, la source
//! d'un `//move` avant sa destination, la copie d'un `//stack` après la
//! précédente. Qu'une écriture refuse — disque plein, droits, fichier tenu —
//! et ce qui était déjà écrit restait dans la copie de travail SANS entrée de
//! journal : invisible à l'annulation, et de quoi faire diverger celle des
//! actions précédentes sur les mêmes chunks. Pour un `//move`, c'était un
//! build effacé dont la copie n'arrivait jamais.
//!
//! La panne est PROVOQUÉE (`MemorySource::tomber_en_panne`) : sans elle, le
//! chemin d'erreur n'est jamais pris, donc jamais vérifié.

use tf_anvil::Interner;
use tf_bench::mobiles::{region_entites, Occupant, DV_1_18_2};
use tf_bench::poi::{region_poi, SectionPoi};
use tf_bench::{region, region_en, Terrain};
use tf_ops::edition::{appliquer, coller, copier, deplacer, empiler, rejouer, Erreur, Pas, Sens};
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource, Panne};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };
const R1: RegionPos = RegionPos { x: 1, z: 0 };
type St = Staging<MemorySource, MemorySource>;

/// Deux régions CONTIGUËS de terrain, pleines en largeur et basses : x ∈
/// [0, 512) dans r.0.0, x ∈ [512, 1024) dans r.1.0, y ∈ [−64, 0).
fn monde() -> St {
    let t = Terrain {
        side: 32,
        sections: 4,
        ..Terrain::default()
    };
    let src = MemorySource::new();
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&t),
    );
    src.put_region(SURFACE, Folder::Region, R1, region_en(&t, 1, 0));
    Staging::new(src, MemorySource::new())
}

fn plan(bloc: &str, i: &mut Interner) -> Plan {
    Plan::nouveau(Masque::Tout, Motif::Bloc(i.intern(bloc)))
}

fn etat(st: &St, p: BlockPos) -> String {
    let mut i = Interner::new();
    let pr = copier(st, &SURFACE, Folder::Region, &BBox::single(p), &mut i).unwrap();
    i.resolve(pr.blocs[0]).unwrap().to_string()
}

fn boite(a: (i32, i32, i32), b: (i32, i32, i32)) -> BBox {
    BBox::new(BlockPos::new(a.0, a.1, a.2), BlockPos::new(b.0, b.1, b.2))
}

fn pas(d: [i32; 3], i: &mut Interner) -> Pas {
    Pas {
        d,
        avec_air: false,
        air: i.intern("minecraft:air"),
        compter: false,
    }
}

/// **Un `//set` sur deux régions, dont la seconde refuse** : la première est
/// défaite, la copie de travail revient à la save — et l'action d'AVANT, qui
/// avait écrit dans les mêmes chunks, se défait encore.
#[test]
fn un_set_dont_la_seconde_region_refuse_ne_laisse_rien() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    // L'action d'avant : de la terre sur les mêmes chunks de r.0.0.
    let avant = boite((240, -60, 10), (250, -58, 12));
    let origine = etat(&st, BlockPos::new(245, -60, 10));
    let r = appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &avant,
        &plan("minecraft:dirt", &mut i),
        &i,
    )
    .unwrap();
    r.journaliser(&mut journal, "terre", "poser", Vec::new(), 0)
        .unwrap();

    // Le `//set` qui traverse les deux régions.
    st.overlay().tomber_en_panne(Panne {
        region: Some(R1),
        ..Default::default()
    });
    let sel = boite((240, -60, 10), (520, -58, 12));
    let r = appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &sel,
        &plan("minecraft:gold_block", &mut i),
        &i,
    );
    assert!(matches!(r, Err(Erreur::Source(_))), "{r:?}");
    assert_eq!(
        etat(&st, BlockPos::new(245, -60, 10)),
        "minecraft:dirt",
        "la région écrite avant la panne est restée"
    );

    // L'action d'avant se défait : ses empreintes tiennent toujours.
    st.overlay().tomber_en_panne(Panne::default());
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).expect("l'annulation d'avant diverge");
    assert_eq!(etat(&st, BlockPos::new(245, -60, 10)), origine);
    assert!(st.touched().is_empty(), "{:?}", st.touched());
}

/// **Un `//move` dont la copie n'arrive pas ne perd pas le build.** La
/// première passe EFFACE la source ; que le collage refuse ensuite, et le
/// build disparaissait — le cas même que le refus du terrain absent devait
/// empêcher, arrivé par un autre chemin.
#[test]
fn un_move_dont_la_copie_refuse_ne_perd_pas_le_build() {
    let st = monde();
    let mut i = Interner::new();
    let build = boite((240, -60, 10), (244, -58, 14));
    appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &build,
        &plan("minecraft:bricks", &mut i),
        &i,
    )
    .unwrap();
    st.overlay().tomber_en_panne(Panne {
        region: Some(R1),
        ..Default::default()
    });
    let air = i.intern("minecraft:air");
    let r = deplacer(
        &st,
        &SURFACE,
        Folder::Region,
        &build,
        pas([300, 0, 0], &mut i),
        air,
        &mut i,
    );
    assert!(matches!(r, Err(Erreur::Source(_))), "{r:?}");
    for p in [BlockPos::new(240, -60, 10), BlockPos::new(244, -58, 14)] {
        assert_eq!(
            etat(&st, p),
            "minecraft:bricks",
            "le build a été effacé en {p:?}"
        );
    }
}

/// **Un `//stack` qui échoue à la deuxième copie** défait la première — et
/// laisse l'original.
#[test]
fn un_stack_qui_echoue_en_route_defait_les_copies_d_avant() {
    let st = monde();
    let mut i = Interner::new();
    // Un mur au bout de r.0.0 ; la première copie y reste, la deuxième
    // tombe dans r.1.0, qui refuse.
    let mur = boite((480, -60, 10), (495, -58, 10));
    appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &mur,
        &plan("minecraft:bricks", &mut i),
        &i,
    )
    .unwrap();
    let premiere = BlockPos::new(496, -60, 10);
    let avant = etat(&st, premiere);
    st.overlay().tomber_en_panne(Panne {
        region: Some(R1),
        ..Default::default()
    });
    let r = empiler(
        &st,
        &SURFACE,
        Folder::Region,
        &mur,
        pas([16, 0, 0], &mut i),
        3,
        &mut i,
    );
    assert!(matches!(r, Err(Erreur::Source(_))), "{r:?}");
    assert_eq!(etat(&st, premiere), avant, "la première copie est restée");
    assert_eq!(etat(&st, mur.min), "minecraft:bricks");
}

/// Et si même défaire échoue, l'erreur le DIT.
#[test]
fn un_echec_qu_on_ne_peut_pas_defaire_se_dit() {
    let st = monde();
    let mut i = Interner::new();
    // Une seule écriture de région passe : r.0.0. Celle de r.1.0 refuse, et
    // la réécriture de r.0.0 pour la défaire aussi.
    st.overlay().tomber_en_panne(Panne {
        budget: Some(1),
        ..Default::default()
    });
    let sel = boite((240, -60, 10), (520, -58, 12));
    let r = appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &sel,
        &plan("minecraft:gold_block", &mut i),
        &i,
    );
    match r {
        Err(e @ Erreur::AMoitie { .. }) => {
            assert!(e.to_string().contains("n'a pas pu être défait"), "{e}")
        }
        autre => panic!("{autre:?}"),
    }
}

// ── les passes qui ne sont pas des blocs ────────────────────────────────────

/// Le terrain de r.0.0, un porte-armure en (5,5 ; 64 ; 5,5), et les points
/// d'intérêt d'une section du chunk (0, 0).
fn monde_habite() -> St {
    let src = MemorySource::new();
    src.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    src.put_region(
        SURFACE,
        Folder::Entities,
        ZERO,
        region_entites(
            0,
            0,
            &[(
                0,
                0,
                DV_1_18_2,
                vec![Occupant::nouveau(
                    "minecraft:armor_stand",
                    [5.5, 64.0, 5.5],
                    0.0,
                    [1, 2, 3, 4],
                )],
            )],
        ),
    );
    src.put_region(
        SURFACE,
        Folder::Poi,
        ZERO,
        region_poi(
            0,
            0,
            &[(
                0,
                0,
                vec![SectionPoi {
                    y: -2,
                    valid: Some(true),
                    lits: vec![[3, -30, 4]],
                }],
            )],
        ),
    );
    Staging::new(src, MemorySource::new())
}

fn en_panne(st: &St, dossier: Folder) {
    st.overlay().tomber_en_panne(Panne {
        region: Some(ZERO),
        dossier: Some(dossier),
        ..Default::default()
    });
}

/// **Les points d'intérêt refusent APRÈS les blocs** : les blocs sont
/// défaits. Sans ça, un `//set` échoué gardait ses blocs, que la table des
/// lits ignorait.
#[test]
fn un_set_dont_les_points_d_interet_refusent_ne_laisse_rien() {
    let st = monde_habite();
    let mut i = Interner::new();
    let p = BlockPos::new(3, -30, 4);
    let avant = etat(&st, p);
    en_panne(&st, Folder::Poi);
    let r = appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &BBox::single(p),
        &plan("minecraft:gold_block", &mut i),
        &i,
    );
    // L'échec vient bien de la passe visée, pas des blocs.
    assert!(
        matches!(&r, Err(e @ Erreur::Source(_)) if e.to_string().contains("(Poi)")),
        "{r:?}"
    );
    assert_eq!(etat(&st, p), avant);
    assert!(st.touched().is_empty(), "{:?}", st.touched());
}

/// **Un `//move` dont les ENTITÉS refusent** ne perd pas le build : la
/// source effacée et la copie posée sont défaites toutes les deux.
#[test]
fn un_move_dont_les_entites_refusent_ne_perd_pas_le_build() {
    let st = monde_habite();
    let mut i = Interner::new();
    let build = boite((2, 60, 2), (6, 64, 6));
    appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &boite((2, 60, 2), (6, 62, 6)),
        &plan("minecraft:bricks", &mut i),
        &i,
    )
    .unwrap();
    let arrivee = BlockPos::new(34, 60, 2);
    let avant = etat(&st, arrivee);
    en_panne(&st, Folder::Entities);
    let air = i.intern("minecraft:air");
    let r = deplacer(
        &st,
        &SURFACE,
        Folder::Region,
        &build,
        pas([32, 0, 0], &mut i),
        air,
        &mut i,
    );
    // L'échec vient bien de la passe visée, pas des blocs.
    assert!(
        matches!(&r, Err(e @ Erreur::Source(_)) if e.to_string().contains("(Entities)")),
        "{r:?}"
    );
    assert_eq!(etat(&st, BlockPos::new(2, 60, 2)), "minecraft:bricks");
    assert_eq!(etat(&st, arrivee), avant);
}

/// **Un collage dont les entités refusent** défait ses blocs.
#[test]
fn un_collage_dont_les_entites_refusent_ne_laisse_rien() {
    let st = monde_habite();
    let mut i = Interner::new();
    appliquer(
        &st,
        &SURFACE,
        Folder::Region,
        &boite((2, 60, 2), (6, 62, 6)),
        &plan("minecraft:bricks", &mut i),
        &i,
    )
    .unwrap();
    let p = copier(
        &st,
        &SURFACE,
        Folder::Region,
        &boite((2, 60, 2), (6, 64, 6)),
        &mut i,
    )
    .unwrap();
    assert_eq!(p.mobiles.len(), 1, "le porte-armure vient avec");
    let arrivee = BlockPos::new(34, 60, 2);
    let avant = etat(&st, arrivee);
    en_panne(&st, Folder::Entities);
    let r = coller(
        &st,
        &SURFACE,
        Folder::Region,
        &p,
        arrivee,
        pas([0, 0, 0], &mut i),
        &i,
    );
    // L'échec vient bien de la passe visée, pas des blocs.
    assert!(
        matches!(&r, Err(e @ Erreur::Source(_)) if e.to_string().contains("(Entities)")),
        "{r:?}"
    );
    assert_eq!(etat(&st, arrivee), avant);
}
