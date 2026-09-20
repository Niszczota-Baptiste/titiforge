//! `//naturalize` — et la portée `Colonne` qui la rend possible.
//!
//! Le défaut qu'on ferme ici est visible et inexplicable : « la première
//! couche solide » est une propriété de la COLONNE, et une opération qui la
//! décide section par section pose une bande d'herbe tous les seize blocs, au
//! milieu de chaque falaise. Le test qui compte vise donc **une colonne qui
//! traverse plusieurs sections**, parce que c'est le seul cas où le défaut
//! apparaît.

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, copier};
use tf_ops::plan::Plan;
use tf_ops::{Colonnes, Masque, Motif, Naturaliser, Portee};
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

struct Blocs {
    air: u32,
    herbe: u32,
    terre: u32,
    roche: u32,
    pierre: u32,
}

fn blocs(i: &mut Interner) -> Blocs {
    Blocs {
        air: i.intern("minecraft:air"),
        herbe: i.intern("minecraft:grass_block"),
        terre: i.intern("minecraft:dirt"),
        roche: i.intern("minecraft:deepslate"),
        pierre: i.intern("minecraft:stone"),
    }
}

// ── la règle, sans monde ────────────────────────────────────────────────────

#[test]
fn la_couche_suit_la_coupe_de_minecraft() {
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let n = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    assert_eq!(n.couche(0), b.herbe, "un bloc d'herbe");
    for k in 1..=3 {
        assert_eq!(n.couche(k), b.terre, "trois de terre");
    }
    for k in 4..40 {
        assert_eq!(n.couche(k), b.roche, "et le reste");
    }
}

#[test]
fn la_profondeur_est_reglable() {
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let mut n = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    n.profondeur = 0;
    assert_eq!(n.couche(0), b.herbe);
    assert_eq!(n.couche(1), b.roche, "sans sous-sol, la roche suit l'herbe");
}

#[test]
fn elle_declare_lire_la_colonne() {
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let n = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    assert_eq!(
        tf_ops::plan::Operation::portee(&n),
        Portee::Colonne,
        "sans ça, `edition.rs` l'appellerait section par section"
    );
}

// ── la vue par colonne ──────────────────────────────────────────────────────

#[test]
fn une_colonne_vide_ne_rend_rien_plutot_que_de_l_air() {
    // « Pas de section ici » et « c'est de l'air » ne sont PAS la même chose.
    // Les confondre écrit de la pierre là où il n'y a rien à écrire — le
    // piège `cold_read`, sous une autre forme.
    let c = Colonnes::depuis(Vec::new());
    assert!(c.est_vide());
    assert_eq!(c.bornes_y(), None);
    assert_eq!(c.get(0, 0, 0), None);
}

// ── sur un vrai chunk ───────────────────────────────────────────────────────

/// Le test qui porte tout : **une colonne qui traverse plusieurs sections**.
///
/// Une opération de section y poserait de l'herbe à chaque frontière. Ici, il
/// n'y en a qu'une, tout en haut du solide.
#[test]
fn une_colonne_a_traversee_de_sections_n_a_qu_une_surface() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);

    // Un pilier plein de 40 blocs — donc trois sections — posé à la main.
    let colonne = boite((4, -60, 4), (4, -21, 4));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(b.pierre));
    appliquer(&st, &SURFACE, DOSSIER, &colonne, &poser, &i).unwrap();
    // …et de l'air au-dessus, pour que le sommet soit vraiment un sommet.
    let ciel = boite((4, -20, 4), (4, 0, 4));
    let vider = Plan::nouveau(Masque::Tout, Motif::Bloc(b.air));
    appliquer(&st, &SURFACE, DOSSIER, &ciel, &vider, &i).unwrap();

    let sel = boite((4, -64, 4), (4, 0, 4));
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air).en_comptant();
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    assert!(r.blocs.unwrap() > 0, "la naturalisation doit écrire");

    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    let a = |wy: i32| relu.get(0, (wy - sel.min.y) as u32, 0);

    // Exactement UNE herbe dans toute la colonne, et elle est au sommet.
    let herbes: Vec<i32> = (sel.min.y..=sel.max.y)
        .filter(|&y| a(y) == Some(b.herbe))
        .collect();
    assert_eq!(
        herbes,
        vec![-21],
        "une seule surface, au sommet du solide — pas une par section"
    );
    // Puis trois terres, puis de la roche.
    for y in -24..=-22 {
        assert_eq!(a(y), Some(b.terre), "sous-sol en y={y}");
    }
    for y in -60..=-25 {
        assert_eq!(a(y), Some(b.roche), "roche en y={y}");
    }
    // Et le ciel est resté du ciel.
    assert_eq!(a(-20), Some(b.air));
}

