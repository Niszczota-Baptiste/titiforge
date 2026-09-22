//! **Ce que la caméra demande, et dans quel ordre.**
//!
//! Une décision pure : elle se vérifie sans disque, sans GPU et sans horloge.
//! Ce qu'elle décide n'est pas cosmétique — une région bâtie pèse 186 Mo
//! résidents et met 867 ms à charger (`tf-app --example residence`), donc on
//! n'ira jamais au bout de la liste. L'ORDRE est la fonctionnalité.

use tf_world::coords::BlockPos;
use tf_world::demande::{par_region, planifier, voulues, Voulue};
use tf_world::{Cellule, Niveau, RAYON_MAX};

/// Regard vers l'est (+X), le repère Minecraft du dépôt.
const EST: [f32; 3] = [1.0, 0.0, 0.0];
const HAUTEUR: (i32, i32) = (-64, 319);

fn au_chunk(oeil: BlockPos, regard: [f32; 3], rayon: u32) -> Vec<Voulue> {
    voulues(oeil, regard, rayon, Niveau::Chunk, HAUTEUR)
}

/// La cellule où l'on se tient est toujours la première : c'est la seule dont
/// on est SÛR qu'elle est visible, quelle que soit la direction du regard.
#[test]
fn la_cellule_de_l_oeil_passe_devant_tout() {
    for regard in [EST, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.3, -0.9, 0.3]] {
        let v = au_chunk(BlockPos::new(8, 64, 8), regard, 4);
        assert_eq!(
            (v[0].cellule.x, v[0].cellule.z),
            (0, 0),
            "regard {regard:?} : la cellule de l'œil doit être première"
        );
        assert_eq!(v[0].score, 0.0, "elle est à distance nulle");
    }
}

/// **À distance égale, ce qui est devant passe avant ce qui est derrière.**
/// C'est toute la raison d'être du score : en volant vers l'est, on a besoin
/// de l'est, et le dos peut attendre.
#[test]
fn devant_passe_avant_derriere_a_distance_egale() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 6);
    let rang = |x: i32, z: i32| {
        v.iter()
            .position(|w| w.cellule.x == x && w.cellule.z == z)
            .unwrap_or_else(|| panic!("cellule ({x}, {z}) absente de la demande"))
    };
    for d in 1..=6 {
        assert!(
            rang(d, 0) < rang(-d, 0),
            "à {d} cellules, l'est (devant) doit passer avant l'ouest (dos)"
        );
        // Et le côté est entre les deux : le score est continu, pas un
        // classement en deux camps.
        assert!(
            rang(d, 0) < rang(0, d) && rang(0, d) < rang(-d, 0),
            "à {d} cellules : devant < côté < dos"
        );
    }
}

/// Ce qui est plus proche ET devant passe toujours avant ce qui est plus loin
/// et devant. Le score ne doit pas inverser la distance sur un même cap.
#[test]
fn sur_un_meme_cap_le_proche_passe_avant_le_loin() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 8);
    let mut precedent = -1i32;
    for d in 1..=8 {
        let rang = v
            .iter()
            .position(|w| w.cellule.x == d && w.cellule.z == 0)
            .expect("cellule sur le cap") as i32;
        assert!(
            rang > precedent,
            "à {d} cellules droit devant, le rang doit croître avec la distance"
        );
        precedent = rang;
    }
}

/// **Un DISQUE, pas un carré** : les coins sont jetés.
///
/// Ce n'est pas cosmétique. Un carré de rayon 8 porte 289 cellules, un disque
/// 201 — 30 % de chargement en moins pour le même horizon. Et un disque est
/// invariant par rotation, donc tourner sur place ne fait entrer ni sortir
/// personne.
#[test]
fn la_demande_est_un_disque_et_les_coins_sont_jetes() {
    let r = 8;
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, r);
    let coin = v
        .iter()
        .any(|w| w.cellule.x == r as i32 && w.cellule.z == r as i32);
    assert!(
        !coin,
        "le coin du carré est hors du disque, il ne doit pas être demandé"
    );
    assert!(
        v.iter()
            .any(|w| w.cellule.x == r as i32 && w.cellule.z == 0),
        "le bord du disque sur l'axe doit y être"
    );
    let carre = (2 * r as usize + 1).pow(2);
    assert!(
        v.len() < carre * 8 / 10,
        "un disque doit peser nettement moins qu'un carré : {} contre {carre}",
        v.len()
    );
}

