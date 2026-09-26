//! La copie de travail non destructive.
//!
//! L'invariant n° 1 du projet — **on ne touche jamais au fichier source** —
//! n'est pas une intention, c'est une propriété qui se vérifie : à la fin de
//! chaque test, la source doit être octet pour octet ce qu'elle était.

mod commun;

use std::cell::RefCell;

use commun::TempDir;
use tf_world::source::{
    Dimension, Folder, LockProbe, MemorySource, RegionInfo, RegionSink, RegionSource, Result,
    SourceError,
};
use tf_world::{
    classer, CommitError, CommitReport, EtatRegion, FsSource, RegionPos, RegionStore, Staging,
};

const SURFACE: Dimension = Dimension::Overworld;
const R: Folder = Folder::Region;

/// Une vraie région, marquée — voir `commun::region_marquee`.
fn octets(n: usize, marque: u8) -> Vec<u8> {
    commun::region_marquee(marque, n)
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
            commun::marque(bytes)
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

    fn remove_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<()> {
        self.ecrits.borrow_mut().push(format!(
            "r {} {} {}",
            dim.label(),
            folder.dir_name(),
            pos.file_name()
        ));
        Ok(())
    }

    fn write_meta(&self, nom: &str, _: &[u8]) -> Result<()> {
        self.ecrits.borrow_mut().push(format!("m {nom}"));
        Ok(())
    }

    fn remove_meta(&self, nom: &str) -> Result<()> {
        self.ecrits.borrow_mut().push(format!("-m {nom}"));
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
        commun::marque(&st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()),
        1
    );

    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(31_000, 99))
        .unwrap();

    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()),
        99,
        "après modification, c'est la couche"
    );
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, RegionPos::new(1, 0)).unwrap()),
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
        commun::marque(&st.read_region(&SURFACE, R, RegionPos::new(9, 9)).unwrap()),
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

    let taille = |n, m| octets(n, m).len() as u64;
    assert_eq!(
        par_pos,
        vec![
            (RegionPos::new(0, 0), taille(31_000, 99)),
            (RegionPos::new(1, 0), taille(20_000, 2)),
            (RegionPos::new(7, 7), taille(12_000, 98)),
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
        commun::marque(&st.read_region(&SURFACE, R, RegionPos::new(0, 0)).unwrap()),
        99
    );
    assert!(st.is_dirty(&Dimension::Nether, R, RegionPos::new(0, 0)));
    assert_eq!(st.touched().len(), 2);
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
            fichiers_ecrits: 0,
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
    assert_eq!(
        etat(&st, ZERO),
        EtatRegion::EnAttente,
        "le puits n'est pas la save : la copie GARDE son travail — la rendre à \
         la save, ce serait le perdre"
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
    // La région change AUSSI : une charge orpheline qui disparaît seule ne
    // change pas le contenu de la région, et une région au contenu inchangé
    // ne s'écrit pas.
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(9, 99))
        .unwrap();
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

// ── la save qui change sous la copie ────────────────────────────────────────

const ZERO: RegionPos = RegionPos { x: 0, z: 0 };
const UN: RegionPos = RegionPos { x: 1, z: 0 };

/// L'état d'une région de la surface, `None` si la copie ne la recouvre pas.
fn couverte<S: RegionSource, O: RegionStore>(
    st: &Staging<S, O>,
    pos: RegionPos,
) -> Option<EtatRegion> {
    st.etats()
        .unwrap()
        .into_iter()
        .find(|((d, f, p), _)| *d == SURFACE && *f == R && *p == pos)
        .map(|(_, e)| e)
}

/// L'état d'une région de la surface, qui DOIT être recouverte.
fn etat<S: RegionSource, O: RegionStore>(st: &Staging<S, O>, pos: RegionPos) -> EtatRegion {
    couverte(st, pos).expect("la région devrait être recouverte")
}

#[test]
fn la_table_de_verite_des_trois_empreintes() {
    use EtatRegion::*;
    let (a, b, c) = (Some(1), Some(2), Some(3));
    // La save porte ce que porte la copie : à jour, quelle que soit la base.
    assert_eq!(classer(None, a, a), AJour);
    assert_eq!(classer(Some(b), a, a), AJour);
    assert_eq!(classer(Some(None), None, None), AJour);
    // Base inconnue : on ne sait pas si la save a bougé, on ne réécrit pas.
    assert_eq!(classer(None, a, b), EnConflit);
    assert_eq!(classer(None, None, b), EnConflit);
    // La save n'a pas bougé, la copie oui — y compris une région CRÉÉE.
    assert_eq!(classer(Some(a), a, b), EnAttente);
    assert_eq!(classer(Some(None), None, b), EnAttente);
    // La copie n'a pas bougé, la save oui.
    assert_eq!(classer(Some(a), b, a), Perimee);
    assert_eq!(classer(Some(None), a, None), Perimee);
    // Les deux ont bougé — y compris le jeu qui génère une région que la
    // copie créait aussi.
    assert_eq!(classer(Some(a), b, c), EnConflit);
    assert_eq!(classer(Some(None), a, b), EnConflit);

    assert!(EnAttente.porte_du_travail() && EnConflit.porte_du_travail());
    assert!(!AJour.porte_du_travail() && !Perimee.porte_du_travail());
}

#[test]
fn ecrire_par_dessus_une_partie_jouee_est_refuse_avant_la_sauvegarde() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    // Le joueur joue : le jeu réécrit la région SOUS la copie.
    st.source().put_region(SURFACE, R, ZERO, octets(20_000, 42));

    let puits = Puits::default();
    let mut sauvegardes = 0;
    let e = st
        .commit(&puits, libre(), false, &mut || {
            sauvegardes += 1;
            Ok(())
        })
        .unwrap_err();

    assert_eq!(e, CommitError::SaveModifiee(vec![(SURFACE, R, ZERO)]));
    assert_eq!(
        sauvegardes, 0,
        "on ne prend pas une sauvegarde pour une écriture qui n'aura pas lieu"
    );
    assert!(puits.ecrits.borrow().is_empty(), "rien n'est écrit");
    assert!(
        e.to_string().contains("region/r.0.0.mca"),
        "le message nomme ce qu'on peut aller regarder : {e}"
    );
}

