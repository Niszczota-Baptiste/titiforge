//! **Faire VOLER une caméra, et vérifier que le monde arrive.**
//!
//! C'est la dernière jonction de la chaîne, et la seule qu'on ne pouvait pas
//! vérifier tant qu'elle vivait dans le gestionnaire d'image de la fenêtre :
//! derrière un serveur graphique, donc jamais en intégration continue, donc
//! jamais chez quelqu'un qui n'a pas d'écran. Ce dépôt a déjà payé deux fois
//! « une jonction que personne n'écrit est une jonction que chaque hôte
//! réécrira » ; celle-ci est dans la bibliothèque (`tf_app::pilote`) et la
//! coque ne fait plus que lui donner un œil et un regard.
//!
//! Trois défauts qu'elle attrape, et qu'aucune des trois pièces ne peut voir
//! seule :
//!
//! 1. **Redemander à chaque image** remplace la file du chargeur soixante
//!    fois par seconde, donc annule en boucle la région qu'il lit. Le monde ne
//!    se charge jamais — et la machine a l'air occupée, ce qui est le pire des
//!    symptômes. On COMPTE les lectures, on ne chronomètre pas.
//! 2. **Ne jamais intégrer sans arrivée** laisse la scène au-dessus de son
//!    budget dès que le vol s'arrête, c'est-à-dire pendant qu'on regarde son
//!    build.
//! 3. **Voler doit charger et LÂCHER** : une caméra qui traverse un monde
//!    plus gros que sa mémoire doit arriver au bout sans grandir.

mod commun;

use std::time::{Duration, Instant};

use commun::{codex, monde, semer, semer_build, Jetable};
use tf_app::pilote::Pilote;
use tf_app::scene::Ouvert;
use tf_world::coords::BlockPos;
use tf_world::{Dimension, Niveau};

const EST: [f32; 3] = [1.0, 0.0, 0.0];
const HAUTEUR: (i32, i32) = (-64, 319);

/// Fait tourner des images jusqu'à ce que la condition tienne, ou rend faux.
///
/// Le vol est donné en fonction du rang d'image : c'est ce qui permet
/// d'écrire « immobile » et « en ligne droite » avec le même harnais.
#[derive(Default)]
struct Vol {
    /// Images vraiment jouées.
    images: usize,
    /// Cellules arrivées du fil.
    arrivees: usize,
    /// Images où des sections évincées ont été RETIRÉES.
    degagees: usize,
    /// Images où le GPU aurait dû être regarni.
    changees: usize,
}

fn images<F, C>(p: &mut Pilote, o: &mut Ouvert, n: usize, ou: F, mut fini: C) -> Vol
where
    F: Fn(usize) -> BlockPos,
    C: FnMut(&Ouvert) -> bool,
{
    let debut = Instant::now();
    let mut vol = Vol::default();
    for i in 0..n {
        if fini(o) {
            return vol;
        }
        if debut.elapsed() > Duration::from_secs(120) {
            panic!("le vol n'avance plus après {i} images");
        }
        let f = p
            .image(o, ou(i), EST, tf_app::pilote::CELLULES_PAR_IMAGE)
            .expect("une image de streaming");
        vol.images += 1;
        vol.arrivees += f.arrivees;
        vol.degagees += (f.degagees > 0) as usize;
        vol.changees += f.a_change() as usize;
        // Une image sans rien à faire est normale : le fil lit. On ne tourne
        // pas à vide pour autant.
        if !f.a_change() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    vol
}

/// **Voler fait venir le monde.**
///
/// La propriété de base, et elle n'a jamais été vérifiée : chaque pièce
/// marchait de son côté. On part d'un chunk et on regarde la scène grandir
/// sans qu'on ait rien demandé à la main.
#[test]
fn voler_fait_venir_le_monde() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let depart = o.monde.quads;
    let cellules_au_depart = o.residentes();
    assert_eq!(cellules_au_depart, 1, "on part d'UN chunk");

    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, 6, HAUTEUR);
    // L'œil ne bouge pas : c'est le RAYON qui doit faire venir les voisins.
    let vol = images(
        &mut p,
        &mut o,
        4000,
        |_| BlockPos::new(8, 64, 8),
        |o| o.residentes() >= 100,
    );
    p.arreter();

    assert!(
        o.residentes() >= 100,
        "le disque de rayon 6 porte 137 cellules : seulement {} arrivées en {} images",
        o.residentes(),
        vol.images
    );
    assert!(
        o.monde.quads > depart,
        "et la scène doit porter PLUS de géométrie qu'au départ : {} contre {depart}",
        o.monde.quads
    );
    assert_eq!(o.rechargements, 0, "streamer ne recharge jamais la zone");
    println!(
        "{} cellules arrivées en {} images, {} quads",
        vol.arrivees, vol.images, o.monde.quads
    );
}

