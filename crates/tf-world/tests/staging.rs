//! La copie de travail non destructive.
//!
//! L'invariant n° 1 du projet — **on ne touche jamais au fichier source** —
//! n'est pas une intention, c'est une propriété qui se vérifie : à la fin de
//! chaque test, la source doit être octet pour octet ce qu'elle était.

use std::cell::RefCell;

use tf_world::source::{
    Dimension, Folder, LockProbe, MemorySource, RegionInfo, RegionSink, RegionSource, Result,
    SourceError,
};
use tf_world::{CommitError, CommitReport, RegionPos, Staging};

const SURFACE: Dimension = Dimension::Overworld;
const R: Folder = Folder::Region;

fn octets(n: usize, marque: u8) -> Vec<u8> {
    let mut v = vec![0u8; n];
    v[0] = marque;
    v[n - 1] = marque;
    v
}

/// Une source d'origine avec deux régions et une charge déportée.
fn origine() -> MemorySource {
    let m = MemorySource::new();
    m.put_region(SURFACE, R, RegionPos::new(0, 0), octets(20_000, 1));
    m.put_region(SURFACE, R, RegionPos::new(1, 0), octets(20_000, 2));
    m.put_region(Dimension::Nether, R, RegionPos::new(0, 0), octets(9_000, 3));
    m.put_external(SURFACE, R, "c.5.5.mcc", b"origine".to_vec());
    m
}

/// Empreinte complète d'une source : tout ce qu'elle porte, contenu compris.
fn empreinte(s: &dyn RegionSource) -> Vec<(String, String, i64, Vec<u8>)> {
    let mut out = Vec::new();
    for dim in s.dimensions().unwrap() {
        for folder in Folder::ALL {
            for r in s.overview(&dim, folder).unwrap().regions {
                let b = s.read_region(&dim, folder, r.pos).unwrap();
                out.push((
                    dim.label(),
                    format!("{}/{}", folder.dir_name(), r.pos.file_name()),
                    b.len() as i64,
                    b,
                ));
            }
            for n in s.external_names(&dim, folder).unwrap() {
                let b = s.read_external(&dim, folder, &n).unwrap();
                out.push((dim.label(), format!("{}/{n}", folder.dir_name()), -1, b));
            }
        }
    }
    out.sort();
    out
}

/// Un puits qui compte et retient ce qu'on lui écrit.
#[derive(Default)]
struct Puits {
    ecrits: RefCell<Vec<String>>,
    /// Si vrai, la première écriture échoue : sert à prouver qu'on n'écrit
    /// jamais avant la sauvegarde.
    casse: bool,
}

// La source d'origine est partagée en lecture seule dans les tests ; le puits
// n'est touché que par le fil du test.
unsafe impl Sync for Puits {}

impl RegionSink for Puits {
    fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()> {
        if self.casse {
            return Err(SourceError::ReadOnly);
        }
        self.ecrits.borrow_mut().push(format!(
            "w {} {} {} {}",
            dim.label(),
            folder.dir_name(),
            pos.file_name(),
            bytes[0]
        ));
        Ok(())
    }

    fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.ecrits.borrow_mut().push(format!(
            "x {} {} {name} {}",
            dim.label(),
            folder.dir_name(),
            bytes.len()
        ));
        Ok(())
    }

    fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()> {
        self.ecrits
            .borrow_mut()
            .push(format!("- {} {} {name}", dim.label(), folder.dir_name()));
        Ok(())
    }
}

fn libre() -> LockProbe {
    LockProbe::LIBRE_SUR
}

fn sauvegarde_ok() -> impl FnMut() -> std::result::Result<(), String> {
    || Ok(())
}

// ── l'invariant n° 1 ────────────────────────────────────────────────────────

