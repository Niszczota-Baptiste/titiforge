//! L'accrochage : viser ce qui est BÂTI, pas la grille.
//!
//! Sur une grille de blocs, accrocher à la grille ne vaut rien — tout y est
//! déjà. Ce qui compte est d'accrocher à ce qui EXISTE, et de DIRE à quoi.

use tf_world::coords::{BBox, BlockPos};
use tf_world::inference::{accrocher, Ancre, Reference, TOLERANCE};

fn p(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(x, y, z)
}

fn coin(x: i32, y: i32, z: i32) -> Reference {
    Reference::new(p(x, y, z), Ancre::Coin)
}

const LIBRE: [Option<i32>; 3] = [None, None, None];

// ── un seul mécanisme, trois accroches ──────────────────────────────────────

/// **UN axe accroché donne un PLAN** — le nu d'un mur, une altitude.
///
/// C'est l'accroche la plus utile d'un éditeur Minecraft : aligner une
/// nouvelle façade sur celle d'en face. Elle ne demande aucun code à part :
/// c'est l'accrochage par axe, avec une seule coordonnée dans la tolérance.
#[test]
fn un_seul_axe_accroche_donne_un_plan() {
    // La référence est loin en y et z, proche en x seulement.
    let refs = [coin(12, 900, -900)];
    let a = accrocher(p(13, 70, 30), &refs, TOLERANCE, LIBRE);
    assert_eq!(a.position, p(12, 70, 30), "seul x s'aligne");
    assert_eq!(a.axes_accroches(), 1);
    assert_eq!(a.raisons[0].unwrap().ecart, -1);
    assert!(a.raisons[1].is_none() && a.raisons[2].is_none());
}

/// **DEUX axes donnent une DROITE** — c'est la ligne droite que tout le monde
/// attend après avoir posé un premier point. Elle ne demande pas non plus de
/// code à part.
#[test]
fn deux_axes_accroches_donnent_une_droite() {
    let pos1 = Reference::new(p(10, 70, 30), Ancre::Dernier);
    // On tire loin en x, en restant presque à la même hauteur et profondeur.
    let a = accrocher(p(240, 71, 29), &[pos1], TOLERANCE, LIBRE);
    assert_eq!(a.position, p(240, 70, 30), "x libre, y et z alignés");
    assert_eq!(a.axes_accroches(), 2);
}

/// **TROIS axes donnent le POINT.** Et si l'on y était déjà, rien ne bouge —
/// l'accrochage est idempotent, sinon viser juste déplacerait quand même.
#[test]
fn trois_axes_donnent_le_point_et_ne_bougent_pas_deux_fois() {
    let refs = [coin(10, 70, 30)];
    let a = accrocher(p(11, 71, 29), &refs, TOLERANCE, LIBRE);
    assert_eq!(a.position, p(10, 70, 30));
    assert_eq!(a.axes_accroches(), 3);
    assert!(a.a_bouge());

    let b = accrocher(a.position, &refs, TOLERANCE, LIBRE);
    assert_eq!(b.position, a.position, "idempotent");
    assert!(!b.a_bouge(), "et il le DIT : rien n'a bougé");
    assert_eq!(b.axes_accroches(), 3, "mais il accroche toujours");
}

/// Hors tolérance, on ne touche à rien. Un accrochage qui attire de loin est
/// un accrochage qu'on ne peut plus contourner.
#[test]
fn hors_tolerance_rien_ne_bouge() {
    let a = accrocher(p(100, 70, 30), &[coin(10, 70, 30)], TOLERANCE, LIBRE);
    assert_eq!(a.position, p(100, 70, 30), "x est trop loin");
    assert_eq!(a.axes_accroches(), 2, "y et z, eux, coïncident");

    // Tolérance nulle : seul l'exact accroche.
    let b = accrocher(p(11, 70, 30), &[coin(10, 70, 30)], 0, LIBRE);
    assert_eq!(b.position, p(11, 70, 30));
    assert!(b.raisons[0].is_none());
}

/// Sans référence, l'accrochage est l'identité — et ne panique pas.
#[test]
fn sans_reference_c_est_l_identite() {
    let a = accrocher(p(7, 8, 9), &[], TOLERANCE, LIBRE);
    assert_eq!(a.position, p(7, 8, 9));
    assert_eq!(a.axes_accroches(), 0);
    assert!(!a.a_bouge());
}

// ── le départage ────────────────────────────────────────────────────────────

/// Le plus PROCHE gagne, quel que soit son genre.
#[test]
fn le_plus_proche_gagne() {
    let refs = [coin(10, 0, 0), Reference::new(p(13, 0, 0), Ancre::Milieu)];
    let a = accrocher(p(12, 0, 0), &refs, TOLERANCE, LIBRE);
    assert_eq!(a.position.x, 13, "le milieu est à 1, le coin à 2");
    assert_eq!(a.raisons[0].unwrap().genre, Ancre::Milieu);
}