/// **Une caméra immobile ne redemande pas en boucle.**
///
/// C'est le défaut qui ne se voit pas : la fenêtre répond, le fil tourne, et
/// le monde n'arrive jamais parce que chaque image annule la région que le
/// précédent lisait. On COMPTE les lectures de `.mca` — un chronomètre
/// dépendrait de la machine, et ce dépôt a mesuré un facteur 2,4 à code
/// identique.
#[test]
fn une_camera_immobile_ne_relit_pas_le_monde_a_chaque_image() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    // La source du PILOTE compte ses lectures. Elle porte les mêmes octets
    // que celle du disque — même générateur, même graine — donc la scène est
    // la même ; seul le comptage s'ajoute.
    let src = monde(1, 8);
    let mut p = Pilote::neuf(src.clone(), Dimension::Overworld, Niveau::Chunk, 4, HAUTEUR);

    // **L'œil au chunk 8**, rayon 4 : le disque va du chunk 4 au chunk 12,
    // donc entièrement dans la région (0, 0). UNE lecture doit suffire à le
    // servir en entier — et c'est ce qui rend le compteur lisible : deux
    // lectures voudraient dire qu'on a redemandé.
    let disque =
        tf_world::demande::voulues(BlockPos::new(136, 64, 136), EST, 4, Niveau::Chunk, HAUTEUR)
            .len();
    let vol = images(
        &mut p,
        &mut o,
        3000,
        // On remue d'un bloc entre deux images, sans jamais quitter le chunk 8
        // (blocs 128..143) : marcher DANS une cellule ne doit rien redemander.
        |i| BlockPos::new(136, 64, 136 + (i % 2) as i32),
        move |o| o.residentes() > disque,
    );
    assert!(
        o.residentes() > disque,
        "{} cellules résidentes pour un disque de {disque} : le vol n'a pas abouti",
        o.residentes()
    );
    // Et on continue à tourner longtemps SANS bouger : c'est là que le défaut
    // se produirait.
    for _ in 0..300 {
        p.image(&mut o, BlockPos::new(136, 64, 136), EST, 2)
            .expect("image");
    }
    p.arreter();

    let lectures = src.lectures();
    assert_eq!(
        lectures,
        1,
        "{lectures} lectures de .mca pour un disque qui tient dans UNE région, \
         sur {} images — chaque image redemande et annule la précédente",
        vol.images + 300
    );
    println!(
        "{lectures} lecture de .mca pour {} images et {disque} cellules",
        vol.images + 300
    );
}

