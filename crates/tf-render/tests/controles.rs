//! Les deux pilotages, et la bascule de l'un à l'autre.
//!
//! Testés sans écran, sans fenêtre et sans wgpu : ce sont des angles et des
//! distances. Ce qui se teste sans écran doit se tester sans écran — un
//! pilotage qu'on ne peut juger qu'à l'œil est un pilotage qu'on ne juge pas.

use tf_render::controles::{angles, direction, PENTE_MAX, RAYON_MIN};
use tf_render::{Camera, Mode, Orbite, Pilotage, Vol};

const MODELE: Camera = Camera {
    oeil: [0.0; 3],
    cible: [0.0, 0.0, 1.0],
    fov: 1.0,
    proche: 0.1,
    loin: 1000.0,
};

fn proche(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= eps)
}

/// Les deux caméras rendent-elles la MÊME image ?
///
/// **`cible` est un POINT SUR le rayon, pas le rayon.** Un vol pose sa cible
/// à une unité devant le nez, une orbite la pose sur son pivot, à soixante
/// blocs — deux points différents, la même direction, donc la même image, la
/// matrice de vue ne gardant que la direction normalisée. Comparer les points
/// ferait échouer un test sur un choix de représentation, ce qui envoie
/// chercher un bug dans du code juste.
fn meme_image(a: &Camera, b: &Camera, eps: f32) -> bool {
    let dir = |c: &Camera| {
        let d = [
            c.cible[0] - c.oeil[0],
            c.cible[1] - c.oeil[1],
            c.cible[2] - c.oeil[2],
        ];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
        [d[0] / n, d[1] / n, d[2] / n]
    };
    proche(a.oeil, b.oeil, eps) && proche(dir(a), dir(b), eps)
}

// ── la trigonométrie, une seule fois ────────────────────────────────────────

/// `direction` et `angles` sont inverses l'une de l'autre.
///
/// Deux copies de la même trigonométrie finissent par diverger sur un signe —
/// c'est arrivé quatre fois dans ce dépôt, sur les rotations de modèles. Ici
/// il n'y en a qu'une, et l'aller-retour le prouve.
#[test]
fn direction_et_angles_sont_inverses() {
    for cap in [-3.0f32, -1.0, 0.0, 0.7, 2.5, 3.1] {
        for pente in [-1.5f32, -0.4, 0.0, 0.4, 1.5] {
            let d = direction(cap, pente);
            let (c2, p2) = angles(d);
            let d2 = direction(c2, p2);
            assert!(
                proche(d, d2, 1e-5),
                "cap {cap} pente {pente} : {d:?} != {d2:?}"
            );
        }
    }
}

/// Le repère Minecraft, en toutes lettres : cap 0 regarde l'EST (+X), et le
/// cap croît vers le SUD (+Z). Un sens inversé ferait avancer à reculons, et
/// aucun test de géométrie pure ne le dirait.
#[test]
fn le_cap_zero_regarde_l_est_et_croit_vers_le_sud() {
    assert!(proche(direction(0.0, 0.0), [1.0, 0.0, 0.0], 1e-6), "Est");
    assert!(
        proche(
            direction(std::f32::consts::FRAC_PI_2, 0.0),
            [0.0, 0.0, 1.0],
            1e-6
        ),
        "un quart de tour plus loin : le Sud"
    );
    assert!(
        proche(direction(0.0, PENTE_MAX), [0.0, 1.0, 0.0], 0.01),
        "pente maximale : presque le ciel"
    );
}

// ── le vol ──────────────────────────────────────────────────────────────────

/// **La pente est BORNÉE, jamais enroulée.**
///
/// Passer par-dessus la tête retourne l'image d'un coup : la direction devient
/// colinéaire au haut du monde, leur produit vectoriel s'annule, et la base de
/// la vue est dégénérée. C'est le défaut qu'on met des heures à décrire alors
/// qu'il suffit de ne pas le permettre.
#[test]
fn la_pente_du_vol_ne_passe_pas_par_dessus_la_tete() {
    let mut v = Vol::nouveau([0.0; 3], 0.0, 0.0);
    v.tourner(0.0, 100.0);
    assert!(v.pente <= PENTE_MAX, "pente {} non bornée", v.pente);
    v.tourner(0.0, -100.0);
    assert!(v.pente >= -PENTE_MAX);
    // Et la caméra qui en sort a une direction qui n'est PAS verticale pure.
    let c = v.camera(&MODELE);
    let d = [
        c.cible[0] - c.oeil[0],
        c.cible[1] - c.oeil[1],
        c.cible[2] - c.oeil[2],
    ];
    let plat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    assert!(plat > 1e-4, "la composante plate ne doit pas s'annuler");
}