#[test]
fn un_chunk_deporte_modifie_en_jeu_compte_comme_un_changement() {
    // Un chunk déporté que le jeu réécrit ne change pas le `.mca` : seulement
    // son `.mcc`. Une empreinte du seul `.mca` laisserait passer l'écrasement.
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    st.source()
        .put_external(SURFACE, R, "c.5.5.mcc", b"jouee".to_vec());

    let r = st.commit(&Puits::default(), libre(), false, &mut sauvegarde_ok());
    assert!(matches!(r, Err(CommitError::SaveModifiee(_))), "{r:?}");
}

#[test]
fn une_partie_jouee_ailleurs_ne_bloque_rien() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    // Le joueur joue dans une AUTRE région que celles de la copie.
    st.source().put_region(SURFACE, R, UN, octets(20_000, 42));

    let r = st
        .commit(&Puits::default(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(r.regions_ecrites, 1);
}

#[test]
fn une_region_deja_ecrite_ne_se_reecrit_pas() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    st.write_region(&SURFACE, R, UN, &octets(31_000, 98))
        .unwrap();
    // Le puits EST la save, comme dans l'application.
    let r = st
        .commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(r.regions_ecrites, 2);
    assert!(
        st.etats().unwrap().is_empty() && !st.is_dirty(&SURFACE, R, ZERO),
        "la save porte la copie : les régions QUITTENT la copie — gardées sans \
         travail, elles deviendraient des conflits le jour où le joueur y remet \
         les pieds"
    );
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        99
    );

    // On retouche UNE région : c'est la seule à réécrire.
    st.write_region(&SURFACE, R, UN, &octets(31_000, 97))
        .unwrap();
    assert_eq!(etat(&st, UN), EtatRegion::EnAttente);
    assert_eq!(couverte(&st, ZERO), None);
    let r = st
        .commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(
        r.regions_ecrites, 1,
        "réécrire une région que la save porte déjà, c'est risquer d'y \
         remettre ce que le jeu a changé depuis"
    );
}