/// **Un vol continu charge et LÂCHE, sans jamais recharger la zone.**
///
/// Le contrat de sortie de la phase 5, vu depuis l'hôte : la caméra traverse
/// un monde plus gros que la mémoire qu'on lui donne, et la scène ne grandit
/// pas sans fin.
#[test]
fn un_vol_continu_charge_et_lache() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    // **Deux régions PLEINES en x**, pas une : la traversée doit franchir une
    // frontière de `.mca`, ce qui est le cas où le chargeur travaille vraiment.
    semer(m.chemin(), 2, 32);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, 4, HAUTEUR);

    // **Le champ d'abord, le budget ensuite.** On attend que le disque de
    // départ soit entièrement là, puis on donne deux fois sa taille : de quoi
    // tenir ce qu'on regarde et une traînée, très loin des soixante chunks de
    // la traversée. Un budget plus SERRÉ que le champ ne mesurerait pas la
    // même chose — voir `un_budget_trop_petit_ne_tourne_pas_a_vide`.
    let disque =
        tf_world::demande::voulues(BlockPos::new(8, 64, 8), EST, 4, Niveau::Chunk, HAUTEUR).len();
    images(
        &mut p,
        &mut o,
        3000,
        |_| BlockPos::new(8, 64, 8),
        |o| o.residentes() > disque,
    );
    let budget = o.octets_residents() * 2;
    o.budget_residence(budget);

    // Plein est, quatre blocs par image : 960 blocs, soit 60 chunks, et la
    // frontière de région au chunk 32. On reste DANS le monde semé — voler au
    // delà ne ferait plus arriver que des cellules vides, qui ne pèsent rien
    // et n'évincent donc rien : le test aurait l'air de passer sans avoir
    // jamais mis la fenêtre sous pression.
    let vol = images(
        &mut p,
        &mut o,
        240,
        |i| BlockPos::new(8 + i as i32 * 4, 64, 8),
        |_| false,
    );
    // **On s'arrête, et on continue à jouer des images sur place.** C'est ce
    // qui doit résorber le dégagement en attente : le retrait de ce que la
    // fenêtre a évincé se fait à l'intégration SUIVANTE, et sans appel il ne
    // se ferait jamais. Une caméra immobile est le cas NORMAL — on regarde
    // son build bien plus qu'on ne le survole.
    //
    // On joue des images jusqu'à ce que plus rien n'arrive NI ne reste à
    // dégager. S'arrêter de voler n'arrête pas le fil : sa file tient encore
    // ce qu'on lui a demandé, et chaque cellule qui se pose évince à son tour.
    let fin = BlockPos::new(8 + 240 * 4, 64, 8);
    let mut images_a_l_arret = 0;
    // **Cinq images calmes d'affilée**, pas une. Le fil peut avoir une région
    // en cours : une seule image sans arrivée ne prouve pas qu'il a fini, et
    // ce qui suit veut une scène vraiment stable.
    let mut calme = 0;
    for _ in 0..600 {
        let f = p.image(&mut o, fin, EST, 2).expect("image à l'arrêt");
        images_a_l_arret += 1;
        calme = if f.a_change() || o.en_attente() > 0 {
            0
        } else {
            calme + 1
        };
        if calme >= 5 {
            break;
        }
        if f.arrivees == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert!(
        calme >= 5,
        "la scène ne se stabilise pas à l'arrêt après {images_a_l_arret} images : \
         {} résidentes, {} en attente, {} évictions, {} dégagées, occupé {}",
        o.residentes(),
        o.en_attente(),
        o.evictions(),
        o.degagees(),
        p.occupe()
    );
    p.arreter();
    assert_eq!(
        o.en_attente(),
        0,
        "rester immobile doit finir de dégager ({images_a_l_arret} images), \
         sans quoi la scène reste au-dessus de son budget tant qu'on ne vole \
         pas — c'est-à-dire pendant qu'on regarde son build"
    );

    assert!(
        vol.arrivees > 40,
        "la prémisse : la traversée doit vraiment charger, pas {} cellules",
        vol.arrivees
    );
    assert!(
        vol.degagees > 0,
        "et vraiment retirer de la scène, image après image"
    );
    // **Une image qui ne fait que DÉGAGER**, provoquée exprès : on resserre
    // le budget une fois tout arrêté. Rien n'arrive, et des sections partent
    // quand même. C'est celle-là qu'un hôte qui regarnit son GPU sur les
    // seules ARRIVÉES laisserait à l'écran — le maillage de ce qu'il vient
    // d'évincer, le piège « un chunk qui se VIDE ne figure plus dans la liste
    // des chunks », payé dans `ExeWorldEdit`.
    o.budget_residence(o.octets_residents() / 2);
    let f = p
        .image(&mut o, fin, EST, 2)
        .expect("image après resserrement");
    assert_eq!(f.arrivees, 0, "plus rien n'arrive : tout est déjà là");
    assert!(
        f.degagees > 0,
        "resserrer le budget doit RETIRER des cellules dans le même appel"
    );
    assert!(
        f.a_change(),
        "et l'image doit le DIRE, sinon le GPU garde le maillage d'avant"
    );
    assert!(o.evictions() > 0, "et vraiment lâcher");
    assert_eq!(o.rechargements, 0, "sans jamais recharger la zone");
    assert!(
        o.octets_residents() <= budget,
        "{} octets portés après {} images pour un plafond de {budget}",
        o.octets_residents(),
        vol.images
    );
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "et la comptabilité reste exacte au bout du vol"
    );
    println!(
        "{} images, {} cellules arrivées, {} évictions, {:.1} Mo tenus sur {:.1} autorisés",
        vol.images,
        vol.arrivees,
        o.evictions(),
        o.octets_residents() as f64 / 1e6,
        budget as f64 / 1e6
    );
}

