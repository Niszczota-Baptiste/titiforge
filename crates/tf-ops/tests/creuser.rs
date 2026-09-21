//! `//hollow` — le critère est TOPOLOGIQUE, pas géométrique.
//!
//! Creuser n'est pas « enlever l'intérieur d'une boîte » : c'est enlever ce
//! qu'aucun chemin de vide ne relie au dehors. Une salle ouverte par une
//! porte ne se remplit pas, une sphère pleine se vide, une paroi d'un bloc
//! reste une paroi. Les tests visent cette différence, parce que c'est elle
//! qu'une implémentation géométrique raterait en ayant l'air de marcher.

use tf_anvil::{Interner, StateId};
use tf_bench::{region, Terrain};
use tf_ops::creuser::{creuser, extrait_creuse};
use tf_ops::edition::{appliquer, copier};
use tf_ops::plan::Plan;
use tf_ops::{Collage, Masque, Motif, Presse};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

const AIR: StateId = 0;
const MUR: StateId = 1;

fn plein(taille: [u32; 3]) -> Presse {
    Presse::uniforme(taille, MUR)
}

fn solide() -> Masque {
    Masque::Non(Box::new(Masque::Etat(AIR)))
}

fn pose(p: &mut Presse, x: u32, y: u32, z: u32, id: StateId) {
    let i = p.index(x, y, z).expect("dans la boîte");
    p.blocs[i] = id;
}

// ── la topologie ────────────────────────────────────────────────────────────

#[test]
fn un_cube_plein_se_vide_en_gardant_sa_peau() {
    let p = plein([5, 5, 5]);
    let c = creuser(&p, &solide(), 1);
    // 5³ − 3³ = 125 − 27 : la peau reste, le cœur part.
    assert_eq!(c.cases, 27);
    let v = extrait_creuse(&p, &c, AIR);
    assert_eq!(v.get(0, 0, 0), Some(MUR), "le coin est de la peau");
    assert_eq!(v.get(2, 2, 2), Some(AIR), "le cœur est vidé");
    assert_eq!(v.get(1, 1, 1), Some(AIR));
    assert_eq!(v.get(0, 2, 2), Some(MUR), "la face reste");
}

#[test]
fn une_epaisseur_de_trois_garde_trois_couches() {
    let p = plein([9, 9, 9]);
    let c = creuser(&p, &solide(), 3);
    // 9³ − 3³ gardés : il ne reste qu'un cube de 3 au centre.
    assert_eq!(c.cases, 27);
    let v = extrait_creuse(&p, &c, AIR);
    assert_eq!(v.get(2, 4, 4), Some(MUR), "troisième couche gardée");
    assert_eq!(v.get(3, 4, 4), Some(AIR), "quatrième vidée");
}

/// **Le test qui distingue le topologique du géométrique.**
///
/// Une porte change ce qui est « dehors ». Avec elle, l'air de la salle
/// communique avec l'extérieur, donc la paroi qui l'entoure TOUCHE du dehors
/// et se garde. Sans elle, cet air est enfermé : la paroi ne touche plus rien
/// d'ouvert et se fait manger.
///
/// C'est une différence qu'aucune implémentation géométrique ne peut produire
/// — les deux formes ont exactement la même boîte — et c'est elle qui décide
/// si `//hollow` sert à quelque chose.
#[test]
fn une_porte_change_ce_qui_est_garde() {
    let salle = |avec_porte: bool| {
        let mut p = plein([7, 7, 7]);
        for y in 2..5 {
            for z in 2..5 {
                for x in 2..5 {
                    pose(&mut p, x, y, z, AIR);
                }
            }
        }
        if avec_porte {
            for x in 0..2 {
                pose(&mut p, x, 3, 3, AIR);
            }
        }
        let c = creuser(&p, &solide(), 1);
        (c.cases, extrait_creuse(&p, &c, AIR))
    };
    let (n_ouverte, ouverte) = salle(true);
    let (n_fermee, fermee) = salle(false);

    // Une case de paroi qui touche la salle SANS être la porte : (1, 3, 2)
    // est à côté de (2, 3, 2), qui est de l'air de la salle. Prendre
    // (1, 3, 3) ne prouverait rien — c'est la porte elle-même, donc de l'air
    // dans les deux cas.
    assert_eq!(
        ouverte.get(1, 3, 2),
        Some(MUR),
        "la salle est ouverte : sa paroi touche du dehors et reste"
    );
    assert_eq!(
        fermee.get(1, 3, 2),
        Some(AIR),
        "la salle est fermée : sa paroi ne touche plus rien d'ouvert"
    );
    assert!(
        n_fermee > n_ouverte,
        "une salle fermée laisse plus de massif à retirer : {n_fermee} contre {n_ouverte}"
    );

    // Et dans les deux cas, l'air de la salle reste de l'air : on ne REMPLIT
    // jamais, on ne fait que vider.
    for f in [&ouverte, &fermee] {
        assert_eq!(f.get(3, 3, 3), Some(AIR), "la salle ne se remplit pas");
    }
}

