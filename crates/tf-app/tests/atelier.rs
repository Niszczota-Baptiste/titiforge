//! **Le maillage hors du fil principal, et l'ordre dans lequel il revient.**
//!
//! Le maillage part sur la réserve de fils avec un extrait de la grille, et
//! revient plus tard. Tout ce qui fait sa justesse tient à l'ORDRE : chaque
//! extrait est la grille à son départ, donc les résultats doivent s'appliquer
//! dans l'ordre où les travaux sont partis — un vieux maillage appliqué après
//! un neuf remettrait à l'écran ce qui n'est plus.
//!
//! Aucun vol ne le vérifierait sinon que par chance : il faut qu'un travail
//! revienne APRÈS un plus récent. `retarder_le_prochain_maillage` le force, et
//! `maillage_juste` compare la scène à un maillage complet de sa grille.

mod commun;

use std::time::{Duration, Instant};

use commun::{codex, semer, Jetable};
use tf_app::chargeur::{Chargeur, Reponse};
use tf_app::moteur::{Commande, Moteur};
use tf_app::scene::{Arrivee, Ouvert};
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::Journal;

/// Les arrivées d'UNE cellule — lues par le fil, pas intégrées.
fn arrivee_de(o: &Ouvert, bx: i32, bz: i32) -> Vec<Arrivee> {
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    c.demander(tf_world::demande::par_region(&tf_world::demande::voulues(
        BlockPos::new(bx, 64, bz),
        [1.0, 0.0, 0.0],
        0,
        tf_world::Niveau::Chunk,
        (-64, 319),
    )));
    let debut = Instant::now();
    let mut out = Vec::new();
    while out.is_empty() {
        for r in c.recevoir(0) {
            if let Reponse::Prete {
                cellule,
                sections,
                interner,
            } = r
            {
                out.push(Arrivee {
                    cellule,
                    sections,
                    interner,
                });
            }
        }
        assert!(debut.elapsed() < Duration::from_secs(60), "rien n'arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
    c.arreter();
    out
}

fn ouvrir() -> (Jetable, Jetable, Ouvert) {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);
    let o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    (j, m, o)
}

#[test]
fn un_vieux_maillage_ne_passe_pas_par_dessus_un_neuf() {
    let (_j, _m, mut o) = ouvrir();
    let pleine = arrivee_de(&o, 24, 8);
    let cellule = pleine[0].cellule.clone();
    assert!(
        !pleine[0].sections.is_empty(),
        "la prémisse : la cellule porte quelque chose"
    );

    // Le premier travail — la cellule PLEINE — revient en dernier.
    o.retarder_le_prochain_maillage(Duration::from_millis(400));
    o.integrer(pleine).expect("intégration");
    // Le second — la même cellule revenue VIDE — revient tout de suite.
    o.integrer(vec![Arrivee {
        cellule,
        sections: Vec::new(),
        interner: tf_anvil::Interner::new(),
    }])
    .expect("intégration du vide");
    assert_eq!(
        o.maillages_en_vol(),
        2,
        "la prémisse : deux travaux en route"
    );
    o.attendre_maillage().expect("maillage");

    assert_eq!(
        o.maillage_juste(),
        Ok(()),
        "le travail parti le PREMIER doit s'appliquer le premier, même revenu \\
         le dernier : sinon la cellule vidée se redessine pleine"
    );
}

#[test]
fn un_rechargement_oublie_ce_qui_etait_en_route() {
    let (_j, _m, mut o) = ouvrir();
    let pleine = arrivee_de(&o, 24, 8);
    o.retarder_le_prochain_maillage(Duration::from_millis(300));
    o.integrer(pleine).expect("intégration");
    assert_eq!(o.maillages_en_vol(), 1);
    // Le rechargement remplace le monde par la zone : la cellule (1, 0) n'y
    // est plus, et le travail qui la maillait ne doit rien y remettre.
    o.remailler(None).expect("rechargement");
    o.attendre_maillage().expect("maillage");
    std::thread::sleep(Duration::from_millis(400));
    o.integrer(Vec::new()).expect("image vide");
    assert_eq!(
        o.maillage_juste(),
        Ok(()),
        "le vieux travail a été appliqué"
    );
    assert_eq!(o.rechargements, 1);
}

#[test]
fn une_edition_passe_apres_ce_qui_etait_en_route() {
    let (_j, m, mut o) = ouvrir();
    let pleine = arrivee_de(&o, 24, 8);
    // La cellule arrive, et son maillage traîne.
    o.retarder_le_prochain_maillage(Duration::from_millis(400));
    o.integrer(pleine).expect("intégration");

    // Pendant ce temps, on édite DANS cette cellule.
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:emerald_block"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        // En plein ciel : un bloc enfoui dans la pierre ne change aucune face
        // visible, et un vieux maillage posé par-dessus ne se verrait pas.
        sel: BBox::new(BlockPos::new(20, 150, 4), BlockPos::new(22, 150, 6)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let debut = Instant::now();
    let bornes = loop {
        if let Some(r) = moteur.recevoir().into_iter().next() {
            break r.bornes();
        }
        assert!(debut.elapsed() < Duration::from_secs(60), "pas de réponse");
        std::thread::sleep(Duration::from_millis(5));
    };
    moteur.arreter();
    o.remailler(bornes).expect("remaillage");
    o.attendre_maillage().expect("maillage");

    assert_eq!(o.monde.etat_en(21, 150, 5), "minecraft:emerald_block");
    assert_eq!(
        o.maillage_juste(),
        Ok(()),
        "le maillage d'avant l'édition, revenu après elle, l'a recouverte"
    );
}
