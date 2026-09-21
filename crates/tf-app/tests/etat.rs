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
use tf_world::selection::Direction;

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

// ── le POUSSER-TIRER ────────────────────────────────────────────────────────

/// Une sélection de dix blocs de côté, et l'œil à l'est qui la regarde.
fn devant_un_cube() -> (Etat, Camera) {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    // **L'accrochage est ÉTEINT ici.** Ces tests mesurent le geste nu ; le
    // mélanger à l'inférence ferait passer une faute de l'un pour un réglage
    // de l'autre. Les tests d'accrochage l'allument explicitement.
    e.tolerance = 0;
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(9, 9, 9));
    let cam = Camera {
        oeil: [30.0, 5.0, 5.0],
        cible: [29.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    (e, cam)
}

/// **Le geste complet : attraper la face est, tirer de quatre, lâcher.**
///
/// Ce que l'opération écrit est la TRANCHE, jamais la sélection entière —
/// tirer une face de trois blocs sur un bâtiment de cent mille ne doit pas
/// réécrire le bâtiment.
#[test]
fn pousser_tirer_ecrit_la_tranche_et_rien_d_autre() {
    let (mut e, cam) = devant_un_cube();
    e.bloc_tirage = "minecraft:stone".into();
    assert!(e.attraper(&cam, 1.0), "la face doit être attrapée");
    assert_eq!(e.tirage.as_ref().unwrap().face, Direction::PlusX);

    // La souris vise x = 14 depuis un autre point de vue : quatre blocs.
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [5.0, 5.0, -39.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    // Le NDC qui regarde x = 14 : on le trouve en visant, mais le plus simple
    // ici est de pointer droit devant depuis une caméra déjà orientée.
    let vue = Camera {
        cible: [14.0, 5.0, 5.0],
        ..vue
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert_eq!(e.tirage.as_ref().unwrap().blocs, 4);
    assert_eq!(e.selection.boite().unwrap().max.x, 13);

    let cmd = e.lacher().expect("un tirage non nul rend une opération");
    assert!(e.tirage.is_none());
    let tf_app::moteur::Commande::Appliquer {
        op, sel, params, ..
    } = cmd
    else {
        panic!("un tirage pose des blocs");
    };
    assert_eq!(op, "poser");
    // **La tranche, et elle seule** : x = 10..13, pas 0..13.
    assert_eq!(
        sel,
        BBox::new(BlockPos::new(10, 0, 0), BlockPos::new(13, 9, 9))
    );
    assert_eq!(
        params.get("bloc"),
        Some(&tf_ops::catalogue::Valeur::texte("minecraft:stone"))
    );
}

/// **Pousser pose de l'AIR.** C'est le modèle mental de SketchUp : la même
/// poignée ajoute et retire de la matière. Sans ça, pousser ne ferait que
/// rétrécir une boîte, ce qui n'est pas un geste de construction.
#[test]
fn pousser_retire_la_matiere() {
    let (mut e, cam) = devant_un_cube();
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [7.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert!(
        e.tirage.as_ref().unwrap().blocs < 0,
        "le geste doit pousser"
    );

    let cmd = e.lacher().unwrap();
    let tf_app::moteur::Commande::Appliquer { sel, params, .. } = cmd else {
        panic!();
    };
    assert_eq!(
        params.get("bloc"),
        Some(&tf_ops::catalogue::Valeur::texte("minecraft:air"))
    );
    // Ce qui vient de SORTIR de la sélection : la face solide était à x = 10,
    // on vise x = 7, donc trois blocs poussés — et ce qui sort est 7..9.
    assert!(e.tirage.is_none());
    assert_eq!(
        sel,
        BBox::new(BlockPos::new(7, 0, 0), BlockPos::new(9, 9, 9))
    );
}

/// **On repart de la sélection de DÉPART à chaque image.** Cumuler les
/// tirages ferait accélérer la face à mesure qu'on la tire — « la poignée
/// s'emballe », et personne ne sait d'où ça vient.
#[test]
fn un_tirage_ne_cumule_pas() {
    let (mut e, cam) = devant_un_cube();
    assert!(e.attraper(&cam, 1.0));
    let vue = |x: f32| Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [x, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    for _ in 0..5 {
        assert!(e.tirer(&vue(13.0), 1.0, [0.0, 0.0]));
    }
    assert_eq!(e.tirage.as_ref().unwrap().blocs, 3);
    assert_eq!(e.selection.boite().unwrap().max.x, 12);
}

/// Un rayon dans l'axe ne bouge rien, et surtout ne REMET PAS la face en
/// place : elle ne doit pas revenir parce qu'on a regardé dans l'axe une
/// image.
#[test]
fn regarder_dans_l_axe_ne_ramene_pas_la_face() {
    let (mut e, cam) = devant_un_cube();
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [13.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    let tenu = e.selection.boite().unwrap();

    // Puis on regarde le long de +X, depuis l'axe même.
    let dans_l_axe = Camera {
        oeil: [-40.0, 5.0, 5.0],
        cible: [-39.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(!e.tirer(&dans_l_axe, 1.0, [0.0, 0.0]));
    assert_eq!(e.selection.boite().unwrap(), tenu);
}

/// Un clic à côté ne démarre pas un geste fantôme qui déplacerait la
/// sélection au premier mouvement de souris.
#[test]
fn attraper_a_cote_ne_demarre_rien() {
    let (mut e, _) = devant_un_cube();
    let ailleurs = Camera {
        oeil: [30.0, 200.0, 5.0],
        cible: [29.0, 200.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(!e.attraper(&ailleurs, 1.0));
    assert!(e.tirage.is_none());
    assert!(e.lacher().is_none());
}

/// Abandonner remet la sélection d'avant. Un geste qu'on ne peut pas annuler
/// est un geste qu'on n'ose pas commencer.
#[test]
fn abandonner_un_tirage_remet_la_selection() {
    let (mut e, cam) = devant_un_cube();
    let avant = e.selection.boite().unwrap();
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [16.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert_ne!(e.selection.boite().unwrap(), avant);

    assert!(e.abandonner());
    assert_eq!(e.selection.boite().unwrap(), avant);
    assert!(e.tirage.is_none());
    assert!(!e.abandonner(), "rien à abandonner deux fois");
}

/// Un tirage de zéro bloc n'écrit rien : une entrée de journal pour zéro
/// changement est exactement ce que la jonction refuse déjà plus bas.
#[test]
fn un_tirage_nul_n_ecrit_rien() {
    let (mut e, cam) = devant_un_cube();
    assert!(e.attraper(&cam, 1.0));
    assert!(e.lacher().is_none());
}

/// **Le mot qui manquait au geste : « en accrochant ».**
///
/// Sans inférence, on tire au jugé et on recommence trois fois pour faire un
/// cube. Avec, la face se colle à ce qui est déjà bâti — ici le milieu de la
/// boîte de départ — et l'outil DIT à quoi elle tient.
#[test]
fn un_tirage_s_accroche_a_ce_qui_est_bati_et_le_dit() {
    let (mut e, cam) = devant_un_cube();
    e.tolerance = 2;
    assert!(e.attraper(&cam, 1.0));

    // La boîte va de 0 à 9, sa face solide est à x = 10, son milieu à x = 5.
    // Viser x = 7 pousse de trois : la face arrive à 6, donc à UN bloc du
    // milieu — dans la tolérance.
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [7.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert_eq!(
        e.selection.boite().unwrap().max.x,
        5,
        "la face devait s'accrocher au milieu"
    );
    let r = e
        .tirage
        .as_ref()
        .unwrap()
        .raison
        .expect("l'accroche se DIT");
    assert_eq!(r.reference.x, 5);
    assert_eq!(r.ecart, -1);
}

/// **L'accrochage ne déplace QUE l'axe de la face.** Les deux autres sont
/// verrouillés : laisser une face glisser de côté pendant qu'on la tire
/// déformerait la sélection sans que rien ne le dise.
#[test]
fn un_tirage_ne_deplace_que_son_axe() {
    let (mut e, cam) = devant_un_cube();
    e.tolerance = 2;
    let avant = e.selection.boite().unwrap();
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [14.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    let apres = e.selection.boite().unwrap();
    assert_eq!((apres.min.y, apres.max.y), (avant.min.y, avant.max.y));
    assert_eq!((apres.min.z, apres.max.z), (avant.min.z, avant.max.z));
    assert_eq!(apres.min.x, avant.min.x, "la face opposée n'a pas bougé");
}

/// **La position de DÉPART de la face n'est pas une accroche.**
///
/// Elle est toujours dans la tolérance au premier bloc tiré : sans filtre, la
/// face reviendrait se coller là d'où elle part, et un petit déplacement
/// deviendrait impossible. Ce n'est pas un alignement, c'est un non-mouvement
/// — l'accroche est juste, le geste est juste, c'est leur COMPOSITION qui ne
/// l'est pas.
#[test]
fn une_face_ne_s_accroche_pas_a_son_propre_point_de_depart() {
    let (mut e, cam) = devant_un_cube();
    e.tolerance = 2;
    assert!(e.attraper(&cam, 1.0));
    // Un seul bloc tiré : le départ (x = 9) est à un bloc, donc dans la
    // tolérance.
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [11.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert_eq!(
        e.tirage.as_ref().unwrap().blocs,
        1,
        "la face est revenue à son point de départ"
    );
    assert_eq!(e.selection.boite().unwrap().max.x, 10);
}

/// Tolérance nulle : le geste est nu, et aucune raison n'est annoncée.
#[test]
fn une_tolerance_nulle_eteint_l_accrochage_du_tirage() {
    let (mut e, cam) = devant_un_cube();
    e.tolerance = 0;
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [7.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    // Sans accrochage la face reste où le geste l'a mise — à 6, pas au
    // milieu de la boîte.
    assert_eq!(e.selection.boite().unwrap().max.x, 6);
    assert!(e.tirage.as_ref().unwrap().raison.is_none());
}

/// **Tolérance nulle veut dire AUCUNE inférence, pas même une coïncidence.**
///
/// Sans la garde, un tirage qui tombe pile sur une référence serait annoncé
/// comme une accroche — l'outil dirait avoir décidé là où il n'a rien décidé,
/// et l'utilisateur chercherait un réglage qui n'existe pas. Mesuré par
/// mutation : retirer la garde ne rougissait nulle part tant que le test ne
/// visait pas une coïncidence exacte.
#[test]
fn une_tolerance_nulle_ne_rapporte_meme_pas_une_coincidence() {
    let (mut e, cam) = devant_un_cube();
    e.tolerance = 0;
    assert!(e.attraper(&cam, 1.0));
    // Viser x = 6 pousse de quatre : la face arrive PILE sur le milieu (5).
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [6.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    assert_eq!(e.selection.boite().unwrap().max.x, 5, "le geste va bien là");
    assert!(
        e.tirage.as_ref().unwrap().raison.is_none(),
        "une coïncidence n'est pas une accroche"
    );
}

/// **La face NÉGATIVE tire dans l'autre sens.** Tirer la face ouest vers
/// l'ouest fait DIMINUER la coordonnée : convertir l'accroche en blocs sans
/// retourner le signe enverrait la face du mauvais côté. Mesuré par mutation —
/// tous mes tests tiraient la face est, et le défaut passait.
#[test]
fn tirer_une_face_negative_va_dans_le_bon_sens() {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    e.tolerance = 2;
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(9, 9, 9));
    // L'œil à l'ouest, regardant vers +X : on attrape la face OUEST.
    let cam = Camera {
        oeil: [-30.0, 5.0, 5.0],
        cible: [-29.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.attraper(&cam, 1.0));
    assert_eq!(e.tirage.as_ref().unwrap().face, Direction::MoinsX);

    // Viser x = −4 tire la face vers l'ouest : la boîte s'AGRANDIT.
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [-4.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    let b = e.selection.boite().unwrap();
    assert_eq!(b.min.x, -4, "la face ouest est partie du mauvais côté");
    assert_eq!(b.max.x, 9, "la face opposée n'a pas bougé");
    assert!(e.tirage.as_ref().unwrap().blocs > 0, "c'est un TIRAGE");

    // Et la tranche est bien celle qui s'ajoute à l'ouest.
    let cmd = e.lacher().unwrap();
    let tf_app::moteur::Commande::Appliquer { sel, .. } = cmd else {
        panic!()
    };
    assert_eq!(
        sel,
        BBox::new(BlockPos::new(-4, 0, 0), BlockPos::new(-1, 9, 9))
    );
}

// ── ce qu'un « Appliquer » envoie vraiment ──────────────────────────────────

fn avec_selection() -> Etat {
    let mut e = Etat::cadre([0.0; 3], [64.0; 3], 1.0);
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(31, 31, 31));
    e
}

/// **La FORME choisie doit arriver au moteur.** Elle était câblée à
/// `Forme::Boite` dans le bouton : sphère, cylindre, pyramide, murs et faces
/// étaient écrits, testés, offerts par la ligne de commande — et
/// inatteignables depuis l'interface. « Déclaré, branché, testé, et personne
/// ne le propose », dans sa version la plus coûteuse : cinq formes perdues.
#[test]
fn la_commande_porte_la_forme_choisie() {
    let mut e = avec_selection();
    assert!(e.atelier.choisir("poser"));

    // Sans forme : toute la sélection.
    let tf_app::moteur::Commande::Appliquer { forme, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    assert!(matches!(forme, tf_ops::Forme::Boite));

    // Avec une sphère : la forme est bornée, et PLUS PETITE que la sélection.
    e.volume = tf_ops::Volume::Sphere { rayon: 5.0 };
    let tf_app::moteur::Commande::Appliquer { forme, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    let b = forme.bornes().expect("une sphère est bornée");
    assert_eq!(b.size(), (11, 11, 11));

    // Et « creuse » en fait une coque.
    e.creux = Some(2.0);
    let tf_app::moteur::Commande::Appliquer { forme, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    assert!(matches!(forme, tf_ops::Forme::Coque { .. }));
}

/// **Une opération qui n'accepte pas de forme n'en reçoit pas.** Le catalogue
/// le dit (`cout.forme`) ; lui en passer une quand même ferait `//move`
/// déplacer une sphère de son contenu, et le réglage resterait à l'écran en
/// laissant croire qu'il fait quelque chose.
#[test]
fn une_operation_sans_forme_n_en_recoit_pas() {
    let mut e = avec_selection();
    e.volume = tf_ops::Volume::Sphere { rayon: 5.0 };

    assert!(e.atelier.choisir("deplacer"));
    assert!(!e.atelier.descripteur().cout.forme);
    let tf_app::moteur::Commande::Appliquer { forme, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    assert!(
        matches!(forme, tf_ops::Forme::Boite),
        "« déplacer » a reçu une forme"
    );

    // Alors que « remplir », qui en accepte une, la reçoit.
    assert!(e.atelier.choisir("poser"));
    assert!(e.atelier.descripteur().cout.forme);
    let tf_app::moteur::Commande::Appliquer { forme, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    assert!(!matches!(forme, tf_ops::Forme::Boite));
}

/// **Le comptage est un CHOIX, la graine un réglage.** Les deux étaient câblés
/// en dur dans le bouton : `compter: true` viole un invariant écrit noir sur
/// blanc (× 31 à l'étage palette, jamais rendu d'office), et `seed: 0` rendait
/// tous les mélanges identiques d'un projet à l'autre.
#[test]
fn la_commande_porte_le_comptage_et_la_graine() {
    let mut e = avec_selection();
    e.compter = false;
    e.seed = 4242;
    let tf_app::moteur::Commande::Appliquer { compter, seed, .. } = e.demande_operation().unwrap()
    else {
        panic!()
    };
    assert!(!compter);
    assert_eq!(seed, 4242);

    e.compter = true;
    let tf_app::moteur::Commande::Appliquer { compter, .. } = e.demande_operation().unwrap() else {
        panic!()
    };
    assert!(compter);
}

/// Sans sélection, il n'y a rien à envoyer — et surtout pas une commande sur
/// une boîte inventée.
#[test]
fn sans_selection_il_n_y_a_pas_de_commande() {
    let e = Etat::cadre([0.0; 3], [64.0; 3], 1.0);
    assert!(e.selection.boite().is_none());
    assert!(e.demande_operation().is_none());
}

/// Le pousser-tirer porte les mêmes réglages, SAUF la forme : on tire une
/// face, donc on remplit une dalle. Une sphère appliquée à une tranche n'a
/// aucun sens.
#[test]
fn le_tirage_porte_les_reglages_mais_pas_la_forme() {
    let (mut e, cam) = devant_un_cube();
    e.compter = false;
    e.seed = 7;
    e.volume = tf_ops::Volume::Sphere { rayon: 5.0 };
    assert!(e.attraper(&cam, 1.0));
    let vue = Camera {
        oeil: [5.0, 5.0, -40.0],
        cible: [14.0, 5.0, 5.0],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    assert!(e.tirer(&vue, 1.0, [0.0, 0.0]));
    let tf_app::moteur::Commande::Appliquer {
        forme,
        compter,
        seed,
        ..
    } = e.lacher().unwrap()
    else {
        panic!()
    };
    assert!(
        matches!(forme, tf_ops::Forme::Boite),
        "une tranche est une dalle"
    );
    assert!(!compter);
    assert_eq!(seed, 7);
}

// ── les outils de Conception ────────────────────────────────────────────────

/// **Le geste qui rend l'inférence ATTEIGNABLE.**
///
/// Elle était écrite, testée, affichée dans le panneau — et aucun outil ne
/// s'en servait. « Déclaré, branché, testé, et inatteignable » dans sa forme
/// la plus discrète : la pièce marche, personne ne l'appelle.
#[test]
fn poser_un_bloc_passe_par_l_accrochage() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    let accroche = e.reticule.accroche.unwrap().position;
    assert_ne!(
        Some(accroche),
        e.reticule.pose,
        "sinon le test ne distingue rien"
    );

    e.bloc_tirage = "minecraft:glowstone".into();
    let tf_app::moteur::Commande::Appliquer {
        op, sel, params, ..
    } = e.poser_un_bloc().expect("le réticule désigne une case")
    else {
        panic!()
    };
    assert_eq!(op, "poser");
    // **Une case, et une seule** : l'invariant « une opération ne paie que sa
    // portée » à sa plus petite échelle.
    assert_eq!(sel, BBox::single(accroche));
    assert_eq!(
        params.get("bloc"),
        Some(&tf_ops::catalogue::Valeur::texte("minecraft:glowstone"))
    );
}

/// **Poser et casser ne visent pas la même case.** Un rayon touche une FACE,
/// donc un plan ENTRE deux cases : c'est le piège d'`ExeWorldEdit` que `viser`
/// existe pour fermer, et il se refermerait si l'un des deux gestes prenait la
/// case de l'autre.
#[test]
fn casser_vise_la_case_arretee_et_poser_celle_d_avant() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &mur);
    let visee = e.reticule.case.unwrap();

    let tf_app::moteur::Commande::Appliquer { sel, params, .. } = e.casser_un_bloc().unwrap()
    else {
        panic!()
    };
    assert_eq!(sel, BBox::single(visee), "casser doit viser le bloc touché");
    assert_eq!(
        params.get("bloc"),
        Some(&tf_ops::catalogue::Valeur::texte("minecraft:air"))
    );

    // Et la pose est AILLEURS — devant le mur, pas dedans.
    let tf_app::moteur::Commande::Appliquer { sel: pose, .. } = e.poser_un_bloc().unwrap() else {
        panic!()
    };
    assert_ne!(pose, sel, "poser dans le mur qu'on casse");
}

/// Rien sous le réticule : aucun geste. Un clic dans le ciel ne doit pas
/// poser un bloc à une coordonnée inventée.
#[test]
fn sans_reticule_il_n_y_a_ni_pose_ni_cassure() {
    let mut e = etat_devant_le_mur();
    e.relever_reticule(&camera_face_au_mur(), 1.0, 64.0, &|_| false);
    assert!(e.poser_un_bloc().is_none());
    assert!(e.casser_un_bloc().is_none());
}

/// La légende de la barre vient de l'OUTIL. Deux constantes indépendantes
/// finissent par diverger, et une barre qui annonce le mauvais bouton est pire
/// qu'une barre muette.
#[test]
fn chaque_outil_dit_ce_que_font_les_boutons() {
    use tf_app::etat::Outil;
    let mut vus = std::collections::BTreeSet::new();
    for o in Outil::TOUS {
        assert!(!o.nom().is_empty());
        let l = o.legende();
        assert!(l.contains("gauche") && l.contains("droit"), "{l}");
        assert!(vus.insert(l), "deux outils annoncent la même chose : {l}");
    }
    assert_eq!(vus.len(), Outil::TOUS.len());
}