/// **À écart égal, le genre le plus FORT gagne** — un coin l'emporte sur un
/// milieu, comme dans SketchUp. Sans cette règle, viser un coin attraperait
/// parfois le milieu de l'arête qui y aboutit.
#[test]
fn a_egalite_le_coin_l_emporte_sur_le_milieu() {
    let refs = [Reference::new(p(9, 0, 0), Ancre::Milieu), coin(11, 0, 0)];
    let a = accrocher(p(10, 0, 0), &refs, TOLERANCE, LIBRE);
    assert_eq!(a.raisons[0].unwrap().genre, Ancre::Coin);
    assert_eq!(a.position.x, 11);
    // Et l'ordre de la liste ne change rien.
    let inverse = [coin(11, 0, 0), Reference::new(p(9, 0, 0), Ancre::Milieu)];
    assert_eq!(
        accrocher(p(10, 0, 0), &inverse, TOLERANCE, LIBRE)
            .position
            .x,
        11
    );
}

/// **Le départage est TOTAL : deux références identiques en tout ne se
/// relaient pas.**
///
/// L'accrochage clignoterait d'une image à l'autre, et un clignotement se lit
/// « l'outil est instable » sans qu'on sache quoi regarder. Le dernier critère
/// est la coordonnée, qui tranche toujours.
#[test]
fn deux_references_symetriques_ne_se_relaient_pas() {
    let refs = [coin(8, 0, 0), coin(12, 0, 0)];
    let attendu = accrocher(p(10, 0, 0), &refs, TOLERANCE, LIBRE).position;
    // Le même appel, la liste retournée : le résultat ne doit pas changer.
    let inverse = [coin(12, 0, 0), coin(8, 0, 0)];
    assert_eq!(
        accrocher(p(10, 0, 0), &inverse, TOLERANCE, LIBRE).position,
        attendu
    );
    assert_eq!(attendu.x, 8, "et c'est la plus petite coordonnée");
}

// ── le verrou d'axe ─────────────────────────────────────────────────────────

/// **Un verrou l'emporte sur TOUT**, y compris sur une référence plus proche.
///
/// Il a été demandé explicitement — c'est le maintien d'une direction dans
/// SketchUp. Un verrou qu'une accroche peut défaire n'est pas un verrou, et
/// c'est précisément quand on verrouille qu'on a besoin d'être sûr.
#[test]
fn un_verrou_l_emporte_sur_une_reference_plus_proche() {
    let refs = [coin(10, 0, 0)];
    let a = accrocher(p(10, 0, 0), &refs, TOLERANCE, [Some(99), None, None]);
    assert_eq!(a.position.x, 99, "le verrou gagne malgré l'accroche exacte");
    assert_eq!(a.raisons[0].unwrap().ecart, 89);
    // Les autres axes restent libres d'accrocher.
    let b = accrocher(p(10, 1, 0), &refs, TOLERANCE, [Some(99), None, None]);
    assert_eq!(b.position, p(99, 0, 0));
}

// ── les points remarquables d'une boîte ─────────────────────────────────────

/// Huit coins, douze milieux d'arête, un centre — et rien en double.
#[test]
fn une_boite_rend_ses_vingt_et_un_points() {
    let refs = BBox::new(p(0, 0, 0), p(15, 15, 15)).references();
    assert_eq!(refs.len(), 21);
    let coins: Vec<_> = refs.iter().filter(|r| r.genre == Ancre::Coin).collect();
    assert_eq!(coins.len(), 8);
    assert_eq!(refs.iter().filter(|r| r.genre == Ancre::Milieu).count(), 12);
    assert_eq!(refs.iter().filter(|r| r.genre == Ancre::Centre).count(), 1);

    let points: std::collections::BTreeSet<(i32, i32, i32)> = refs
        .iter()
        .map(|r| (r.point.x, r.point.y, r.point.z))
        .collect();
    assert_eq!(points.len(), 21, "aucun point en double");
    // Les huit coins sont bien les huit combinaisons.
    for c in [0, 15] {
        for b in [0, 15] {
            for a in [0, 15] {
                assert!(points.contains(&(a, b, c)), "coin ({a},{b},{c}) manquant");
            }
        }
    }
}

