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

/// Le curseur au MILIEU de l'écran. La plupart des essais vérifient ce qui
/// est visé, pas d'où l'on vise : ils gardent le milieu, et ceux qui portent
/// sur le curseur le déplacent.
const CENTRE: [f32; 2] = [0.0, 0.0];

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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);

    let r = e.vise;
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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let a = e.vise.accroche.unwrap();

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

/// Une visée périmée fait poser un bloc là où l'utilisateur ne montre plus.
#[test]
fn un_rayon_qui_ne_touche_rien_efface_la_visee() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    assert!(e.vise.case.is_some());

    // Le mur a disparu — ou l'on s'est tourné vers le ciel.
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &|_| false);
    assert_eq!(e.vise.case, None);
    assert_eq!(e.vise.pose, None);
    assert!(e.vise.accroche.is_none());
    assert_eq!(e.point_de_pose(), None);
}

/// La portée borne le rayon : le mur est à dix blocs, on n'en regarde que
/// cinq.
#[test]
fn au_dela_de_la_portee_on_ne_vise_rien() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 5.0, &mur);
    assert_eq!(e.vise.case, None);
}

#[test]
fn le_point_de_pose_prefere_l_accroche() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let a = e.vise.accroche.unwrap();
    assert_eq!(e.point_de_pose(), Some(a.position));
    assert_ne!(e.point_de_pose(), e.vise.pose, "sinon on ne teste rien");
}

/// Zéro éteint l'accrochage — et rend la pose BRUTE, pas une pose figée par
/// le dernier verrou.
#[test]
fn une_tolerance_nulle_eteint_l_accrochage() {
    let mut e = etat_devant_le_mur();
    e.tolerance = 0;
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    assert!(e.vise.accroche.is_none());
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

/// Le verrou que `relever_vise` pose est bien celui de la face TRAVERSÉE,
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
    e.relever_vise(&cam, 1.0, Some(CENTRE), 64.0, &plafond);

    assert_eq!(e.vise.pose, Some(BlockPos::new(7, 9, 0)));
    let a = e.vise.accroche.unwrap();
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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let vise = e.vise.case.unwrap();
    let pose = e.vise.pose.unwrap();
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
    e.relever_vise(&cam, 1.0, Some(CENTRE), 64.0, &mur);
    let autre = e.vise.case.unwrap();
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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &|_| false);
    assert!(!e.poser_coin(true));
    assert!(!e.poser_coin(false));
    assert_eq!(e.selection.boite(), avant);
}

// ── le POUSSER-TIRER ────────────────────────────────────────────────────────

// ── viser SOUS LE CURSEUR ───────────────────────────────────────────────────

/// **On vise sous la souris, pas au centre de l'écran.** Le premier retour de
/// l'essai sous Windows : la sélection se posait au milieu de la vue, là où
/// était le réticule, et non là où l'on cliquait.
///
/// Devant le mur plein, le centre désigne (10, 7, 0). Le curseur en haut à
/// droite de l'écran — le Sud, puisqu'on regarde l'Est — doit désigner une
/// case plus HAUTE et plus au SUD, sur la même face ouest du mur ; et le coin
/// qu'un clic pose est celle-là.
#[test]
fn on_vise_sous_le_curseur_pas_au_centre() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    assert_eq!(e.vise.case, Some(BlockPos::new(10, 7, 0)));

    e.relever_vise(&camera_face_au_mur(), 1.0, Some([0.5, 0.5]), 64.0, &mur);
    let c = e.vise.case.expect("le mur est partout à l'est");
    assert_eq!(c.x, 10, "toujours la face ouest du mur");
    assert!(c.y > 7 && c.z > 0, "plus haut et plus au sud : {c:?}");
    assert_eq!(c, BlockPos::new(10, 10, 3));
    assert_eq!(e.vise.pose, Some(BlockPos::new(9, 10, 3)));

    assert!(e.poser_coin(true));
    assert_eq!(
        e.selection.coin1,
        Some(c),
        "le coin est posé SOUS LA SOURIS"
    );
}

/// **Hors de la scène, la visée se FIGE.** On quitte la scène pour lire
/// l'inspecteur ou taper dans le sélecteur de blocs, qui propose le bloc visé
/// en tête : l'effacer à ce moment effaçait ce qu'on venait lire. Même si la
/// caméra bouge entre-temps — le monde, lui, n'est pas relu.
#[test]
fn hors_de_la_scene_la_visee_se_fige() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    e.nommer_vise(|_| "minecraft:stone");
    let avant = e.vise;
    assert!(avant.case.is_some());

    let ailleurs = Camera {
        oeil: [0.5, 70.5, 0.5],
        cible: [0.5, 80.5, 0.5],
        ..camera_face_au_mur()
    };
    e.relever_vise(&ailleurs, 1.0, None, 64.0, &|_| {
        panic!("hors de la scène, on ne lance aucun rayon")
    });
    assert_eq!(e.vise, avant);
    assert_eq!(e.bloc_vise.as_deref(), Some("minecraft:stone"));
}