/// **Un budget plus petit que le champ de vision ne fait pas tourner la
/// machine à vide.**
///
/// C'est le défaut que seul le pilote pouvait montrer, et il était silencieux :
/// le LRU évince une cellule que la caméra regarde encore, la demande la
/// redemande aussitôt, elle arrive, elle en évince une autre du champ, et ainsi
/// de suite. Mesuré avant la correction : **180 évictions en 100 images** sur
/// une scène qui n'avance pas d'un bloc, sans fin. La fenêtre répond, le fil
/// tourne, le disque chauffe, et rien ne le dit.
///
/// Ce que la caméra REGARDE est donc épinglé : le LRU ne prend que dans la
/// traînée. Si le champ à lui seul ne tient pas, on dépasse le budget en le
/// DISANT (`deborde`) — « le plafond est une CIBLE, pas une limite dure », et
/// montrer ce que l'utilisateur regarde vaut mieux que de ne rien montrer.
#[test]
fn un_budget_trop_petit_ne_tourne_pas_a_vide() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 32);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, 4, HAUTEUR);
    let oeil = BlockPos::new(136, 64, 136);
    let disque = tf_world::demande::voulues(oeil, EST, 4, Niveau::Chunk, HAUTEUR).len();

    images(&mut p, &mut o, 4000, |_| oeil, |o| o.residentes() > disque);
    assert!(
        o.residentes() > disque,
        "la prémisse : le champ doit d'abord être entièrement là"
    );
    assert!(!o.deborde(), "et tenir dans le budget par défaut");

    // La MOITIÉ de ce qu'il faut pour le champ.
    o.budget_residence(o.octets_residents() / 2);
    images(&mut p, &mut o, 100, |_| oeil, |_| false);
    let (evictions, residentes, octets) = (o.evictions(), o.residentes(), o.octets_residents());
    assert!(o.deborde(), "un budget trop petit doit se DIRE");

    // Cent images de plus, sans bouger d'un bloc : rien ne doit changer.
    let vol = images(&mut p, &mut o, 100, |_| oeil, |_| false);
    p.arreter();

    assert_eq!(
        o.evictions(),
        evictions,
        "après stabilisation, plus une seule éviction : {} de plus en 100 \
         images veut dire qu'on charge et jette en boucle ce que la caméra \
         regarde",
        o.evictions() - evictions
    );
    assert_eq!(
        o.residentes(),
        residentes,
        "ni une cellule de plus ou de moins"
    );
    assert_eq!(o.octets_residents(), octets, "ni un octet");
    assert_eq!(vol.arrivees, 0, "et le fil n'a plus rien à lire");
    assert_eq!(o.rechargements, 0);
    assert!(
        residentes >= disque,
        "le champ reste entier : {residentes} cellules pour un disque de {disque}"
    );
    println!(
        "budget à la moitié du champ : {residentes} cellules tenues ({:.2} Mo \
         pour {:.2} autorisés), 0 éviction en 100 images",
        octets as f64 / 1e6,
        o.budget_actuel() as f64 / 1e6
    );
}

