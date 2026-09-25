//! **L'accueil : ce qu'on propose d'ouvrir, et ce qu'un chemin désigne.**
//!
//! Sans fenêtre : ce qui se décide ici — quelles saves, lesquelles portent du
//! travail pas encore écrit, ce qu'un chemin collé veut dire — se vérifie sur
//! des arborescences fabriquées.

mod commun;

use std::fs;
use std::path::{Path, PathBuf};

use commun::Jetable;
use tf_app::accueil::{fichier_recents, lire_recents, noter_recent, Accueil, MAX_RECENTS};
use tf_world::session::dossier_de;

/// Une version « téléchargée » : un dossier et son `.jar` du même nom.
fn version(i: &Path, nom: &str) {
    fs::create_dir_all(i.join("versions").join(nom)).unwrap();
    fs::write(i.join("versions").join(nom).join(format!("{nom}.jar")), b"").unwrap();
}

/// Une installation avec deux saves et un intrus.
fn installation(racine: &Path) -> PathBuf {
    let i = racine.join(".minefield_1_18");
    fs::create_dir_all(i.join("versions")).unwrap();
    version(&i, "1.18.2");
    for s in ["Ville", "Arène"] {
        fs::create_dir_all(i.join("saves").join(s)).unwrap();
        fs::write(i.join("saves").join(s).join("level.dat"), b"").unwrap();
    }
    fs::create_dir_all(i.join("saves/captures")).unwrap();
    i
}

#[test]
fn les_saves_se_proposent_et_le_travail_pas_ecrit_se_signale() {
    let j = Jetable::neuf("accueil-saves");
    let inst = installation(j.chemin());
    let seances = j.chemin().join("seances");
    // Une séance gardée pour « Ville » : du travail n'y est pas écrit.
    let ville = inst.join("saves/Ville");
    fs::create_dir_all(dossier_de(&seances, &ville).join("couche")).unwrap();

    let a = Accueil::explorer(std::slice::from_ref(&inst), Some(&seances), &[]);
    assert_eq!(a.installations.len(), 1);
    assert_eq!(a.installations[0].nom, ".minefield_1_18");
    assert_eq!(a.nombre_de_saves(), 2, "« captures » n'a pas de level.dat");
    let en_cours: Vec<bool> = a.installations[0]
        .saves
        .iter()
        .map(|s| s.seance_en_cours)
        .collect();
    let ville_vue = a.installations[0]
        .saves
        .iter()
        .find(|s| s.save.chemin == ville)
        .unwrap();
    assert!(ville_vue.seance_en_cours, "{en_cours:?}");
    assert_eq!(
        en_cours.iter().filter(|b| **b).count(),
        1,
        "l'autre save n'a rien en attente"
    );
}

#[test]
fn choisir_refuse_ce_qui_n_est_pas_une_save_en_le_disant() {
    let j = Jetable::neuf("accueil-choisir");
    let inst = installation(j.chemin());
    let mut a = Accueil::default();

    a.choisir(inst.join("saves/captures"));
    assert_eq!(a.demande, None);
    assert!(
        a.erreur.as_deref().unwrap().contains("level.dat"),
        "{:?}",
        a.erreur
    );

    // Le level.dat lui-même désigne son dossier : c'est souvent lui qu'on
    // glisse sur la fenêtre.
    a.choisir(inst.join("saves/Ville/level.dat"));
    assert_eq!(a.demande, Some(inst.join("saves/Ville")));
    assert_eq!(a.erreur, None, "un choix juste efface l'erreur d'avant");
}

#[test]
fn un_chemin_colle_avec_ses_guillemets_se_comprend() {
    // « Copier en tant que chemin d'accès » de l'explorateur de Windows
    // entoure le chemin de guillemets.
    let j = Jetable::neuf("accueil-colle");
    let inst = installation(j.chemin());
    let mut a = Accueil {
        chemin: format!("  \"{}\"  ", inst.join("saves/Arène").display()),
        ..Default::default()
    };
    a.choisir_texte();
    assert_eq!(a.demande, Some(inst.join("saves/Arène")));

    let mut vide = Accueil::default();
    vide.choisir_texte();
    assert_eq!(vide.demande, None);
    assert!(
        vide.erreur.is_some(),
        "un clic sur « Ouvrir » sans chemin se dit"
    );
}

