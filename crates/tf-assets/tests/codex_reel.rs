//! Le pack RÉEL, quand il est là.
//!
//! Aucune donnée du serveur n'est copiée dans ce dépôt : ce test ne tourne que
//! si on lui désigne un pack, comme le test de croisement avec le moteur JS.
//!
//! ```text
//! TF_PACK=../titisite/public/codex cargo test -p tf-assets --test codex_reel -- --nocapture
//! ```
//!
//! Un test qu'on ne peut pas jouer sans une donnée privée ne doit pas faire
//! échouer la suite de quelqu'un qui ne l'a pas — mais il doit exister, parce
//! qu'un pack écrit à la main ne reproduit jamais ce qu'un vrai serveur
//! contient.

use tf_assets::catalogue::{classer, Classement, Disposition};
use tf_assets::{Catalogue, Dossier};

fn pack() -> Option<Catalogue> {
    let racine = std::env::var("TF_PACK").ok()?;
    let src = Dossier::ouvrir(&racine).ok()?;
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).ok()?;
    cat.resoudre_modeles(&src);
    Some(cat)
}

#[test]
fn le_pack_du_serveur_se_lit_sans_le_moindre_trou() {
    let Some(cat) = pack() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    eprintln!(
        "{} blocs, {} modèles, {} introuvables",
        cat.nb_blocs(),
        cat.nb_modeles(),
        cat.introuvables.len()
    );
    assert!(
        cat.introuvables.is_empty(),
        "un recensement qui laisse des trous ne dit rien : {:?}",
        &cat.introuvables[..cat.introuvables.len().min(8)]
    );
    assert!(cat.nb_blocs() > 2000, "le codex en déclare 2 560");
}

#[test]
fn la_cible_minefield_reste_faite_aux_deux_tiers_de_modeles() {
    let Some(cat) = pack() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    let mut cube = 0usize;
    let mut modele = 0usize;
    let mut vide = 0usize;
    for (nom, _) in cat.blocs() {
        if !nom.starts_with("minefield:") {
            continue;
        }
        let Some(m) = cat.modele_de(nom) else {
            continue;
        };
        match classer(m) {
            Classement::Cube => cube += 1,
            Classement::Modele => modele += 1,
            Classement::Vide => vide += 1,
        }
    }
    let total = cube + modele + vide;
    let part = 100.0 * modele as f64 / total as f64;
    eprintln!("minefield : {cube} cubes, {modele} modèles, {vide} vides ({part:.1} % de modèles)");
    assert!(
        (60.0..75.0).contains(&part),
        "le chiffre qui dimensionne toute la phase 2 : {part:.1} % de modèles"
    );
}

#[test]
fn les_modeles_les_plus_lourds_sont_ceux_qu_on_attend() {
    let Some(cat) = pack() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    let mut pire: Vec<(usize, String)> = cat
        .blocs()
        .filter_map(|(nom, _)| cat.modele_de(nom).map(|m| (m.elements.len(), nom.clone())))
        .collect();
    pire.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    eprintln!("les plus lourds : {:?}", &pire[..pire.len().min(5)]);
    assert!(
        pire[0].0 >= 80,
        "le pire cas du serveur fait 82 cuboïdes ; en trouver moins voudrait \
         dire qu'on ne résout pas tout : {:?}",
        &pire[..3]
    );
}
