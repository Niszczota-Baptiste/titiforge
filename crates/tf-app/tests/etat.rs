//! **Ce que la coque FAIT, vérifié sans ouvrir une fenêtre.**
//!
//! Tout ce qui décide quelque chose dans `tf-app` vit dans `etat.rs`, en types
//! purs : ces tests ne montent ni surface, ni serveur graphique, ni GPU. Ce
//! n'est pas une commodité — un morceau d'interface qui n'existe que derrière
//! un écran ne se teste pas, et ce dépôt a déjà tranché la question pour le
//! rendu.
//!
//! Ce qui est vérifié ici est exactement ce qu'aucune moitié ne peut prouver
//! seule : la JONCTION entre viser, accrocher et poser.

use tf_app::etat::{direction, Etat, Quadrillage};
use tf_render::Camera;
use tf_world::coords::{BBox, BlockPos};
use tf_world::decoupe::Niveau;
use tf_world::inference::{accrocher, Ancre};

/// Un mur plein à partir de x = 10, et rien d'autre.
fn mur(case: [i32; 3]) -> bool {
    case[0] >= 10
}

/// L'œil devant ce mur, regardant plein est. La hauteur est choisie pour que
/// la case de POSE tombe à un bloc d'une référence de la sélection — c'est ce
/// qui rend l'accrochage observable.
fn camera_face_au_mur() -> Camera {
    Camera {
        oeil: [0.5, 7.5, 0.5],
        cible: [1.5, 7.5, 0.5],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    }
}

fn etat_devant_le_mur() -> Etat {
    let mut e = Etat::cadre([0.0, 0.0, 0.0], [32.0, 32.0, 32.0], 1.0);
    // La sélection touche le mur : son coin est à x = 10, donc À UN BLOC de la
    // case de pose. Sans verrouillage d'axe, elle est dans la tolérance.
    e.selection.poser_coin1(BlockPos::new(10, 0, 0));
    e.selection.poser_coin2(BlockPos::new(20, 8, 8));
    e
}

/// **Le piège que seule la jonction peut montrer.**
///
/// On vise la face ouest d'une paroi, on pose donc à `x − 1`. Mais la paroi
/// est à un bloc, donc dans la tolérance : l'accrochage ramènerait le bloc
/// neuf DANS le mur qu'on visait. L'accroche est juste, la pose aussi ; c'est
/// leur COMPOSITION qui ne l'est pas.
#[test]
fn l_axe_de_pose_ne_s_accroche_pas() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);

    let r = e.reticule;
    assert_eq!(r.case, Some(BlockPos::new(10, 7, 0)), "la case qu'on casse");
    assert_eq!(r.pose, Some(BlockPos::new(9, 7, 0)), "la case où l'on pose");

    let a = r.accroche.expect("une accroche est attendue");
    assert_eq!(a.position.x, 9, "la pose est revenue DANS le mur");

    // Le verrou se DIT, comme toute accroche : un axe figé en silence se lit
    // « l'outil ne s'aligne pas » sans qu'on sache que c'est voulu.
    let raison = a.raisons[0].expect("l'axe verrouillé doit porter sa raison");
    assert_eq!(raison.genre, Ancre::Dernier);
    assert_eq!(raison.ecart, 0);

    // TÉMOIN — sans le verrou, la référence à x = 10 gagne vraiment. Sans
    // cette moitié, le test ci-dessus serait vert même si le danger
    // n'existait pas, et ne prouverait rien.
    let refs = e.selection.boite().unwrap().references();
    let sans_verrou = accrocher(BlockPos::new(9, 7, 0), &refs, e.tolerance, [None; 3]);
    assert_eq!(
        sans_verrou.position.x, 10,
        "le témoin doit tomber dans le mur, sinon le verrou ne protège de rien"
    );
}

/// L'autre moitié de la même règle : ce sont les axes LIBRES qui portent tout
/// l'intérêt, puisque c'est dans le plan de la face qu'on s'aligne.
#[test]
fn les_axes_libres_s_accrochent_et_le_disent() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    let a = e.reticule.accroche.unwrap();

    // y : la pose brute est à 7, le haut de la sélection à 8.
    assert_eq!(a.position.y, 8);
    let ry = a.raisons[1].expect("l'axe y s'est accroché, il doit le dire");
    assert_eq!(ry.ecart, 1);
    assert_eq!(ry.reference.y, 8);

    // z : déjà sur une référence — écart nul, mais la raison existe, sinon
    // l'utilisateur ne sait pas à quoi il tient.
    assert_eq!(a.position.z, 0);
    assert_eq!(a.raisons[2].expect("z tient à une référence").ecart, 0);

    assert_eq!(a.axes_accroches(), 3);
    assert!(a.a_bouge());
}

