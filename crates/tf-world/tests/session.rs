//! **Une séance survit à la fermeture** — et ne réécrit jamais par-dessus le
//! jeu en revenant.
//!
//! Sur de vrais dossiers : le verrou, le renommage d'une séance mise de côté
//! et la troncature d'un journal sont des propriétés du DISQUE, qu'une
//! source en mémoire ne mettrait pas en jeu.

mod commun;

use std::fs;
use std::path::{Path, PathBuf};

use commun::TempDir;
use tf_anvil::Edit;
use tf_nbt::Span;
use tf_world::journal::{ChunkPatch, Cible, Correction, Genre, Journal, Record};
use tf_world::session::{dossier_de, ErreurSeance};
use tf_world::{
    Dimension, Fermeture, Folder, FsSource, LockProbe, RegionPos, RegionSource, Reprise, Seance,
};

const SURFACE: Dimension = Dimension::Overworld;
const R: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };
const UN: RegionPos = RegionPos { x: 1, z: 0 };

/// Une vraie région, marquée — voir `commun::region_marquee`.
fn octets(n: usize, marque: u8) -> Vec<u8> {
    commun::region_marquee(marque, n)
}

/// Une save sur disque, deux régions.
fn save(d: &TempDir) -> PathBuf {
    let s = d.sous("save");
    fs::write(s.join("level.dat"), b"faux").unwrap();
    let r = d.sous("save/region");
    fs::write(r.join("r.0.0.mca"), octets(20_000, 1)).unwrap();
    fs::write(r.join("r.1.0.mca"), octets(20_000, 2)).unwrap();
    s
}

/// Le jeu réécrit une région de la save.
fn jouer(save: &Path, fichier: &str, marque: u8) {
    fs::write(save.join("region").join(fichier), octets(21_000, marque)).unwrap();
}

/// Des octets incompressibles : le journal compresse, et un test qui mesure
/// la taille de son FICHIER ne mesurerait rien sur des zéros.
fn bruit(n: usize, graine: u64) -> Vec<u8> {
    let mut x = graine;
    (0..n)
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (x >> 56) as u8
        })
        .collect()
}

/// Une action du journal, d'un poids choisi.
fn action(j: &mut Journal, label: &str, poids: usize) -> Vec<Record> {
    let graine = label
        .bytes()
        .fold(7u64, |h, b| h.wrapping_mul(31) + b as u64);
    let avant = bruit(poids, graine);
    let apres = bruit(poids, graine ^ 0xFF);
    let edits = vec![Edit {
        span: Span {
            start: 0,
            end: poids,
        },
        bytes: apres.clone(),
    }];
    let cible = Cible {
        dim: SURFACE,
        folder: R,
        region: ZERO,
        chunk: 0,
    };
    let p = ChunkPatch::record(cible, &avant, &apres, &edits).unwrap();
    j.pousser(
        label,
        0,
        Genre::Operation {
            op: "poser".into(),
            params: Vec::new(),
            bounds: None,
            corrections: vec![Correction::Chunk(p)],
        },
    )
}

fn labels(j: &Journal) -> Vec<String> {
    j.entrees().iter().map(|e| e.label.clone()).collect()
}

