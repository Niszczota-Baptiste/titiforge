//! `//move` et `//stack` — les opérations COMPOSÉES, et leur annulation.
//!
//! Ce qu'elles ajoutent n'est pas du calcul : copier, effacer et coller
//! existaient. C'est leur enchaînement sous **une seule entrée de journal**,
//! et c'est là qu'est le piège : deux passes peuvent toucher le même chunk,
//! leurs correctifs s'enchaînent par leurs empreintes, et les rejouer dans
//! l'ordre d'enregistrement fait échouer le second. L'annulation les rejoue
//! donc À L'ENVERS — et c'est ce fichier qui le prouve, puisqu'aucune
//! opération simple ne repasse deux fois sur un chunk.

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, copier, deplacer, empiler};
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif, Pas};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::{Correction, Genre, Journal};
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn monde() -> (MemorySource, Vec<u8>) {
    let brut = region(&Terrain::peuplee(2));
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, brut.clone());
    (m, brut)
}

fn staging(src: MemorySource) -> Staging<MemorySource, MemorySource> {
    Staging::new(src, MemorySource::new())
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

fn pas(d: [i32; 3], air: tf_anvil::StateId) -> Pas {
    Pas {
        d,
        avec_air: true,
        air,
        compter: false,
    }
}

// ── déplacer ────────────────────────────────────────────────────────────────

#[test]
fn deplacer_emporte_le_contenu_et_laisse_le_remplissage() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:bedrock");

    // Une source reconnaissable, posée d'abord.
    let sel = boite((4, -40, 4), (11, -35, 11));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(marque));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &poser, &i).unwrap();

    let d = [64, 0, 64];
    deplacer(&st, &SURFACE, DOSSIER, &sel, pas(d, air), air, &mut i).unwrap();

    // Là-bas : la marque. Ici : de l'air.
    let arrivee = boite((68, -40, 68), (75, -35, 75));
    let la_bas = copier(&st, &SURFACE, DOSSIER, &arrivee, &mut i).unwrap();
    assert!(
        la_bas.blocs.iter().all(|b| *b == marque),
        "le contenu doit être arrivé entier"
    );
    let ici = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(
        ici.blocs.iter().all(|b| *b == air),
        "la source doit être remplie du remplissage"
    );
}

#[test]
fn deplacer_d_un_bloc_ne_s_efface_pas_lui_meme() {
    // Le cas qui casse une implémentation naïve : source et destination se
    // recouvrent presque entièrement. Effacer APRÈS avoir collé mangerait ce
    // qu'on vient de poser.
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:bedrock");

    let sel = boite((4, -40, 4), (11, -35, 11));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(marque));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &poser, &i).unwrap();

    deplacer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        pas([1, 0, 0], air),
        air,
        &mut i,
    )
    .unwrap();

    let apres = copier(
        &st,
        &SURFACE,
        DOSSIER,
        &boite((4, -40, 4), (12, -35, 11)),
        &mut i,
    )
    .unwrap();
    // La colonne d'origine est vidée, les huit suivantes portent la marque.
    for y in 0..6u32 {
        for z in 0..8u32 {
            assert_eq!(apres.get(0, y, z), Some(air), "la tranche libérée");
            for x in 1..9u32 {
                assert_eq!(apres.get(x, y, z), Some(marque), "le corps déplacé");
            }
        }
    }
}

#[test]
fn deplacer_emporte_les_coffres() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");

    let sel = boite((0, -64, 0), (15, 127, 15));
    let avant = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(!avant.entites.is_empty(), "rien à prouver sans coffre");

    let r = deplacer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        pas([64, 0, 64], air),
        air,
        &mut i,
    )
    .unwrap();
    assert_eq!(r.entites_posees, avant.entites.len() as u64);

    let arrivee = boite((64, -64, 64), (79, 127, 79));
    let la_bas = copier(&st, &SURFACE, DOSSIER, &arrivee, &mut i).unwrap();
    assert_eq!(la_bas.entites.len(), avant.entites.len());
    for (a, b) in avant.entites.iter().zip(&la_bas.entites) {
        assert_eq!(a.case, b.case, "la case LOCALE se conserve");
        assert_eq!(a.id(), b.id());
    }
    // Et il n'en reste aucun derrière : leur bloc a disparu avec la source.
    let ici = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(ici.entites.is_empty(), "aucun coffre fantôme à la source");
}

// ── empiler ─────────────────────────────────────────────────────────────────

