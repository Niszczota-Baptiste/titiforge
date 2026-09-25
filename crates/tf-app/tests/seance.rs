//! **La séance de bout en bout** : un vrai monde sur disque, le fil moteur,
//! la fermeture, la reprise.
//!
//! Les pièces se testent chacune de leur côté (`tf-world/tests/session.rs`,
//! `moteur.rs`). Ce fichier teste leur JONCTION : que ce que le fil range est
//! bien ce que la reprise relit, et que fermer après avoir tout annulé ne
//! laisse rien derrière.

mod commun;

use std::time::{Duration, Instant};

use commun::{semer, Jetable};
use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::session::dossier_de;
use tf_world::{Dimension, Reprise, Seance};

const SURFACE: Dimension = Dimension::Overworld;

fn poser(bloc: &str) -> Commande {
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte(bloc));
    Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }
}

fn attendre(m: &mut Moteur) -> Reponse {
    let debut = Instant::now();
    loop {
        if let Some(r) = m.recevoir().into_iter().next() {
            return r;
        }
        assert!(debut.elapsed() < Duration::from_secs(20), "pas de réponse");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Ouvre la séance et lance le fil dessus, comme la coque.
fn lancer(racine: &std::path::Path, monde: &std::path::Path) -> (Moteur, Reprise) {
    let (s, journal, reprise) = Seance::ouvrir(racine, monde).unwrap();
    let st = s.staging();
    let m = Moteur::lancer_en_seance(st, SURFACE, journal, Some(monde.to_path_buf()), Box::new(s));
    (m, reprise)
}

#[test]
fn editer_fermer_rouvrir_annuler_fermer() {
    let j = Jetable::neuf("seance-bout-en-bout");
    let monde = j.chemin().join("monde");
    semer(&monde, 1, 4);
    let racine = j.chemin().join("seances");
    let region = monde.join("region/r.0.0.mca");
    let origine = std::fs::read(&region).unwrap();

    // Première séance : une édition, puis on ferme SANS écrire.
    let (mut m, reprise) = lancer(&racine, &monde);
    assert_eq!(reprise, Reprise::Neuve);
    assert!(m.envoyer(poser("minecraft:gold_block")));
    assert!(matches!(attendre(&mut m), Reponse::Fait { .. }));
    m.arreter();
    assert!(
        dossier_de(&racine, &monde).join("couche").is_dir(),
        "du travail pas écrit : la séance reste"
    );

    // Seconde séance : le travail ET l'annulation sont là.
    let (mut m, reprise) = lancer(&racine, &monde);
    assert!(
        matches!(
            reprise,
            Reprise::Reprise {
                actions: 1,
                regions: 1,
                ..
            }
        ),
        "{reprise:?}"
    );
    assert!(m.envoyer(Commande::Annuler));
    let r = attendre(&mut m);
    assert!(
        matches!(r, Reponse::Defait { .. }),
        "l'annulation d'hier se joue aujourd'hui : {r:?}"
    );
    m.arreter();

    // Tout est annulé : la copie ne porte plus rien que la save n'ait, et la
    // séance ne traîne pas.
    assert!(
        !dossier_de(&racine, &monde).exists(),
        "éditer puis tout annuler ne laisse pas de séance derrière"
    );
    assert_eq!(
        std::fs::read(&region).unwrap(),
        origine,
        "et la save n'a jamais bougé"
    );
}