#[test]
fn une_paroi_d_un_bloc_reste_une_paroi() {
    // Une coque déjà creuse : il n'y a rien à retirer, et surtout pas la
    // paroi elle-même.
    let mut p = plein([5, 5, 5]);
    for y in 1..4 {
        for z in 1..4 {
            for x in 1..4 {
                pose(&mut p, x, y, z, AIR);
            }
        }
    }
    let c = creuser(&p, &solide(), 1);
    assert_eq!(c.cases, 0);
    assert_eq!(extrait_creuse(&p, &c, AIR), p, "rien ne doit bouger");
}

#[test]
fn le_vide_deja_present_n_est_pas_compte_comme_du_travail() {
    // Une case vide enfermée reste vide. La compter gonflerait le rapport
    // d'un travail qui n'a pas lieu.
    // Sur un cube plein de 5, l'intérieur fait 27 cases, dont une est DÉJÀ
    // vide : il en reste 26 à faire.
    let mut p = plein([5, 5, 5]);
    pose(&mut p, 2, 2, 2, AIR);
    let c = creuser(&p, &solide(), 1);
    assert_eq!(c.cases, 26, "27 cases intérieures moins celle déjà vide");
    assert!(
        !c.interieur[p.index(2, 2, 2).unwrap()],
        "la case déjà vide n'est pas du travail"
    );
}

/// **La diffusion est arrêtée par la roche.**
///
/// Si elle la traversait, une seule case d'air au bord suffirait à déclarer
/// tout l'extrait « dehors », et `//hollow` ne creuserait plus jamais rien —
/// sans erreur, sans différence visible sur une forme simple, et avec un
/// rapport qui annoncerait sereinement zéro.
#[test]
fn la_diffusion_ne_passe_pas_a_travers_la_roche() {
    let mut p = plein([5, 5, 5]);
    // Une fossette au bord : la graine.
    pose(&mut p, 0, 2, 2, AIR);
    // Et une poche scellée au centre, qu'aucun chemin ne relie à elle.
    pose(&mut p, 2, 2, 2, AIR);

    let c = creuser(&p, &solide(), 1);
    // 27 cases intérieures, moins la poche déjà vide, moins la case (1,2,2)
    // qui touche la fossette et se trouve donc gardée.
    assert_eq!(
        c.cases, 25,
        "la fossette ne doit ouvrir qu'un bloc, pas tout l'extrait"
    );
    let v = extrait_creuse(&p, &c, AIR);
    assert_eq!(
        v.get(1, 2, 2),
        Some(MUR),
        "la case qui touche la fossette est de la paroi"
    );
    assert_eq!(v.get(1, 1, 1), Some(AIR), "le reste du cœur part");
}

#[test]
fn un_extrait_vide_ne_fait_rien_planter() {
    let p = Presse::uniforme([0, 0, 0], AIR);
    let c = creuser(&p, &solide(), 1);
    assert_eq!(c.cases, 0);
    assert!(c.interieur.is_empty());
}

#[test]
fn une_epaisseur_nulle_vaut_une() {
    // Creuser jusqu'à ne rien garder viderait la sélection entière, ce qu'un
    // `//set air` fait déjà et dit mieux.
    let p = plein([5, 5, 5]);
    assert_eq!(creuser(&p, &solide(), 0), creuser(&p, &solide(), 1));
}

/// Une sélection d'un million de cases ne doit pas déborder la pile. **Un
/// débordement de pile n'est pas rattrapable en Rust** : le processus meurt
/// sans message, sur la sauvegarde de quelqu'un.
#[test]
fn une_grande_selection_ne_deborde_pas_la_pile() {
    let mut p = Presse::uniforme([100, 100, 100], AIR);
    // Une coque solide autour d'un million de cases de vide : la diffusion
    // doit parcourir tout l'intérieur d'un coup.
    for y in 0..100u32 {
        for z in 0..100u32 {
            for x in 0..100u32 {
                let bord = x == 0 || y == 0 || z == 0 || x == 99 || y == 99 || z == 99;
                if bord {
                    pose(&mut p, x, y, z, MUR);
                }
            }
        }
    }
    // La coque touche le bord de la sélection, donc elle est gardée, et le
    // vide intérieur ne communique avec rien de solide.
    let c = creuser(&p, &solide(), 1);
    assert_eq!(c.cases, 0);
}

// ── de bout en bout, sur un vrai monde ──────────────────────────────────────

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

