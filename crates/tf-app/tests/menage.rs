//! **Ce qu'une séance laisse derrière elle.**
//!
//! Une copie de travail se supprime à la fermeture ; un `kill`, une panne ou
//! un plantage la laissent. Elles ne se voient pas — elles vivent dans le
//! dossier temporaire — et elles pèsent ce que pèsent les régions éditées.
//! Mesuré après une séance de développement : dix-sept mégaoctets en dix-sept
//! dossiers.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// Donne à un fichier — ou à un dossier — la date voulue.
///
/// `File::open` sur un dossier suffit sous Unix, qui est là où tourne la
/// suite ; ce n'est pas une brique du programme, seulement de la fixture.
fn dater(chemin: &Path, quand: SystemTime) {
    let f = std::fs::File::open(chemin).unwrap();
    f.set_times(std::fs::FileTimes::new().set_modified(quand))
        .unwrap();
}

/// Fabrique un faux dossier abandonné : `<nom>/region/r.0.0.mca`.
///
/// `age_racine` vieillit le dossier, `age_region` le fichier — SÉPARÉMENT,
/// parce que c'est précisément là que les deux critères possibles divergent.
fn faux_abandon(nom: &str, age_racine: Duration, age_region: Duration) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(nom);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("region")).unwrap();
    std::fs::write(d.join("region/r.0.0.mca"), b"des octets").unwrap();
    let maintenant = SystemTime::now();
    // Du plus profond vers la racine : dater un fichier ne change pas la date
    // de son dossier, mais autant ne pas dépendre de cet ordre.
    dater(&d.join("region/r.0.0.mca"), maintenant - age_region);
    dater(&d.join("region"), maintenant - age_racine);
    dater(&d, maintenant - age_racine);
    d
}

/// **On n'efface que ce qui porte notre préfixe ET qui est vieux.**
///
/// Le risque de ce balayage n'est pas de perdre une save — la source n'est
/// jamais touchée — mais de jeter la copie de travail d'une séance encore
/// ouverte, donc des opérations non écrites. D'où les deux conditions, et pas
/// une seule.
///
/// **Un seul test, et c'est délibéré.** J'en avais écrit deux ; ils partagent
/// le dossier temporaire, ils tournent en parallèle, et le second balayait les
/// fixtures du premier. Deux tests qui partagent un état GLOBAL ne sont pas
/// deux tests — c'est la même leçon que deux mondes ouverts qui partageaient
/// leur copie de travail, un cran plus haut.
#[test]
fn le_balayage_n_efface_que_les_abandons_vieux() {
    let jour = Duration::from_secs(48 * 3600);
    let instant = Duration::from_secs(60);

    let vieux = faux_abandon("titiforge-essai-vieux", jour, jour);
    let frais = faux_abandon("titiforge-essai-frais", instant, instant);
    let etranger = faux_abandon("pas-a-nous-essai", jour, jour);
    // Une séance ouverte depuis deux jours et TOUJOURS en train d'éditer : la
    // copie a été faite avant-hier, la région vient d'être réécrite.
    let en_cours = faux_abandon("titiforge-essai-en-cours", jour, instant);

    let n = tf_app::scene::balayer_les_abandons();
    assert!(n >= 1, "le vieux devait partir");

    assert!(!vieux.exists(), "une copie abandonnée de 48 h est restée");
    assert!(
        frais.exists(),
        "une copie d'il y a une minute a été effacée — une séance ouverte \
         aurait perdu son travail"
    );
    assert!(
        etranger.exists(),
        "un dossier qui n'est pas à nous a été effacé"
    );
    // **Le cas qui décide du critère.** Mesuré : réécrire `region/r.0.0.mca`
    // ne change ni la date de la racine, ni celle de `region/` — un dossier ne
    // voit passer que les créations et les suppressions d'entrées. Dater la
    // racine, c'est donc effacer sous ses pieds la copie de travail d'une
    // séance qui édite depuis plus d'un jour.
    assert!(
        en_cours.exists(),
        "une séance ouverte depuis 48 h mais qui vient d'éditer a été effacée \
         — ses opérations non écrites n'existaient nulle part ailleurs"
    );

    // Et un second passage ne trouve plus rien : le balayage est idempotent,
    // et ne pas pouvoir faire le ménage n'empêche jamais de travailler.
    assert_eq!(tf_app::scene::balayer_les_abandons(), 0);

    for restant in [&frais, &etranger, &en_cours] {
        let _ = std::fs::remove_dir_all(restant);
    }
}
