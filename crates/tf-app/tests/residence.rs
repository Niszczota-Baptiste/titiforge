//! **Ce que la scène s'autorise à tenir.**
//!
//! La demande dit quoi charger ; la résidence dit quoi LÂCHER. Sans la
//! seconde, la première suffit à tuer l'application : mesuré
//! (`cargo run --release -p tf-app --example residence`), une région BÂTIE
//! laisse 186 Mo résidents, donc onze tiennent dans deux gigaoctets et la
//! douzième ne tient pas. Un vol continu en traverse bien plus que onze.
//!
//! Trois propriétés, et la première porte les deux autres :
//!
//! 1. **La comptabilité n'est pas une fiction.** Ce que la fenêtre croit
//!    tenir et ce que la scène porte vraiment sont le MÊME nombre. Un budget
//!    qui ne se compare à rien est un budget qu'on peut tenir en se trompant
//!    — et ce dépôt a déjà payé « deux constantes pour une même vérité
//!    finissent par diverger ».
//! 2. **Un vol continu se borne**, sans jamais recharger la zone. Le
//!    rechargement est le défaut qui a coûté le plus cher aux deux
//!    applications qui précèdent celle-ci ; le borner ne doit pas le
//!    ressusciter.
//! 3. **Ce qui reste après éviction est ce qu'un chargement direct
//!    donnerait.** C'est la règle du dépôt — toute stratégie rapide se
//!    compare au résultat de la stratégie lente — et c'est le seul moyen de
//!    voir la marge du dégagement : retirer une cellule DÉCOUVRE les faces de
//!    ses voisines, et sans le débordement d'une case il resterait un mur de
//!    faces fantômes le long de chaque frontière dégagée.

mod commun;

use commun::{canon, codex, montre, montre_modeles, semer_build, streamer, Jetable};
use tf_app::chargeur::Chargeur;
use tf_app::scene::Ouvert;
use tf_world::coords::BlockPos;
use tf_world::demande::{par_region, voulues};
use tf_world::Niveau;

const EST: [f32; 3] = [1.0, 0.0, 0.0];
const HAUTEUR: (i32, i32) = (-64, 319);
/// Le côté du bâti d'essai, en chunks.
const COTE: u32 = 8;

/// Vide ce que les évictions ont laissé en attente.
///
/// Un lot VIDE suffit, et c'est voulu : un hôte qui appelle `integrer` à
/// chaque image converge même quand plus rien n'arrive. Il en faut parfois
/// deux — le premier élague, le second repèse les voisines découvertes — d'où
/// la boucle, avec un plafond pour qu'une non-convergence se voie au lieu de
/// tourner sans fin.
fn degager(o: &mut Ouvert) {
    for _ in 0..8 {
        if o.en_attente() == 0 && o.octets_comptes() <= o.budget_actuel() {
            return;
        }
        o.integrer(Vec::new()).expect("dégagement");
        // Le retrait se maille hors du fil principal : il ne compte qu'une
        // fois revenu.
        o.attendre_maillage().expect("maillage");
    }
    panic!(
        "le dégagement ne converge pas : {} en attente, {} octets comptés",
        o.en_attente(),
        o.octets_comptes()
    );
}