#[test]
fn creuser_un_bloc_massif_du_monde_le_vide_en_gardant_sa_peau() {
    let st = staging();
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let roche = i.intern("minecraft:bedrock");

    let sel = boite((4, -40, 4), (10, -34, 10)); // 7³
    appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(roche)),
        &i,
    )
    .unwrap();

    let extrait = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    let c = creuser(&extrait, &Masque::Non(Box::new(Masque::Etat(air))), 1);
    assert_eq!(c.cases, 125, "7³ − 5³");
    let creuse = extrait_creuse(&extrait, &c, air);

    let collage = Collage {
        presse: &creuse,
        coin: sel.min,
        avec_air: true,
        air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &collage, &i).unwrap();
    assert_eq!(r.blocs, Some(125), "exactement les cases intérieures");

    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(relu.get(0, 0, 0), Some(roche), "la peau reste");
    assert_eq!(relu.get(3, 3, 3), Some(air), "le cœur est vidé");
    assert_eq!(relu.get(0, 3, 3), Some(roche), "la face aussi");
}

// ── les coffres ─────────────────────────────────────────────────────────────

/// **Un coffre dont le bloc part s'en va avec lui.**
///
/// C'est un piège de JONCTION, et il ne se voit d'aucun côté pris seul : la
/// jonction retire bien les block entities orphelines, mais une entité POSÉE
/// par un collage gagne sur ce verdict — exprès, c'est ce qui fait qu'un
/// extrait reposé garde ses coffres. Un coffre laissé dans l'extrait creusé
/// serait donc reposé DANS LE VIDE, et on l'apprendrait en ouvrant un coffre
/// qui n'existe plus.
#[test]
fn un_coffre_sur_une_case_videe_quitte_l_extrait() {
    let mut p = plein([5, 5, 5]);
    // Deux coffres : l'un au cœur (vidé), l'autre sur la peau (gardé). Un
    // seul ne prouverait que la moitié — une règle qui jetterait TOUTES les
    // entités passerait aussi.
    pose(&mut p, 2, 2, 2, MUR);
    p.entites = vec![
        tf_anvil::Entite {
            case: [2, 2, 2],
            nbt: b"coeur".to_vec(),
            champs: [0, 0, 0],
        },
        tf_anvil::Entite {
            case: [0, 2, 2],
            nbt: b"peau".to_vec(),
            champs: [0, 0, 0],
        },
    ];

    let c = creuser(&p, &solide(), 1);
    let v = extrait_creuse(&p, &c, AIR);

    let restantes: Vec<[i32; 3]> = v.entites.iter().map(|e| e.case).collect();
    assert_eq!(
        restantes,
        vec![[0, 2, 2]],
        "le coffre du cœur part, celui de la peau reste"
    );
}

/// La même propriété, mais de bout en bout sur un monde — c'est la seule
/// façon de vérifier que la JONCTION ne repose pas le coffre malgré tout.
#[test]
fn creuser_un_monde_ne_laisse_pas_de_coffre_fantome() {
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, region(&Terrain::peuplee(3)));
    let st = Staging::new(m, MemorySource::new());
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");

    // Le sous-sol de la fixture est PLEIN : pas besoin de le remplir d'abord,
    // et surtout pas — un `//set` orphelinerait le coffre avant le creusage,
    // et le test passerait sans rien prouver.
    let coeur = Terrain::case_coffre(0, 0, 1);
    let sel = boite(
        (coeur[0] - 2, coeur[1] - 2, coeur[2] - 2),
        (coeur[0] + 2, coeur[1] + 2, coeur[2] + 2),
    );

    let extrait = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(
        extrait.entites.len(),
        1,
        "le test ne prouve rien sans coffre au cœur"
    );
    let c = creuser(&extrait, &Masque::Non(Box::new(Masque::Etat(air))), 1);
    assert_eq!(c.cases, 27, "5³ − 3³ : le cœur de la sélection");
    let creuse = extrait_creuse(&extrait, &c, air);

    let collage = Collage {
        presse: &creuse,
        coin: sel.min,
        avec_air: true,
        air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &collage, &i).unwrap();
    assert_eq!(r.entites_posees, 0, "aucune entité ne doit être REPOSÉE");
    assert_eq!(r.entites_retirees, 1, "celle du cœur part");

    // Et on le relit : les deux autres coffres du chunk sont intacts, le
    // troisième a disparu. Les cases de l'extrait sont LOCALES — on les
    // ramène en MONDE par le coin de la boîte, jamais par des constantes
    // recopiées.
    let tout = chunk_zero();
    let relu = copier(&st, &SURFACE, DOSSIER, &tout, &mut i).unwrap();
    let cases: Vec<[i32; 3]> = relu
        .entites
        .iter()
        .map(|e| {
            [
                e.case[0] + tout.min.x,
                e.case[1] + tout.min.y,
                e.case[2] + tout.min.z,
            ]
        })
        .collect();
    assert!(
        !cases.contains(&coeur),
        "le coffre du cœur ne doit plus être là : {cases:?}"
    );
    assert_eq!(cases.len(), 2, "les deux autres coffres sont intacts");
}

/// La boîte qui contient les trois coffres du chunk (0, 0).
fn chunk_zero() -> BBox {
    boite((0, -64, 0), (15, 127, 15))
}
