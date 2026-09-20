//! **Un build déplacé emporte ses coffres.**
//!
//! Le contenu d'un coffre n'est pas dans la grille de blocs : c'est une liste
//! à part du chunk, dont chaque entrée porte ses propres coordonnées. Rien ne
//! la fait suivre les blocs toute seule, et le format ne signale aucune
//! incohérence — un build pivoté sort vide, et on l'apprend en ouvrant un
//! coffre. `ExeWorldEdit` a payé ce piège ; ce fichier le ferme ici.
//!
//! Ce sont des tests de JONCTION : la copie ramasse en coordonnées MONDE, le
//! presse-papiers range en LOCAL, le collage repose en MONDE. Une unité qui
//! traverse une frontière se vérifie EN TRAVERSANT, pas de chaque côté.

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_blocks::Transfo;
use tf_ops::edition::{appliquer, copier};
use tf_ops::plan::Plan;
use tf_ops::{Collage, Masque, Motif, Presse};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };
/// Trois coffres par chunk : assez pour qu'un ordre de liste puisse se tromper.
const PAR_CHUNK: u32 = 3;

fn staging() -> Staging<MemorySource, MemorySource> {
    let t = Terrain::peuplee(PAR_CHUNK);
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&t));
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

/// Ces octets figurent-ils dans ceux-là ?
fn contient(foin: &[u8], aiguille: &[u8]) -> bool {
    foin.windows(aiguille.len()).any(|w| w == aiguille)
}

/// Une boîte qui contient tous les coffres du chunk (0, 0), et eux seuls.
fn chunk_zero() -> BBox {
    boite((0, -64, 0), (15, 127, 15))
}

/// Les cases MONDE que la fixture a peuplées dans le chunk (cx, cz).
fn attendues(cx: i32, cz: i32) -> Vec<[i32; 3]> {
    let mut v: Vec<[i32; 3]> = (0..PAR_CHUNK)
        .map(|k| Terrain::case_coffre(cx, cz, k))
        .collect();
    v.sort_by_key(|c| (c[1], c[2], c[0]));
    v
}

// ── copier ──────────────────────────────────────────────────────────────────

#[test]
fn copier_ramasse_les_coffres_et_les_range_en_local() {
    let st = staging();
    let mut i = Interner::new();
    let sel = chunk_zero();
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();

    let locales: Vec<[i32; 3]> = p.entites.iter().map(|e| e.case).collect();
    let voulues: Vec<[i32; 3]> = attendues(0, 0)
        .iter()
        .map(|c| [c[0] - sel.min.x, c[1] - sel.min.y, c[2] - sel.min.z])
        .collect();
    assert_eq!(locales, voulues, "les cases doivent être LOCALES et triées");

    // Et le contenu est là : c'est lui qu'on ne veut pas perdre.
    for e in &p.entites {
        assert_eq!(e.id(), Some("minecraft:chest"));
        assert!(
            contient(&e.nbt, b"CustomName"),
            "le nom personnalisé doit voyager avec l'entrée"
        );
    }
}

#[test]
fn un_coffre_hors_selection_ne_part_pas_dans_l_extrait() {
    let st = staging();
    let mut i = Interner::new();
    // Une colonne d'un bloc de large : aucune des cases peuplées n'y tombe.
    let sel = boite((1, -64, 1), (1, 127, 1));
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(p.entites.is_empty());
}

#[test]
fn copier_n_ecrit_rien_meme_avec_des_coffres() {
    let st = staging();
    let mut i = Interner::new();
    let _ = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut i).unwrap();
    assert!(
        st.is_clean(),
        "`//copy` est la seule opération qui n'écrit rien"
    );
}

// ── tourner ─────────────────────────────────────────────────────────────────

#[test]
fn tourner_un_extrait_emporte_ses_coffres() {
    let st = staging();
    let mut i = Interner::new();
    let sel = chunk_zero();
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(!p.entites.is_empty(), "le test ne prouve rien sans coffre");

    let r = p.transformer(Transfo::Rot90, &mut i, &|_, _| None);
    assert_eq!(r.presse.entites.len(), p.entites.len());
    // Un quart de tour envoie (x, z) sur (sz − 1 − z, x) — la même formule que
    // les cases, parce que c'en est une.
    let [_, _, sz] = p.taille;
    for (a, b) in p.entites.iter().zip(&r.presse.entites) {
        assert_eq!(b.case, [sz as i32 - 1 - a.case[2], a.case[1], a.case[0]]);
        assert_eq!(b.nbt, a.nbt, "le CONTENU ne tourne pas, la case seulement");
    }
}