/// Le vol : un œil au COIN proche, un disque qui couvre tout le bâti.
///
/// Au coin et non au centre, et c'est le point du test d'éviction : les
/// cellules arrivent par urgence croissante, donc celles du coin d'abord.
/// Elles sont donc les plus FROIDES quand la pression arrive, et ce sont
/// elles que le LRU doit lâcher. Un œil au centre ferait arriver le coin en
/// dernier, et le test ne prouverait plus rien sur l'ordre.
fn voler(o: &mut Ouvert, c: &mut Chargeur) -> usize {
    let lots = par_region(&voulues(
        BlockPos::new(8, 64, 8),
        EST,
        COTE,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    assert_eq!(streamer(o, c, n), n, "toutes les cellules doivent arriver");
    n
}

/// **Ce que la fenêtre compte est ce que la scène porte.**
///
/// Sans éviction, donc sans rien en attente : les deux nombres doivent être
/// ÉGAUX, pas du même ordre. C'est ce qui rend le reste vérifiable.
///
/// Cette égalité tient une chose qu'on ne voit nulle part ailleurs : une
/// cellule MAIGRIT quand sa voisine arrive — les faces de son bord, jusque-là
/// exposées à du vide, se retrouvent masquées. Ne repeser que les arrivées
/// laisserait chaque cellule inscrite au poids qu'elle avait SEULE, et la
/// fenêtre tiendrait une fraction de ce qu'elle croit tenir.
#[test]
fn ce_que_la_fenetre_compte_est_ce_que_la_scene_porte() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), COTE);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    // La zone d'ouverture compte déjà : sinon elle ne serait jamais
    // évinçable, et les chunks du départ resteraient en mémoire pour toujours.
    assert!(
        o.octets_comptes() > 0,
        "la zone d'ouverture doit être inscrite"
    );
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "à l'ouverture déjà, les deux nombres sont le même"
    );

    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    voler(&mut o, &mut c);
    c.arreter();

    assert_eq!(
        o.evictions(),
        0,
        "le budget par défaut ne peut pas déborder"
    );
    assert_eq!(o.en_attente(), 0, "rien d'évincé, donc rien en attente");
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "la fenêtre compte {} octets pour {} portés — une cellule a été pesée \
         seule et jamais repesée quand sa voisine l'a masquée",
        o.octets_comptes(),
        o.octets_residents()
    );
    println!(
        "{} cellules, {:.1} Mo résidents, comptés à l'octet près",
        o.residentes(),
        o.octets_residents() as f64 / 1e6
    );
}

/// **Un vol continu ne grandit pas sans fin — et ne recharge jamais.**
///
/// Le budget se prend sur la mesure du témoin plutôt que sur une constante :
/// un nombre écrit en dur cesserait de serrer le jour où la fixture change,
/// et le test passerait sans jamais avoir déclenché une seule éviction. La
/// prémisse est donc VÉRIFIÉE (`evictions() > 0`), pas espérée.
#[test]
fn un_vol_continu_reste_borne() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), COTE);

    // Le témoin : le même vol, sans plafond utile. Donne ce que le bâti pèse.
    let mut plein = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("témoin");
    let mut c = Chargeur::lancer(
        plein.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
    );
    let n = voler(&mut plein, &mut c);
    c.arreter();
    let tout = plein.octets_residents();
    let sections_pleines = plein.monde.grille.len();
    assert!(tout > 0 && sections_pleines > 0, "le bâti doit peser");

    // Le même vol sous un tiers de la place.
    let budget = tout / 3;
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    o.budget_residence(budget);
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    assert_eq!(voler(&mut o, &mut c), n);
    c.arreter();
    degager(&mut o);

    assert!(
        o.evictions() > 0,
        "la prémisse du test : à {budget} octets, l'éviction doit arriver"
    );
    assert_eq!(
        o.rechargements, 0,
        "borner la mémoire ne doit PAS ressusciter le rechargement de zone"
    );
    assert!(
        o.octets_residents() <= budget,
        "{} octets portés pour un plafond de {budget}",
        o.octets_residents()
    );
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "la comptabilité doit rester exacte une fois dégagé"
    );
    assert!(
        o.monde.grille.len() < sections_pleines,
        "la grille doit avoir VRAIMENT rendu des sections : {} contre {sections_pleines}",
        o.monde.grille.len()
    );
    // Le coin est arrivé en premier, donc c'est le plus froid : c'est lui que
    // le LRU doit avoir lâché. Y compris la cellule de la zone d'OUVERTURE,
    // qui sans inscription serait restée là pour toujours.
    assert!(
        o.monde.grille.section((0, 0, 0)).is_none(),
        "le chunk du départ est le plus froid : il doit être parti"
    );
    println!(
        "{n} cellules survolées : {:.1} Mo au lieu de {:.1}, {} évictions, 0 rechargement",
        o.octets_residents() as f64 / 1e6,
        tout as f64 / 1e6,
        o.evictions()
    );
}

