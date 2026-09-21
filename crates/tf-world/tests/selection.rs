//! La sélection : deux coins, les gestes qui les bougent, et la face qu'on
//! attrape pour la tirer.

use tf_world::coords::{BBox, BlockPos};
use tf_world::decoupe::Niveau;
use tf_world::selection::{Direction, Selection, DIRECTIONS};

fn p(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(x, y, z)
}

fn sel(a: (i32, i32, i32), b: (i32, i32, i32)) -> Selection {
    let mut s = Selection::nouvelle();
    s.poser_coin1(p(a.0, a.1, a.2));
    s.poser_coin2(p(b.0, b.1, b.2));
    s
}

// ── les deux coins ──────────────────────────────────────────────────────────

/// **Un seul coin n'est PAS une sélection d'un bloc.**
///
/// Rendre une boîte dès le premier clic laisserait une opération partir sur
/// une case choisie à moitié — et `//set` sur un bloc ne ressemble pas assez à
/// une erreur pour qu'on la remarque.
#[test]
fn un_seul_coin_ne_fait_pas_de_selection() {
    let mut s = Selection::nouvelle();
    assert_eq!(s.boite(), None);
    s.poser_coin1(p(4, 5, 6));
    assert_eq!(s.boite(), None, "un coin ne suffit pas");
    s.poser_coin2(p(4, 5, 6));
    assert_eq!(s.boite(), Some(BBox::single(p(4, 5, 6))), "deux, oui");
}

/// Les coins se posent dans n'importe quel ordre : ce sont deux clics, pas
/// un minimum et un maximum.
#[test]
fn les_coins_se_posent_dans_n_importe_quel_ordre() {
    let a = sel((10, 20, 30), (0, 5, 7));
    let b = sel((0, 5, 7), (10, 20, 30));
    assert_eq!(a.boite(), b.boite());
    assert_eq!(a.boite().unwrap().min, p(0, 5, 7));
}

/// Reposer un coin remplace ce coin-là et garde l'autre — c'est ce qu'un
/// deuxième clic gauche doit faire, sinon il faudrait tout recommencer.
#[test]
fn reposer_un_coin_garde_l_autre() {
    let mut s = sel((0, 0, 0), (10, 10, 10));
    s.poser_coin1(p(-5, 0, 0));
    assert_eq!(s.boite().unwrap(), BBox::new(p(-5, 0, 0), p(10, 10, 10)));
}

/// `etendre_a` n'enlève jamais rien. Le premier point pose les deux coins :
/// une sélection d'un bloc est légitime quand on l'a DEMANDÉE, contrairement
/// à une moitié de geste.
#[test]
fn etendre_n_enleve_jamais_rien() {
    let mut s = Selection::nouvelle();
    s.etendre_a(p(5, 5, 5));
    assert_eq!(s.boite(), Some(BBox::single(p(5, 5, 5))));
    s.etendre_a(p(-2, 9, 5));
    let b = s.boite().unwrap();
    assert!(b.contains(p(5, 5, 5)) && b.contains(p(-2, 9, 5)));
    assert_eq!(b, BBox::new(p(-2, 5, 5), p(5, 9, 5)));
}

// ── agrandir et rétrécir ────────────────────────────────────────────────────

/// `//expand` pousse UNE face et laisse les cinq autres où elles sont.
#[test]
fn agrandir_ne_bouge_qu_une_face() {
    for d in DIRECTIONS {
        let mut s = sel((0, 0, 0), (10, 10, 10));
        let avant = s.boite().unwrap();
        assert!(s.agrandir(d, 3));
        let apres = s.boite().unwrap();
        let k = d.axe();
        let (a0, a1) = (
            [avant.min.x, avant.min.y, avant.min.z],
            [avant.max.x, avant.max.y, avant.max.z],
        );
        let (b0, b1) = (
            [apres.min.x, apres.min.y, apres.min.z],
            [apres.max.x, apres.max.y, apres.max.z],
        );
        for j in 0..3 {
            if j == k {
                continue;
            }
            assert_eq!((a0[j], a1[j]), (b0[j], b1[j]), "{d:?} a bougé l'axe {j}");
        }
        if d.positif() {
            assert_eq!(b1[k], a1[k] + 3);
            assert_eq!(b0[k], a0[k]);
        } else {
            assert_eq!(b0[k], a0[k] - 3);
            assert_eq!(b1[k], a1[k]);
        }
    }
}