#[test]
fn le_jeu_qui_rejoue_une_region_ecrite_ne_fait_aucun_conflit() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    st.commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    // Le joueur va voir le résultat EN JEU, puis revient éditer.
    st.source().put_region(SURFACE, R, ZERO, octets(25_000, 42));
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        42,
        "la copie lit ce que le jeu a laissé"
    );
    st.write_region(&SURFACE, R, ZERO, &octets(26_000, 43))
        .unwrap();
    assert_eq!(
        etat(&st, ZERO),
        EtatRegion::EnAttente,
        "le va-et-vient avec le jeu est le cas COURANT : il ne doit rien \
         refuser"
    );
}

#[test]
fn une_region_perimee_ne_se_reecrit_pas_et_se_rafraichit() {
    let st = Staging::new(origine(), MemorySource::new());
    // La copie porte la région TELLE QUE la save — puis le jeu la change.
    st.write_region(&SURFACE, R, ZERO, &octets(20_000, 1))
        .unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::AJour);
    st.source().put_region(SURFACE, R, ZERO, octets(25_000, 42));
    assert_eq!(etat(&st, ZERO), EtatRegion::Perimee);
    st.write_region(&SURFACE, R, UN, &octets(9, 97)).unwrap();

    // Écrire n'est pas refusé — la copie n'a rien à y ajouter — et ne remet
    // surtout pas la save d'avant.
    let puits = Puits::default();
    let r = st
        .commit(&puits, libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(r.regions_ecrites, 1, "seulement l'autre région");
    assert!(!puits
        .ecrits
        .borrow()
        .iter()
        .any(|l| l.contains("r.0.0.mca")));

    // La vue montre encore le monde d'avant ; rafraîchir la rend à la save.
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        1
    );
    st.rafraichir(&(SURFACE, R, ZERO)).unwrap();
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        42
    );
    assert!(!st.is_dirty(&SURFACE, R, ZERO));
    assert_eq!(couverte(&st, ZERO), None, "plus recouverte");
}

#[test]
fn rafraichir_emporte_les_charges_deportees_et_les_pierres_tombales() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    st.write_external(&SURFACE, R, "c.8.8.mcc", b"neuve")
        .unwrap();
    st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();
    // Une charge d'une AUTRE région reste où elle est.
    st.write_external(&SURFACE, R, "c.40.0.mcc", b"voisine")
        .unwrap();

    st.rafraichir(&(SURFACE, R, ZERO)).unwrap();

    assert_eq!(
        st.read_external(&SURFACE, R, "c.5.5.mcc").unwrap(),
        b"origine".to_vec(),
        "la pierre tombale est partie avec la région"
    );
    assert!(matches!(
        st.read_external(&SURFACE, R, "c.8.8.mcc"),
        Err(SourceError::NotFound)
    ));
    assert_eq!(
        st.read_external(&SURFACE, R, "c.40.0.mcc").unwrap(),
        b"voisine".to_vec()
    );
}

#[test]
fn une_ecriture_interrompue_se_rattrape_sans_conflit() {
    // Le puits a écrit la région, puis l'écriture a cédé AVANT de noter la
    // nouvelle base : la save porte déjà ce qu'on allait y mettre.
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
        .unwrap();
    st.source().put_region(SURFACE, R, ZERO, octets(31_000, 99));
    assert_eq!(etat(&st, ZERO), EtatRegion::AJour);

    let r = st
        .commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(
        r.regions_ecrites, 0,
        "une écriture qui a cédé à mi-chemin ne bloque pas la suivante pour \
         toujours"
    );
}

#[test]
fn les_bases_et_les_pierres_tombales_survivent_a_une_reprise() {
    let d = TempDir::new("reprise-bases");
    let couche = d.sous("couche");
    {
        let st = Staging::new(origine(), FsSource::open(&couche).unwrap());
        st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
        st.remove_external(&SURFACE, R, "c.5.5.mcc").unwrap();
    }

    let st = Staging::reopen(origine(), FsSource::open(&couche).unwrap()).unwrap();
    assert!(
        matches!(
            st.read_external(&SURFACE, R, "c.5.5.mcc"),
            Err(SourceError::NotFound)
        ),
        "la suppression tient après la reprise"
    );
    assert_eq!(
        etat(&st, ZERO),
        EtatRegion::EnAttente,
        "la base a survécu : la save n'a pas bougé, la copie oui"
    );

    // Et la reprise voit une partie jouée ENTRE les deux séances.
    let jouee = origine();
    jouee.put_region(SURFACE, R, ZERO, octets(20_000, 42));
    let st = Staging::reopen(jouee, FsSource::open(&couche).unwrap()).unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::EnConflit);
}