/// Un réticule périmé fait poser un bloc là où l'utilisateur ne regarde plus.
#[test]
fn un_rayon_qui_ne_touche_rien_efface_le_reticule() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    assert!(e.reticule.case.is_some());

    // Le mur a disparu — ou l'on s'est tourné vers le ciel.
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &|_| false);
    assert_eq!(e.reticule.case, None);
    assert_eq!(e.reticule.pose, None);
    assert!(e.reticule.accroche.is_none());
    assert_eq!(e.point_de_pose(), None);
}

/// La portée borne le rayon : le mur est à dix blocs, on n'en regarde que
/// cinq.
#[test]
fn au_dela_de_la_portee_on_ne_vise_rien() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 5.0, &mur);
    assert_eq!(e.reticule.case, None);
}

#[test]
fn le_point_de_pose_prefere_l_accroche() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    let a = e.reticule.accroche.unwrap();
    assert_eq!(e.point_de_pose(), Some(a.position));
    assert_ne!(e.point_de_pose(), e.reticule.pose, "sinon on ne teste rien");
}

/// Zéro éteint l'accrochage — et rend la pose BRUTE, pas une pose figée par
/// le dernier verrou.
#[test]
fn une_tolerance_nulle_eteint_l_accrochage() {
    let mut e = etat_devant_le_mur();
    e.tolerance = 0;
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    assert!(e.reticule.accroche.is_none());
    assert_eq!(e.point_de_pose(), Some(BlockPos::new(9, 7, 0)));
}

/// **« Alignée sur les chunks » est vrai et ne prouve rien.**
///
/// Une sélection alignée en x et z dont la hauteur tombe au milieu d'une
/// tranche de seize ne couvre AUCUNE section entière. Le résumé compte les
/// sections plutôt que d'annoncer une propriété qui a l'air suffisante.
#[test]
fn le_verdict_compte_les_sections_au_lieu_de_les_deduire() {
    let mut e = Etat::cadre([0.0, 0.0, 0.0], [32.0, 32.0, 32.0], 1.0);
    e.selection.poser_coin1(BlockPos::new(0, -40, 0));
    e.selection.poser_coin2(BlockPos::new(15, -21, 15));

    let r = e.resume_selection().unwrap();
    assert!(r.alignee_chunk, "la sélection EST alignée sur les chunks");
    assert_eq!(r.sections, (0, 2), "et ne couvre pourtant aucune section");
    assert!(
        r.verdict().contains("BLOC"),
        "le verdict doit annoncer l'étage bloc : {}",
        r.verdict()
    );

    // L'autre moitié du geste — et c'est là que le verdict change.
    assert!(e.selection.aligner_sections());
    let r = e.resume_selection().unwrap();
    assert_eq!(r.sections, (2, 2));
    assert!(r.verdict().contains("palette"), "{}", r.verdict());
    assert_eq!(r.taille, (16, 32, 16));
    assert_eq!(r.volume, 16 * 32 * 16);
}

#[test]
fn une_selection_vide_n_a_pas_de_resume() {
    let e = Etat::cadre([0.0, 0.0, 0.0], [32.0, 32.0, 32.0], 1.0);
    assert!(e.resume_selection().is_none());
    assert!(e.selection.boite().is_none());
}

/// Le seul endroit du dépôt qui traduise une `Face` du mailleur en
/// `Direction` de l'éditeur. Ce dépôt a payé QUATRE fois le piège des tables
/// qui divergent : la traduction porte sur le SENS, et ce test le fige.
#[test]
fn la_conversion_de_face_porte_le_sens_et_pas_le_rang() {
    use std::collections::BTreeSet;
    use tf_mesh::forme::FACES;

    let mut pas = BTreeSet::new();
    for f in FACES {
        let d = direction(f);
        assert_eq!(d.pas(), f.pas(), "{f:?} : pas différent");
        assert_eq!(d.axe(), f.axe(), "{f:?} : axe différent");
        assert_eq!(d.positif(), f.positif(), "{f:?} : signe différent");
        assert_eq!(direction(f.opposee()).pas(), d.opposee().pas());
        pas.insert(d.pas());
    }
    assert_eq!(pas.len(), 6, "deux faces ont été traduites pareil");
}