/// **Attraper et tirer partent du MÊME rayon.** Attraper se faisait au centre
/// de l'écran, tirer sous la souris : la face sautait de tout l'écart entre
/// les deux au premier mouvement. Attrapée sous un curseur éloigné du centre,
/// elle ne doit pas bouger tant que la souris ne bouge pas.
#[test]
fn une_face_attrapee_sous_le_curseur_ne_saute_pas() {
    let (mut e, cam) = devant_un_cube();
    let depart = e.selection;
    let curseur = [0.15, -0.12];
    assert!(
        e.attraper(&cam, 1.0, curseur),
        "la face est sous ce curseur"
    );
    assert_eq!(e.tirage.as_ref().unwrap().face, Direction::PlusX);
    e.tirer(&cam, 1.0, curseur);
    assert_eq!(
        e.selection, depart,
        "la souris n'a pas bougé : la face non plus"
    );
    assert_eq!(e.tirage.as_ref().unwrap().blocs, 0);

    // Et un curseur à côté du cube n'attrape rien, même si le CENTRE, lui,
    // tombe sur une face.
    let (mut e, cam) = devant_un_cube();
    assert!(!e.attraper(&cam, 1.0, [0.9, 0.9]));
    assert!(e.tirage.is_none());
}

/// **Le bouton « pipette » ARME la pipette**, le clic suivant sur la scène la
/// prend. Pour atteindre le bouton, la souris a quitté ce qu'elle désignait :
/// prendre le bloc à ce moment-là prendrait celui du bord de la scène.
#[test]
fn le_bouton_arme_la_pipette_et_le_clic_suivant_la_prend() {
    let mut e = etat_devant_le_mur();
    assert!(
        !e.clic_de_pipette(false),
        "sans Alt ni bouton : le clic est à l'outil"
    );

    e.armer_pipette(true);
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    e.nommer_vise(|_| "minecraft:oak_log|axis=x");
    assert!(e.clic_de_pipette(false), "armée : le clic est à la pipette");
    assert_eq!(e.bloc_tirage, "minecraft:oak_log[axis=x]");
    assert!(!e.pipette_armee, "une pipette sert une fois");
    assert!(
        !e.clic_de_pipette(false),
        "le clic d'après est de nouveau à l'outil"
    );

    // Alt + clic prend directement, sans l'armer.
    e.nommer_vise(|_| "minecraft:stone");
    assert!(e.clic_de_pipette(true));
    assert_eq!(e.bloc_tirage, "minecraft:stone");
}

/// Une pipette armée qui ne trouve rien sous le curseur RESTE armée, et le
/// clic ne va pas à l'outil : cliquer dans le ciel ne doit ni prendre de
/// l'air, ni poser un coin à la place.
#[test]
fn une_pipette_armee_dans_le_vide_reste_armee() {
    let mut e = etat_devant_le_mur();
    e.bloc_tirage = "minecraft:stone".into();
    e.armer_pipette(true);
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &|_| false);
    e.nommer_vise(|_| unreachable!());
    assert!(e.clic_de_pipette(false));
    assert!(e.pipette_armee);
    assert_eq!(e.bloc_tirage, "minecraft:stone");
}

/// Échap range une pipette armée AVANT de faire quoi que ce soit d'autre :
/// sinon, sans tirage en cours, il quitte l'application.
#[test]
fn echap_range_la_pipette_avant_de_quitter() {
    let mut e = etat_devant_le_mur();
    e.armer_pipette(true);
    assert!(e.abandonner(), "Échap a trouvé quelque chose à abandonner");
    assert!(!e.pipette_armee);
    assert!(!e.abandonner(), "plus rien : l'Échap suivant quitterait");
}

/// Le contour du bloc visé : rien sans visée, les douze arêtes d'un cube
/// sinon — un cheveu PLUS grand que le bloc, pour ne pas se confondre avec
/// l'arête d'une sélection qui s'arrête là.
#[test]
fn le_bloc_vise_a_son_contour() {
    assert_eq!(tf_app::scene::contour_vise(None).len(), 0);
    let l = tf_app::scene::contour_vise(Some(BlockPos::new(4, -2, 7)));
    assert_eq!(l.len(), 12);
    for v in &l.sommets {
        for (k, (a, b)) in [(4.0, 5.0), (-2.0, -1.0), (7.0, 8.0)].iter().enumerate() {
            let p = v.position[k];
            assert!(
                (p - a).abs() < 0.1 || (p - b).abs() < 0.1,
                "sur une arête du bloc : {p}"
            );
            assert!(p < *a || p > *b, "à l'extérieur, d'un cheveu : {p}");
        }
    }
}

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
    assert!(e.attraper(&cam, 1.0, CENTRE), "la face doit être attrapée");
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(!e.attraper(&ailleurs, 1.0, CENTRE));
    assert!(e.tirage.is_none());
    assert!(e.lacher().is_none());
}

/// Abandonner remet la sélection d'avant. Un geste qu'on ne peut pas annuler
/// est un geste qu'on n'ose pas commencer.
#[test]
fn abandonner_un_tirage_remet_la_selection() {
    let (mut e, cam) = devant_un_cube();
    let avant = e.selection.boite().unwrap();
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));

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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    assert!(e.attraper(&cam, 1.0, CENTRE));
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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let accroche = e.vise.accroche.unwrap().position;
    assert_ne!(
        Some(accroche),
        e.vise.pose,
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
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let visee = e.vise.case.unwrap();

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

/// Rien sous le curseur : aucun geste. Un clic dans le ciel ne doit pas
/// poser un bloc à une coordonnée inventée.
#[test]
fn sans_visee_il_n_y_a_ni_pose_ni_cassure() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &|_| false);
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

