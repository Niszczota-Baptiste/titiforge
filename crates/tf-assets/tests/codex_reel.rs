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

/// **La rotation d'une variante fait un vrai travail sur le pack du serveur.**
///
/// Un pack ne décrit pas seize escaliers : il en décrit un et le tourne. Si la
/// rotation de la variante est ignorée, tous les états d'un bloc rendent la
/// géométrie de l'état non tourné — donc tous les escaliers d'un build
/// regardent dans la même direction, sans la moindre erreur à l'écran.
///
/// On ne peut pas exiger que deux angles donnent deux géométries : une
/// enclume, une grappe d'améthyste, un éventail de corail sont réellement
/// invariants — seules leurs TEXTURES tournent. Vouloir le contraire
/// demanderait de savoir d'avance quels modèles sont symétriques, c'est-à-dire
/// de refaire le calcul qu'on veut vérifier.
///
/// Le signal est ailleurs, et il est net : **la rotation ignorée rend 100 %
/// des couples identiques.** Mesuré sur le pack du serveur avec la rotation
/// appliquée, il en reste 9,5 %, tous des symétries réelles. Un plafond très
/// large suffit donc à attraper la régression sans avoir à nommer un seul
/// bloc.
#[test]
fn la_rotation_d_une_variante_fait_un_vrai_travail_sur_le_pack() {
    let Some(cat) = pack() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    use std::collections::BTreeMap;
    use tf_assets::blockstates::Blockstate;
    use tf_assets::rotation::tourner;

    let mut couples = 0usize;
    let mut invariants = 0usize;
    let mut symetriques: BTreeMap<String, usize> = BTreeMap::new();

    for (_nom, bs) in cat.blocs() {
        // Les angles auxquels chaque modèle est posé, groupés par modèle.
        let mut par_modele: BTreeMap<String, Vec<(u16, u16)>> = BTreeMap::new();
        let variantes: Vec<_> = match bs {
            Blockstate::Variants(v) => v.iter().flat_map(|(_, w)| w.iter()).collect(),
            Blockstate::Multipart(r) => r.iter().flat_map(|r| r.modeles.iter()).collect(),
        };
        for v in variantes {
            par_modele
                .entry(format!("{}:{}", v.modele.namespace, v.modele.chemin))
                .or_default()
                .push((v.x, v.y));
        }
        for (id, mut angles) in par_modele {
            angles.sort_unstable();
            angles.dedup();
            if angles.len() < 2 {
                continue;
            }
            let Some(m) = cat.modele(&tf_assets::Id::parse(&id)) else {
                continue;
            };
            let base = tf_assets::cuboides(m);
            let formes: Vec<_> = angles
                .iter()
                .map(|&(x, y)| tourner(base.clone(), x, y))
                .collect();
            for i in 0..formes.len() {
                for j in (i + 1)..formes.len() {
                    couples += 1;
                    if formes[i] == formes[j] {
                        invariants += 1;
                        *symetriques.entry(id.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let part = invariants as f64 * 100.0 / couples.max(1) as f64;
    eprintln!("{couples} couples d'angles · {invariants} invariants ({part:.1} %)");
    let mut top: Vec<_> = symetriques.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    for (id, n) in top.iter().take(5) {
        eprintln!("   invariant {n} fois : {id}");
    }
    assert!(
        couples > 100,
        "le pack devrait poser des centaines de modèles à plusieurs angles, il en a {couples}"
    );
    assert!(
        part < 25.0,
        "{part:.1} % des couples rendent la même géométrie — la rotation de la \
         variante n'est pas appliquée (elle donnerait 100 %)"
    );
}