#[test]
fn une_premiere_ouverture_est_neuve() {
    let d = TempDir::new("seance-neuve");
    let save = save(&d);
    let racine = d.path().join("seances");

    let (s, j, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(reprise, Reprise::Neuve);
    assert!(reprise.texte().is_none(), "rien à dire");
    assert!(j.entrees().is_empty());
    assert_eq!(s.dossier(), dossier_de(&racine, &save));
    let monde = fs::read_to_string(s.dossier().join("monde")).unwrap();
    assert!(
        monde.ends_with("save"),
        "le chemin de la save, en clair, pour qui ouvre le dossier : {monde}"
    );
}

#[test]
fn fermer_sans_travail_efface_la_seance() {
    let d = TempDir::new("seance-vide");
    let save = save(&d);
    let racine = d.path().join("seances");
    let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
    let dossier = s.dossier().to_path_buf();

    assert_eq!(s.fermer().unwrap(), Fermeture::Effacee);
    assert!(!dossier.exists(), "une séance sans travail ne traîne pas");
}

#[test]
fn le_travail_et_l_annulation_survivent_a_la_fermeture() {
    let d = TempDir::new("seance-reprise");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (mut s, mut j, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.staging()
            .write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
        let r = action(&mut j, "Remplir", 100);
        s.noter(&mut j, &r).unwrap();
        let r = action(&mut j, "Remplacer", 100);
        s.noter(&mut j, &r).unwrap();
        let (_, curseur) = j.annuler().unwrap();
        s.noter(&mut j, &[curseur]).unwrap();

        assert_eq!(
            s.fermer().unwrap(),
            Fermeture::Gardee {
                regions: 1,
                fichiers: 0
            }
        );
    }

    let (s, j, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(
        reprise,
        Reprise::Reprise {
            actions: 2,
            regions: 1,
            fichiers: 0,
            rafraichies: 0,
            interrompue: None
        }
    );
    assert_eq!(labels(&j), ["Remplir", "Remplacer"]);
    assert_eq!(j.curseur(), 1, "l'annulation aussi a survécu");
    assert!(j.peut_refaire());
    assert_eq!(
        commun::marque(&s.staging().read_region(&SURFACE, R, ZERO).unwrap()),
        99,
        "la copie de travail est là"
    );
    assert_eq!(
        commun::marque(&fs::read(save.join("region/r.0.0.mca")).unwrap()),
        1,
        "et la save n'a pas bougé"
    );
    assert!(reprise.texte().unwrap().contains("1 région(s)"));
}

#[test]
fn une_partie_jouee_ailleurs_n_empeche_pas_la_reprise() {
    let d = TempDir::new("seance-ailleurs");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.staging()
            .write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
        s.fermer().unwrap();
    }
    jouer(&save, "r.1.0.mca", 42);

    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(
        matches!(reprise, Reprise::Reprise { regions: 1, .. }),
        "{reprise:?}"
    );
    assert_eq!(
        commun::marque(&s.staging().read_region(&SURFACE, R, ZERO).unwrap()),
        99
    );
    assert_eq!(
        commun::marque(&s.staging().read_region(&SURFACE, R, UN).unwrap()),
        42,
        "la région jouée se lit telle que le jeu l'a laissée"
    );
}

#[test]
fn ecrire_puis_jouer_puis_revenir_ne_fait_aucun_conflit() {
    // Le va-et-vient COURANT : écrire, aller voir en jeu, revenir éditer.
    let d = TempDir::new("seance-va-et-vient");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        let st = s.staging();
        st.write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
        st.write_region(&SURFACE, R, UN, &octets(31_000, 98))
            .unwrap();
        st.commit(
            &FsSource::open(&save).unwrap(),
            LockProbe::LIBRE_SUR,
            false,
            &mut || Ok(()),
        )
        .unwrap();
        // Retouché d'un seul côté après l'écriture.
        st.write_region(&SURFACE, R, UN, &octets(31_000, 97))
            .unwrap();
        assert_eq!(
            s.fermer().unwrap(),
            Fermeture::Gardee {
                regions: 1,
                fichiers: 0
            }
        );
    }
    // Le joueur joue là où l'on avait écrit — la copie n'y porte plus rien.
    jouer(&save, "r.0.0.mca", 42);

    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(
        reprise,
        Reprise::Reprise {
            actions: 0,
            regions: 1,
            fichiers: 0,
            rafraichies: 0,
            interrompue: None
        },
        "écrite, la région a quitté la copie : le jeu peut la changer sans \
         que rien ne soit en conflit"
    );
    let st = s.staging();
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        42
    );
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, UN).unwrap()),
        97
    );
}

#[test]
fn une_region_perimee_se_relit_a_la_reprise() {
    let d = TempDir::new("seance-perimee");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        let st = s.staging();
        // La copie porte une région TELLE QUE la save, et du travail ailleurs
        // — puis le programme s'arrête sans fermer la séance.
        st.write_region(&SURFACE, R, ZERO, &octets(20_000, 1))
            .unwrap();
        st.write_region(&SURFACE, R, UN, &octets(31_000, 97))
            .unwrap();
    }
    jouer(&save, "r.0.0.mca", 42);

    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(
        reprise,
        Reprise::Reprise {
            actions: 0,
            regions: 1,
            fichiers: 0,
            rafraichies: 1,
            interrompue: None
        }
    );
    let st = s.staging();
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, ZERO).unwrap()),
        42,
        "relue depuis la save : la garder montrerait le monde d'avant"
    );
    assert!(!st.is_dirty(&SURFACE, R, ZERO));
    assert_eq!(
        commun::marque(&st.read_region(&SURFACE, R, UN).unwrap()),
        97
    );
}