/// Une grotte ne remet PAS le compteur à zéro : sinon son plafond porterait
/// de l'herbe, à l'envers, sous terre.
#[test]
fn une_cavite_ne_fait_pas_repousser_d_herbe_sous_terre() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);

    let colonne = boite((6, -60, 6), (6, -21, 6));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &colonne,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(b.pierre)),
        &i,
    )
    .unwrap();
    // Le ciel, et une cavité de trois blocs au milieu.
    for creux in [
        boite((6, -20, 6), (6, 0, 6)),
        boite((6, -40, 6), (6, -38, 6)),
    ] {
        appliquer(
            &st,
            &SURFACE,
            DOSSIER,
            &creux,
            &Plan::nouveau(Masque::Tout, Motif::Bloc(b.air)),
            &i,
        )
        .unwrap();
    }

    let sel = boite((6, -64, 6), (6, 0, 6));
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();

    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    let a = |wy: i32| relu.get(0, (wy - sel.min.y) as u32, 0);
    let herbes: Vec<i32> = (sel.min.y..=sel.max.y)
        .filter(|&y| a(y) == Some(b.herbe))
        .collect();
    assert_eq!(herbes, vec![-21], "une seule herbe, malgré la cavité");
    assert_eq!(
        a(-41),
        Some(b.roche),
        "le plancher de la cavité reste roche"
    );
    assert_eq!(a(-40), Some(b.air), "et la cavité reste vide");
}

#[test]
fn naturaliser_ne_touche_pas_hors_de_la_selection() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);

    let tout = boite((0, -64, 0), (15, 0, 15));
    let avant = copier(&st, &SURFACE, DOSSIER, &tout, &mut i).unwrap();

    let sel = boite((2, -50, 2), (5, -40, 5));
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    let bornes = r.bornes.expect("elle a écrit quelque chose");
    assert!(
        bornes.min.x >= sel.min.x
            && bornes.max.x <= sel.max.x
            && bornes.min.y >= sel.min.y
            && bornes.max.y <= sel.max.y
            && bornes.min.z >= sel.min.z
            && bornes.max.z <= sel.max.z,
        "les bornes {bornes:?} sortent de la sélection {sel:?}"
    );

    let apres = copier(&st, &SURFACE, DOSSIER, &tout, &mut i).unwrap();
    for p in (tout.min.y..=tout.max.y).flat_map(|y| {
        (tout.min.z..=tout.max.z)
            .flat_map(move |z| (tout.min.x..=tout.max.x).map(move |x| BlockPos { x, y, z }))
    }) {
        if sel.contains(p) {
            continue;
        }
        let k = (
            (p.x - tout.min.x) as u32,
            (p.y - tout.min.y) as u32,
            (p.z - tout.min.z) as u32,
        );
        assert_eq!(
            apres.get(k.0, k.1, k.2),
            avant.get(k.0, k.1, k.2),
            "la case {p:?} est hors sélection et a changé"
        );
    }
}

/// **Naturaliser deux fois ne doit rien écrire la seconde.**
///
/// Une opération qui ne change rien ne doit pas salir le chunk : sinon elle
/// remplit le journal d'annulation d'entrées vides, et `Ctrl+Z` défait des
/// non-changements.
#[test]
fn naturaliser_deux_fois_ne_change_rien_la_seconde() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let sel = boite((0, -64, 0), (15, 0, 15));
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);

    let un = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    assert!(!un.patches.is_empty(), "la première passe doit écrire");
    let deux = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    assert!(
        deux.patches.is_empty(),
        "la seconde passe a produit {} correctif(s) pour zéro changement",
        deux.patches.len()
    );
}