/// **La pipette** : on regarde un bloc, il devient le bloc EN MAIN — sous
/// l'état exact que la scène tient, pas son seul nom. Et ce que « Poser »
/// envoie ensuite arrive au moteur sous la clé que le décodeur rend pour ce
/// même bloc : deux clés pour un état dédoubleraient la palette.
#[test]
fn la_pipette_prend_le_bloc_vise_sous_son_etat_exact() {
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let visee = e.vise.case.unwrap();
    let cle = "minecraft:oak_stairs|facing=east,half=top";
    let mut demandee = None;
    e.nommer_vise(|c| {
        demandee = Some(c);
        cle
    });
    assert_eq!(demandee, Some(visee), "la case qui ARRÊTE le rayon");
    assert_eq!(e.bloc_vise.as_deref(), Some(cle));

    assert!(e.pipette());
    assert_eq!(e.bloc_tirage, "minecraft:oak_stairs[facing=east,half=top]");
    assert_eq!(
        e.nuancier.recents(),
        [cle],
        "le bloc en main est le plus récent"
    );

    let tf_app::moteur::Commande::Appliquer { op, params, .. } = e.poser_un_bloc().unwrap() else {
        panic!()
    };
    let d = tf_ops::catalogue::descripteur(op).unwrap();
    let n = tf_ops::catalogue::normaliser(d, &params).unwrap();
    assert_eq!(n.get("bloc"), Some(&tf_ops::catalogue::Valeur::texte(cle)));

    // Rien sous le curseur : la pipette ne prend rien, et le bloc en main
    // RESTE — un clic dans le ciel ne doit pas vider la main.
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &|_| false);
    e.nommer_vise(|_| panic!("rien n'est visé : on ne demande rien à la scène"));
    assert_eq!(e.bloc_vise, None);
    assert!(!e.pipette());
    assert_eq!(e.bloc_tirage, "minecraft:oak_stairs[facing=east,half=top]");
}

// ── l'outil Composant ───────────────────────────────────────────────────────

/// Un document d'un composant 3 × 1 × 1 et d'une instance posée sur la case
/// visée par la caméra devant le mur.
fn document(case: BlockPos) -> tf_app::moteur::Composants {
    use tf_ops::composant::{Contenu, Definition, Instance, Projet};
    let p = Projet {
        prochain: 3,
        definitions: vec![Definition {
            id: 1,
            nom: "banc".into(),
            contenu: Contenu {
                taille: [3, 1, 1],
                palette: vec!["minecraft:oak_planks".into()],
                cases: vec![0; 3],
                entites: Vec::new(),
            },
        }],
        instances: vec![Instance {
            id: 2,
            definition: 1,
            dim: tf_world::source::Dimension::Overworld,
            coin: BlockPos::new(case.x - 1, case.y, case.z),
            transfo: None,
        }],
    };
    tf_app::moteur::Composants {
        projet: std::sync::Arc::new(p),
        erreur: None,
        version: 1,
    }
}

/// **L'outil Composant** pose la définition CHOISIE au point de pose, dans
/// l'orientation choisie ; le clic droit la tourne d'un quart de tour, et
/// quatre quarts reviennent au départ. Sans définition choisie, rien ne part
/// — et on le dit.
#[test]
fn l_outil_composant_pose_le_choisi_et_le_droit_le_tourne() {
    use tf_app::moteur::{ActionComposant, Commande};
    use tf_blocks::Transfo;
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    assert!(e.poser_un_composant().is_none());
    assert!(e.message.contains("aucun composant"), "{}", e.message);

    e.composant_choisi = Some(1);
    let coin = e.point_de_pose().unwrap();
    let pose = |e: &mut tf_app::etat::Etat| match e.poser_un_composant() {
        Some(Commande::Composant(ActionComposant::Poser {
            definition,
            coin: c,
            transfo,
        })) => {
            assert_eq!((definition, c), (1, coin));
            transfo
        }
        autre => panic!("{autre:?}"),
    };
    assert_eq!(pose(&mut e), None);
    for attendu in [
        Some(Transfo::Rot90),
        Some(Transfo::Rot180),
        Some(Transfo::Rot270),
        None,
    ] {
        e.tourner_le_composant();
        assert_eq!(pose(&mut e), attendu);
    }
    // Un miroir choisi dans l'inspecteur : le clic droit repart de « tel
    // quel », il ne compose pas au jugé.
    e.orientation = Some(Transfo::MiroirX);
    e.tourner_le_composant();
    assert_eq!(e.orientation, None);
}