#[test]
fn ecrire_dans_le_staging_ne_touche_jamais_la_source() {
    let src = origine();
    let avant = empreinte(&src);

    let st = Staging::new(src, MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();
    st.write_region(&SURFACE, R, RegionPos::new(7, 7), &octets(12_000, 98))
        .unwrap();
    st.write_external(&SURFACE, R, "c.5.5.mcc", b"modifiee")
        .unwrap();
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();

    assert_eq!(
        empreinte(st.source()),
        avant,
        "la source doit être octet pour octet ce qu'elle était"
    );
}

#[test]
fn le_staging_lit_la_couche_d_abord_puis_la_source() {
    let st = Staging::new(origine(), MemorySource::new());

    // Avant modification : c'est la source qu'on lit.
    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()[0],
        1
    );

    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();

    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()[0],
        99,
        "après modification, c'est la couche"
    );
    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(1, 0)).unwrap()[0],
        2,
        "la voisine non modifiée vient toujours de la source"
    );
    assert!(st.is_dirty(&SURFACE, R, RegionPos::new(0, 0)));
    assert!(!st.is_dirty(&SURFACE, R, RegionPos::new(1, 0)));
    assert!(
        !st.is_dirty(&Dimension::Nether, R, RegionPos::new(0, 0)),
        "la même position dans une autre dimension n'est pas la même région"
    );
    assert!(
        !st.is_dirty(&SURFACE, Folder::Entities, RegionPos::new(0, 0)),
        "ni dans un autre dossier"
    );
}

#[test]
fn une_region_creee_dans_le_staging_est_lisible_alors_qu_elle_n_existe_pas_en_source() {
    let st = Staging::new(origine(), MemorySource::new());
    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(9, 9)),
        Err(SourceError::NotFound)
    );

    st.write_region(&SURFACE, R, RegionPos::new(9, 9), &octets(5_000, 42))
        .unwrap();

    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(9, 9)).unwrap()[0],
        42,
        "étendre un monde est une écriture comme une autre"
    );
}

// ── pierres tombales ────────────────────────────────────────────────────────

#[test]
fn supprimer_une_charge_deportee_ne_la_fait_pas_reapparaitre_depuis_la_source() {
    let st = Staging::new(origine(), MemorySource::new());
    assert_eq!(
        st.read_external(&SURFACE, R, "c.5.5.mcc").unwrap(),
        b"origine".to_vec()
    );

    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();

    assert_eq!(
        st.read_external(&SURFACE, R, "c.5.5.mcc"),
        Err(SourceError::NotFound),
        "sans pierre tombale, la suppression serait silencieusement annulée \
         et un chunk surdimensionné ressusciterait avec son ancien contenu"
    );
    assert!(
        st.external_names(&SURFACE, R).unwrap().is_empty(),
        "et elle ne doit pas non plus se lister"
    );
}

#[test]
fn reecrire_une_charge_supprimee_annule_la_pierre_tombale() {
    let st = Staging::new(origine(), MemorySource::new());
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();
    st.write_external(&SURFACE, R, "c.5.5.mcc", b"nouvelle")
        .unwrap();

    assert_eq!(
        st.read_external(&SURFACE, R, "c.5.5.mcc").unwrap(),
        b"nouvelle".to_vec(),
        "sinon on écrirait un fichier que la lecture suivante refuserait de voir"
    );
    assert_eq!(st.external_names(&SURFACE, R).unwrap(), vec!["c.5.5.mcc"]);
}

#[test]
fn les_charges_deportees_se_listent_en_union_des_deux_couches() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_external(&SURFACE, R, "c.8.8.mcc", b"neuve")
        .unwrap();

    assert_eq!(
        st.external_names(&SURFACE, R).unwrap(),
        vec!["c.5.5.mcc".to_string(), "c.8.8.mcc".to_string()],
        "celle de la source ET celle de la couche"
    );
}

// ── la carte ────────────────────────────────────────────────────────────────

#[test]
fn la_carte_est_l_union_des_deux_et_la_couche_l_emporte() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();
    st.write_region(&SURFACE, R, RegionPos::new(7, 7), &octets(12_000, 98))
        .unwrap();

    let ov = st.overview(&SURFACE, R).unwrap();
    let mut par_pos: Vec<(RegionPos, u64)> = ov.regions.iter().map(|r| (r.pos, r.bytes)).collect();
    par_pos.sort_by_key(|(p, _)| (p.z, p.x));

    assert_eq!(
        par_pos,
        vec![
            (RegionPos::new(0, 0), 31_000),
            (RegionPos::new(1, 0), 20_000),
            (RegionPos::new(7, 7), 12_000),
        ],
        "(0,0) doit peser sa TAILLE COURANTE, pas celle de la source — sinon \
         un compteur d'occupation annoncerait la taille d'avant l'édition"
    );
}