/// **Ce qui reste après éviction est ce qu'un chargement direct donnerait.**
///
/// La règle du dépôt, appliquée au dégagement : toute stratégie rapide se
/// compare au résultat de la stratégie lente.
///
/// C'est le seul test qui voit la MARGE du dégagement. Retirer une cellule
/// découvre les faces de ses voisines ; sans le débordement d'une case, le
/// chunk resté au bord garderait ses faces masquées par un voisin qui n'est
/// plus là — moins de quads que la vérité, et un mur invisible le long de
/// chaque frontière dégagée. Rien d'autre ne le dirait : la comptabilité
/// resterait cohérente avec elle-même.
///
/// La zone est une RANGÉE, et c'est ce qui rend le témoin écrivable : les
/// cellules sont inscrites de gauche à droite, donc les survivantes forment
/// toujours un suffixe contigu.
#[test]
fn ce_qui_survit_est_ce_qu_un_chargement_direct_donnerait() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), COTE);

    let dernier = COTE as i32 - 1;
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, dernier, 0]).expect("rangée");
    assert!(o.monde.quads > 10_000, "la prémisse : du bâti");
    let tout = o.octets_residents();

    // On resserre le budget APRÈS coup : c'est le geste d'un utilisateur qui
    // baisse le réglage, et il doit prendre effet sans attendre qu'une
    // cellule arrive.
    o.budget_residence(tout / 2);
    degager(&mut o);
    assert!(
        o.evictions() > 0,
        "la prémisse : à la moitié, ça doit couper"
    );
    assert_eq!(o.rechargements, 0, "dégager n'est pas recharger");

    // Quelles colonnes restent ? Un suffixe contigu, sinon le témoin n'a pas
    // de sens et le test doit le DIRE plutôt que comparer n'importe quoi.
    let restants: Vec<i32> = (0..=dernier)
        .filter(|cx| (-4..20).any(|sy| o.monde.grille.section((*cx, 0, sy)).is_some()))
        .collect();
    assert!(!restants.is_empty(), "tout a été évincé : rien à comparer");
    let premier = restants[0];
    assert_eq!(
        restants,
        (premier..=dernier).collect::<Vec<_>>(),
        "les survivantes doivent former un suffixe contigu"
    );
    assert!(premier > 0, "au moins une colonne doit être partie");

    let temoin = Ouvert::ouvrir(&pack, Some(m.texte()), [premier, 0, dernier, 0]).expect("témoin");
    let mut mots = Vec::new();
    let ct = canon(&temoin, &mut mots);
    let attendu = montre(&temoin, &ct);
    let cs = canon(&o, &mut mots);
    let obtenu = montre(&o, &cs);

    assert_eq!(
        obtenu.len(),
        attendu.len(),
        "colonnes {premier}..{dernier} : {} quads après éviction contre {} \
         chargés directement — la marge du dégagement",
        obtenu.len(),
        attendu.len()
    );
    if let Some(k) = (0..obtenu.len()).find(|&k| obtenu[k] != attendu[k]) {
        panic!(
            "quad {k} sur {} : évincé {:?} contre direct {:?}",
            obtenu.len(),
            obtenu[k],
            attendu[k]
        );
    }
    assert_eq!(o.monde.poses, temoin.monde.poses, "les blocs-modèles aussi");
    // Face par face : l'éviction laisse des TROUS dans la passe de modèles,
    // et un trou mal fermé dessinerait les faces d'un bloc parti.
    let (a, b) = (montre_modeles(&o, &cs), montre_modeles(&temoin, &ct));
    assert!(
        !b.is_empty(),
        "la prémisse : du bâti porte des faces de modèles"
    );
    assert_eq!(
        a.len(),
        b.len(),
        "pas le même nombre de faces de modèles dessinées"
    );
    assert!(
        a == b,
        "les faces de modèles dessinées diffèrent d'un chargement direct"
    );
    println!(
        "{} colonnes évincées sur {COTE} : les {} restantes donnent exactement \
         les {} quads d'un chargement direct",
        premier,
        restants.len(),
        obtenu.len()
    );
}