/// **Une face ne traverse JAMAIS la face opposée.**
///
/// Contracter au-delà laisserait une boîte retournée, que `BBox::new`
/// normaliserait sans rien dire : la sélection couvrirait brusquement l'autre
/// côté. On s'arrête à un bloc d'épaisseur, comme WorldEdit.
#[test]
fn retrecir_s_arrete_a_un_bloc_et_ne_se_retourne_pas() {
    for d in DIRECTIONS {
        let mut s = sel((0, 0, 0), (10, 10, 10));
        s.agrandir(d, -1000);
        let b = s.boite().unwrap();
        let (sx, sy, sz) = b.size();
        assert!(sx >= 1 && sy >= 1 && sz >= 1, "{d:?} : boîte vide {b:?}");
        let k = d.axe();
        assert_eq!(
            [sx, sy, sz][k],
            1,
            "{d:?} : l'axe écrasé doit valoir 1, pas {:?}",
            [sx, sy, sz]
        );
        // Et elle n'a pas sauté de l'autre côté.
        assert!(
            b.min.x >= 0 && b.min.y >= 0 && b.min.z >= 0,
            "{d:?} : {b:?}"
        );
        assert!(
            b.max.x <= 10 && b.max.y <= 10 && b.max.z <= 10,
            "{d:?} : {b:?}"
        );
    }
}

/// Un geste qui ne change rien le DIT. Une interface qui ne sait pas
/// distinguer « rien à faire » de « fait » propose des boutons qui ont l'air
/// cassés.
#[test]
fn un_geste_sans_effet_rend_faux() {
    let mut vide = Selection::nouvelle();
    assert!(!vide.agrandir(Direction::PlusX, 5));
    assert!(!vide.deplacer([1, 0, 0]));
    assert!(!vide.aligner(Niveau::Chunk));

    let mut s = sel((0, 0, 0), (10, 10, 10));
    assert!(!s.agrandir(Direction::PlusX, 0), "zéro ne change rien");
    assert!(s.agrandir(Direction::PlusX, 1));
    // Déjà alignée : aligner ne fait rien et le dit.
    let mut a = sel((0, 0, 0), (15, 10, 15));
    assert!(!a.aligner(Niveau::Chunk));
}

/// **`//expand` près de `i32::MAX` ne ramène pas la face de l'autre côté du
/// monde.** Un débordement silencieux enverrait l'opération suivante écrire
/// à l'opposé, sans la moindre erreur.
#[test]
fn agrandir_ne_deborde_pas() {
    let mut s = sel((i32::MAX - 5, 0, 0), (i32::MAX - 1, 10, 10));
    s.agrandir(Direction::PlusX, i32::MAX);
    let b = s.boite().unwrap();
    assert!(b.max.x >= b.min.x, "boîte retournée : {b:?}");
    assert_eq!(b.max.x, i32::MAX);

    let mut s = sel((i32::MIN + 1, 0, 0), (i32::MIN + 5, 10, 10));
    s.agrandir(Direction::MoinsX, i32::MAX);
    let b = s.boite().unwrap();
    assert!(b.max.x >= b.min.x);
    assert_eq!(b.min.x, i32::MIN);
}

/// Déplacer garde la TAILLE. Une sélection qui change de taille en se
/// déplaçant est une sélection dont on ne peut plus rien prévoir.
#[test]
fn deplacer_garde_la_taille() {
    let mut s = sel((0, 0, 0), (10, 4, 7));
    let avant = s.boite().unwrap().size();
    assert!(s.deplacer([-100, 20, 3]));
    assert_eq!(s.boite().unwrap().size(), avant);
    assert_eq!(s.boite().unwrap().min, p(-100, 20, 3));
}

// ── le solide contre les bornes incluses ────────────────────────────────────

/// **`max + 1` : le bloc `max` occupe une case entière.**
///
/// Confondre les bornes incluses d'une `BBox` avec la géométrie du solide
/// rend la dernière rangée de blocs impossible à attraper — ce qui se lit
/// « le bord de la sélection ne répond pas » et ne désigne pas la cause.
#[test]
fn le_solide_deborde_d_un_bloc_les_bornes_incluses() {
    let b = BBox::new(p(0, 0, 0), p(15, 15, 15));
    let (min, max) = b.coins();
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_eq!(max, [16.0, 16.0, 16.0], "une section fait 16 d'arête");
    // Et un bloc seul fait bien une unité.
    let (a, z) = BBox::single(p(-3, 7, 2)).coins();
    assert_eq!(a, [-3.0, 7.0, 2.0]);
    assert_eq!(z, [-2.0, 8.0, 3.0]);
}