#[test]
fn une_couche_reprise_sans_ses_metadonnees_doute() {
    // Une couche écrite par une version d'avant, ou dont la métadonnée s'est
    // abîmée : une table à moitié lue donnerait des bases FAUSSES.
    let couche = MemorySource::new();
    couche.put_region(SURFACE, R, ZERO, octets(31_000, 99));
    couche.write_meta("couche", b"TFC1\x01\x00").unwrap();

    let st = Staging::reopen(origine(), couche).unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::EnConflit);

    // Éditer n'y change rien : noter la base maintenant affirmerait que la
    // copie part de la save d'aujourd'hui.
    st.write_region(&SURFACE, R, ZERO, &octets(31_000, 98))
        .unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::EnConflit);
    // Une région qu'elle ne portait pas, elle, a une base.
    st.write_region(&SURFACE, R, UN, &octets(9, 97)).unwrap();
    assert_eq!(etat(&st, UN), EtatRegion::EnAttente);
}

#[test]
fn une_couche_qui_n_a_ecrit_que_des_entites_se_reprend() {
    // Une source sur disque découvre une dimension par son dossier `region/`.
    // Une couche qui n'a écrit QUE des entités n'en a pas : sa reprise la
    // perdait, et la première lecture retombait sur la save.
    let d = TempDir::new("reprise-entites");
    let couche = d.sous("couche");
    {
        let st = Staging::new(origine(), FsSource::open(&couche).unwrap());
        st.write_region(&SURFACE, Folder::Entities, ZERO, &octets(500, 7))
            .unwrap();
    }
    let st = Staging::reopen(origine(), FsSource::open(&couche).unwrap()).unwrap();
    assert!(st.is_dirty(&SURFACE, Folder::Entities, ZERO));
}

#[test]
fn une_region_recompressee_est_a_jour() {
    // Une annulation rend un chunk identique, mais RECOMPRESSÉ : mêmes blocs,
    // autres octets. Jugée aux octets, la région restait « modifiée » pour
    // toujours.
    let st = Staging::new(origine(), MemorySource::new());
    let meme = commun::region_marquee_niveau(1, 20_000, 1);
    assert_ne!(meme, octets(20_000, 1), "le test ne prouve rien sinon");
    st.write_region(&SURFACE, R, ZERO, &meme).unwrap();

    assert_eq!(etat(&st, ZERO), EtatRegion::AJour);
    let puits = Puits::default();
    let r = st
        .commit(&puits, libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(
        r.regions_ecrites, 0,
        "rien à écrire : le contenu est le même"
    );
}

#[test]
fn une_table_de_bases_suivie_d_octets_de_trop_est_refusee() {
    let couche = MemorySource::new();
    {
        let st = Staging::new(origine(), MemorySource::new());
        st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
        let mut meta = st.overlay().read_meta("couche").unwrap();
        meta.push(0);
        couche.write_meta("couche", &meta).unwrap();
        couche.put_region(SURFACE, R, ZERO, octets(31_000, 99));
    }
    let st = Staging::reopen(origine(), couche).unwrap();
    assert_eq!(
        etat(&st, ZERO),
        EtatRegion::EnConflit,
        "une table à moitié comprise donnerait des bases FAUSSES : on la jette"
    );
}

#[test]
fn une_charge_portee_seule_par_une_couche_sans_metadonnees_se_voit() {
    // Seule une charge déportée, sans région ni base : c'est pourtant ce que
    // la vue montre, donc ce qu'une écriture devrait écrire.
    let (region, charge) = commun::region_deportee(1);
    let src = MemorySource::new();
    src.put_region(SURFACE, R, ZERO, region);
    src.put_external(SURFACE, R, "c.5.5.mcc", charge);
    let couche = MemorySource::new();
    couche.put_external(SURFACE, R, "c.5.5.mcc", commun::region_deportee(2).1);

    let st = Staging::reopen(src, couche).unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::EnConflit);
}