#[test]
fn empiler_repete_l_extrait_a_pas_reguliers() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:bedrock");

    let sel = boite((0, -40, 0), (3, -38, 3));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(marque));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &poser, &i).unwrap();

    empiler(&st, &SURFACE, DOSSIER, &sel, pas([4, 0, 0], air), 3, &mut i).unwrap();

    // Quatre exemplaires : l'original plus trois.
    let tout = copier(
        &st,
        &SURFACE,
        DOSSIER,
        &boite((0, -40, 0), (15, -38, 3)),
        &mut i,
    )
    .unwrap();
    for x in 0..16u32 {
        assert_eq!(tout.get(x, 0, 0), Some(marque), "case x={x}");
    }
}

#[test]
fn empiler_zero_fois_ne_fait_rien() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let r = empiler(
        &st,
        &SURFACE,
        DOSSIER,
        &boite((0, -40, 0), (3, -38, 3)),
        pas([4, 0, 0], air),
        0,
        &mut i,
    )
    .unwrap();
    assert!(r.patches.is_empty());
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        brut,
        "la source reste octet pour octet ce qu'elle était"
    );
}

// ── l'annulation d'une opération à PLUSIEURS passes ──────────────────────────

/// **Le test que ce fichier existe pour porter.**
///
/// `//move` repasse deux fois sur les chunks que source et destination ont en
/// commun. Leurs correctifs s'enchaînent par leurs empreintes : rejoués dans
/// l'ordre d'enregistrement, le second échoue sur `Divergence`. Il faut les
/// rejouer À L'ENVERS pour annuler, et à l'endroit pour refaire.
#[test]
fn annuler_un_deplacement_qui_repasse_sur_le_meme_chunk() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:bedrock");

    let sel = boite((2, -40, 2), (9, -36, 9));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(marque));
    appliquer(&st, &SURFACE, DOSSIER, &sel, &poser, &i).unwrap();
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    // Un pas de deux blocs : tout tient dans le même chunk, donc les deux
    // passes y produisent chacune un correctif.
    let r = deplacer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        pas([2, 0, 0], air),
        air,
        &mut i,
    )
    .unwrap();
    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    assert_ne!(avant, apres, "le déplacement doit avoir écrit");

    let doublons = {
        let mut vus = std::collections::HashMap::<u16, usize>::new();
        for p in &r.patches {
            *vus.entry(p.cible.chunk).or_default() += 1;
        }
        vus.values().filter(|n| **n > 1).count()
    };
    assert!(
        doublons > 0,
        "le test ne prouve rien si aucun chunk n'est touché deux fois"
    );

    let mut journal = Journal::new();
    journal.pousser(
        "Déplacer",
        0,
        Genre::Operation {
            op: "move".into(),
            bounds: r.bornes,
            corrections: r.patches.into_iter().map(Correction::Chunk).collect(),
        },
    );

    let (entree, _) = journal.annuler().unwrap();
    let mut defait = apres.clone();
    for c in entree.a_annuler() {
        if let Correction::Chunk(p) = c {
            defait = rejouer(&defait, p, true);
        }
    }
    assert_eq!(
        charges(&defait),
        charges(&avant),
        "annuler doit rendre le monde d'avant"
    );

    let (entree, _) = journal.refaire().unwrap();
    let mut refait = defait.clone();
    for c in entree.a_refaire() {
        if let Correction::Chunk(p) = c {
            refait = rejouer(&refait, p, false);
        }
    }
    assert_eq!(
        charges(&refait),
        charges(&apres),
        "refaire doit rendre le monde d'après"
    );
}

/// Les charges INFLATÉES d'une région, chunk par chunk.
///
/// On compare ça et pas les octets du `.mca` : le niveau de compression et la
/// disposition des secteurs sont des choix d'écriture, pas du contenu. Deux
/// fichiers différents peuvent porter exactement le même monde — et c'est le
/// monde qu'une annulation doit rendre.
fn charges(region: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let r = tf_anvil::read(region, 0, 0).unwrap();
    let mut out: Vec<(u16, Vec<u8>)> = r
        .iter()
        .map(|c| {
            (
                c.index,
                tf_anvil::inflate(&c.payload, c.compression).unwrap(),
            )
        })
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Rejoue un correctif sur la région, dans un sens ou dans l'autre.
fn rejouer(region: &[u8], p: &tf_world::journal::ChunkPatch, annuler: bool) -> Vec<u8> {
    use std::borrow::Cow;
    use tf_anvil::{deflate, inflate, read, write};

    let mut r = read(region, 0, 0).unwrap();
    let (lx, lz) = ((p.cible.chunk % 32) as i32, (p.cible.chunk / 32) as i32);
    let brut = r.get_mut(lx, lz).expect("le chunk visé existe");
    let courant = inflate(&brut.payload, brut.compression).unwrap();
    let voulu = if annuler {
        p.undo(&courant).expect("annuler doit s'appliquer")
    } else {
        p.redo(&courant).expect("refaire doit s'appliquer")
    };
    brut.payload = Cow::Owned(deflate(&voulu, brut.compression).unwrap());
    write(&r).unwrap().region
}