#[test]
fn une_dimension_qui_n_existe_que_dans_la_couche_se_decouvre() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&Dimension::End, R, RegionPos::new(0, 0), &octets(4_000, 7))
        .unwrap();

    let dims = st.dimensions().unwrap();
    assert!(dims.contains(&Dimension::End));
    assert!(dims.contains(&SURFACE), "sans perdre celles de la source");
    assert_eq!(
        dims.iter().filter(|d| **d == SURFACE).count(),
        1,
        "et sans doublon : elle est dans les deux"
    );
}

// ── reprise et abandon ──────────────────────────────────────────────────────

#[test]
fn rouvrir_un_staging_retrouve_le_travail_en_cours() {
    let couche = MemorySource::new();
    couche.put_region(SURFACE, R, RegionPos::new(0, 0), octets(31_000, 99));
    couche.put_region(Dimension::Nether, R, RegionPos::new(0, 0), octets(7, 55));

    let st = Staging::reopen(origine(), couche).unwrap();

    assert!(
        st.is_dirty(&SURFACE, R, RegionPos::new(0, 0)),
        "sans relire la couche, la première lecture retomberait sur la source \
         — donc annulerait silencieusement tout le travail en cours"
    );
    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()[0],
        99
    );
    assert!(st.is_dirty(&Dimension::Nether, R, RegionPos::new(0, 0)));
    assert_eq!(st.touched().len(), 2);
}

#[test]
fn abandonner_redevient_transparent_sans_rien_effacer() {
    let couche = MemorySource::new();
    let st = Staging::new(origine(), couche);
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();
    assert!(!st.is_clean());

    st.discard();

    assert!(st.is_clean());
    assert_eq!(
        st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()[0],
        1,
        "on relit la source"
    );
    assert_eq!(
        st.read_external(&SURFACE, R, "c.5.5.mcc").unwrap(),
        b"origine".to_vec(),
        "la charge supprimée revient"
    );
    assert_eq!(
        st.overlay()
            .read_region(&SURFACE, R, RegionPos::new(0, 0))
            .unwrap()[0],
        99,
        "les octets restent dans la couche : effacer serait plus propre sur le \
         disque et irréversible pour l'utilisateur"
    );
}

// ── l'invariant n° 5 : l'ORDRE ──────────────────────────────────────────────

#[test]
fn ecrire_pendant_que_minecraft_tient_le_monde_est_refuse() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();
    let puits = Puits::default();

    let e = st
        .commit(&puits, LockProbe::TENU, true, &mut sauvegarde_ok())
        .unwrap_err();

    assert_eq!(e, CommitError::WorldLocked);
    assert!(
        puits.ecrits.borrow().is_empty(),
        "rien ne doit partir : écrire pendant que le jeu tourne perd les deux côtés"
    );
}

#[test]
fn une_sonde_qui_ne_conclut_pas_exige_une_confirmation() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();
    let puits = Puits::default();

    assert_eq!(
        st.commit(&puits, LockProbe::INDECIDABLE, false, &mut sauvegarde_ok()),
        Err(CommitError::LockUnknown),
        "hors Windows le verrou est consultatif : un succès d'ouverture ne \
         prouve RIEN, et décider à la place de l'utilisateur ferait écrire \
         pendant que le jeu tourne"
    );
    assert!(puits.ecrits.borrow().is_empty());

    // Confirmé, ça passe.
    assert!(st
        .commit(&puits, LockProbe::INDECIDABLE, true, &mut sauvegarde_ok())
        .is_ok());
    assert_eq!(
        puits.ecrits.borrow().clone(),
        vec!["w Surface region r.0.0.mca 99".to_string()],
        "la région modifiée, et ELLE SEULE : la charge déportée de la source \
         n'a pas été touchée, la réécrire serait un risque pour rien"
    );
}

#[test]
fn une_sauvegarde_qui_echoue_bloque_toute_ecriture() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();
    let puits = Puits::default();

    let e = st
        .commit(&puits, libre(), false, &mut || {
            Err("disque plein".to_string())
        })
        .unwrap_err();

    assert_eq!(e, CommitError::BackupFailed("disque plein".into()));
    assert!(
        puits.ecrits.borrow().is_empty(),
        "une écriture sans filet est exactement ce que l'invariant interdit"
    );
}

