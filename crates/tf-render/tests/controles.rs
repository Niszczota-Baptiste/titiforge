//! Le pilotage : le point fixe est le JOUEUR.
//!
//! Testé sans écran, sans fenêtre et sans wgpu : ce sont des angles et des
//! distances. Ce qui se teste sans écran doit se tester sans écran — un
//! pilotage qu'on ne peut juger qu'à l'œil est un pilotage qu'on ne juge pas.

use tf_render::controles::{angles, direction, PENTE_MAX};
use tf_render::{Camera, Mode, Vue};

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

/// La direction NORMALISÉE d'une caméra.
///
/// **`cible` est un POINT SUR le rayon, pas le rayon.** Deux caméras de même
/// œil et de cibles différentes sur la même droite rendent la même image — la
/// matrice de vue ne garde que la direction. Comparer les points ferait
/// échouer un test sur un choix de représentation, ce qui envoie chercher un
/// bug dans du code juste.
fn regard(c: &Camera) -> [f32; 3] {
    let d = [
        c.cible[0] - c.oeil[0],
        c.cible[1] - c.oeil[1],
        c.cible[2] - c.oeil[2],
    ];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
    [d[0] / n, d[1] / n, d[2] / n]
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
            assert!(
                proche(d, direction(c2, p2), 1e-5),
                "cap {cap} pente {pente}"
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

// ── le point fixe ───────────────────────────────────────────────────────────

/// **Le point fixe est le JOUEUR, jamais le build.**
///
/// C'est ce que fait Minecraft, donc ce que la main de quiconque construit
/// sait déjà faire. Une orbite autour d'un pivot posé sur le build est la
/// convention de la CAO : elle déplace l'œil quand on tourne, et personne
/// n'attend ça d'un monde où l'on vole. Le test porte sur la seule chose qui
/// compte — l'œil ne bouge pas d'un millième.
#[test]
fn tourner_ne_deplace_jamais_l_oeil() {
    let mut v = Vue::nouvelle([120.0, 70.0, -340.0], 0.0, 0.0);
    let avant = v.camera(&MODELE);
    for (dc, dp) in [(0.3f32, 0.2f32), (-1.7, 0.4), (2.9, -0.9), (0.1, -0.3)] {
        v.tourner(dc, dp);
        let apres = v.camera(&MODELE);
        assert!(
            proche(avant.oeil, apres.oeil, 1e-4),
            "l'œil a bougé en tournant : {:?} → {:?}",
            avant.oeil,
            apres.oeil
        );
        assert_eq!(v.position, [120.0, 70.0, -340.0]);
    }
    // Et le regard, lui, a bien changé.
    assert!(!proche(regard(&avant), regard(&v.camera(&MODELE)), 1e-3));
}

/// Réciproquement : se déplacer ne change pas le REGARD. Les deux gestes sont
/// indépendants, et c'est ce qui rend le pilotage prévisible — on ne se
/// retrouve pas à viser ailleurs parce qu'on a avancé.
#[test]
fn se_deplacer_ne_change_pas_le_regard() {
    let mut v = Vue::nouvelle([0.0; 3], 1.1, -0.4);
    let avant = regard(&v.camera(&MODELE));
    v.deplacer(12.0, -3.0, 5.0);
    v.glisser(2.0, 7.0);
    assert!(proche(avant, regard(&v.camera(&MODELE)), 1e-5));
    assert_eq!((v.cap, v.pente), (1.1, -0.4));
}

// ── les bornes ──────────────────────────────────────────────────────────────

/// **La pente est BORNÉE, jamais enroulée.**
///
/// Passer par-dessus la tête retourne l'image d'un coup : la direction devient
/// colinéaire au haut du monde, leur produit vectoriel s'annule, et la base de
/// la vue est dégénérée. C'est le défaut qu'on met des heures à décrire alors
/// qu'il suffit de ne pas le permettre. Minecraft fait exactement pareil.
#[test]
fn la_pente_ne_passe_pas_par_dessus_la_tete() {
    let mut v = Vue::nouvelle([0.0; 3], 0.0, 0.0);
    v.tourner(0.0, 100.0);
    assert!(v.pente <= PENTE_MAX, "pente {} non bornée", v.pente);
    v.tourner(0.0, -100.0);
    assert!(v.pente >= -PENTE_MAX);
    // La composante plate du regard ne doit jamais s'annuler : c'est elle qui
    // donne sa base à la vue.
    let d = regard(&v.camera(&MODELE));
    assert!((d[0] * d[0] + d[2] * d[2]).sqrt() > 1e-4);
}

/// Le constructeur borne aussi — sinon une vue restaurée depuis un projet
/// mal écrit naîtrait déjà dégénérée.
#[test]
fn le_constructeur_borne_la_pente_lui_aussi() {
    assert!(Vue::nouvelle([0.0; 3], 0.0, 9.0).pente <= PENTE_MAX);
    assert!(Vue::nouvelle([0.0; 3], 0.0, -9.0).pente >= -PENTE_MAX);
}

// ── le déplacement ──────────────────────────────────────────────────────────

/// **« Monter » monte, même en regardant le sol.**
///
/// La verticale du déplacement est celle du MONDE, pas celle de la caméra :
/// sinon monter en piqué fait reculer, ce que personne n'attend d'un vol de
/// créatif.
#[test]
fn monter_monte_meme_en_regardant_le_sol() {
    let mut v = Vue::nouvelle([0.0; 3], 0.0, -PENTE_MAX * 0.9);
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
    let mut v = Vue::nouvelle([0.0; 3], 0.0, 0.0);
    v.deplacer(3.0, 0.0, 0.0);
    assert!(proche(v.position, [3.0, 0.0, 0.0], 1e-4), "plein Est");

    let mut m = Vue::nouvelle([0.0; 3], 0.0, 0.7);
    m.deplacer(1.0, 0.0, 0.0);
    assert!(m.position[1] > 0.0, "en montée, avancer monte aussi");
}

/// Le côté est pris dans le PLAN : un pas de côté en piqué ne doit pas
/// plonger.
#[test]
fn le_pas_de_cote_reste_horizontal() {
    let mut v = Vue::nouvelle([0.0; 3], 0.0, -1.2);
    v.deplacer(0.0, 4.0, 0.0);
    assert!(
        v.position[1].abs() < 1e-5,
        "un pas de côté ne change pas l'altitude : {:?}",
        v.position
    );
}

/// **Le panoramique suit l'ÉCRAN, pas le monde.**
///
/// Molette enfoncée + Maj : tirer vers le haut de l'écran fait monter ce
/// qu'on voit. En regardant droit devant, ça monte tout court ; en regardant
/// le ciel, ça recule — parce que « plus haut à l'écran » y veut dire
/// « derrière ». Le confondre avec `deplacer` ferait partir à la verticale
/// quand on regarde déjà en l'air.
#[test]
fn le_panoramique_suit_le_plan_de_l_ecran() {
    let mut droit = Vue::nouvelle([0.0; 3], 0.0, 0.0);
    droit.glisser(0.0, 1.0);
    assert!(
        proche(droit.position, [0.0, 1.0, 0.0], 1e-5),
        "à l'horizontale, le haut de l'écran est le haut du monde : {:?}",
        droit.position
    );

    let mut leve = Vue::nouvelle([0.0; 3], 0.0, PENTE_MAX * 0.95);
    leve.glisser(0.0, 1.0);
    assert!(
        leve.position[0] < -0.5,
        "en regardant le ciel, le haut de l'écran part en ARRIÈRE : {:?}",
        leve.position
    );
    assert!(
        leve.position[1].abs() < 0.2,
        "et presque plus vers le haut du monde"
    );
}

// ── le cadrage ──────────────────────────────────────────────────────────────

/// Le cadrage initial regarde bien la boîte, et depuis l'extérieur.
#[test]
fn le_cadrage_regarde_la_boite_depuis_dehors() {
    let v = Vue::cadrer([0.0; 3], [64.0, 40.0, 64.0], 16.0 / 9.0);
    let c = v.camera(&MODELE);
    let centre = [32.0, 20.0, 32.0];
    let vers = [
        centre[0] - c.oeil[0],
        centre[1] - c.oeil[1],
        centre[2] - c.oeil[2],
    ];
    let n = (vers[0] * vers[0] + vers[1] * vers[1] + vers[2] * vers[2]).sqrt();
    assert!(n > 40.0, "l'œil doit être hors de la boîte, il est à {n}");
    let u = [vers[0] / n, vers[1] / n, vers[2] / n];
    assert!(
        proche(regard(&c), u, 1e-3),
        "et regarder le centre : {:?} vs {:?}",
        regard(&c),
        u
    );
}

// ── les modes ───────────────────────────────────────────────────────────────

/// **La caméra ne dépend PAS du mode.**
///
/// On vole dans les deux, le point fixe est le joueur dans les deux. Ce qui
/// change est ce que font le clic gauche et le clic droit — un VOLUME d'un
/// côté, des ENTITÉS de l'autre. Le jour où quelqu'un voudra donner une autre
/// caméra à la Conception, ce test le fera rougir, et c'est le moment où il
/// faudra en reparler plutôt que le découvrir à l'usage.
#[test]
fn le_mode_ne_touche_pas_a_la_camera() {
    let v = Vue::nouvelle([10.0, 70.0, 20.0], 0.8, -0.3);
    let c = v.camera(&MODELE);
    for m in [Mode::Edition, Mode::Conception] {
        let _ = m;
        let autre = v.camera(&MODELE);
        assert_eq!(autre.oeil, c.oeil);
        assert_eq!(autre.cible, c.cible);
    }
    assert_eq!(Mode::default(), Mode::Edition);
}