#[test]
fn les_recents_montent_en_tete_sans_doublon_et_restent_bornes() {
    let j = Jetable::neuf("accueil-recents");
    let seances = j.chemin().join("donnees/seances");
    let f = fichier_recents(&seances).unwrap();
    assert!(
        lire_recents(&f).is_empty(),
        "absent : aucun récent, sans erreur"
    );

    for i in 0..(MAX_RECENTS + 3) {
        noter_recent(&f, &j.chemin().join(format!("monde{i}"))).unwrap();
    }
    // Le FICHIER aussi, et pas seulement ce qu'on en relit : la lecture le
    // plafonne, ce qui cachait un fichier d'une ligne de trop.
    assert_eq!(fs::read_to_string(&f).unwrap().lines().count(), MAX_RECENTS);
    noter_recent(&f, &j.chemin().join("monde5")).unwrap();
    let r = lire_recents(&f);
    assert_eq!(r.len(), MAX_RECENTS);
    assert_eq!(r[0], j.chemin().join("monde5"), "le dernier ouvert d'abord");
    assert_eq!(
        r.iter().filter(|p| p.ends_with("monde5")).count(),
        1,
        "une seule fois"
    );
    assert!(
        !r.iter().any(|p| p.ends_with("monde0")),
        "le plus vieux est parti"
    );
}

#[test]
fn un_recent_qui_n_est_plus_une_save_ne_se_propose_pas() {
    // Un monde effacé, déplacé, sur une clé débranchée : le proposer ferait
    // cliquer sur une erreur.
    let j = Jetable::neuf("accueil-recents-morts");
    let inst = installation(j.chemin());
    let vivant = inst.join("saves/Ville");
    let mort = j.chemin().join("parti");
    let a = Accueil::explorer(&[], None, &[mort, vivant.clone()]);
    let proposes: Vec<&PathBuf> = a.recents.iter().map(|s| &s.save.chemin).collect();
    assert_eq!(proposes, [&vivant]);
    assert_eq!(
        a.recents[0].save.nom, "Ville",
        "à défaut de level.dat lisible"
    );
}

#[test]
fn un_monde_s_ouvre_la_ou_l_on_joue() {
    use tf_app::accueil::zone_d_ouverture;
    use tf_world::niveau::Niveau;
    let n = Niveau {
        joueur: Some([-33.5, 70.0, 4100.0]),
        ..Default::default()
    };
    // x = −33,5 est dans le chunk −3 (division PLANCHER), z = 4 100 dans le
    // chunk 256.
    assert_eq!(zone_d_ouverture(Some(&n)), [-4, 255, -2, 257]);
    assert_eq!(zone_d_ouverture(None), [0, 0, 1, 1], "à défaut, l'origine");
}

#[test]
fn les_assets_viennent_de_l_installation_du_monde() {
    use tf_app::accueil::{assets_par_defaut, installation_de};
    let j = Jetable::neuf("accueil-assets");
    let serveur = installation(j.chemin());
    let vanilla = j.chemin().join(".minecraft");
    version(&vanilla, "1.20.1");
    fs::create_dir_all(vanilla.join("saves/Survie")).unwrap();
    fs::write(vanilla.join("saves/Survie/level.dat"), b"").unwrap();

    let ville = serveur.join("saves/Ville");
    assert_eq!(installation_de(&ville), Some(serveur.clone()));
    assert_eq!(installation_de(&j.chemin().join("ailleurs")), None);
    // Rangé DANS l'installation mais hors de `saves/` — une copie de secours,
    // un monde d'une autre version : ce n'est pas le jeu qui l'y a mis.
    assert_eq!(installation_de(&serveur.join("secours/Ville")), None);
    let deux = [vanilla.clone(), serveur.clone()];
    assert_eq!(
        assets_par_defaut(&deux, Some(&ville)),
        Some(serveur.clone()),
        "les textures `minefield:*` sont dans le launcher du SERVEUR"
    );

    // Sans monde demandé : l'installation de la dernière partie.
    let dater = |p: PathBuf, s: u64| {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s))
            .unwrap();
    };
    // Toutes les saves des deux installations : une seule laissée à la date
    // du jour gagnerait toujours.
    dater(serveur.join("saves/Arène/level.dat"), 500);
    dater(vanilla.join("saves/Survie/level.dat"), 1_000);
    dater(ville.join("level.dat"), 2_000);
    assert_eq!(assets_par_defaut(&deux, None), Some(serveur));
    dater(vanilla.join("saves/Survie/level.dat"), 3_000);
    assert_eq!(assets_par_defaut(&deux, None), Some(vanilla.clone()));
    assert_eq!(assets_par_defaut(&[], None), None);

    // Un launcher sans aucune version téléchargée n'a rien à donner, même
    // quand le monde vient de chez lui.
    let vide = j.chemin().join(".vide");
    fs::create_dir_all(vide.join("versions")).unwrap();
    fs::create_dir_all(vide.join("saves/Neuf")).unwrap();
    fs::write(vide.join("saves/Neuf/level.dat"), b"").unwrap();
    dater(vide.join("saves/Neuf/level.dat"), 9_000);
    let trois = [vanilla.clone(), vide.clone()];
    assert_eq!(
        assets_par_defaut(&trois, Some(&vide.join("saves/Neuf"))),
        Some(vanilla.clone())
    );
    assert_eq!(assets_par_defaut(&trois, None), Some(vanilla));
}