/// **Tourner sur place ne change pas l'ENSEMBLE demandé**, seulement son
/// ordre. Sur une fenêtre qui évince, un ensemble qui bouge quand la caméra
/// tourne est du chargement jeté à chaque coup d'œil.
#[test]
fn tourner_sur_place_ne_change_que_l_ordre() {
    let oeil = BlockPos::new(8, 64, 8);
    let cles = |regard| {
        let mut k: Vec<(i32, i32)> = au_chunk(oeil, regard, 6)
            .iter()
            .map(|w| (w.cellule.x, w.cellule.z))
            .collect();
        k.sort();
        k
    };
    let est = cles(EST);
    for regard in [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.7, 0.0, -0.7]] {
        assert_eq!(
            cles(regard),
            est,
            "regard {regard:?} : l'ensemble doit être le même"
        );
    }
    // …et l'ordre, lui, change vraiment. Sans quoi le score ne servirait à rien.
    let a: Vec<_> = au_chunk(oeil, EST, 6)
        .iter()
        .map(|w| (w.cellule.x, w.cellule.z))
        .collect();
    let b: Vec<_> = au_chunk(oeil, [-1.0, 0.0, 0.0], 6)
        .iter()
        .map(|w| (w.cellule.x, w.cellule.z))
        .collect();
    assert_ne!(a, b, "faire demi-tour doit réordonner la demande");
}

/// **Un regard vertical n'a pas de direction horizontale**, et une cellule est
/// une COLONNE. On classe alors à la distance seule — pas en `NaN`, qui ne
/// plante pas : il se propage dans la comparaison, répond `false` dans les
/// deux sens, et rend un ordre arbitraire que rien ne signale.
#[test]
fn un_regard_degenere_classe_a_la_distance_seule() {
    for regard in [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], [f32::NAN; 3]] {
        let v = au_chunk(BlockPos::new(8, 64, 8), regard, 5);
        assert!(
            !v.is_empty(),
            "regard {regard:?} : la demande ne doit pas être vide"
        );
        for w in &v {
            assert!(w.score.is_finite(), "regard {regard:?} : score non fini");
            assert!(w.devant.is_finite(), "regard {regard:?} : cap non fini");
        }
        // Les distances sont croissantes : c'est ce que « à la distance
        // seule » veut dire, et c'est vérifiable sans connaître le score.
        for p in v.windows(2) {
            assert!(
                p[0].distance <= p[1].distance + 1e-4,
                "regard {regard:?} : les distances doivent croître"
            );
        }
    }
}

/// L'ordre est TOTAL et déterministe. Deux appels identiques doivent rendre
/// la même liste : un ordre qui dépendrait du hasard d'une table donnerait
/// deux chargements différents de la même scène.
#[test]
fn l_ordre_est_deterministe() {
    let oeil = BlockPos::new(100, 64, -250);
    let a = au_chunk(oeil, [0.4, 0.2, -0.9], 7);
    let b = au_chunk(oeil, [0.4, 0.2, -0.9], 7);
    assert_eq!(a, b);
    // **À score égal, `(z, x)` départage — et ça se vérifie EN GRAND.**
    // Écrit d'abord avec un rayon de 3, ce contrôle ne décidait rien : sur une
    // trentaine d'éléments le tri retombe sur une insertion, qui est stable,
    // donc les ex æquo gardaient l'ordre d'émission de `cellules_autour` —
    // lequel est justement (z, x). La mutation qui retirait le départage
    // survivait. À quelques milliers de cellules, c'est le comparateur seul
    // qui décide, et la ligne redevient porteuse.
    let v = au_chunk(BlockPos::new(8, 64, 8), [0.0, 1.0, 0.0], 40);
    assert!(
        v.len() > 2000,
        "il faut de quoi sortir du tri par insertion"
    );
    for p in v.windows(2) {
        assert!(
            p[0].score < p[1].score
                || (p[0].cellule.z, p[0].cellule.x) < (p[1].cellule.z, p[1].cellule.x),
            "à score égal, (z, x) doit départager"
        );
    }
}

/// La sortie est bornée par construction : le rayon est plafonné, donc aucune
/// entrée ne peut la faire exploser. Et près des bornes du monde, rien ne
/// déborde — un débordement rendrait une cellule à l'autre bout du monde,
/// qu'aucune image ne montrerait comme une erreur.
#[test]
fn la_sortie_est_bornee_et_ne_deborde_pas() {
    let enorme = au_chunk(BlockPos::new(0, 0, 0), EST, u32::MAX);
    let plafond = (2 * RAYON_MAX as usize + 1).pow(2);
    assert!(
        !enorme.is_empty() && enorme.len() <= plafond,
        "{} cellules pour un rayon démesuré",
        enorme.len()
    );
    for extreme in [i32::MIN / 2, i32::MAX / 2, i32::MIN + 16, i32::MAX - 16] {
        let v = au_chunk(BlockPos::new(extreme, 64, extreme), EST, 3);
        assert!(
            !v.is_empty(),
            "à x = {extreme}, la demande ne doit pas être vide"
        );
        for w in &v {
            assert!(w.score.is_finite(), "à x = {extreme} : score non fini");
        }
    }
}