#[test]
fn une_region_sans_travail_ne_survit_pas_a_la_fermeture() {
    let d = TempDir::new("seance-allegee");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        let st = s.staging();
        st.write_region(&SURFACE, R, ZERO, &octets(20_000, 1))
            .unwrap();
        st.write_region(&SURFACE, R, UN, &octets(31_000, 97))
            .unwrap();
        assert_eq!(
            s.fermer().unwrap(),
            Fermeture::Gardee {
                regions: 1,
                fichiers: 0
            }
        );
    }
    assert!(
        !dossier_de(&racine, &save)
            .join("couche/region/r.0.0.mca")
            .exists(),
        "la copie ne garde que le travail"
    );
    // Si le joueur y joue maintenant, il n'y a rien à mettre de côté.
    jouer(&save, "r.0.0.mca", 42);
    let (_, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(
        matches!(
            reprise,
            Reprise::Reprise {
                regions: 1,
                rafraichies: 0,
                ..
            }
        ),
        "{reprise:?}"
    );
}

#[test]
fn une_partie_jouee_sur_le_travail_met_la_seance_de_cote() {
    let d = TempDir::new("seance-conflit");
    let save = save(&d);
    let racine = d.path().join("seances");
    let mut vers_precedent: Option<PathBuf> = None;
    // Deux fois : une seconde mise de côté ne doit pas écraser la première.
    for tour in 0..2u8 {
        {
            let (mut s, mut j, _) = Seance::ouvrir(&racine, &save).unwrap();
            s.staging()
                .write_region(&SURFACE, R, ZERO, &octets(31_000, 99 - tour))
                .unwrap();
            let r = action(&mut j, "Remplir", 10);
            s.noter(&mut j, &r).unwrap();
            assert_eq!(
                s.fermer().unwrap(),
                Fermeture::Gardee {
                    regions: 1,
                    fichiers: 0
                }
            );
        }
        jouer(&save, "r.0.0.mca", 42 + tour);

        let (s, j, reprise) = Seance::ouvrir(&racine, &save).unwrap();
        let Reprise::MiseDeCote { vers, conflits } = &reprise else {
            panic!("{reprise:?}");
        };
        assert_eq!(conflits, &vec![(SURFACE, R, ZERO)]);
        assert_eq!(
            commun::marque(&fs::read(vers.join("couche/region/r.0.0.mca")).unwrap()),
            99 - tour,
            "le travail est mis de côté, INTACT"
        );
        assert!(vers.join("journal.tfj").is_file(), "avec son journal");
        assert_ne!(Some(vers.clone()), vers_precedent, "tour {tour}");
        if let Some(p) = &vers_precedent {
            assert!(p.join("couche").is_dir(), "la première est toujours là");
        }
        vers_precedent = Some(vers.clone());

        assert!(j.entrees().is_empty(), "on repart de la save");
        assert_eq!(
            commun::marque(&s.staging().read_region(&SURFACE, R, ZERO).unwrap()),
            42 + tour,
            "la vue montre la partie jouée"
        );
        assert_eq!(
            commun::marque(&fs::read(save.join("region/r.0.0.mca")).unwrap()),
            42 + tour,
            "et rien n'a été écrit par-dessus le jeu"
        );
        assert!(reprise.texte().unwrap().contains("mise de côté"));
        s.fermer().unwrap();
    }
}

#[test]
fn deux_fenetres_sur_le_meme_monde_sont_refusees() {
    let d = TempDir::new("seance-verrou");
    let save = save(&d);
    let racine = d.path().join("seances");
    let (premiere, _, _) = Seance::ouvrir(&racine, &save).unwrap();

    let e = Seance::ouvrir(&racine, &save).err().expect("refusée");
    assert!(matches!(e, ErreurSeance::DejaOuverte(_)), "{e:?}");
    assert!(e.to_string().contains("autre fenêtre"));

    drop(premiere);
    assert!(
        Seance::ouvrir(&racine, &save).is_ok(),
        "le verrou part avec la séance — rien à nettoyer à la main"
    );
}