#[test]
fn quatre_quarts_de_tour_ramenent_les_coffres_ou_ils_etaient() {
    let st = staging();
    let mut i = Interner::new();
    let depart = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut i).unwrap();
    let mut q = depart.clone();
    for _ in 0..4 {
        q = q.transformer(Transfo::Rot90, &mut i, &|_, _| None).presse;
    }
    assert_eq!(q.entites, depart.entites);
}

// ── coller ──────────────────────────────────────────────────────────────────

#[test]
fn coller_repose_les_coffres_avec_leur_contenu() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let sel = chunk_zero();
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();

    // Loin de la source : aucun recouvrement ne peut masquer une erreur de
    // coordonnées.
    let coin = BlockPos {
        x: 160,
        y: -64,
        z: 176,
    };
    let c = Collage {
        presse: &p,
        coin,
        avec_air: true,
        air,
        compter: false,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();
    assert_eq!(r.entites_posees, p.entites.len() as u64);

    let relu = copier(&st, &SURFACE, DOSSIER, &c.bornes(), &mut i).unwrap();
    assert_eq!(
        relu.entites.len(),
        p.entites.len(),
        "tous les coffres doivent être retrouvés là-bas"
    );
    for (a, b) in p.entites.iter().zip(&relu.entites) {
        assert_eq!(a.case, b.case, "la case locale doit se conserver");
        // Les OCTETS relus sont ceux d'origine, aux trois coordonnées près.
        assert_eq!(a.nbt.len(), b.nbt.len());
        assert_eq!(a.id(), b.id());
        let differents = a.nbt.iter().zip(&b.nbt).filter(|(x, y)| x != y).count();
        assert!(
            differents <= 12,
            "seules les coordonnées changent, {differents} octets ont bougé"
        );
    }
}

/// **Le tour complet, et le seul test qui prouve la jonction entière.**
///
/// Copier, tourner quatre fois — donc l'identité — et reposer à sa propre
/// place ne doit produire AUCUN correctif. Une erreur de repère, un ordre de
/// liste instable, un octet ré-encodé : tout se voit ici et nulle part
/// ailleurs, puisque le résultat attendu est « le fichier d'avant ».
#[test]
fn copier_tourner_quatre_fois_reposer_ne_change_aucun_octet() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let sel = chunk_zero();

    let mut p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert!(!p.entites.is_empty(), "le test ne prouve rien sans coffre");
    for _ in 0..4 {
        p = p.transformer(Transfo::Rot90, &mut i, &|_, _| None).presse;
    }

    let c = Collage {
        presse: &p,
        coin: sel.min,
        avec_air: true,
        air,
        compter: false,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &c, &i).unwrap();
    // Les coffres sont bien PASSÉS par le chemin de pose — sans quoi le test
    // serait vert pour la pire des raisons : un collage qui les ignore ne
    // change rien non plus. Une absence ne se voit pas ; on l'exige.
    assert_eq!(r.entites_posees, p.entites.len() as u64);
    assert_eq!(r.entites_retirees, 0);
    assert!(
        r.patches.is_empty(),
        "reposer un extrait à l'identique ne doit rien réécrire, {} correctif(s)",
        r.patches.len()
    );
}

