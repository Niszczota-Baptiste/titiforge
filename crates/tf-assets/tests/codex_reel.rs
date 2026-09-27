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

use std::path::{Path, PathBuf};

use tf_assets::catalogue::{classer, table_formes, Classement, Disposition};
use tf_assets::fluides;
use tf_assets::{textures_des_etats, Atlas, Catalogue, Dossier};
use tf_mesh::{Formes, GenreFluide, TextureFluide};

/// Le chemin de `TF_PACK`, lu comme la documentation l'écrit : RELATIF À LA
/// RACINE du dépôt quand il ne se trouve pas d'ici.
///
/// Cargo lance un test depuis le dossier de SON crate : la commande du
/// `CLAUDE.md`, `TF_PACK=../titisite/public/codex`, y désignait un dossier
/// qui n'existe pas. Le test se disait alors « TF_PACK non défini » — il
/// l'était — et passait au vert sans avoir rien lu.
fn chemin_du_pack() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("TF_PACK")?);
    if p.is_relative() && !p.exists() {
        let depuis_la_racine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(&p);
        if depuis_la_racine.exists() {
            return Some(depuis_la_racine);
        }
    }
    Some(p)
}

/// Le pack désigné et sa source. `None` seulement si RIEN n'est désigné : un
/// chemin donné mais illisible n'est pas un test à sauter, c'est une commande
/// qui ne fait pas ce qu'on croit.
fn pack_et_source() -> Option<(Dossier, Catalogue)> {
    let racine = chemin_du_pack()?;
    let src = Dossier::ouvrir(&racine)
        .unwrap_or_else(|e| panic!("TF_PACK = {} ne s'ouvre pas : {e}", racine.display()));
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src)
        .unwrap_or_else(|e| panic!("TF_PACK = {} n'est pas un codex : {e}", racine.display()));
    cat.resoudre_modeles(&src);
    Some((src, cat))
}

fn pack() -> Option<Catalogue> {
    pack_et_source().map(|(_, cat)| cat)
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

#[test]
fn l_eau_et_la_lave_du_codex_sont_des_fluides_qu_aucun_cube_ne_double() {
    let Some((src, cat)) = pack_et_source() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    // Le codex ne livre aucun modèle d'eau, et celui de la lave n'a pas
    // d'élément (`"render": "cube"` et une particule). Un cube lu là-dedans
    // se dessinerait PAR-DESSUS la surface : une eau opaque, une lave
    // doublée.
    let cles = [
        "minecraft:water|level=0",
        "minecraft:water|level=5",
        "minecraft:lava|level=0",
        "minecraft:kelp|age=3",
        "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=true",
    ];
    let t = table_formes(&cat, cles.iter().map(|c| c.to_string()), &|_| false);
    for (i, cle) in cles.iter().enumerate() {
        assert!(t.fluide(i as u32).is_some(), "{cle} porte un fluide");
    }
    for (i, cle) in cles.iter().enumerate().take(3) {
        let i = i as u32;
        assert!(
            t.est_air(i) && !t.opaque(i) && t.cuboides(i).is_empty(),
            "{cle} : aucun cube ne doit doubler la surface"
        );
    }
    assert!(
        !t.cuboides(4).is_empty(),
        "l'escalier inondé reste un escalier"
    );
    // Les textures : l'immobile et le courant de chaque fluide, trouvés
    // pour de vrai — pas par le repli, qui masquerait une texture absente.
    let atlas = Atlas::batir(
        &src,
        textures_des_etats(&cat, cles.iter().map(|c| c.to_string())),
        &|n| Disposition::Codex.chemins_texture(n),
    );
    for (genre, texture, nom) in [
        (
            GenreFluide::Eau,
            TextureFluide::Immobile,
            fluides::EAU_IMMOBILE,
        ),
        (
            GenreFluide::Eau,
            TextureFluide::Courant,
            fluides::EAU_COURANTE,
        ),
        (
            GenreFluide::Lave,
            TextureFluide::Immobile,
            fluides::LAVE_IMMOBILE,
        ),
        (
            GenreFluide::Lave,
            TextureFluide::Courant,
            fluides::LAVE_COURANTE,
        ),
    ] {
        let couche = atlas
            .couche(nom)
            .unwrap_or_else(|| panic!("{nom} dans l'atlas"));
        assert_eq!(fluides::couche(&atlas, genre, texture), couche, "{nom}");
    }
    eprintln!(
        "voile de l'eau dans le codex : {}",
        if atlas.couche(fluides::EAU_VOILE).is_some() {
            "présent"
        } else {
            "absent, replié sur le courant"
        }
    );
}

/// **Ce que le rendu ne sait pas encore poser**, compté sur le vrai pack.
///
/// Une face gloutonne répète la tuile ENTIÈRE par bloc : un cube plein qui
/// ne déclare qu'une PORTION de tuile sur une face la montre entière. Et un
/// élément tourné d'un angle qui n'est pas droit (`rotation` d'élément, les
/// croix de plantes à 45°) est dessiné droit. Le chiffre dit si l'un ou
/// l'autre mérite du travail — pas une impression.
#[test]
fn ce_que_le_rendu_ne_pose_pas_encore_se_compte() {
    let Some(cat) = pack() else {
        eprintln!("TF_PACK non défini : test sauté");
        return;
    };
    let mut cubes = std::collections::BTreeSet::new();
    let mut portions = std::collections::BTreeSet::new();
    let mut penches = std::collections::BTreeSet::new();
    for (nom, bs) in cat.blocs() {
        for v in bs.modeles() {
            let Some(m) = cat.modele(&v.modele) else {
                continue;
            };
            if m.elements
                .iter()
                .any(|e| e.rotation.is_some_and(|r| r.angle % 90.0 != 0.0))
            {
                penches.insert(nom.clone());
            }
            let plein = m.elements.iter().find(|e| {
                (0..3).all(|k| e.from[k].min(e.to[k]) <= 0.0 && e.from[k].max(e.to[k]) >= 16.0)
            });
            let Some(e) = plein else {
                continue;
            };
            cubes.insert(nom.clone());
            for (f, fd) in &e.faces {
                let uv = tf_assets::modele::uv_de(e, *f, fd);
                if (uv[2] - uv[0]).abs() != 16.0 || (uv[3] - uv[1]).abs() != 16.0 {
                    portions.insert(nom.clone());
                }
            }
        }
    }
    eprintln!(
        "{} blocs à cube plein, dont {} déclarent une PORTION de tuile sur une face ; \
         {} blocs ont un élément penché hors angle droit",
        cubes.len(),
        portions.len(),
        penches.len()
    );
    eprintln!(
        "portions : {:?}",
        portions.iter().take(12).collect::<Vec<_>>()
    );
}