/// **« Monter » monte, même en regardant le sol.**
///
/// La verticale du déplacement est celle du MONDE, pas celle de la caméra :
/// sinon monter en piqué fait reculer, ce que personne n'attend d'un vol de
/// créatif.
#[test]
fn monter_monte_meme_en_regardant_le_sol() {
    let mut v = Vol::nouveau([0.0; 3], 0.0, -PENTE_MAX * 0.9);
    v.deplacer(0.0, 0.0, 5.0);
    assert!(
        proche(v.position, [0.0, 5.0, 0.0], 1e-4),
        "{:?}",
        v.position
    );
}

/// Avancer avance le long du REGARD — y compris vers le haut.
#[test]
fn avancer_suit_le_regard() {
    let mut v = Vol::nouveau([0.0; 3], 0.0, 0.0);
    v.deplacer(3.0, 0.0, 0.0);
    assert!(proche(v.position, [3.0, 0.0, 0.0], 1e-4), "plein Est");

    let mut m = Vol::nouveau([0.0; 3], 0.0, 0.7);
    m.deplacer(1.0, 0.0, 0.0);
    assert!(m.position[1] > 0.0, "en montée, avancer monte aussi");
}

/// Le côté est pris dans le PLAN, jamais incliné : un pas de côté en piqué ne
/// doit pas plonger.
#[test]
fn le_pas_de_cote_reste_horizontal() {
    let mut v = Vol::nouveau([0.0; 3], 0.0, -1.2);
    v.deplacer(0.0, 4.0, 0.0);
    assert!(
        v.position[1].abs() < 1e-5,
        "un pas de côté ne change pas l'altitude : {:?}",
        v.position
    );
}

// ── l'orbite ────────────────────────────────────────────────────────────────