#[test]
fn un_enregistrement_tronque_est_coupe_avant_d_ajouter() {
    let d = TempDir::new("seance-tronque");
    let save = save(&d);
    let racine = d.path().join("seances");
    let journal = dossier_de(&racine, &save).join("journal.tfj");
    {
        let (mut s, mut j, _) = Seance::ouvrir(&racine, &save).unwrap();
        for l in ["un", "deux"] {
            let r = action(&mut j, l, 50);
            s.noter(&mut j, &r).unwrap();
        }
    }
    // Un arrêt brutal au milieu de l'écriture du second enregistrement.
    let b = fs::read(&journal).unwrap();
    fs::write(&journal, &b[..b.len() - 7]).unwrap();

    {
        let (mut s, mut j, _) = Seance::ouvrir(&racine, &save).unwrap();
        assert_eq!(
            labels(&j),
            ["un"],
            "la dernière action est perdue, pas l'historique"
        );
        let r = action(&mut j, "trois", 50);
        s.noter(&mut j, &r).unwrap();
    }
    let (_, j, _) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(
        labels(&j),
        ["un", "trois"],
        "sans la coupe, « trois » s'écrivait DERRIÈRE le morceau tronqué et ne se \
         relisait jamais"
    );
}

#[test]
fn une_action_interrompue_se_dit_une_fois() {
    let d = TempDir::new("seance-interrompue");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (mut s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.commencer("Remplir").unwrap();
        // Le programme s'arrête ici : ni `terminer`, ni `fermer`.
    }
    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(
        matches!(&reprise, Reprise::Reprise { interrompue: Some(l), .. } if l == "Remplir"),
        "{reprise:?}"
    );
    assert!(reprise
        .texte()
        .unwrap()
        .contains("« Remplir » a été interrompue"));
    drop(s);
    let (_, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(
        reprise,
        Reprise::Neuve,
        "dit une fois, pas à chaque ouverture"
    );
}

#[test]
fn terminer_retire_la_marque() {
    let d = TempDir::new("seance-terminee");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (mut s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.commencer("Remplir").unwrap();
        s.terminer().unwrap();
        s.terminer().unwrap();
    }
    let (_, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(reprise, Reprise::Neuve);
}

#[test]
fn le_journal_s_elague_au_dela_de_son_plafond() {
    let d = TempDir::new("seance-plafond");
    let save = save(&d);
    let racine = d.path().join("seances");
    let fichier = dossier_de(&racine, &save).join("journal.tfj");
    let mut taille_max = 0;
    {
        let (mut s, mut j, _) = Seance::ouvrir(&racine, &save).unwrap();
        // Chaque action pèse ~2 × 1 000 octets d'éditions.
        s.budget_journal(10_000);
        for i in 0..20 {
            let r = action(&mut j, &format!("a{i}"), 1_000);
            s.noter(&mut j, &r).unwrap();
            taille_max = taille_max.max(fs::metadata(&fichier).unwrap().len());
            assert!(j.poids() <= 10_000, "action {i} : {} octets", j.poids());
        }
        assert!(j.entrees().len() < 20, "les plus vieilles sont parties");
        assert_eq!(j.entrees().last().unwrap().label, "a19");
    }
    assert!(
        taille_max < 20 * 2_000,
        "le FICHIER aussi rétrécit — il est en ajout seul, donc il faut le réécrire"
    );
    let (_, relu, _) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(relu.entrees().len() < 20);
    assert_eq!(relu.entrees().last().unwrap().label, "a19");
}

#[test]
fn un_journal_illisible_est_mis_de_cote_pas_efface() {
    let d = TempDir::new("seance-illisible");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.staging()
            .write_region(&SURFACE, R, ZERO, &octets(31_000, 99))
            .unwrap();
    }
    let dossier = dossier_de(&racine, &save);
    fs::write(dossier.join("journal.tfj"), b"pas un journal du tout").unwrap();

    let (s, j, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(j.entrees().is_empty());
    assert!(
        matches!(reprise, Reprise::Reprise { regions: 1, .. }),
        "la copie de travail, elle, est intacte : {reprise:?}"
    );
    assert_eq!(
        commun::marque(&s.staging().read_region(&SURFACE, R, ZERO).unwrap()),
        99
    );
    let mis_de_cote = fs::read_dir(&dossier).unwrap().flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("journal.tfj.illisible-")
    });
    assert!(mis_de_cote);
}

#[test]
fn le_dossier_d_une_seance_suit_la_save_et_pas_son_orthographe() {
    let d = TempDir::new("seance-chemin");
    let save = save(&d);
    let racine = d.path().join("seances");
    let detour = save.join("region").join("..");
    assert_eq!(dossier_de(&racine, &save), dossier_de(&racine, &detour));

    // Deux saves du même NOM dans deux dossiers : deux séances.
    let autre = d.sous("ailleurs/save");
    assert_ne!(dossier_de(&racine, &save), dossier_de(&racine, &autre));
    let nom = dossier_de(&racine, &save)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(nom.starts_with("save-"), "lisible : {nom}");
}

