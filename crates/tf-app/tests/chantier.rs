//! **Ce que le fil écrit, la coque le relit.**
//!
//! C'est la jonction que ni le moteur ni la scène ne peuvent prouver seuls :
//! le fil écrit dans la copie de travail, la coque remaille DEPUIS elle. Relire
//! la source rendrait le monde d'AVANT — ce qui se lit « le bouton ne fait
//! rien », et ne désigne pas la cause.
//!
//! Le test demande un pack (`TF_PACK`), comme les autres tests qui touchent à
//! de vrais assets : un test qu'on ne peut pas jouer sans une donnée qu'on n'a
//! pas ne doit pas casser la suite de quelqu'un, mais il doit EXISTER.

use std::time::{Duration, Instant};

use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_app::scene::Ouvert;
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::journal::Journal;

/// Écrit un monde jetable à partir de la fixture de terrain.
fn semer(dir: &std::path::Path) -> std::io::Result<()> {
    let brut = tf_bench::region(&tf_bench::Terrain::petite());
    std::fs::create_dir_all(dir.join("region"))?;
    std::fs::write(dir.join("region/r.0.0.mca"), brut)?;
    // Un `level.dat` vide suffit à `FsSource` pour reconnaître une save.
    std::fs::write(dir.join("level.dat"), [])?;
    Ok(())
}

fn attendre(m: &mut Moteur) -> Reponse {
    let debut = Instant::now();
    loop {
        if let Some(r) = m.recevoir().into_iter().next() {
            return r;
        }
        assert!(debut.elapsed() < Duration::from_secs(60), "pas de réponse");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn ce_que_le_fil_ecrit_la_coque_le_relit() {
    let Ok(pack) = std::env::var("TF_PACK") else {
        eprintln!("TF_PACK absent — test sauté (il a besoin d'un vrai pack)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("tf-chantier-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    semer(&dir).expect("monde jetable");

    let mut ouvert = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), [0, 0, 1, 1])
        .expect("le monde doit s'ouvrir");
    assert!(ouvert.editable(), "un monde ouvert est éditable");

    // Une case qu'on va écraser, et ce qu'elle vaut AVANT.
    let case = [8, -40, 8];
    let avant = ouvert.monde.grille.bloc(case[0], case[1], case[2]);

    let mut moteur = Moteur::lancer(
        ouvert.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:glowstone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    assert!(!r.echoue(), "{}", r.texte());
    assert!(r.bornes().is_some(), "l'opération doit avoir écrit");

    // **Avant de remailler, la coque voit encore le monde d'avant** : elle
    // tient une grille, pas une vue vivante sur le disque.
    assert_eq!(ouvert.monde.grille.bloc(case[0], case[1], case[2]), avant);

    ouvert.remailler().expect("remaillage");
    let apres = ouvert.monde.grille.bloc(case[0], case[1], case[2]);
    assert_ne!(apres, avant, "la coque relit toujours le monde d'avant");

    // Et l'annulation revient en arrière, par le même chemin.
    assert!(moteur.envoyer(Commande::Annuler));
    let r = attendre(&mut moteur);
    assert!(matches!(r, Reponse::Defait { .. }), "{r:?}");
    ouvert.remailler().expect("remaillage");
    let defait = ouvert.monde.grille.bloc(case[0], case[1], case[2]);
    assert_ne!(defait, apres, "annuler doit se voir");

    moteur.arreter();
    drop(ouvert);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Écrire dans la save : l'ordre, et la preuve qu'il est tenu.**
///
/// Refuser si le jeu tient le monde, SAUVEGARDER, puis écrire. Une sauvegarde
/// prise après la première écriture ne sauvegarde plus rien — et c'est le
/// genre de faute qu'on ne découvre que le jour où on en a besoin.
#[test]
fn ecrire_sauvegarde_avant_d_ecrire() {
    let Ok(pack) = std::env::var("TF_PACK") else {
        eprintln!("TF_PACK absent — test sauté");
        return;
    };
    let dir = std::env::temp_dir().join(format!("tf-ecrire-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    semer(&dir).expect("monde jetable");
    let avant = std::fs::read(dir.join("region/r.0.0.mca")).unwrap();

    let ouvert = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), [0, 0, 1, 1]).unwrap();
    let mut moteur = Moteur::lancer(
        ouvert.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );

    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:glowstone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    assert!(!attendre(&mut moteur).echoue());

    // Tant qu'on n'a pas écrit, la SAVE est intacte — invariant n° 1.
    assert_eq!(
        std::fs::read(dir.join("region/r.0.0.mca")).unwrap(),
        avant,
        "la save a été touchée avant l'ordre d'écrire"
    );

    assert!(moteur.envoyer(Commande::Ecrire {
        confirme_sans_verrou: true
    }));
    let r = attendre(&mut moteur);
    let Reponse::Ecrit {
        regions,
        sauvegarde,
    } = &r
    else {
        panic!("attendu Ecrit, reçu {r:?} — {}", r.texte());
    };
    assert!(*regions > 0);

    // La save a changé…
    assert_ne!(std::fs::read(dir.join("region/r.0.0.mca")).unwrap(), avant);
    // … et la SAUVEGARDE porte le monde d'AVANT, octet pour octet. C'est
    // toute la raison d'être de l'étape 2.
    let copie = std::path::Path::new(sauvegarde).join("region/r.0.0.mca");
    assert!(copie.exists(), "pas de sauvegarde en {sauvegarde}");
    assert_eq!(
        std::fs::read(&copie).unwrap(),
        avant,
        "la sauvegarde ne porte pas le monde d'avant"
    );

    moteur.arreter();
    drop(ouvert);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(sauvegarde);
}