/// **Tourner en orbite ne déplace pas le pivot.** C'est toute la différence
/// avec le vol : l'objet reste au centre et c'est nous qui tournons autour.
#[test]
fn orbiter_laisse_le_pivot_en_place() {
    let mut o = Orbite::nouvelle([10.0, 20.0, 30.0], 0.0, 0.0, 12.0);
    let c0 = o.camera(&MODELE);
    o.tourner(1.3, 0.4);
    let c1 = o.camera(&MODELE);
    assert_eq!(o.pivot, [10.0, 20.0, 30.0]);
    assert_eq!(c1.cible, c0.cible, "la cible EST le pivot");
    assert_ne!(c1.oeil, c0.oeil, "mais l'œil a tourné");
    // Et il est resté à la même distance.
    let r = |c: &Camera| {
        let d = [
            c.cible[0] - c.oeil[0],
            c.cible[1] - c.oeil[1],
            c.cible[2] - c.oeil[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    assert!((r(&c0) - r(&c1)).abs() < 1e-3, "le rayon ne bouge pas");
}

/// **Le zoom est un FACTEUR, pas un pas.**
///
/// Un pas fixe est inutilisable aux deux bouts : trop lent pour traverser un
/// build, et il traverse l'objet d'un cran quand on est contre. Le facteur
/// rend le geste identique à toutes les échelles — ce qu'on veut d'un outil
/// qui sert du bloc à la ville.
#[test]
fn le_zoom_est_un_facteur_et_ne_traverse_jamais_le_pivot() {
    let mut o = Orbite::nouvelle([0.0; 3], 0.0, 0.0, 1000.0);
    for _ in 0..200 {
        o.zoomer(0.5);
    }
    assert!(
        o.rayon >= RAYON_MIN,
        "le rayon ne doit jamais atteindre zéro : {}",
        o.rayon
    );
    // Rayon nul = œil confondu avec la cible = direction nulle = matrice de
    // vue pleine de NaN. Et NaN ne plante pas : il affiche du noir.
    let c = o.camera(&MODELE);
    assert!(c.oeil.iter().all(|v| v.is_finite()));
    assert_ne!(c.oeil, c.cible);
}

/// Le panoramique déplace le PIVOT, pas l'œil : sinon le rayon changerait et
/// le geste suivant tournerait autour d'autre chose.
#[test]
fn le_panoramique_deplace_le_pivot_sans_changer_le_rayon() {
    let mut o = Orbite::nouvelle([0.0; 3], 0.9, 0.3, 40.0);
    let avant = o.rayon;
    o.glisser(5.0, 3.0);
    assert_ne!(o.pivot, [0.0; 3]);
    assert!((o.rayon - avant).abs() < 1e-5);
}

// ── la bascule ──────────────────────────────────────────────────────────────

/// **Le piège du bouton : le pivot d'une orbite se DÉCIDE.**
///
/// `Camera` porte déjà un œil et une cible, donc la bascule a l'air gratuite.
/// Elle ne l'est pas : en vol, la cible est un point arbitraire posé à un
/// mètre devant le nez. Orbiter autour d'elle ferait pivoter l'utilisateur
/// autour de son propre nez — ce qui se lit « la caméra est devenue folle » et
/// ne désigne pas la cause.
#[test]
fn basculer_en_conception_orbite_autour_de_ce_qu_on_vise() {
    let mut p = Pilotage {
        mode: Mode::Edition,
        vol: Vol::nouveau([0.0, 70.0, 0.0], 0.0, 0.0),
        orbite: Orbite::nouvelle([0.0; 3], 0.0, 0.0, 1.0),
    };
    let vise = [64.0, 70.0, 0.0]; // le bloc visé, à 64 d'ici
    p.basculer(Mode::Conception, Some(vise));

    assert_eq!(p.mode, Mode::Conception);
    assert_eq!(p.orbite.pivot, vise, "le pivot est la CIBLE visée");
    assert!(
        (p.orbite.rayon - 64.0).abs() < 1e-3,
        "et le rayon est la distance réelle, pas un défaut : {}",
        p.orbite.rayon
    );
    // L'œil n'a pas bougé : l'image est la même à l'instant du clic. Seul le
    // centre de rotation a changé, et c'était tout l'objet de la bascule.
    let c = p.camera(&MODELE);
    assert!(
        proche(c.oeil, [0.0, 70.0, 0.0], 1e-3),
        "l'œil doit rester en place : {:?}",
        c.oeil
    );
}

/// **L'aller-retour ne DÉRIVE pas.**
///
/// Basculer deux fois sans rien toucher doit rendre la caméra de départ.
/// Sinon chaque appui coûte un recadrage — une dérive lente qu'on attribue à
/// sa souris pendant des semaines, et que personne ne pense à imputer au
/// bouton.
#[test]
fn l_aller_retour_de_bascule_rend_la_meme_camera() {
    for (cap, pente) in [(0.0f32, 0.0f32), (1.2, 0.5), (-2.7, -1.1), (3.0, 1.4)] {
        let mut p = Pilotage {
            mode: Mode::Edition,
            vol: Vol::nouveau([12.0, 70.0, -5.0], cap, pente),
            orbite: Orbite::nouvelle([0.0; 3], 0.0, 0.0, 8.0),
        };
        let depart = p.camera(&MODELE);
        p.basculer(Mode::Conception, Some([40.0, 65.0, 30.0]));
        p.basculer(Mode::Edition, None);
        let arrivee = p.camera(&MODELE);
        assert_eq!(p.mode, Mode::Edition);
        assert!(
            meme_image(&depart, &arrivee, 1e-3),
            "cap {cap} pente {pente} : la caméra a dérivé, {:?} → {:?}",
            depart.oeil,
            arrivee.oeil
        );
    }
}

/// Dix allers-retours ne dérivent pas plus qu'un : l'erreur ne s'accumule pas.
#[test]
fn dix_allers_retours_ne_derivent_pas_davantage() {
    let mut p = Pilotage {
        mode: Mode::Edition,
        vol: Vol::nouveau([12.0, 70.0, -5.0], 1.2, 0.5),
        orbite: Orbite::nouvelle([0.0; 3], 0.0, 0.0, 8.0),
    };
    let depart = p.camera(&MODELE);
    for _ in 0..10 {
        p.basculer(Mode::Conception, Some([40.0, 65.0, 30.0]));
        p.basculer(Mode::Edition, None);
    }
    let arrivee = p.camera(&MODELE);
    assert!(
        proche(depart.oeil, arrivee.oeil, 1e-2),
        "{:?} → {:?}",
        depart.oeil,
        arrivee.oeil
    );
}

/// **Rien à viser n'est pas une raison de téléporter.**
///
/// Quand le rayon ne touche rien — on regarde le ciel — le pivot se pose
/// devant le nez, pas à l'origine du monde. Sur un monde Minefield, un pivot
/// inventé à (0, 0, 0) enverrait l'utilisateur à des milliers de blocs de là.
#[test]
fn sans_cible_le_pivot_se_pose_devant_soi_et_pas_a_l_origine() {
    let mut p = Pilotage {
        mode: Mode::Edition,
        vol: Vol::nouveau([5000.0, 90.0, -3000.0], 0.4, 0.2),
        orbite: Orbite::nouvelle([0.0; 3], 0.0, 0.0, 20.0),
    };
    p.basculer(Mode::Conception, None);
    let d = [
        p.orbite.pivot[0] - 5000.0,
        p.orbite.pivot[1] - 90.0,
        p.orbite.pivot[2] + 3000.0,
    ];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    assert!(
        distance < 100.0,
        "le pivot doit rester près de l'utilisateur, il est à {distance}"
    );
    let c = p.camera(&MODELE);
    assert!(
        proche(c.oeil, [5000.0, 90.0, -3000.0], 1e-2),
        "et l'œil ne bouge toujours pas : {:?}",
        c.oeil
    );
}

/// Basculer vers le mode où l'on est déjà ne fait RIEN — en particulier, ne
/// recalcule pas le pivot. Sinon un double clic sur le bouton déplacerait le
/// centre de rotation sans que rien ne l'explique.
#[test]
fn basculer_vers_le_mode_courant_est_sans_effet() {
    let mut p = Pilotage::cadrer([0.0; 3], [64.0, 64.0, 64.0], 16.0 / 9.0);
    let avant = p;
    p.basculer(Mode::Edition, Some([1.0, 2.0, 3.0]));
    assert_eq!(p, avant);
}

/// Le cadrage initial est cohérent des DEUX côtés : on peut basculer
/// immédiatement sans que l'image saute.
#[test]
fn le_cadrage_initial_donne_la_meme_image_dans_les_deux_modes() {
    let mut p = Pilotage::cadrer([0.0; 3], [64.0, 40.0, 64.0], 16.0 / 9.0);
    let edition = p.camera(&MODELE);
    p.mode = Mode::Conception;
    let conception = p.camera(&MODELE);
    assert!(
        meme_image(&edition, &conception, 1e-3),
        "{:?} vs {:?}",
        edition.oeil,
        conception.oeil
    );
}

/// **Un pivot HORS de l'axe du regard garde sa profondeur, pas sa position.**
///
/// C'est la faute de la première écriture, et elle se mesure : avec un pivot
/// posé tel quel à quarante blocs de l'axe, l'œil sautait de quarante blocs au
/// moment du clic. Projeter sur le rayon rend la bascule TOTALE — elle marche
/// quel que soit le point qu'on lui donne — au lieu d'imposer à l'appelant un
/// contrat qu'il violerait en silence.
#[test]
fn un_pivot_hors_axe_ne_fait_pas_sauter_l_image() {
    let mut p = Pilotage {
        mode: Mode::Edition,
        vol: Vol::nouveau([0.0, 70.0, 0.0], 0.0, 0.0), // regarde plein Est
        orbite: Orbite::nouvelle([0.0; 3], 0.0, 0.0, 1.0),
    };
    let avant = p.camera(&MODELE);
    // Franchement à côté de l'axe : 40 blocs au Sud et 5 plus bas.
    p.basculer(Mode::Conception, Some([64.0, 65.0, 40.0]));
    let apres = p.camera(&MODELE);

    assert!(
        meme_image(&avant, &apres, 1e-3),
        "la bascule ne doit RIEN bouger : {:?} → {:?}",
        avant.oeil,
        apres.oeil
    );
    // La profondeur est conservée : 64 blocs devant, sur l'axe.
    assert!(
        (p.orbite.rayon - 64.0).abs() < 1e-3,
        "rayon {} au lieu de 64",
        p.orbite.rayon
    );
    assert!(proche(p.orbite.pivot, [64.0, 70.0, 0.0], 1e-3));
}

/// **`recentrer`, elle, bouge la caméra — et c'est pour ça qu'elle a un autre
/// nom.**
///
/// « Tourner autour de ma sélection » quand la sélection n'est pas sous le
/// réticule est un geste délibéré. Le confondre avec la bascule ferait d'un
/// bouton de mode quelque chose qui déplace le point de vue, ce que personne
/// n'attend d'un bouton de mode.
#[test]
fn recentrer_bouge_la_camera_la_ou_basculer_ne_le_fait_pas() {
    let mut p = Pilotage {
        mode: Mode::Conception,
        vol: Vol::nouveau([0.0, 70.0, 0.0], 0.0, 0.0),
        orbite: Orbite::nouvelle([0.0, 70.0, 0.0], 0.0, 0.0, 10.0),
    };
    let avant = p.camera(&MODELE);
    p.recentrer([200.0, 40.0, -80.0], Some(25.0));
    let apres = p.camera(&MODELE);

    assert!(!proche(avant.oeil, apres.oeil, 1.0), "elle DOIT bouger");
    assert!(proche(apres.cible, [200.0, 40.0, -80.0], 1e-3));
    // Et le vol est tenu synchrone : revenir en Édition repart d'où l'orbite
    // a laissé l'œil, pas d'un état périmé.
    p.basculer(Mode::Edition, None);
    assert!(
        proche(p.camera(&MODELE).oeil, apres.oeil, 1e-3),
        "le vol doit avoir suivi le recentrage"
    );
}