#[test]
fn coller_sur_un_coffre_existant_le_remplace_sans_doublon() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let sel = chunk_zero();
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();

    // On repose l'extrait DU CHUNK (0,0) sur le chunk (1,0), qui a déjà ses
    // propres coffres — dont un partage sa case avec un coffre de l'extrait.
    let coin = BlockPos {
        x: 16,
        y: sel.min.y,
        z: 0,
    };
    let c = Collage {
        presse: &p,
        coin,
        avec_air: true,
        air,
        compter: false,
    };
    appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();

    let relu = copier(&st, &SURFACE, DOSSIER, &c.bornes(), &mut i).unwrap();
    let cases: Vec<[i32; 3]> = relu.entites.iter().map(|e| e.case).collect();
    let mut uniques = cases.clone();
    uniques.dedup();
    assert_eq!(cases, uniques, "aucune case ne doit porter deux entrées");
    assert_eq!(
        relu.entites.len(),
        p.entites.len(),
        "l'extrait écrase, il ne s'ajoute pas"
    );
    // Et ce sont bien CEUX DE L'EXTRAIT : la fixture nomme chaque coffre
    // d'après son chunk d'origine, donc un collage qui n'aurait rien posé
    // relirait « Coffre 1/0 » et serait démasqué.
    for k in 0..PAR_CHUNK {
        let voulu = Terrain::nom_coffre(0, 0, k);
        assert!(
            relu.entites
                .iter()
                .any(|e| contient(&e.nbt, voulu.as_bytes())),
            "le coffre {voulu} de l'extrait doit avoir remplacé celui d'en dessous"
        );
    }
    for k in 0..PAR_CHUNK {
        let parti = Terrain::nom_coffre(1, 0, k);
        assert!(
            !relu
                .entites
                .iter()
                .any(|e| contient(&e.nbt, parti.as_bytes())),
            "le coffre {parti} d'origine devait être remplacé"
        );
    }
}

// ── écraser ─────────────────────────────────────────────────────────────────

/// Un coffre dont le bloc a disparu est un FANTÔME : une entrée sans rien
/// pour la porter. Elle part avec lui — et le rapport le dit, parce que c'est
/// une perte de contenu et qu'elle ne doit pas être silencieuse.
#[test]
fn ecraser_la_case_d_un_coffre_le_retire() {
    let st = staging();
    let i = {
        let mut i = Interner::new();
        i.intern("minecraft:stone");
        i
    };
    let mut lecture = i.clone();
    let cible = Terrain::case_coffre(0, 0, 0);
    let sel = BBox::single(BlockPos {
        x: cible[0],
        y: cible[1],
        z: cible[2],
    });

    let avant = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut lecture).unwrap();
    let n = avant.entites.len();

    // Un bloc que le terrain ne porte pas à cette case : la case CHANGE.
    let mut i2 = i.clone();
    let marque = i2.intern("minecraft:bedrock");
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(marque));
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &i2).unwrap();
    assert_eq!(r.entites_retirees, 1, "le coffre écrasé doit être retiré");
    assert_eq!(r.entites_posees, 0, "un `//set` ne pose aucun coffre");

    let apres = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut lecture).unwrap();
    assert_eq!(apres.entites.len(), n - 1);
    // La case LOCALE de l'extrait : la sélection part de y = −64.
    let locale = [cible[0], cible[1] + 64, cible[2]];
    assert!(!apres.entites.iter().any(|e| e.case == locale));
}

/// Et l'inverse, qui est la partie dangereuse : une opération qui traverse la
/// case d'un coffre **sans changer son état** ne doit RIEN lui faire. Juger
/// sur la boîte `bornes` plutôt que sur la case détruirait ici le coffre
/// qu'un `//replace` n'a pas touché.
#[test]
fn un_replace_qui_ne_touche_pas_la_case_laisse_le_coffre() {
    let st = staging();
    let mut i = Interner::new();
    let absent = i.intern("titiforge:rien_du_tout");
    let terre = i.intern("minecraft:dirt");
    let mut lecture = i.clone();

    let avant = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut lecture).unwrap();
    assert!(!avant.entites.is_empty());

    // Le masque n'accepte aucune entrée d'aucune palette : rien ne change.
    let plan = Plan::nouveau(Masque::Etat(absent), Motif::Bloc(terre));
    let r = appliquer(&st, &SURFACE, DOSSIER, &chunk_zero(), &plan, &i).unwrap();
    assert_eq!(r.entites_retirees, 0);

    let apres = copier(&st, &SURFACE, DOSSIER, &chunk_zero(), &mut lecture).unwrap();
    assert_eq!(apres.entites, avant.entites);
}

#[test]
fn un_extrait_sans_coffre_n_en_invente_pas() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:bedrock");
    let mut p = Presse::uniforme([2, 2, 2], marque);
    p.ancre = [0, 0, 0];

    let c = Collage {
        presse: &p,
        coin: BlockPos {
            x: 200,
            y: 0,
            z: 200,
        },
        avec_air: false,
        air,
        compter: false,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();
    assert_eq!(r.entites_posees, 0);
    assert_eq!(r.entites_retirees, 0);
}
