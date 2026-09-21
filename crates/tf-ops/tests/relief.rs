//! Le lissage — et l'unité qui traverse la frontière.
//!
//! `ExeWorldEdit` a payé le piège en entier : `toHeights` rendait des BLOCS,
//! `applyHeightmap` attendait un rapport 0..1, chaque moitié passait ses
//! tests, et ensemble toute cellule non nulle devenait 1 — un plateau plat à
//! la place du relief, 1 022 cellules fausses sur 1 024. Ici les deux moitiés
//! sont testées ET leur jonction : on relève une carte d'un vrai monde, on la
//! lisse, on l'applique, puis on RELÈVE À NOUVEAU et on compare.

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, copier};
use tf_ops::plan::Plan;
use tf_ops::relief::{relever, Carte, Lissage, SANS_SOL};
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn staging() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&Terrain::petite()));
    Staging::new(m, MemorySource::new())
}

fn boite(a: (i32, i32, i32), b: (i32, i32, i32)) -> BBox {
    BBox::new(
        BlockPos {
            x: a.0,
            y: a.1,
            z: a.2,
        },
        BlockPos {
            x: b.0,
            y: b.1,
            z: b.2,
        },
    )
}

// ── la carte, sans monde ────────────────────────────────────────────────────

#[test]
fn lisser_une_marche_la_rabote() {
    // Une falaise d'un bloc de large : la moyenne doit l'étaler.
    let mut c = Carte::vide(0, 0, 8, 1);
    for x in 0..8 {
        c.set(x, 0, if x < 4 { 0 } else { 8 });
    }
    let l = c.lissee(1, 1);
    // Le bord descend, le creux monte, et rien ne dépasse les extrêmes.
    assert!(l.get(3, 0).unwrap() > 0, "le pied de la marche monte");
    assert!(l.get(4, 0).unwrap() < 8, "le sommet descend");
    for x in 0..8 {
        let v = l.get(x, 0).unwrap();
        assert!(
            (0..=8).contains(&v),
            "la moyenne ne dépasse pas les extrêmes"
        );
    }
}

#[test]
fn un_plateau_reste_un_plateau() {
    // La propriété qui manquait à `ExeWorldEdit` : lisser du plat ne doit RIEN
    // changer. Une moyenne qui déborderait de la carte tirerait les bords vers
    // zéro et creuserait une cuvette.
    let mut c = Carte::vide(-10, -10, 12, 12);
    for z in -10..2 {
        for x in -10..2 {
            c.set(x, z, 42);
        }
    }
    assert_eq!(c.lissee(2, 3), c, "le plat lissé reste plat, bords compris");
}

#[test]
fn une_colonne_sans_sol_ne_tire_personne_vers_le_bas() {
    // `SANS_SOL` n'est pas une hauteur basse. Le compter comme zéro
    // creuserait une fosse au bord de chaque sélection — là où l'utilisateur
    // regarde.
    let mut c = Carte::vide(0, 0, 3, 1);
    c.set(0, 0, 100);
    c.set(2, 0, 100);
    // la colonne 1 reste SANS_SOL
    let l = c.lissee(1, 1);
    assert_eq!(l.get(0, 0), Some(100), "la moyenne ignore le vide");
    assert_eq!(l.get(2, 0), Some(100));
    assert_eq!(l.get(1, 0), None, "et ne remplit pas ce qui n'existe pas");
    assert_eq!(l.h[1], SANS_SOL);
}

#[test]
fn les_hauteurs_negatives_se_moyennent_par_division_plancher() {
    // Le monde descend à −64 depuis 1.18. Une division qui tronque vers zéro
    // remonterait le relief d'un bloc sous y = 0 et pas au-dessus : une
    // marche d'un bloc à l'altitude zéro, exactement là où personne ne la
    // cherche.
    let mut c = Carte::vide(0, 0, 2, 1);
    c.set(0, 0, -3);
    c.set(1, 0, -4);
    let l = c.lissee(1, 1);
    // (−3 + −4) / 2 = −3,5 → −4 en plancher, −3 en troncature.
    assert_eq!(l.get(0, 0), Some(-4));
    assert_eq!(l.get(1, 0), Some(-4));
}

#[test]
fn un_rayon_nul_ne_change_rien() {
    let mut c = Carte::vide(0, 0, 4, 4);
    for z in 0..4 {
        for x in 0..4 {
            c.set(x, z, x * 7 - z * 3);
        }
    }
    assert_eq!(c.lissee(0, 5), c);
}

#[test]
fn resserrer_garde_les_bonnes_colonnes() {
    let mut c = Carte::vide(-2, -2, 6, 6);
    for z in -2..4 {
        for x in -2..4 {
            c.set(x, z, x * 100 + z);
        }
    }
    let r = c.resserree(0, 0, 2, 2);
    assert_eq!(r.x0, 0);
    assert_eq!(r.get(0, 0), Some(0));
    assert_eq!(r.get(1, 1), Some(101));
    assert_eq!(r.get(2, 2), None, "hors de la boîte resserrée");
}

// ── la jonction : relever → lisser → appliquer → RELEVER À NOUVEAU ──────────