/// Le verrou que `relever_reticule` pose est bien celui de la face TRAVERSÉE,
/// pas celui de la case qu'on casse. L'inverser ferait poser de l'autre côté
/// du mur.
#[test]
fn le_verrou_porte_sur_l_axe_de_la_face_traversee() {
    // On vise vers le HAUT : le sol est plein à partir de y = 10.
    let cam = Camera {
        oeil: [7.5, 0.5, 0.5],
        cible: [7.5, 1.5, 0.5],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    let plafond = |c: [i32; 3]| c[1] >= 10;

    let mut e = Etat::cadre([0.0, 0.0, 0.0], [32.0, 32.0, 32.0], 1.0);
    e.selection.poser_coin1(BlockPos::new(0, 10, 0));
    e.selection.poser_coin2(BlockPos::new(8, 20, 8));
    e.relever_reticule(&cam, 1.0, 64.0, &plafond);

    assert_eq!(e.reticule.pose, Some(BlockPos::new(7, 9, 0)));
    let a = e.reticule.accroche.unwrap();
    assert_eq!(a.position.y, 9, "l'axe vertical devait être verrouillé");
    assert_eq!(a.raisons[1].unwrap().genre, Ancre::Dernier);
    // ... et x, lui, est libre : 7 s'accroche au coin 8.
    assert_eq!(a.position.x, 8);
}

/// Un premier écran lisible : les chunks servent à chaque geste, les `.mca` à
/// décider d'un export. Les deux d'office donneraient une grille illisible.
#[test]
fn le_quadrillage_s_ouvre_sur_les_chunks_seuls() {
    let q = Quadrillage::default();
    assert!(q.chunks.is_some());
    assert_eq!(q.mca, None);
    assert_eq!(Etat::cadre([0.0; 3], [1.0; 3], 1.0).quadrillage, q);
}

/// L'état d'ouverture cadre sur ce qu'on vient de charger : l'œil regarde le
/// contenu, pas le vide. Sans ça, la première image d'un monde est noire et
/// personne ne sait dans quel sens tourner.
#[test]
fn l_etat_d_ouverture_regarde_le_contenu() {
    let e = Etat::cadre([0.0, 0.0, 0.0], [64.0, 32.0, 64.0], 16.0 / 9.0);
    let modele = Camera {
        oeil: [0.0; 3],
        cible: [0.0, 0.0, 1.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    let cam = e.vue.camera(&modele);
    let centre = [32.0f32, 16.0, 32.0];
    let d = [
        cam.cible[0] - cam.oeil[0],
        cam.cible[1] - cam.oeil[1],
        cam.cible[2] - cam.oeil[2],
    ];
    let v = [
        centre[0] - cam.oeil[0],
        centre[1] - cam.oeil[1],
        centre[2] - cam.oeil[2],
    ];
    let n = |a: [f32; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let cos = (d[0] * v[0] + d[1] * v[1] + d[2] * v[2]) / (n(d) * n(v));
    assert!(
        cos > 0.99,
        "la caméra ne regarde pas le centre : cos = {cos}"
    );
    // Et elle est DEHORS : cadrer à l'intérieur du build ne montre rien.
    assert!(n(v) > 32.0, "l'œil est dans le build");
}

/// `Niveau` sert des deux côtés — le résumé et le quadrillage. Un `.mca` fait
/// trente-deux chunks, et se tromper d'un facteur ferait teinter la mauvaise
/// moitié du monde.
#[test]
fn les_deux_niveaux_de_decoupe_ne_se_confondent_pas() {
    assert_eq!(Niveau::Chunk.cote(), 16);
    assert_eq!(Niveau::Region.cote(), 512);
}

/// **Le geste de sélection : gauche pose le coin 1, droit le coin 2, sur la
/// case VISÉE.** On sélectionne le bloc qu'on regarde, pas l'air devant lui —
/// c'est la convention de WorldEdit, et la confondre avec la case de POSE
/// décalerait toute sélection d'un bloc vers l'observateur.
#[test]
fn un_clic_pose_le_coin_sur_la_case_visee() {
    let mut e = etat_devant_le_mur();
    e.selection.vider();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    let vise = e.reticule.case.unwrap();
    let pose = e.reticule.pose.unwrap();
    assert_ne!(vise, pose, "sinon le test ne distingue rien");

    assert!(e.poser_coin(true));
    // **Un coin n'est pas une sélection.** WorldEdit non plus : il faut les
    // deux, et annoncer un volume sur un seul clic ferait écrire dans un
    // bloc que personne n'a désigné.
    assert_eq!(e.selection.boite(), None);

    // Le second coin ailleurs : la boîte s'étend.
    let cam = Camera {
        oeil: [0.5, 2.5, 0.5],
        cible: [1.5, 2.5, 0.5],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    e.relever_reticule(&cam, 1.0, 64.0, &mur);
    let autre = e.reticule.case.unwrap();
    assert!(e.poser_coin(false));
    let b = e.selection.boite().unwrap();
    assert_eq!(b, BBox::new(vise, autre));

    // Et l'ACCROCHAGE ne s'y applique pas : un coin qu'une inférence
    // déplacerait sélectionnerait autre chose que ce qu'on a visé, et le bord
    // d'une paroi — ce qu'on vise le plus souvent — deviendrait inattrapable.
    assert_eq!(b.max.x.max(b.min.x), 10, "le coin a été déplacé");
}

/// Un clic dans le ciel ne doit pas déplacer une sélection existante : le
/// réticule ne désigne rien, il n'y a pas de coin à poser.
#[test]
fn un_clic_dans_le_vide_ne_touche_pas_la_selection() {
    let mut e = etat_devant_le_mur();
    let avant = e.selection.boite();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &|_| false);
    assert!(!e.poser_coin(true));
    assert!(!e.poser_coin(false));
    assert_eq!(e.selection.boite(), avant);
}