/// **L'instance sous le réticule vient du document PUBLIÉ** — et les actions
/// qui la visent portent son identifiant. Une définition choisie qui
/// disparaît du document est oubliée, et changer de monde oublie tout : des
/// identifiants d'un document ne désignent rien dans un autre.
#[test]
fn l_instance_visee_vient_du_document_publie() {
    use tf_app::moteur::{ActionComposant, Commande};
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let case = e.vise.case.unwrap();
    assert!(e.instance_visee().is_none());
    e.suivre_composants(document(case));
    assert_eq!(e.instance_visee().map(|i| i.id), Some(2));
    assert!(matches!(
        e.demande_mettre_a_jour(),
        Some(Commande::Composant(ActionComposant::MettreAJour {
            instance: 2
        }))
    ));
    assert!(matches!(
        e.demande_detacher(),
        Some(Commande::Composant(ActionComposant::Detacher {
            instance: 2
        }))
    ));

    // Sans nom tapé, un composant s'appelle « composant » — jamais rien.
    assert!(matches!(
        e.demande_creer_composant(),
        Some(Commande::Composant(ActionComposant::Creer { ref nom, .. })) if nom == "composant"
    ));
    // Créer et renommer prennent le nom tapé, sans ses espaces.
    e.nom_composant = "  table ".into();
    assert!(matches!(
        e.demande_creer_composant(),
        Some(Commande::Composant(ActionComposant::Creer { ref nom, .. })) if nom == "table"
    ));
    assert!(e.demande_renommer().is_none(), "aucun composant choisi");
    e.composant_choisi = Some(1);
    let tape = std::mem::replace(&mut e.nom_composant, "   ".into());
    assert!(e.demande_renommer().is_none(), "un nom vide ne renomme pas");
    e.nom_composant = tape;
    assert!(matches!(
        e.demande_renommer(),
        Some(Commande::Composant(ActionComposant::Renommer { definition: 1, ref nom })) if nom == "table"
    ));

    // La même version n'est pas recopiée ; une version neuve sans la
    // définition choisie la fait oublier.
    let mut vide = tf_app::moteur::Composants {
        version: 1,
        ..Default::default()
    };
    e.suivre_composants(vide.clone());
    assert_eq!(e.composant_choisi, Some(1), "même version : rien ne change");
    vide.version = 2;
    e.suivre_composants(vide);
    assert_eq!(e.composant_choisi, None);
    assert!(e.instance_visee().is_none());

    e.suivre_composants(document(case));
    e.composant_choisi = Some(1);
    e.recadrer([0.0; 3], [32.0; 3], 1.0);
    assert_eq!(e.composant_choisi, None);
    assert!(e.composants.projet.instances.is_empty());
}

// ── les échanges ────────────────────────────────────────────────────────────

/// **Chaque format se range où son outil le cherche** : Litematica à la
/// racine de l'installation, WorldEdit en solo sous `config/`, un bloc de
/// structure DANS le monde.
#[test]
fn chaque_format_se_range_ou_son_outil_le_cherche() {
    use std::path::Path;
    use tf_app::etat::dossier_par_defaut;
    use tf_formats::Format;
    let i = Path::new("/jeu/.minefield_1_18");
    let m = Path::new("/jeu/.minefield_1_18/saves/Ville");
    assert_eq!(
        dossier_par_defaut(Format::Litematic, Some(i), Some(m)),
        Some(i.join("schematics"))
    );
    for f in [Format::SpongeV2, Format::SpongeV3] {
        assert_eq!(
            dossier_par_defaut(f, Some(i), Some(m)),
            Some(i.join("config").join("worldedit").join("schematics"))
        );
    }
    assert_eq!(
        dossier_par_defaut(Format::Structure, Some(i), Some(m)),
        Some(m.join("generated").join("minecraft").join("structures"))
    );
    // Un monde hors de toute installation : pas de dossier Litematica à
    // deviner, mais le sien pour les structures.
    assert_eq!(dossier_par_defaut(Format::Litematic, None, Some(m)), None);
    assert!(dossier_par_defaut(Format::Structure, None, Some(m)).is_some());
}

/// **Un nom devient un nom de fichier sans rien perdre** de ce qu'un système
/// de fichiers accepte — les accents, le hangeul — et sans rien garder de ce
/// qu'il refuse.
#[test]
fn un_nom_devient_un_nom_de_fichier() {
    use tf_app::etat::nom_de_fichier;
    for (nom, attendu) in [
        ("Vallée", "Vallée"),
        ("한국어 건물", "한국어 건물"),
        ("porte/nord", "porte_nord"),
        ("a:b*c?d\"e<f>g|h\\i", "a_b_c_d_e_f_g_h_i"),
        ("  tour  ", "tour"),
        ("tour.", "tour"),
        ("CON", "_CON"),
        ("lpt1.txt", "_lpt1.txt"),
        ("console", "console"),
        // Quatre lettres commençant comme un port, sans chiffre : un nom.
        ("COMX", "COMX"),
        ("com9", "_com9"),
        // Deux chiffres : un nom comme un autre pour Windows.
        ("COM10", "COM10"),
        // Les exposants sont des ports aussi, et Windows juge le nom avant
        // le premier point, espaces de fin retirées.
        ("COM¹", "_COM¹"),
        ("lpt³.schem", "_lpt³.schem"),
        ("con .txt", "_con .txt"),
        ("CONIN$", "_CONIN$"),
        ("", "export"),
        ("...", "export"),
    ] {
        assert_eq!(nom_de_fichier(nom), attendu, "« {nom} »");
    }
}