#[test]
fn une_save_introuvable_ne_laisse_pas_de_seance() {
    let d = TempDir::new("seance-introuvable");
    let racine = d.path().join("seances");
    let e = Seance::ouvrir(&racine, &d.path().join("nulle-part"))
        .err()
        .expect("refusée");
    assert_eq!(e.to_string(), "save introuvable");
    assert!(!racine.exists());
}

#[test]
fn une_variable_d_environnement_vide_vaut_absente() {
    use std::ffi::OsString;
    use tf_world::session::racine_selon;
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |n: &str| {
            vars.iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| OsString::from(*v))
        }
    };
    // Désignée, elle l'emporte.
    assert_eq!(
        racine_selon(&env(&[("TITIFORGE_SEANCES", "/ailleurs")])),
        Some(PathBuf::from("/ailleurs"))
    );
    // Vides, toutes : la règle XDG. Prises au mot, elles rangeaient les
    // séances dans un chemin RELATIF — là où l'application a été lancée.
    let r = racine_selon(&env(&[
        ("TITIFORGE_SEANCES", ""),
        ("XDG_DATA_HOME", ""),
        ("LOCALAPPDATA", "/donnees"),
        ("HOME", "/maison"),
    ]))
    .unwrap();
    assert!(r.is_absolute(), "{}", r.display());
    assert!(r.ends_with("titiforge/seances"), "{}", r.display());
    // Rien du tout : on ne sait pas où ranger, et on le dit.
    assert_eq!(
        racine_selon(&env(&[("HOME", ""), ("LOCALAPPDATA", "")])),
        None
    );
}

/// **Le document des composants est du travail.** Une séance qui n'a que
/// lui en attente ne s'efface pas en se fermant — elle perdrait des
/// définitions que la save n'a jamais reçues — et la reprise le DIT.
#[test]
fn une_seance_qui_n_a_que_son_document_se_garde() {
    let d = TempDir::new("seance-document");
    let save = save(&d);
    let racine = d.path().join("seances");
    {
        let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
        s.staging()
            .ecrire_fichier("projet", b"quatre fenetres")
            .unwrap();
        assert_eq!(
            s.fermer().unwrap(),
            Fermeture::Gardee {
                regions: 0,
                fichiers: 1
            }
        );
    }
    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert!(
        matches!(reprise, Reprise::Reprise { fichiers: 1, .. }),
        "{reprise:?}"
    );
    assert!(
        reprise.texte().unwrap().contains("document des composants"),
        "{reprise:?}"
    );
    assert_eq!(
        s.staging().lire_fichier("projet").unwrap(),
        b"quatre fenetres"
    );

    // Écrit dans la save, puis fermé : la save porte tout, la séance s'efface
    // — et le document est dans le dossier du monde, qui voyage avec lui.
    s.staging()
        .commit(
            &FsSource::open(&save).unwrap(),
            LockProbe::LIBRE_SUR,
            false,
            &mut || Ok(()),
        )
        .unwrap();
    assert_eq!(s.fermer().unwrap(), Fermeture::Effacee);
    assert_eq!(
        fs::read(save.join("titiforge-projet")).unwrap(),
        b"quatre fenetres"
    );
    let (s, _, reprise) = Seance::ouvrir(&racine, &save).unwrap();
    assert_eq!(reprise, Reprise::Neuve);
    assert_eq!(
        s.staging().lire_fichier("projet").unwrap(),
        b"quatre fenetres",
        "relu depuis la save"
    );
}

/// Un document revenu à l'identique de la save — tout annulé — n'est plus du
/// travail : la séance s'efface.
#[test]
fn un_document_revenu_a_celui_de_la_save_n_est_plus_du_travail() {
    let d = TempDir::new("seance-document-annule");
    let save = save(&d);
    fs::write(save.join("titiforge-projet"), b"celui de la save").unwrap();
    let racine = d.path().join("seances");
    let (s, _, _) = Seance::ouvrir(&racine, &save).unwrap();
    s.staging().ecrire_fichier("projet", b"modifie").unwrap();
    s.staging()
        .ecrire_fichier("projet", b"celui de la save")
        .unwrap();
    assert_eq!(s.fermer().unwrap(), Fermeture::Effacee);
}