/// **Repeser une voisine ne la rend pas récente.**
///
/// Une cellule maigrit quand sa voisine arrive, donc son poids se corrige
/// sans que personne ne l'ait REGARDÉE. Corriger par une insertion la ferait
/// remonter en tête de la liste de récence : le LRU garderait exactement ce
/// qu'il faudrait lâcher, et la traînée d'un vol en ligne droite ne serait
/// jamais rendue — chaque cellule étant réchauffée par l'arrivée de la
/// suivante. D'où `Residency::update`, qui corrige le poids sans toucher à la
/// récence.
///
/// Rien d'autre ne le voit : la comptabilité reste exacte, la scène reste
/// juste, la mémoire reste bornée. Seul CE QUI est lâché change — et c'est
/// tout ce qui sépare une fenêtre utile d'une fenêtre qui rend au hasard.
///
/// La zone est une rangée inscrite de gauche à droite, donc la colonne 0 est
/// la plus froide. On fait arriver une cellule au SUD de la colonne 0 : sa
/// marge touche les colonnes 0 et 1 sans que la caméra s'en soit approchée.
#[test]
fn repeser_une_voisine_ne_la_rend_pas_recente() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), COTE);

    let dernier = COTE as i32 - 1;
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, dernier, 0]).expect("rangée");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);

    // Un rayon de zéro : la seule cellule où l'œil se trouve, au sud de la
    // colonne 0. Son remaillage déborde d'une case, donc il TOUCHE les
    // colonnes 0 et 1 de la rangée.
    let lots = par_region(&voulues(
        BlockPos::new(8, 64, 24),
        EST,
        0,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    assert_eq!(n, 1, "un rayon de zéro demande UNE cellule");
    c.demander(lots);
    assert_eq!(streamer(&mut o, &mut c, n), n);
    c.arreter();

    assert!(
        o.monde.grille.section((0, 0, 0)).is_some(),
        "la prémisse : la colonne 0 est encore là avant qu'on resserre"
    );

    o.budget_residence(o.octets_residents() / 2);
    degager(&mut o);
    assert!(
        o.evictions() > 0,
        "la prémisse : à la moitié, ça doit couper"
    );

    let presente =
        |cx: i32, cz: i32| (-4..20).any(|sy| o.monde.grille.section((cx, cz, sy)).is_some());
    assert!(
        !presente(0, 0),
        "la colonne 0 est la plus FROIDE — l'arrivée de sa voisine du sud \
         corrige son poids, elle ne la regarde pas. Si elle survit, c'est \
         qu'une correction de poids a été prise pour un accès."
    );
    assert!(
        presente(0, 1),
        "la cellule qu'on vient de charger est la plus récente : elle reste"
    );
    assert!(
        presente(dernier, 0),
        "la dernière colonne inscrite est plus chaude que la première"
    );
}

/// **Une cellule de RÉGION couvre 32 × 32 colonnes, pas une.**
///
/// Le chargeur sert les deux niveaux depuis toujours ; la scène, elle, ne
/// retrouvait ce qu'une cellule portait qu'en filtrant la grille sur
/// `adresse.0 == cellule.x`. Vrai au niveau chunk, faux d'un facteur mille au
/// niveau région : une cellule de région n'aurait rendu qu'une colonne sur
/// 1 024, et le reste serait resté dans la grille pour toujours — invisible,
/// puisque le contenu affiché aurait été juste.
///
/// La comptabilité le dit sans détour : une cellule pesée sur une colonne au
/// lieu de mille porterait un millième de son poids, donc la fenêtre croirait
/// tenir mille fois moins qu'elle ne tient.
#[test]
fn une_cellule_de_region_est_pesee_en_entier() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), COTE);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let avant = o.residentes();
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);

    // Un rayon de zéro au niveau RÉGION : la seule région où l'œil se trouve.
    let lots = par_region(&voulues(
        BlockPos::new(64, 64, 64),
        EST,
        0,
        Niveau::Region,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    assert_eq!(n, 1, "un rayon de zéro demande UNE cellule de région");
    c.demander(lots);
    assert_eq!(streamer(&mut o, &mut c, n), n);
    c.arreter();

    assert_eq!(
        o.residentes(),
        avant + 1,
        "la région doit être inscrite, en plus de la zone d'ouverture"
    );
    assert_eq!(o.rechargements, 0);
    assert_eq!(o.en_attente(), 0, "rien d'évincé : le budget est large");
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "la fenêtre compte {} octets pour {} portés — une cellule de région \
         pesée sur une seule de ses 1 024 colonnes",
        o.octets_comptes(),
        o.octets_residents()
    );

    // Et le contenu est bien celui du bâti entier, pas d'une colonne.
    let temoin = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, COTE as i32 - 1, 0])
        .expect("une rangée pour comparer");
    assert!(
        o.monde.quads > temoin.monde.quads,
        "la région porte {} quads, une rangée {} : elle doit en porter PLUS",
        o.monde.quads,
        temoin.monde.quads
    );
}