/// Le niveau RÉGION demande la même chose à la même échelle : un rayon est en
/// cellules, pas en blocs, donc « 4 » veut dire la même chose des deux côtés.
#[test]
fn le_rayon_est_en_cellules_a_tous_les_niveaux() {
    let oeil = BlockPos::new(8, 64, 8);
    let c = voulues(oeil, EST, 4, Niveau::Chunk, HAUTEUR);
    let r = voulues(oeil, EST, 4, Niveau::Region, HAUTEUR);
    assert_eq!(
        c.len(),
        r.len(),
        "le même rayon doit demander le même NOMBRE de cellules"
    );
    assert!(
        r[0].cellule.boite.max.x - r[0].cellule.boite.min.x
            > c[0].cellule.boite.max.x - c[0].cellule.boite.min.x,
        "mais une cellule de région est plus grande"
    );
}

/// **Croiser la demande avec ce qui est là.** Les deux listes viennent du même
/// parcours : deux fonctions finiraient par ne plus être d'accord sur ce qui
/// est « voulu », et la cellule serait chargée puis jetée en boucle.
#[test]
fn le_plan_dit_quoi_charger_et_quoi_jeter() {
    let oeil = BlockPos::new(8, 64, 8);
    let v = au_chunk(oeil, EST, 3);
    let proche = v[0].cellule.clone();
    let lointaine: Cellule = voulues(BlockPos::new(8, 64, 8), EST, 1, Niveau::Chunk, HAUTEUR)
        .into_iter()
        .map(|w| w.cellule)
        .find(|c| c.x == 1 && c.z == 0)
        .expect("une voisine");
    // Une cellule résidente hors de portée : elle doit devenir jetable.
    let hors = voulues(
        BlockPos::new(8000, 64, 8000),
        EST,
        0,
        Niveau::Chunk,
        HAUTEUR,
    )
    .into_iter()
    .next()
    .expect("une cellule au loin")
    .cellule;

    let plan = planifier(
        v.clone(),
        &[proche.clone(), lointaine.clone(), hors.clone()],
    );
    assert!(
        !plan.charger.iter().any(|w| w.cellule == proche),
        "ce qui est déjà résident ne se recharge pas"
    );
    assert_eq!(
        plan.jetables,
        vec![hors],
        "seule la cellule hors demande est jetable"
    );
    assert_eq!(
        plan.charger.len(),
        v.len() - 2,
        "tout le reste est à charger"
    );
    // L'ordre d'urgence SURVIT au filtrage — sinon on chargerait le bord de
    // l'horizon avant ses pieds.
    for p in plan.charger.windows(2) {
        assert!(
            p[0].score <= p[1].score,
            "le plan doit rester trié par urgence"
        );
    }

    // Rien de résident : tout est à charger, rien n'est jetable.
    let vide = planifier(v.clone(), &[]);
    assert_eq!(vide.charger.len(), v.len());
    assert!(vide.jetables.is_empty());
}

/// **Une région n'est lue qu'UNE fois.** C'est la conclusion mesurée qui
/// décide l'unité de lecture : un chunk demandé seul coûte 4,74 ms contre
/// 0,47 ms amorti sur sa région, et servir 1 024 chunks un par un
/// gaspillerait 4,4 s par région en relectures pures.
#[test]
fn chaque_region_ne_figure_qu_une_fois() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 40);
    let lots = par_region(&v);
    let mut vues: Vec<(i32, i32)> = lots.iter().map(|l| (l.region.x, l.region.z)).collect();
    let avant = vues.len();
    vues.sort();
    vues.dedup();
    assert_eq!(avant, vues.len(), "une région apparaît deux fois");
    assert!(lots.len() > 1, "un rayon de 40 chunks déborde de sa région");
}

/// Rien ne se perd et rien ne se duplique : le groupement est une PARTITION
/// de la demande. Une cellule oubliée serait un trou dans le monde que rien
/// ne signalerait.
#[test]
fn le_groupement_partitionne_la_demande() {
    let v = au_chunk(BlockPos::new(300, 64, -700), [0.3, 0.1, -0.9], 24);
    let lots = par_region(&v);
    let total: usize = lots.iter().map(|l| l.cellules.len()).sum();
    assert_eq!(total, v.len(), "le compte doit être conservé");
    let mut dedans: Vec<(i32, i32)> = lots
        .iter()
        .flat_map(|l| l.cellules.iter().map(|w| (w.cellule.x, w.cellule.z)))
        .collect();
    let mut attendu: Vec<(i32, i32)> = v.iter().map(|w| (w.cellule.x, w.cellule.z)).collect();
    dedans.sort();
    attendu.sort();
    assert_eq!(dedans, attendu, "les mêmes cellules, ni plus ni moins");
    // Et chaque cellule est bien dans SA région — sans quoi le lot lirait le
    // mauvais fichier, ce qu'aucune image ne montrerait comme une erreur.
    for l in &lots {
        for w in &l.cellules {
            assert_eq!(
                w.cellule.region, l.region,
                "cellule rangée dans la mauvaise région"
            );
        }
    }
}