#[test]
fn rien_d_une_region_perimee_ne_s_ecrit_pas_meme_ses_charges() {
    let (region, charge) = commun::region_deportee(1);
    let src = MemorySource::new();
    src.put_region(SURFACE, R, ZERO, region.clone());
    src.put_external(SURFACE, R, "c.5.5.mcc", charge.clone());
    let st = Staging::new(src, MemorySource::new());
    // La copie porte la région et sa charge TELLES QUE la save, et cache une
    // charge qu'elle avait créée puis supprimée : rien de tout ça n'est du
    // travail.
    st.write_region(&SURFACE, R, ZERO, &region).unwrap();
    st.write_external(&SURFACE, R, "c.5.5.mcc", &charge)
        .unwrap();
    st.write_external(&SURFACE, R, "c.6.6.mcc", b"passagere")
        .unwrap();
    st.remove_external(&SURFACE, R, "c.6.6.mcc").unwrap();
    assert_eq!(etat(&st, ZERO), EtatRegion::AJour);

    // Le jeu réécrit la région, ses DEUX charges comprises.
    let (region_jeu, charge_jeu) = commun::region_deportee(3);
    st.source().put_region(SURFACE, R, ZERO, region_jeu);
    st.source()
        .put_external(SURFACE, R, "c.5.5.mcc", charge_jeu.clone());
    st.source()
        .put_external(SURFACE, R, "c.6.6.mcc", b"jeu".to_vec());
    assert_eq!(etat(&st, ZERO), EtatRegion::Perimee);

    let r = st
        .commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(r, CommitReport::default());
    assert_eq!(
        st.source().read_external(&SURFACE, R, "c.5.5.mcc").unwrap(),
        charge_jeu,
        "écraser la charge que le jeu vient d'écrire casserait son chunk"
    );
    assert_eq!(
        st.source().read_external(&SURFACE, R, "c.6.6.mcc").unwrap(),
        b"jeu".to_vec(),
        "et l'effacer aussi"
    );
}

// ── les fichiers du monde qui ne sont pas des régions ───────────────────────

/// **La copie d'abord, la save sinon** — comme une région. Et la save n'en
/// voit rien avant l'écriture.
#[test]
fn un_fichier_du_monde_se_lit_dans_la_copie_puis_dans_la_save() {
    let save = origine();
    save.write_meta("projet", b"celui de la save").unwrap();
    let st = Staging::new(save, MemorySource::new());
    assert_eq!(st.lire_fichier("projet").unwrap(), b"celui de la save");
    assert!(st.fichiers_en_attente().unwrap().is_empty());

    st.ecrire_fichier("projet", b"celui de la copie").unwrap();
    assert_eq!(st.lire_fichier("projet").unwrap(), b"celui de la copie");
    assert_eq!(
        st.source().read_meta("projet").unwrap(),
        b"celui de la save",
        "la save n'a rien reçu"
    );
    assert_eq!(st.fichiers_en_attente().unwrap(), ["projet"]);
    // Vue comme une source, la copie montre SON document.
    assert_eq!(
        RegionSource::read_meta(&st, "projet").unwrap(),
        b"celui de la copie"
    );

    // Revenue à l'identique — une annulation — elle n'attend plus rien, et
    // s'allège.
    st.ecrire_fichier("projet", b"celui de la save").unwrap();
    assert!(st.fichiers_en_attente().unwrap().is_empty());
    assert_eq!(st.alleger_fichiers().unwrap(), 1);
    assert!(matches!(
        st.overlay().read_meta("projet"),
        Err(SourceError::NotFound)
    ));
    assert_eq!(st.lire_fichier("projet").unwrap(), b"celui de la save");
}

/// Absent et vide se valent — un document vidé dans la copie, quand la save
/// n'en a pas, n'attend rien ; vidé quand la save en a un, il attend.
#[test]
fn un_document_vide_attend_seulement_quand_la_save_en_a_un() {
    let st = Staging::new(origine(), MemorySource::new());
    assert!(matches!(
        st.lire_fichier("projet"),
        Err(SourceError::NotFound)
    ));
    st.ecrire_fichier("projet", b"").unwrap();
    assert!(st.fichiers_en_attente().unwrap().is_empty());

    let save = origine();
    save.write_meta("projet", b"un document").unwrap();
    let st = Staging::new(save, MemorySource::new());
    st.ecrire_fichier("projet", b"").unwrap();
    assert_eq!(st.fichiers_en_attente().unwrap(), ["projet"]);
}