// ── la face qu'on attrape ───────────────────────────────────────────────────

/// **On attrape la face par laquelle on ENTRE.**
///
/// C'est le geste de pousser-tirer : viser une paroi et la déplacer. La face
/// d'entrée est celle du côté d'où l'on vient — en allant vers +X on
/// rencontre d'abord la face −X. L'inverser ferait tirer la paroi opposée,
/// donc traverser la sélection.
#[test]
fn on_attrape_la_face_par_laquelle_on_entre() {
    let s = sel((0, 0, 0), (15, 15, 15));
    let cas: [([f32; 3], [f32; 3], Direction); 6] = [
        ([-10.0, 8.0, 8.0], [1.0, 0.0, 0.0], Direction::MoinsX),
        ([30.0, 8.0, 8.0], [-1.0, 0.0, 0.0], Direction::PlusX),
        ([8.0, -10.0, 8.0], [0.0, 1.0, 0.0], Direction::MoinsY),
        ([8.0, 30.0, 8.0], [0.0, -1.0, 0.0], Direction::PlusY),
        ([8.0, 8.0, -10.0], [0.0, 0.0, 1.0], Direction::MoinsZ),
        ([8.0, 8.0, 30.0], [0.0, 0.0, -1.0], Direction::PlusZ),
    ];
    for (o, d, attendue) in cas {
        let (f, t) = s
            .face_visee(o, d)
            .unwrap_or_else(|| panic!("depuis {o:?} vers {d:?} : rien"));
        assert_eq!(f, attendue, "depuis {o:?} vers {d:?}");
        assert!(t > 0.0, "la distance doit être devant : {t}");
    }
}