/// **Le milieu porte sur le SOLIDE, pas sur les indices de blocs.**
///
/// Une section va de 0 à 15 inclus, donc de 0 à 16 en géométrie : son milieu
/// est 8, pas 7. Prendre le milieu des indices tomberait à côté du centre
/// visuel — d'un demi-bloc, donc toujours du même côté, ce qui décentre tout
/// ce qu'on construit par symétrie.
#[test]
fn le_milieu_porte_sur_le_solide() {
    let refs = BBox::new(p(0, 0, 0), p(15, 15, 15)).references();
    let centre = refs.iter().find(|r| r.genre == Ancre::Centre).unwrap();
    assert_eq!(centre.point, p(8, 8, 8), "le milieu de 0..16, pas de 0..15");
}

/// **Le milieu d'une arête PAIRE n'existe pas, et le choix doit être STABLE.**
///
/// Une arête de quatre blocs n'a pas de bloc central. On prend toujours le
/// même côté ; alterner selon la parité ferait sauter l'accroche d'un bloc
/// quand on redimensionne, ce qui se lit « le milieu bouge tout seul ».
#[test]
fn le_milieu_d_une_arete_paire_est_stable() {
    // 0..3 inclus : solide de 0 à 4, milieu 2.
    let a = BBox::new(p(0, 0, 0), p(3, 0, 0)).references();
    let c = a.iter().find(|r| r.genre == Ancre::Centre).unwrap();
    assert_eq!(c.point.x, 2);
    // 0..4 inclus : solide de 0 à 5, milieu 2 (plancher de 2,5).
    let b = BBox::new(p(0, 0, 0), p(4, 0, 0)).references();
    let c = b.iter().find(|r| r.genre == Ancre::Centre).unwrap();
    assert_eq!(c.point.x, 2, "plancher, jamais un arrondi qui alterne");
    // Du côté négatif aussi — la division est euclidienne.
    let n = BBox::new(p(-4, 0, 0), p(-1, 0, 0)).references();
    let c = n.iter().find(|r| r.genre == Ancre::Centre).unwrap();
    assert_eq!(c.point.x, -2);
}

/// Un débordement ne rend pas une référence à l'autre bout du monde.
///
/// La première écriture bornait `r.point.x` entre `i32::MIN` et `i32::MAX` —
/// une assertion VACUE, toujours vraie par le type, et clippy l'a dit. Ce qui
/// se vérifie est le seul endroit où la somme pouvait déborder : le MILIEU du
/// domaine entier. En `i32`, `MIN + MAX + 1` s'enroule et le centre part à
/// l'opposé.
#[test]
fn le_milieu_du_domaine_entier_ne_deborde_pas() {
    let refs = BBox::new(p(i32::MIN, 0, 0), p(i32::MAX, 0, 0)).references();
    let c = refs.iter().find(|r| r.genre == Ancre::Centre).unwrap();
    assert!(
        c.point.x.abs() < 2,
        "le milieu du domaine doit être près de zéro, il est à {}",
        c.point.x
    );
    // Et les coins sont bien les bornes, pas des valeurs enroulées.
    let xs: std::collections::BTreeSet<i32> = refs
        .iter()
        .filter(|r| r.genre == Ancre::Coin)
        .map(|r| r.point.x)
        .collect();
    assert_eq!(xs.into_iter().collect::<Vec<_>>(), vec![i32::MIN, i32::MAX]);
}

/// Un écart entre deux points opposés du domaine ne déborde pas non plus —
/// il est calculé en `i64`.
#[test]
fn l_ecart_ne_deborde_pas() {
    let a = accrocher(p(i32::MAX, 0, 0), &[coin(i32::MIN, 0, 0)], TOLERANCE, LIBRE);
    assert_eq!(a.position.x, i32::MAX, "bien trop loin pour accrocher");
    assert!(a.raisons[0].is_none());
}

// ── le geste complet ────────────────────────────────────────────────────────

/// **Aligner une nouvelle façade sur le coin d'un mur existant.**
///
/// C'est le geste que l'inférence existe pour servir, et il tient en un
/// appel : on vise à peu près, les axes proches d'un coin bâti s'alignent, et
/// l'outil DIT lequel — de quoi dessiner le trait pointillé de SketchUp.
#[test]
fn aligner_une_facade_sur_un_mur_existant() {
    let mur = BBox::new(p(0, 64, 0), p(0, 80, 31));
    let refs = mur.references();
    // On vise près du coin haut du mur, à deux blocs près.
    let a = accrocher(p(42, 81, 2), &refs, TOLERANCE, LIBRE);
    assert_eq!(a.position.y, 80, "la hauteur du mur");
    assert_eq!(a.position.z, 0, "et son nu en z");
    assert_eq!(
        a.position.x, 42,
        "x reste libre : on s'éloigne dans cette direction"
    );
    assert_eq!(a.axes_accroches(), 2, "une DROITE le long de x");
    // Et on peut le dire à l'utilisateur.
    let r = a.raisons[1].unwrap();
    assert_eq!(r.genre.nom(), "coin");
    assert_eq!(r.reference.y, 80);
}