/// **L'invariant n° 4, dans le chemin par colonne.**
///
/// La palette ne dédoublonne pas : après un `//replace` qui fusionne deux
/// états, le même état y figure deux fois, et deux indices différents
/// désignent la même chose. Chercher l'état avec un `position()` sans
/// comparer d'abord ce que la case porte DÉJÀ réécrit les cases de la seconde
/// occurrence vers la première — même valeur, indice différent, donc des
/// octets différents et un correctif de journal pour rien.
///
/// C'est la troisième forme du même piège, après `Collage` et l'étage bloc.
#[test]
fn une_palette_dedoublonnee_ne_fait_pas_reecrire() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let sel = boite((0, -64, 0), (15, 0, 15));

    // On DÉDOUBLONNE la palette exprès : `//replace pierre → roche` sur des
    // sections qui contiennent déjà de la roche. L'étage palette écrase
    // l'entrée sur place sans fusionner — c'est voulu, et c'est ce qui rend
    // le chemin rapide possible.
    let fusion = Plan::nouveau(Masque::Etat(b.pierre), Motif::Bloc(b.roche));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &fusion, &i).unwrap();

    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    let encore = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    assert!(
        encore.patches.is_empty(),
        "{} correctif(s) sur une palette dédoublonnée, pour zéro changement",
        encore.patches.len()
    );
}

/// **Le compte doit être celui des cases qui changent VRAIMENT.**
///
/// C'est ce test, et pas l'idempotence, qui attrape l'invariant n° 4 dans le
/// chemin par colonne. Sur une palette dédoublonnée, chercher l'état avec un
/// `position()` sans comparer d'abord ce que la case porte déjà réécrit les
/// cases de la seconde occurrence vers la première : même valeur, indice
/// différent. Le monde ne change pas, mais les octets si — et le compte, lui,
/// annonce un travail qui n'a pas eu lieu.
///
/// L'idempotence ne le voit pas : à la seconde passe, toutes les cases
/// pointent déjà sur la première occurrence. C'est la PREMIÈRE passe qu'il
/// faut regarder, et elle écrit légitimement par ailleurs — donc on compare
/// le compte annoncé à la différence mesurée, case par case.
#[test]
fn le_compte_est_celui_des_cases_qui_changent_vraiment() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);
    let sel = boite((0, -64, 0), (15, 0, 15));

    // On dédoublonne la palette exprès : l'étage palette écrase l'entrée sur
    // place sans fusionner, et c'est ce qui rend le chemin rapide possible.
    let fusion = Plan::nouveau(Masque::Etat(b.pierre), Motif::Bloc(b.roche));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &fusion, &i).unwrap();

    let avant = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air).en_comptant();
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();
    let apres = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();

    let differentes = avant
        .blocs
        .iter()
        .zip(&apres.blocs)
        .filter(|(a, b)| a != b)
        .count() as u64;
    assert!(differentes > 0, "le test ne prouve rien sans changement");
    assert_eq!(
        r.blocs,
        Some(differentes),
        "le compte annonce un travail qui n'a pas eu lieu"
    );
}

/// **Le ciel reste le ciel.** Le défaut « solide » est « tout sauf l'air » ;
/// un `Masque::Tout` remplirait la colonne entière de pierre jusqu'en haut,
/// et un test qui ne compte que des cases ne le verrait pas.
#[test]
fn naturaliser_ne_remplit_pas_le_ciel() {
    let st = staging();
    let mut i = Interner::new();
    let b = blocs(&mut i);

    let colonne = boite((9, -60, 9), (9, -30, 9));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &colonne,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(b.pierre)),
        &i,
    )
    .unwrap();
    let ciel = boite((9, -29, 9), (9, 0, 9));
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &ciel,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(b.air)),
        &i,
    )
    .unwrap();

    let sel = boite((9, -64, 9), (9, 0, 9));
    let nat = Naturaliser::nouveau(b.herbe, b.terre, b.roche, b.air);
    appliquer(&st, &SURFACE, DOSSIER, &sel, &nat, &i).unwrap();

    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    for y in -29..=0 {
        assert_eq!(
            relu.get(0, (y - sel.min.y) as u32, 0),
            Some(b.air),
            "le ciel en y={y} doit rester du ciel"
        );
    }
    assert_eq!(relu.get(0, (-30 - sel.min.y) as u32, 0), Some(b.herbe));
}