/// **Depuis l'INTÉRIEUR, c'est la face de sortie** — celle qu'on regarde.
/// Rendre « rien » y serait faux : on voit bien une paroi, et un outil qui
/// refuse d'attraper ce qu'on voit passe pour cassé.
#[test]
fn depuis_l_interieur_on_attrape_la_paroi_qu_on_regarde() {
    let s = sel((0, 0, 0), (15, 15, 15));
    let (f, t) = s.face_visee([8.0, 8.0, 8.0], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(f, Direction::PlusX, "en regardant l'Est depuis dedans");
    assert!((t - 8.0).abs() < 1e-4, "distance {t}");
    let (f, _) = s.face_visee([8.0, 8.0, 8.0], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(f, Direction::MoinsY);
}

/// Ce qui est DERRIÈRE ne se vise pas. Un rayon qui s'éloigne de la boîte
/// n'attrape rien, même si la droite qui le porte la traverse.
#[test]
fn ce_qui_est_derriere_ne_se_vise_pas() {
    let s = sel((0, 0, 0), (15, 15, 15));
    assert!(s.face_visee([-10.0, 8.0, 8.0], [-1.0, 0.0, 0.0]).is_none());
    assert!(s.face_visee([100.0, 8.0, 8.0], [1.0, 0.0, 0.0]).is_none());
}

/// Un rayon qui passe à côté ne touche rien — et un rayon PARALLÈLE à une
/// paire de plans se décide par sa position, pas par une division.
#[test]
fn un_rayon_a_cote_ne_touche_rien() {
    let s = sel((0, 0, 0), (15, 15, 15));
    // Parallèle à X, mais hors de la tranche en Y.
    assert!(s.face_visee([-10.0, 100.0, 8.0], [1.0, 0.0, 0.0]).is_none());
    // Oblique, mais qui rate la boîte.
    assert!(s.face_visee([-10.0, 8.0, 8.0], [1.0, 10.0, 0.0]).is_none());
    // Et rasant DANS la tranche : il touche.
    assert!(s.face_visee([-10.0, 0.5, 8.0], [1.0, 0.0, 0.0]).is_some());
}

/// Une entrée dégénérée ne vise rien — et une sélection absente non plus.
#[test]
fn une_entree_degeneree_ne_vise_rien() {
    let s = sel((0, 0, 0), (15, 15, 15));
    assert!(s
        .face_visee([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0])
        .is_none());
    assert!(s
        .face_visee([-1.0, 8.0, 8.0], [f32::NAN, 0.0, 0.0])
        .is_none());
    assert!(Selection::nouvelle()
        .face_visee([0.0; 3], [1.0, 0.0, 0.0])
        .is_none());
}

/// **La dernière rangée de blocs s'attrape.** C'est le piège des bornes
/// incluses : avec `max` au lieu de `max + 1`, un rayon qui vise le bloc 15
/// d'une section passerait juste derrière la paroi.
#[test]
fn la_derniere_rangee_de_blocs_s_attrape() {
    let s = sel((0, 0, 0), (15, 15, 15));
    // Droit sur le centre du bloc 15 en Y, depuis le dessus.
    let (f, _) = s
        .face_visee([8.5, 30.0, 8.5], [0.0, -1.0, 0.0])
        .expect("le dessus du bloc 15 doit être atteignable");
    assert_eq!(f, Direction::PlusY);
    // Et le rayon touche bien à y = 16, pas à y = 15.
    let (_, t) = s.face_visee([8.5, 30.0, 8.5], [0.0, -1.0, 0.0]).unwrap();
    assert!((t - 14.0).abs() < 1e-4, "distance {t} au lieu de 14");
}

// ── le geste complet ────────────────────────────────────────────────────────

/// **Pousser-tirer, de bout en bout.** On vise une paroi, on la tire de trois
/// blocs, et c'est bien CETTE paroi qui a bougé. C'est le premier tiers de
/// SketchUp, et il tient en deux appels parce que la sélection et le
/// remplissage existaient déjà.
#[test]
fn pousser_tirer_une_paroi_visee() {
    let mut s = sel((0, 0, 0), (15, 15, 15));
    let (face, _) = s.face_visee([8.0, 8.0, 40.0], [0.0, 0.0, -1.0]).unwrap();
    assert_eq!(face, Direction::PlusZ);
    assert!(s.agrandir(face, 3));
    let b = s.boite().unwrap();
    assert_eq!(b.max.z, 18, "la paroi visée a avancé de trois");
    assert_eq!(b.min.z, 0, "l'opposée n'a pas bougé");
    // Et la partie NEUVE est exactement ce qu'un //set doit remplir.
    let neuve = BBox::new(
        BlockPos::new(b.min.x, b.min.y, 16),
        BlockPos::new(b.max.x, b.max.y, 18),
    );
    assert_eq!(neuve.volume(), 16 * 16 * 3);
}

// ── le verrou de pose ───────────────────────────────────────────────────────

/// **L'axe de la face contre laquelle on pose est décidé par la POSE, pas par
/// l'inférence.**
///
/// Sans ce verrou, poser contre une paroi ramène le bloc DEDANS : la paroi
/// est à un bloc, donc dans la tolérance de l'accrochage, et l'axe s'aligne
/// dessus. L'accroche est juste, la pose aussi ; c'est leur composition qui
/// ne l'est pas — et seule une jonction pouvait le montrer.
#[test]
fn le_verrou_de_pose_ne_bloque_que_l_axe_de_la_face() {
    let pose = p(1, 78, 10);
    for d in DIRECTIONS {
        let v = d.verrou(pose);
        let k = d.axe();
        assert_eq!(
            v[k],
            Some([pose.x, pose.y, pose.z][k]),
            "{d:?} doit verrouiller l'axe {k}"
        );
        for (j, verrou) in v.iter().enumerate() {
            if j != k {
                assert!(verrou.is_none(), "{d:?} ne doit PAS verrouiller l'axe {j}");
            }
        }
    }
}

/// Les deux axes LIBRES sont ceux qui portent tout l'intérêt : c'est dans le
/// plan de la face qu'on veut s'aligner sur ce qui est bâti.
#[test]
fn le_verrou_laisse_libre_le_plan_de_la_face() {
    use tf_world::inference::{accrocher, Reference, TOLERANCE};
    let pose = p(1, 78, 10);
    let refs = [Reference::new(
        p(0, 79, 12),
        tf_world::inference::Ancre::Coin,
    )];
    let a = accrocher(pose, &refs, TOLERANCE, Direction::PlusX.verrou(pose));
    assert_eq!(a.position.x, 1, "verrouillé");
    assert_eq!(a.position.y, 79, "libre, et accroché");
    assert_eq!(a.position.z, 12, "libre, et accroché");
}

/// **Les noms des six directions vivent à UN endroit.** La ligne de commande
/// en avait sa table, une coque en aurait écrit une deuxième — cinquième
/// occurrence du piège. Et c'est sur Z que ça se serait joué : +Z est le SUD,
/// ce dont personne ne se souvient.
#[test]
fn les_noms_des_directions_font_l_aller_retour() {
    use tf_world::selection::{Direction, DIRECTIONS};

    for d in DIRECTIONS {
        assert_eq!(Direction::depuis_nom(d.nom()), Some(d));
        assert_eq!(Direction::depuis_nom(&d.nom().to_uppercase()), Some(d));
    }
    // Les six noms sont distincts — deux homonymes rendraient la même
    // direction pour deux gestes opposés.
    let mut noms: Vec<&str> = DIRECTIONS.iter().map(|d| d.nom()).collect();
    noms.sort_unstable();
    noms.dedup();
    assert_eq!(noms.len(), 6);

    // Le repère, écrit en toutes lettres plutôt que déduit.
    assert_eq!(Direction::depuis_nom("est").unwrap().pas(), [1, 0, 0]);
    assert_eq!(Direction::depuis_nom("sud").unwrap().pas(), [0, 0, 1]);
    assert_eq!(Direction::depuis_nom("haut").unwrap().pas(), [0, 1, 0]);
    assert_eq!(Direction::depuis_nom("nulle part"), None);
}

/// **Le geste de pousser-tirer se mesure sur la DROITE de l'axe**, pas sur un
/// glissement d'écran multiplié par une sensibilité : celui-ci dériverait
/// selon la distance et l'angle, et personne ne sait corriger ça à l'œil.
#[test]
fn tirer_le_long_d_un_axe_compte_des_blocs() {
    use tf_world::selection::{glissement, Direction};

    let ancre = [0.0, 0.0, 0.0];
    // L'œil est à dix blocs en −Z et regarde une cible sur l'axe X.
    let oeil = [0.0, 0.0, -10.0];
    let vers = |cible: [f32; 3]| [cible[0] - oeil[0], cible[1] - oeil[1], cible[2] - oeil[2]];

    // Viser le point x = 5 sur l'axe doit rendre 5.
    assert_eq!(
        glissement(ancre, Direction::PlusX, oeil, vers([5.0, 0.0, 0.0])),
        Some(5)
    );
    // Et de l'autre côté, un négatif — tirer vers l'arrière RAMÈNE la face.
    assert_eq!(
        glissement(ancre, Direction::PlusX, oeil, vers([-3.0, 0.0, 0.0])),
        Some(-3)
    );
    // L'axe compte, pas seulement la position visée.
    assert_eq!(
        glissement(ancre, Direction::PlusY, oeil, vers([0.0, 7.0, 0.0])),
        Some(7)
    );
    // Un demi-bloc s'arrondit au plus proche : tronquer collerait le geste à
    // zéro sur toute la première moitié du premier bloc.
    assert_eq!(
        glissement(ancre, Direction::PlusX, oeil, vers([1.6, 0.0, 0.0])),
        Some(2)
    );
}

/// **Un rayon colinéaire à l'axe ne donne AUCUN nombre.** La projection part à
/// l'infini : un pixel de souris vaudrait des centaines de blocs, et la face
/// sauterait à l'autre bout du monde. Refuser, c'est ne pas bouger — pas
/// bouger n'importe comment.
#[test]
fn un_rayon_dans_l_axe_ne_donne_pas_de_tirage() {
    use tf_world::selection::{glissement, Direction};

    let ancre = [0.0, 0.0, 0.0];
    // L'œil est SUR l'axe X et regarde le long de l'axe X.
    assert_eq!(
        glissement(ancre, Direction::PlusX, [-10.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        None
    );
    // Presque colinéaire : refusé aussi, et c'est le point — à un degré près
    // de l'axe, un pixel vaut déjà des dizaines de blocs.
    let presque = [1.0, 0.01, 0.0];
    assert_eq!(
        glissement(ancre, Direction::PlusX, [-10.0, 0.0, 0.0], presque),
        None
    );
    // Une direction nulle, et des valeurs non finies : refusées, pas
    // propagées. `NaN` ne plante pas, il déplace.
    assert_eq!(
        glissement(ancre, Direction::PlusX, [0.0; 3], [0.0; 3]),
        None
    );
    assert_eq!(
        glissement(
            ancre,
            Direction::PlusX,
            [f32::NAN, 0.0, 0.0],
            [0.0, 0.0, 1.0]
        ),
        None
    );
}

/// Le geste complet, en une propriété : attraper une face et tirer de n doit
/// donner la même sélection que `//expand n` sur cette face.
#[test]
fn tirer_une_face_vaut_un_expand_de_la_meme_face() {
    use tf_world::coords::BlockPos;
    use tf_world::selection::{glissement, Selection};

    let mut s = Selection::nouvelle();
    s.poser_coin1(BlockPos::new(0, 0, 0));
    s.poser_coin2(BlockPos::new(9, 9, 9));

    // On vise la face EST (+X) depuis l'extérieur, en regardant vers −X.
    let oeil = [30.0, 5.0, 5.0];
    let dir = [-1.0, 0.0, 0.0];
    let (face, _) = s.face_visee(oeil, dir).expect("la face est visée");
    assert_eq!(face, tf_world::selection::Direction::PlusX);

    // Puis on tire de 4 blocs vers l'est, la souris visant x = 14.
    let ancre = [10.0, 5.0, 5.0];
    let oeil2 = [5.0, 5.0, -30.0];
    let vise = [14.0 - oeil2[0], 5.0 - oeil2[1], 5.0 - oeil2[2]];
    let n = glissement(ancre, face, oeil2, vise).unwrap();
    assert_eq!(n, 4);

    let mut attendu = Selection::nouvelle();
    attendu.poser_coin1(BlockPos::new(0, 0, 0));
    attendu.poser_coin2(BlockPos::new(9, 9, 9));
    assert!(attendu.agrandir(face, n));
    assert!(s.agrandir(face, n));
    assert_eq!(s.boite(), attendu.boite());
    assert_eq!(s.boite().unwrap().max.x, 13);
}

/// **Un pousser-tirer n'écrit QUE sa tranche.** Tirer une face de trois blocs
/// sur un bâtiment de cent mille ne doit pas réécrire le bâtiment : c'est
/// l'invariant « une opération ne paie que sa portée », appliqué au geste.
#[test]
fn la_tranche_d_un_tirage_est_ce_qui_s_ajoute() {
    use tf_world::coords::{BBox, BlockPos};
    use tf_world::selection::{tranche, Direction, Selection};

    let base = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(9, 9, 9));
    let etendre = |d: Direction, n: i32| {
        let mut s = Selection::nouvelle();
        s.poser_coin1(base.min);
        s.poser_coin2(base.max);
        assert!(s.agrandir(d, n));
        s.boite().unwrap()
    };

    // Tiré de 3 vers l'est : la tranche va de x = 10 à 12 — **pas de 9**.
    // Les bornes sont INCLUSES, et un bloc d'écart écraserait la dernière
    // rangée de l'ancien volume.
    let t = tranche(base, etendre(Direction::PlusX, 3), Direction::PlusX).unwrap();
    assert_eq!(
        t,
        BBox::new(BlockPos::new(10, 0, 0), BlockPos::new(12, 9, 9))
    );

    // Poussé de 3 : ce qui vient d'en SORTIR, x = 7 à 9.
    let t = tranche(base, etendre(Direction::PlusX, -3), Direction::PlusX).unwrap();
    assert_eq!(t, BBox::new(BlockPos::new(7, 0, 0), BlockPos::new(9, 9, 9)));

    // La face négative, symétrique : tirer vers l'ouest ajoute x = −3 à −1.
    let t = tranche(base, etendre(Direction::MoinsX, 3), Direction::MoinsX).unwrap();
    assert_eq!(
        t,
        BBox::new(BlockPos::new(-3, 0, 0), BlockPos::new(-1, 9, 9))
    );

    // Et sur un autre axe, pour que le test ne passe pas par accident sur X.
    let t = tranche(base, etendre(Direction::PlusY, 2), Direction::PlusY).unwrap();
    assert_eq!(
        t,
        BBox::new(BlockPos::new(0, 10, 0), BlockPos::new(9, 11, 9))
    );

    // Rien n'a bougé : rien à écrire. Rendre une tranche vide ferait une
    // entrée de journal pour zéro changement.
    assert_eq!(tranche(base, base, Direction::PlusX), None);
}
