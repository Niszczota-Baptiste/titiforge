//! **Changer de monde dans la même fenêtre.**
//!
//! La scène d'après doit être EXACTEMENT celle qu'une ouverture directe du
//! nouveau monde donnerait — pas un mélange des deux, et rien de l'ancien
//! monde inscrit à la fenêtre de résidence.

mod commun;

use commun::{canon, codex, montre, semer, semer_build, Jetable};
use tf_app::scene::Ouvert;
use tf_world::Seance;

#[test]
fn la_scene_d_apres_est_celle_d_une_ouverture_directe() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let a = Jetable::neuf("monde-a");
    semer(a.chemin(), 1, 4);
    let b = Jetable::neuf("monde-b");
    semer_build(b.chemin(), 4);
    let seances = Jetable::neuf("seances");
    let zone = [0, 0, 1, 1];

    let mut o = Ouvert::ouvrir(&pack, Some(a.texte()), zone).unwrap();
    let avant = o.rechargements;
    let (seance, _, _) = Seance::ouvrir(seances.chemin(), b.chemin()).unwrap();
    o.changer_de_monde(seance.staging(), b.texte(), zone, None)
        .unwrap();

    assert_eq!(o.nom, b.texte());
    assert!(o.editable());
    assert_eq!(
        o.rechargements,
        avant + 1,
        "le GPU doit remonter l'atlas : c'est ce compteur qui le lui dit"
    );

    let direct = Ouvert::ouvrir(&pack, Some(b.texte()), zone).unwrap();
    let mut mots = Vec::new();
    let (c1, c2) = (canon(&o, &mut mots), canon(&direct, &mut mots));
    assert_eq!(
        montre(&o, &c1),
        montre(&direct, &c2),
        "ce qu'on voit après le changement est ce qu'une ouverture directe montre"
    );
    assert_eq!(
        o.cellules_residentes(),
        direct.cellules_residentes(),
        "rien de l'ancien monde ne reste inscrit à la fenêtre de résidence"
    );
    assert_eq!(o.octets_comptes(), direct.octets_comptes());
}

#[test]
fn la_copie_jetable_de_l_ancien_monde_part_avec_lui() {
    // Ouvert par `ouvrir`, l'ancien monde avait une copie JETABLE, dans le
    // dossier temporaire. Changer de monde doit l'effacer — sinon chaque
    // changement en laisserait une, jusqu'au prochain balayage.
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let a = Jetable::neuf("monde-a");
    semer(a.chemin(), 1, 4);
    let b = Jetable::neuf("monde-b");
    semer(b.chemin(), 1, 4);
    let seances = Jetable::neuf("seances");
    let zone = [0, 0, 1, 1];

    let mut o = Ouvert::ouvrir(&pack, Some(a.texte()), zone).unwrap();
    let jetable = o.staging.as_ref().unwrap().overlay().root().to_path_buf();
    assert!(jetable.is_dir());
    let (seance, _, _) = Seance::ouvrir(seances.chemin(), b.chemin()).unwrap();
    o.changer_de_monde(seance.staging(), b.texte(), zone, None)
        .unwrap();
    assert!(!jetable.exists(), "{}", jetable.display());

    // Celle d'une SÉANCE, en revanche, appartient à la séance : la scène n'y
    // touche pas en partant.
    let couche = seance.staging().overlay().root().to_path_buf();
    let c = Jetable::neuf("monde-c");
    semer(c.chemin(), 1, 4);
    let (autre, _, _) = Seance::ouvrir(seances.chemin(), c.chemin()).unwrap();
    o.changer_de_monde(autre.staging(), c.texte(), zone, None)
        .unwrap();
    assert!(
        couche.is_dir(),
        "la séance décide de ce qui survit, pas la scène"
    );
}