/// **Le groupement ne réordonne jamais ce que la caméra a classé.** Les lots
/// suivent leur cellule la plus pressée, et à l'intérieur d'un lot l'ordre
/// d'urgence survit.
#[test]
fn les_lots_suivent_l_urgence() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 40);
    let lots = par_region(&v);
    for p in lots.windows(2) {
        assert!(
            p[0].urgence() <= p[1].urgence(),
            "les lots doivent être triés par urgence"
        );
    }
    for l in &lots {
        for p in l.cellules.windows(2) {
            assert!(
                p[0].score <= p[1].score,
                "l'ordre d'urgence survit dans le lot"
            );
        }
    }
    // La cellule la plus urgente du monde est la première du premier lot :
    // c'est celle sous nos pieds, et elle ne doit pas attendre qu'une autre
    // région soit lue.
    assert_eq!(
        (lots[0].cellules[0].cellule.x, lots[0].cellules[0].cellule.z),
        (v[0].cellule.x, v[0].cellule.z)
    );
}

/// L'urgence d'un lot est celle de sa cellule la plus pressée — le MINIMUM,
/// pas la moyenne. Une région qui porte la cellule sous nos pieds passe devant
/// une région dont tout le contenu est à mi-distance.
#[test]
fn l_urgence_d_un_lot_est_celle_de_sa_meilleure_cellule() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 40);
    let lots = par_region(&v);
    for l in &lots {
        let mini = l
            .cellules
            .iter()
            .map(|w| w.score)
            .fold(f32::INFINITY, f32::min);
        assert!(
            (l.urgence() - mini).abs() < 1e-6,
            "l'urgence du lot doit être le minimum de ses cellules"
        );
    }
}

/// Au niveau RÉGION, une cellule EST une région : chaque lot en porte
/// exactement une, et le groupement ne coûte rien.
#[test]
fn au_niveau_region_un_lot_porte_une_cellule() {
    let v = voulues(BlockPos::new(8, 64, 8), EST, 4, Niveau::Region, HAUTEUR);
    let lots = par_region(&v);
    assert_eq!(lots.len(), v.len());
    for l in &lots {
        assert_eq!(l.cellules.len(), 1);
    }
}

/// Déterministe : l'ordre vient de l'entrée, jamais d'une table de hachage.
/// Deux chargements différents de la même scène seraient invisibles et
/// impossibles à tester.
#[test]
fn le_groupement_est_deterministe() {
    let v = au_chunk(BlockPos::new(-1300, 64, 900), [0.6, 0.0, 0.8], 20);
    assert_eq!(par_region(&v), par_region(&v));
    assert!(
        par_region(&[]).is_empty(),
        "une demande vide ne fait aucun lot"
    );
}

/// **Le groupement remet en ordre une demande qu'on lui donne en désordre.**
///
/// `voulues` rend déjà une liste triée, donc sur son propre résultat le tri
/// des lots ne décide rien — la mutation qui le retirait survivait, faute
/// d'un test qui lui donne autre chose. Or `par_region` est publique et prend
/// une tranche quelconque : un appelant qui filtre, concatène ou construit sa
/// demande lui-même aurait obtenu des lots dans un ordre dépendant de
/// l'ordre d'entrée, sans que rien ne le dise. C'est le même piège que le
/// départage rendu inopérant par un tri stable, dans l'autre sens : là une
/// ligne ne décidait rien, ici elle décide — encore faut-il l'éprouver.
#[test]
fn une_demande_en_desordre_ressort_groupee_dans_le_bon_ordre() {
    let v = au_chunk(BlockPos::new(8, 64, 8), EST, 40);
    let attendu: Vec<(i32, i32)> = par_region(&v)
        .iter()
        .map(|l| (l.region.x, l.region.z))
        .collect();
    assert!(
        attendu.len() > 1,
        "il faut plusieurs régions pour que l'ordre ait un sens"
    );

    // Renversée : l'ordre d'apparition des régions devient le pire possible.
    let mut envers = v.clone();
    envers.reverse();
    let obtenu: Vec<(i32, i32)> = par_region(&envers)
        .iter()
        .map(|l| (l.region.x, l.region.z))
        .collect();
    assert_eq!(
        obtenu, attendu,
        "l'ordre des lots doit venir de l'URGENCE, pas de l'ordre d'entrée"
    );
    for p in par_region(&envers).windows(2) {
        assert!(p[0].urgence() <= p[1].urgence());
    }
}