/// Les fichiers d'échange d'un dossier : les extensions des formats, plus le
/// `.schematic` d'avant 1.13 — que la lecture refusera en le nommant —, du
/// plus récent au plus ancien ; et pas un dossier qui en porte le nom.
#[test]
fn les_fichiers_d_echange_se_listent_du_plus_recent() {
    use tf_app::etat::fichiers_d_echange;
    let d = std::env::temp_dir().join(format!("titiforge-trouves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("faux.litematic")).unwrap();
    let ecrire = |nom: &str, age: u64| {
        let f = d.join(nom);
        std::fs::write(&f, b"x").unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(t)
            .unwrap();
    };
    ecrire("vieux.schem", 3000);
    ecrire("neuf.litematic", 10);
    ecrire("moyen.NBT", 500);
    ecrire("mcedit.schematic", 2000);
    ecrire("notes.txt", 1);
    ecrire("sans-extension", 1);
    let noms: Vec<String> = fichiers_d_echange(&[d.clone(), d.join("absent")])
        .into_iter()
        .map(|t| t.nom)
        .collect();
    assert_eq!(
        noms,
        [
            "neuf.litematic",
            "moyen.NBT",
            "mcedit.schematic",
            "vieux.schem"
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **La liste d'import montre aussi le dossier d'export TAPÉ** — sinon ce
/// qu'on vient d'y exporter n'apparaîtrait pas à côté — et un dossier qui est
/// aussi un dossier par défaut n'y compte qu'une fois.
#[test]
fn la_liste_d_import_montre_aussi_le_dossier_tape() {
    let d = std::env::temp_dir().join(format!("titiforge-tape-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let inst = d.join("inst");
    let we = inst.join("config").join("worldedit").join("schematics");
    let ailleurs = d.join("ailleurs");
    std::fs::create_dir_all(&we).unwrap();
    std::fs::create_dir_all(&ailleurs).unwrap();
    std::fs::write(we.join("porte.schem"), b"x").unwrap();
    std::fs::write(ailleurs.join("maison.litematic"), b"x").unwrap();
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    e.situer_echanges(Some(inst.clone()), None, None, None);
    e.choisir_format(tf_formats::Format::SpongeV2);
    e.chercher_fichiers();
    let noms = |e: &Etat| -> Vec<String> {
        let mut v: Vec<String> = e.echanges.trouves.iter().map(|t| t.nom.clone()).collect();
        v.sort();
        v
    };
    assert_eq!(noms(&e), ["porte.schem"], "le défaut, une seule fois");
    e.echanges.dossier = format!("  {}  ", ailleurs.display());
    e.chercher_fichiers();
    assert_eq!(noms(&e), ["maison.litematic", "porte.schem"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// **Exporter demande une sélection et un dossier**, et emporte la version du
/// MONDE — sinon celle du serveur que ce projet sert, 1.18.2.
#[test]
fn exporter_demande_une_selection_et_un_dossier() {
    use tf_app::moteur::Commande;
    use tf_formats::Format;
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    let i = std::path::PathBuf::from("/jeu");
    e.situer_echanges(
        Some(i.clone()),
        Some(i.join("saves").join("Ville")),
        None,
        Some("Ville / nord"),
    );
    assert_eq!(e.echanges.nom, "Ville _ nord");
    assert_eq!(
        e.echanges.dossier,
        i.join("schematics").display().to_string()
    );
    assert!(e.demande_exporter(7).is_none(), "sans sélection");
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(3, 2, 1));
    match e.demande_exporter(7) {
        Some(Commande::Exporter {
            sel,
            format,
            chemin,
            meta,
        }) => {
            assert_eq!(sel, e.selection.boite().unwrap());
            assert_eq!(format, Format::Litematic);
            assert_eq!(chemin, i.join("schematics").join("Ville _ nord.litematic"));
            assert_eq!(meta.data_version, 2975);
            assert_eq!(meta.date_ms, 7);
        }
        autre => panic!("{autre:?}"),
    }
    // Un autre format change le dossier ; la version du monde voyage.
    e.echanges.version_monde = Some(3465);
    e.choisir_format(Format::Structure);
    match e.demande_exporter(0) {
        Some(Commande::Exporter { chemin, meta, .. }) => {
            assert!(chemin.ends_with("generated/minecraft/structures/Ville _ nord.nbt"));
            assert_eq!(meta.data_version, 3465);
        }
        autre => panic!("{autre:?}"),
    }
    e.echanges.dossier = "  ".into();
    assert!(e.demande_exporter(0).is_none(), "sans dossier");
}

/// **Coller** pose le presse-papiers au point de pose, dans l'orientation
/// choisie — le contour d'arrivée montre la MÊME boîte, tournée — et un
/// presse-papiers vide se dit au lieu de ne rien faire.
#[test]
fn coller_pose_le_presse_papiers_et_son_contour_le_montre() {
    use tf_app::etat::Outil;
    use tf_app::moteur::{Commande, PressePapiers};
    use tf_blocks::Transfo;
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    e.outil = Outil::Coller;
    assert!(e.coller_ici().is_none());
    assert!(
        e.message.contains("presse-papiers est vide"),
        "{}",
        e.message
    );
    assert_eq!(e.contour_d_arrivee(), None);

    // Le fil publie un presse-papiers : l'outil Coller passe en main.
    e.mode = tf_render::controles::Mode::Edition;
    e.outil = Outil::Poser;
    e.suivre_presse(PressePapiers {
        taille: Some([4, 2, 1]),
        source: "porte.litematic".into(),
        version: 1,
        ..Default::default()
    });
    assert_eq!(e.outil, Outil::Coller);
    assert_eq!(e.mode, tf_render::controles::Mode::Conception);
    // Un presse-papiers VIDÉ ne met rien en main.
    e.outil = Outil::Poser;
    e.suivre_presse(PressePapiers {
        taille: None,
        version: 7,
        ..Default::default()
    });
    assert_eq!(e.outil, Outil::Poser, "rien à coller");
    e.suivre_presse(PressePapiers {
        taille: Some([4, 2, 1]),
        source: "porte.litematic".into(),
        version: 1,
        ..Default::default()
    });
    // La même publication ne le remet pas en main une seconde fois.
    e.outil = Outil::Poser;
    e.suivre_presse(PressePapiers {
        taille: Some([4, 2, 1]),
        version: 1,
        ..Default::default()
    });
    assert_eq!(e.outil, Outil::Poser);
    e.outil = Outil::Coller;

    let coin = e.point_de_pose().unwrap();
    e.echanges.avec_air = true;
    assert!(
        matches!(
            e.coller_ici(),
            Some(Commande::Coller {
                coin: c,
                transfo: None,
                avec_air: true
            }) if c == coin
        ),
        "au point de pose, tel quel, avec l'air"
    );
    let b = e.contour_d_arrivee().unwrap();
    assert_eq!((b.min, b.size()), (coin, (4, 2, 1)));
    // Un quart de tour échange largeur et profondeur.
    e.tourner_le_composant();
    let b = e.contour_d_arrivee().unwrap();
    assert_eq!(b.size(), (1, 2, 4));
    assert!(matches!(
        e.coller_ici(),
        Some(Commande::Coller {
            transfo: Some(Transfo::Rot90),
            ..
        })
    ));
    // Changer de monde vide le presse-papiers : il vivait dans l'ancien
    // moteur.
    e.recadrer([0.0; 3], [8.0; 3], 1.0);
    assert_eq!(e.presse.taille, None);
}

/// Le contour d'arrivée de l'outil Composant est la boîte du composant
/// CHOISI.
#[test]
fn le_contour_d_arrivee_d_un_composant_est_sa_boite() {
    use tf_app::etat::Outil;
    let mut e = etat_devant_le_mur();
    e.relever_vise(&camera_face_au_mur(), 1.0, Some(CENTRE), 64.0, &mur);
    let case = e.vise.case.unwrap();
    e.suivre_composants(document(case));
    e.outil = Outil::Composant;
    assert_eq!(e.contour_d_arrivee(), None, "aucun composant choisi");
    e.composant_choisi = Some(1);
    let b = e.contour_d_arrivee().unwrap();
    assert_eq!(b.min, e.point_de_pose().unwrap());
    assert_eq!(b.size(), (3, 1, 1));
}

/// Un fichier d'un Minecraft plus RÉCENT que le monde se signale.
#[test]
fn un_fichier_plus_recent_que_le_monde_se_signale() {
    use tf_app::moteur::PressePapiers;
    let mut e = Etat::cadre([0.0; 3], [8.0; 3], 1.0);
    e.suivre_presse(PressePapiers {
        taille: Some([1, 1, 1]),
        data_version: Some(3465),
        version: 1,
        ..Default::default()
    });
    assert_eq!(e.presse_plus_recente(), None, "monde de version inconnue");
    e.echanges.version_monde = Some(2975);
    assert_eq!(e.presse_plus_recente(), Some((3465, 2975)));
    e.echanges.version_monde = Some(3465);
    assert_eq!(e.presse_plus_recente(), None);
}

// ── le PLAN DE RÉFÉRENCE : viser dans un monde vide ─────────────────────────

/// Un monde VIDE tel que `level.dat` le dit : plat, une couche d'air — le
/// préréglage « The Void » du jeu.
fn niveau_vide(apparition: Option<[i32; 3]>) -> tf_world::niveau::Niveau {
    tf_world::niveau::Niveau {
        apparition,
        data_version: Some(2975),
        generation: Some(tf_world::niveau::Generation {
            genre: "minecraft:flat".into(),
            couches: vec![("minecraft:air".into(), 1)],
            biome: Some("minecraft:the_void".into()),
        }),
        ..Default::default()
    }
}

/// Un plat « Classique » : du terrain, donc pas vide.
fn niveau_plat_classique() -> tf_world::niveau::Niveau {
    let mut n = niveau_vide(Some([3, -60, 9]));
    n.generation.as_mut().unwrap().couches = vec![
        ("minecraft:bedrock".into(), 1),
        ("minecraft:dirt".into(), 2),
        ("minecraft:grass_block".into(), 1),
    ];
    n
}

/// L'œil à y = 80, regardant à 45° vers le bas et vers l'est : il coupe le
/// plan y = 64 seize blocs plus loin, en x = 16,5.
fn camera_au_dessus_du_vide() -> Camera {
    Camera {
        oeil: [0.5, 80.0, 0.5],
        cible: [1.5, 79.0, 0.5],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    }
}

fn etat_dans_le_vide() -> Etat {
    let mut e = Etat::cadre([0.0; 3], [8.0; 3], 1.0);
    e.regler_pour_le_monde(Some(&niveau_vide(Some([0, 64, 0]))));
    e
}

/// **Dans un monde vide, un rayon qui ne touche rien vise le PLAN** — comme
/// il viserait le dessus d'un sol : la case sous le plan est celle qu'on
/// sélectionne, celle au-dessus celle où l'on pose.
#[test]
fn dans_un_monde_vide_on_vise_le_plan_comme_un_sol() {
    let mut e = etat_dans_le_vide();
    assert!(e.monde_vide);
    assert_eq!(e.plan.hauteur(), Some(64));
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        256.0,
        &|_| false,
    );
    assert_eq!(e.vise.case, Some(BlockPos::new(16, 63, 0)));
    assert_eq!(e.vise.pose, Some(BlockPos::new(16, 64, 0)));
    assert!(e.vise.sur_le_plan);

    // Le coin de sélection se pose sur la case visée, comme sur un sol.
    assert!(e.poser_coin(true));
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some([0.0, -0.5]),
        256.0,
        &|_| false,
    );
    assert!(e.poser_coin(false));
    let b = e.selection.boite().unwrap();
    assert_eq!((b.min.y, b.max.y), (63, 63), "la sélection quitte le plan");
}

/// **Un bloc réel gagne toujours** : le plan n'est qu'un repli, il ne vole
/// jamais la visée de ce qu'on voit — ni devant lui, ni derrière.
#[test]
fn un_bloc_gagne_sur_le_plan() {
    let mut e = etat_dans_le_vide();
    // Devant le plan : un mur à partir de x = 8, que le rayon touche en
    // y = 72 avant d'atteindre le plan.
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        256.0,
        &|c| c[0] >= 8,
    );
    assert_eq!(e.vise.case, Some(BlockPos::new(8, 72, 0)));
    assert!(!e.vise.sur_le_plan);
    // DERRIÈRE le plan : un sous-sol en y = 50, que le plan cacherait s'il
    // arrêtait le rayon. On vise à travers.
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        256.0,
        &|c| c[1] <= 50,
    );
    assert_eq!(e.vise.case.map(|c| c.y), Some(50));
    assert!(!e.vise.sur_le_plan);
}

/// Plan éteint, un rayon qui ne touche rien ne vise rien — c'est ce qui se
/// passait partout avant le plan, et ce qui se passe encore dans un monde
/// qui a du terrain.
#[test]
fn plan_eteint_le_vide_ne_se_vise_pas() {
    let mut e = etat_dans_le_vide();
    e.plan.actif = false;
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        256.0,
        &|_| false,
    );
    assert_eq!(e.vise, Default::default());
    // Au-delà de la portée, le plan ne se vise pas non plus.
    e.plan.actif = true;
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        20.0,
        &|_| false,
    );
    assert_eq!(e.vise.case, None, "le plan est à 22,6 blocs");
}

/// Le plan n'est pas un bloc : on ne le NOMME pas — la pipette prendrait de
/// l'air, et « Poser » poserait du vide —, et on ne le casse pas.
#[test]
fn le_plan_ne_se_nomme_ni_ne_se_casse() {
    let mut e = etat_dans_le_vide();
    e.relever_vise(
        &camera_au_dessus_du_vide(),
        1.0,
        Some(CENTRE),
        256.0,
        &|_| false,
    );
    e.nommer_vise(|_| "minecraft:air");
    assert_eq!(e.bloc_vise, None);
    assert!(!e.pipette());
    assert!(e.casser_un_bloc().is_none());
    assert!(e.message.contains("plan de référence"), "{}", e.message);
    // Poser, lui, pose SUR le plan.
    let pose = e.poser_un_bloc().expect("poser sur le plan");
    match pose {
        tf_app::moteur::Commande::Appliquer { sel, .. } => {
            assert_eq!(sel.min, BlockPos::new(16, 64, 0));
        }
        _ => panic!("une pose est une opération"),
    }
}

/// **Ce que le monde ouvert règle** : un monde vide allume le plan à la
/// hauteur de son point d'apparition, bornée au monde ; un monde qui a du
/// terrain l'éteint, mais garde cette hauteur pour qui l'allume à la main.
#[test]
fn le_monde_ouvert_regle_le_plan() {
    let mut e = Etat::cadre([0.0; 3], [8.0; 3], 1.0);
    e.regler_pour_le_monde(Some(&niveau_vide(Some([5, -60, 7]))));
    assert!(e.monde_vide);
    assert_eq!(e.plan.hauteur(), Some(-60));

    // Bornée : au-delà du plafond ou sous le fond, le plan irait où rien
    // ne se pose.
    e.regler_pour_le_monde(Some(&niveau_vide(Some([0, 400, 0]))));
    assert_eq!(e.plan.hauteur(), Some(320));
    assert_eq!(e.plan.y, 320, "le champ montre la hauteur qu'on vise");
    e.regler_pour_le_monde(Some(&niveau_vide(Some([0, -100, 0]))));
    assert_eq!(e.plan.hauteur(), Some(-64));
    // Sans point d'apparition, la hauteur de la mer.
    e.regler_pour_le_monde(Some(&niveau_vide(None)));
    assert_eq!(e.plan.hauteur(), Some(64));

    // Du terrain : éteint, et la hauteur est celle de l'apparition.
    e.regler_pour_le_monde(Some(&niveau_plat_classique()));
    assert!(!e.monde_vide);
    assert_eq!(e.plan.hauteur(), None);
    assert_eq!(e.plan.y, -60);
    // Rien ne se lit : ni vide, ni plan.
    e.regler_pour_le_monde(None);
    assert!(!e.monde_vide);
    assert_eq!(e.plan.hauteur(), None);

    // Changer de monde oublie le plan de l'ancien : c'est le nouveau qui le
    // dit, par `regler_pour_le_monde`.
    e.regler_pour_le_monde(Some(&niveau_vide(Some([0, 10, 0]))));
    e.recadrer([0.0; 3], [8.0; 3], 1.0);
    assert_eq!(e.plan, tf_app::etat::PlanDeReference::default());
    assert!(!e.monde_vide);

    // Une hauteur réglée hors bornes à la main est bornée à la visée.
    e.plan = tf_app::etat::PlanDeReference {
        actif: true,
        y: 9999,
    };
    assert_eq!(e.plan.hauteur(), Some(tf_app::etat::PLAN_Y.1));
}

/// **Un monde qui ne montre rien se cadre sur son plan**, autour de là où
/// l'on joue ; un monde qui montre quelque chose se cadre sur ce qu'il
/// montre.
#[test]
fn un_monde_sans_contenu_se_cadre_sur_le_plan() {
    use tf_app::etat::cadre_du_vide;
    let contenu = Some(([0.0; 3], [8.0; 3]));
    assert_eq!(cadre_du_vide(contenu, Some(64), Some([100, 70, -50])), None);
    assert_eq!(cadre_du_vide(None, None, Some([100, 70, -50])), None);

    let (a, b) = cadre_du_vide(None, Some(64), Some([100, 70, -50])).unwrap();
    assert_eq!(a[1], 64.0, "le cadre part du plan");
    assert!(b[1] > a[1]);
    // Centré sur la colonne où l'on joue.
    assert_eq!((a[0] + b[0]) / 2.0, 100.5);
    assert_eq!((a[2] + b[2]) / 2.0, -49.5);
    // Sans rien pour dire où l'on joue : l'origine.
    let (a, b) = cadre_du_vide(None, Some(-60), None).unwrap();
    assert_eq!(
        ((a[0] + b[0]) / 2.0, a[1], (a[2] + b[2]) / 2.0),
        (0.5, -60.0, 0.5)
    );
}

/// La grille du plan se centre là où le REGARD le coupe — pas sous l'œil,
/// qui peut être à cent blocs de ce qu'on regarde.
#[test]
fn la_grille_du_plan_suit_le_regard() {
    let mut e = etat_dans_le_vide();
    assert_eq!(
        e.plan_a_dessiner(&camera_au_dessus_du_vide()),
        Some((64, [16, 0]))
    );
    // Un regard qui ne coupe pas le plan : sous l'œil.
    let vers_le_ciel = Camera {
        cible: [1.5, 81.0, 0.5],
        ..camera_au_dessus_du_vide()
    };
    assert_eq!(e.plan_a_dessiner(&vers_le_ciel), Some((64, [0, 0])));
    e.plan.actif = false;
    assert_eq!(e.plan_a_dessiner(&camera_au_dessus_du_vide()), None);
}

/// **La grille du plan** : à plat, un cheveu au-dessus du plan ; les lignes
/// vives sur les frontières de chunks, quelle que soit la colonne regardée ;
/// les fines ne repassent jamais sur une vive.
#[test]
fn la_grille_du_plan_est_a_plat_et_alignee_sur_les_chunks() {
    use tf_app::scene::{grille_du_plan, PLAN_CHUNKS, PLAN_FIN};
    for centre in [[0, 0], [37, -5], [-17, 200]] {
        let l = grille_du_plan(-60, centre);
        let (mut vives, mut fines) = (0, 0);
        let vive = l.sommets[0].couleur;
        for s in l.sommets.chunks(2) {
            let (a, b) = (s[0].position, s[1].position);
            for y in [a[1], b[1]] {
                assert!(y > -60.0 && y < -59.9, "hors du plan : {y}");
            }
            // Un segment est parallèle à x ou à z : l'axe CONSTANT dit sur
            // quelle ligne il est.
            let fixe = if a[0] == b[0] { a[0] } else { a[2] };
            if s[0].couleur == vive {
                vives += 1;
                assert_eq!(fixe.rem_euclid(16.0), 0.0, "ligne de chunk hors frontière");
            } else {
                fines += 1;
                assert_ne!(fixe.rem_euclid(16.0), 0.0, "ligne fine sur une frontière");
            }
        }
        assert_eq!(vives, 2 * (2 * PLAN_CHUNKS + 2) as usize);
        // Une par bloc sur le carré fin, moins celles qui tombent sur une
        // frontière de chunk.
        let par_axe = |c: i32| {
            (c - PLAN_FIN..=c + PLAN_FIN)
                .filter(|v| v.rem_euclid(16) != 0)
                .count()
        };
        assert_eq!(fines, par_axe(centre[0]) + par_axe(centre[1]));
    }
}