/// **Le test que ce fichier existe pour porter.**
///
/// Chaque moitié peut être juste et leur raccord faux : c'est très exactement
/// ce qui a produit un plateau plat à la place d'un relief dans
/// `ExeWorldEdit`. On traverse donc la frontière dans les deux sens et on
/// compare des HAUTEURS, pas des comptes.
#[test]
fn une_marche_posee_dans_le_monde_ressort_lissee() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let roche = i.intern("minecraft:bedrock");
    let solide = Masque::Non(Box::new(Masque::Etat(air)));

    // On se fabrique un relief net : une colline de 8 blocs sur la moitié
    // d'une zone, plate ailleurs.
    let zone = boite((0, -60, 0), (31, -20, 31));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &zone,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(air)),
        &i,
    )
    .unwrap();
    let bas = boite((0, -60, 0), (31, -50, 31));
    let haut = boite((16, -60, 0), (31, -42, 31));
    for b in [bas, haut] {
        appliquer(
            &st,
            &SURFACE,
            DOSSIER,
            &b,
            &Plan::nouveau(Masque::Tout, Motif::Bloc(roche)),
            &i,
        )
        .unwrap();
    }

    let avant = relever(&st, &SURFACE, DOSSIER, &zone, &solide, &mut i).unwrap();
    assert_eq!(avant.get(0, 0), Some(-50), "le plateau bas");
    assert_eq!(avant.get(31, 0), Some(-42), "le plateau haut");
    assert_eq!(
        avant.get(15, 0).unwrap() - avant.get(16, 0).unwrap(),
        -8,
        "une marche franche de huit blocs"
    );

    let voulue = avant.lissee(3, 2);
    let op = Lissage {
        carte: &voulue,
        vide: air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &zone, &op, &i).unwrap();
    assert!(r.blocs.unwrap() > 0, "le lissage doit écrire");

    // ── et on RELÈVE À NOUVEAU : c'est ça, traverser la frontière.
    let apres = relever(&st, &SURFACE, DOSSIER, &zone, &solide, &mut i).unwrap();
    for z in 0..32 {
        for x in 0..32 {
            assert_eq!(
                apres.get(x, z),
                voulue.get(x, z),
                "la colonne ({x}, {z}) ne porte pas la hauteur demandée"
            );
        }
    }
    // La marche ne fait plus huit blocs d'un coup.
    let saut = (apres.get(15, 0).unwrap() - apres.get(16, 0).unwrap()).abs();
    assert!(
        saut < 8,
        "la marche doit être rabotée, elle fait encore {saut}"
    );
}

#[test]
fn lisser_deux_fois_ne_reecrit_rien_la_seconde() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let roche = i.intern("minecraft:bedrock");
    let solide = Masque::Non(Box::new(Masque::Etat(air)));
    let zone = boite((0, -60, 0), (31, -20, 31));

    // Il faut du RELIEF : la fixture est stratifiée horizontalement, donc
    // toutes ses colonnes ont la même hauteur et le lissage n'aurait rien à
    // faire. Un test qui passe sur zéro écriture ne prouve rien.
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &zone,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(air)),
        &i,
    )
    .unwrap();
    for b in [
        boite((0, -60, 0), (31, -50, 31)),
        boite((16, -60, 0), (31, -42, 31)),
    ] {
        appliquer(
            &st,
            &SURFACE,
            DOSSIER,
            &b,
            &Plan::nouveau(Masque::Tout, Motif::Bloc(roche)),
            &i,
        )
        .unwrap();
    }

    let carte = relever(&st, &SURFACE, DOSSIER, &zone, &solide, &mut i)
        .unwrap()
        .lissee(2, 1);
    let op = Lissage {
        carte: &carte,
        vide: air,
        compter: false,
    };
    let un = appliquer(&st, &SURFACE, DOSSIER, &zone, &op, &i).unwrap();
    assert!(!un.patches.is_empty(), "la première passe doit écrire");
    let deux = appliquer(&st, &SURFACE, DOSSIER, &zone, &op, &i).unwrap();
    assert!(
        deux.patches.is_empty(),
        "{} correctif(s) pour zéro changement",
        deux.patches.len()
    );
}

#[test]
fn monter_une_colonne_prolonge_son_bloc_de_surface() {
    // Reprendre le bloc de surface, et pas un bloc fixe : c'est ce qui fait
    // qu'une colline d'herbe reste en herbe et une dune de sable en sable.
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let sable = i.intern("minecraft:sand");

    let zone = boite((4, -40, 4), (5, -20, 5));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &zone,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(air)),
        &i,
    )
    .unwrap();
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite((4, -40, 4), (5, -35, 5)),
        &Plan::nouveau(Masque::Tout, Motif::Bloc(sable)),
        &i,
    )
    .unwrap();

    let mut carte = Carte::vide(4, 4, 2, 2);
    for z in 4..6 {
        for x in 4..6 {
            carte.set(x, z, -30); // cinq blocs plus haut
        }
    }
    let op = Lissage {
        carte: &carte,
        vide: air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &zone, &op, &i).unwrap();
    assert_eq!(r.blocs, Some(4 * 5), "quatre colonnes de cinq blocs");

    let relu = copier(&st, &SURFACE, DOSSIER, &zone, &mut i).unwrap();
    for y in -34..=-30 {
        assert_eq!(
            relu.get(0, (y - zone.min.y) as u32, 0),
            Some(sable),
            "le prolongement en y={y} doit être du sable, pas un bloc inventé"
        );
    }
    assert_eq!(
        relu.get(0, (-29 - zone.min.y) as u32, 0),
        Some(air),
        "et pas un bloc de plus"
    );
}

#[test]
fn une_carte_sans_sol_ne_cree_pas_de_terrain() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let zone = boite((8, -40, 8), (9, -20, 9));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &zone,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(air)),
        &i,
    )
    .unwrap();

    // Une carte qui demande du sol là où il n'y en a pas.
    let mut carte = Carte::vide(8, 8, 2, 2);
    for z in 8..10 {
        for x in 8..10 {
            carte.set(x, z, -25);
        }
    }
    let op = Lissage {
        carte: &carte,
        vide: air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &zone, &op, &i).unwrap();
    assert_eq!(
        r.blocs,
        Some(0),
        "sans surface à prolonger, on n'invente pas de terrain"
    );
}