#[test]
fn la_sauvegarde_est_prise_avant_la_premiere_ecriture() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();
    // Le puits refuse tout : si la sauvegarde était prise après, elle
    // sauvegarderait un monde DÉJÀ à moitié écrit.
    let puits = Puits {
        casse: true,
        ..Default::default()
    };
    let mut vu_la_sauvegarde = false;

    let r = st.commit(&puits, libre(), false, &mut || {
        vu_la_sauvegarde = true;
        Ok(())
    });

    assert!(r.is_err(), "l'écriture échoue");
    assert!(
        vu_la_sauvegarde,
        "mais la sauvegarde a bien été prise AVANT — une sauvegarde prise \
         après la première écriture ne sauvegarde plus rien"
    );
}

#[test]
fn un_staging_propre_n_ecrit_rien_mais_sauvegarde_quand_meme() {
    let st = Staging::new(origine(), MemorySource::new());
    let puits = Puits::default();
    let mut compte = 0;

    let rapport = st
        .commit(&puits, libre(), false, &mut || {
            compte += 1;
            Ok(())
        })
        .unwrap();

    assert_eq!(rapport, CommitReport::default());
    assert!(puits.ecrits.borrow().is_empty());
    assert_eq!(
        compte, 1,
        "la sauvegarde ne se saute pas : on ne sait pas ici si l'appelant a \
         d'autres raisons de la vouloir, et se tromper coûte un monde"
    );
}

#[test]
fn le_commit_ecrit_les_regions_les_charges_et_les_suppressions() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();
    st.write_region(
        &Dimension::Nether,
        R,
        RegionPos::new(2, 2),
        &octets(500, 77),
    )
    .unwrap();
    st.write_external(&SURFACE, R, "c.8.8.mcc", b"neuve")
        .unwrap();
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();

    let puits = Puits::default();
    let rapport = st
        .commit(&puits, libre(), false, &mut sauvegarde_ok())
        .unwrap();

    assert_eq!(
        rapport,
        CommitReport {
            regions_ecrites: 2,
            externes_ecrites: 1,
            externes_supprimees: 1,
        }
    );
    let ecrits = puits.ecrits.borrow().clone();
    assert!(
        ecrits
            .iter()
            .any(|l| l.starts_with("w Surface region r.0.0.mca 99")),
        "{ecrits:?}"
    );
    assert!(
        ecrits
            .iter()
            .any(|l| l.starts_with("w Nether region r.2.2.mca 77")),
        "{ecrits:?}"
    );
    assert!(
        ecrits.iter().any(|l| l == "x Surface region c.8.8.mcc 5"),
        "{ecrits:?}"
    );
    assert!(
        ecrits.iter().any(|l| l == "- Surface region c.5.5.mcc"),
        "{ecrits:?}"
    );
    assert_eq!(
        empreinte(st.source()),
        empreinte(&origine()),
        "et la source d'origine n'a toujours pas bougé"
    );
}

#[test]
fn le_commit_n_ecrit_que_ce_qui_a_ete_touche() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();

    let puits = Puits::default();
    st.commit(&puits, libre(), false, &mut sauvegarde_ok())
        .unwrap();

    let ecrits = puits.ecrits.borrow().clone();
    assert!(
        !ecrits.iter().any(|l| l.contains("r.1.0.mca")),
        "réécrire une région intacte, c'est réémettre 20 000 octets pour rien \
         — et risquer de les abîmer : {ecrits:?}"
    );
}

#[test]
fn une_charge_supprimee_ne_se_reecrit_pas_pendant_le_commit() {
    let st = Staging::new(origine(), MemorySource::new());
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();

    let puits = Puits::default();
    let rapport = st
        .commit(&puits, libre(), false, &mut sauvegarde_ok())
        .unwrap();

    assert_eq!(rapport.externes_supprimees, 1);
    assert_eq!(
        rapport.externes_ecrites, 0,
        "elle est supprimée : la réécrire annulerait la suppression"
    );
}

#[test]
fn la_carte_de_la_source_reste_intacte_apres_un_commit() {
    let attendu: Vec<RegionInfo> = origine().overview(&SURFACE, R).unwrap().regions;
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();

    st.commit(&Puits::default(), libre(), false, &mut sauvegarde_ok())
        .unwrap();

    assert_eq!(
        st.source().overview(&SURFACE, R).unwrap().regions,
        attendu,
        "le commit écrit dans le PUITS, jamais dans la source — les deux \
         peuvent être le même dossier, mais c'est l'appelant qui le décide"
    );
}