/// La métadonnée INTERNE de la copie n'est pas un fichier du monde : ni lue,
/// ni écrite par ici, ni jamais comptée comme du travail à écrire.
#[test]
fn la_metadonnee_interne_n_est_pas_un_fichier_du_monde() {
    let st = Staging::new(origine(), MemorySource::new());
    // Une région écrite fait ranger la métadonnée « couche ».
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(21_000, 7))
        .unwrap();
    assert!(st.overlay().read_meta("couche").is_ok());
    assert!(st.lire_fichier("couche").is_err());
    assert!(st.ecrire_fichier("couche", b"x").is_err());
    assert!(st.fichiers_en_attente().unwrap().is_empty());
    assert!(!RegionSource::meta_names(&st)
        .unwrap()
        .contains(&"couche".to_string()));
}

/// **L'écriture emporte le document APRÈS la sauvegarde** — et le retire de
/// la copie seulement si la save le porte désormais.
#[test]
fn l_ecriture_emporte_le_document_apres_la_sauvegarde() {
    let st = Staging::new(origine(), MemorySource::new());
    st.ecrire_fichier("projet", b"quatre fenetres").unwrap();

    // Vers un puits qui n'est pas la save : l'ordre, et le document reste en
    // attente — la save, relue, ne l'a pas.
    let puits = Puits::default();
    let r = st
        .commit(&puits, libre(), false, &mut || {
            puits.ecrits.borrow_mut().push("sauvegarde".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(r.fichiers_ecrits, 1);
    assert_eq!(*puits.ecrits.borrow(), ["sauvegarde", "m projet"]);
    assert_eq!(st.fichiers_en_attente().unwrap(), ["projet"]);

    // Vers la save elle-même : écrit, et la copie s'allège.
    let r = st
        .commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert_eq!(r.fichiers_ecrits, 1);
    assert_eq!(st.source().read_meta("projet").unwrap(), b"quatre fenetres");
    assert!(st.fichiers_en_attente().unwrap().is_empty());
    assert!(matches!(
        st.overlay().read_meta("projet"),
        Err(SourceError::NotFound)
    ));

    // Un document VIDÉ se retire de la save plutôt que de s'y écrire vide.
    st.ecrire_fichier("projet", b"").unwrap();
    st.commit(st.source(), libre(), false, &mut sauvegarde_ok())
        .unwrap();
    assert!(matches!(
        st.source().read_meta("projet"),
        Err(SourceError::NotFound)
    ));
}

/// Un refus — la save a changé sous du travail — n'écrit PAS le document
/// non plus : il part avec les régions, ou pas du tout.
#[test]
fn un_refus_n_ecrit_pas_le_document() {
    let st = Staging::new(origine(), MemorySource::new());
    st.write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(21_000, 7))
        .unwrap();
    st.ecrire_fichier("projet", b"quatre fenetres").unwrap();
    // Le jeu joue la région pendant ce temps.
    st.source()
        .write_region(&SURFACE, R, RegionPos::new(0, 0), &octets(22_000, 8))
        .unwrap();
    let puits = Puits::default();
    assert!(matches!(
        st.commit(&puits, libre(), false, &mut sauvegarde_ok()),
        Err(CommitError::SaveModifiee(_))
    ));
    assert!(puits.ecrits.borrow().is_empty());
}

/// Sur disque : `titiforge-<nom>` à la racine, et le temporaire d'une
/// écriture interrompue n'est pas un fichier du monde.
#[test]
fn sur_disque_un_temporaire_n_est_pas_un_fichier_du_monde() {
    let d = TempDir::new("fichiers-disque");
    let couche = FsSource::open(d.path()).unwrap();
    couche.write_meta("projet", b"doc").unwrap();
    std::fs::write(d.path().join("titiforge-projet.bin.tmp"), b"coupe").unwrap();
    std::fs::write(d.path().join("level.dat"), b"").unwrap();
    assert_eq!(couche.meta_names().unwrap(), ["projet"]);
    assert_eq!(
        std::fs::read(d.path().join("titiforge-projet")).unwrap(),
        b"doc"
    );
    couche.remove_meta("projet").unwrap();
    couche.remove_meta("projet").unwrap();
    assert!(couche.meta_names().unwrap().is_empty());
}