/// **Ce que la caméra protège suit la caméra, même quand rien n'est à
/// charger.**
///
/// Le champ épinglé se recalcule à CHAQUE image, pas seulement quand une
/// demande part. Sinon il reste figé sur la position d'où l'on a chargé pour
/// la dernière fois : voler au-dessus d'un terrain DÉJÀ résident ne demande
/// rien, donc n'actualiserait rien, et la traînée resterait protégée pour
/// toujours. La mémoire ne redescendrait jamais — et le symptôme serait « elle
/// ne rend rien quand je ne charge pas », c'est-à-dire l'inverse de ce à quoi
/// on pense.
///
/// On le provoque en RÉDUISANT la distance d'affichage sur une zone déjà
/// chargée : plus rien à demander, et pourtant presque tout devient
/// évinçable.
#[test]
fn le_champ_protege_suit_la_camera_sans_rien_demander() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 32);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, 6, HAUTEUR);
    let oeil = BlockPos::new(200, 64, 200);
    let large = tf_world::demande::voulues(oeil, EST, 6, Niveau::Chunk, HAUTEUR).len();

    images(&mut p, &mut o, 6000, |_| oeil, |o| o.residentes() > large);
    assert!(
        o.residentes() > large,
        "la prémisse : le large disque doit être entièrement là"
    );
    let plein = o.octets_residents();

    // **On resserre la distance d'affichage.** Tout ce que le petit disque
    // demande est déjà là : plus une seule demande ne partira.
    p.rayon_voulu(2);
    let petit = tf_world::demande::voulues(oeil, EST, 2, Niveau::Chunk, HAUTEUR).len();
    let vol = images(&mut p, &mut o, 60, |_| oeil, |_| false);
    assert_eq!(
        vol.arrivees, 0,
        "rien à charger : le petit disque est compris dans le grand"
    );

    // Le budget tombe à RIEN. Tout ce qui n'est pas épinglé doit partir, et
    // seul le champ du PETIT disque l'est : la traînée du grand disque part,
    // le petit reste, et la fenêtre dit qu'elle déborde.
    //
    // Le budget visait d'abord « la part du petit disque », `plein × petit /
    // large` ; il ne tient plus depuis que les cellules du bord montrent leur
    // paroi vers ce qui n'est pas chargé — un petit disque a proportionnel-
    // lement plus de bord qu'un grand. Un budget nul ne suppose rien.
    o.budget_residence(1);
    images(
        &mut p,
        &mut o,
        200,
        |_| oeil,
        |o| o.residentes() <= petit && o.en_attente() == 0,
    );
    p.arreter();

    assert_eq!(
        o.residentes(),
        petit,
        "seul le petit disque, épinglé, doit rester : le champ épinglé est \
         resté celui du grand disque si la traînée ne part pas"
    );
    assert!(
        o.deborde(),
        "et la fenêtre DIT qu'elle tient plus que son budget"
    );
    assert!(
        o.octets_residents() < plein / 2,
        "la mémoire a suivi : {} octets tenus, {plein} avant",
        o.octets_residents()
    );
    println!(
        "rayon 6 → 2 sans une seule demande : {:.2} Mo ramenés à {:.2}, \
         {} cellules pour un champ de {petit}",
        plein as f64 / 1e6,
        o.octets_residents() as f64 / 1e6,
        o.residentes()
    );
}
