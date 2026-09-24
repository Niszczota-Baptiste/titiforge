//! **Ce que le fil écrit, la coque le relit.**
//!
//! C'est la jonction que ni le moteur ni la scène ne peuvent prouver seuls :
//! le fil écrit dans la copie de travail, la coque remaille DEPUIS elle. Relire
//! la source rendrait le monde d'AVANT — ce qui se lit « le bouton ne fait
//! rien », et ne désigne pas la cause.
//!
//! **Ces tests tournent PARTOUT.** Ils demandaient un vrai pack (`TF_PACK`)
//! et se sautaient sans lui — donc jamais en intégration continue, jamais chez
//! quelqu'un qui n'a pas le serveur, et c'étaient justement ceux qui croisent
//! le remaillage incrémental avec le rechargement complet. Sans `TF_PACK`, le
//! codex minimal des tests est écrit à la volée (`commun::codex`) ; avec, ils
//! se rejouent sur le vrai pack. Un test qui ne tourne pas ne dit rien.

mod commun;

use commun::Jetable;

use std::time::{Duration, Instant};

use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_app::scene::Ouvert;
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::journal::Journal;

/// Écrit un monde jetable à partir de la fixture de terrain.
fn semer(dir: &std::path::Path) -> std::io::Result<()> {
    let brut = tf_bench::region(&tf_bench::Terrain::petite());
    std::fs::create_dir_all(dir.join("region"))?;
    std::fs::write(dir.join("region/r.0.0.mca"), brut)?;
    // Un `level.dat` vide suffit à `FsSource` pour reconnaître une save.
    std::fs::write(dir.join("level.dat"), [])?;
    Ok(())
}

/// **Un monde d'essai qui s'efface quoi qu'il arrive** — celui de `commun`,
/// semé de terrain.
///
/// Ce fichier avait SON `Jetable`, nommé par le seul numéro de processus et
/// l'étiquette. Tant que chaque test prenait une étiquette à lui, ça tenait ;
/// le jour où deux tests en parallèle ont demandé « codex », ils ont partagé
/// le même dossier, et le premier qui finissait effaçait le pack de l'autre —
/// deux à quatre tests rouges sur neuf, au hasard. C'est le piège des deux
/// mondes qui partageaient une copie de travail, une fois de plus : un nom
/// « unique » à l'échelle du processus ne l'est pas. Celui de `commun` porte un
/// compteur, et il n'en existe plus qu'un.
fn monde(etiquette: &str) -> Jetable {
    let j = Jetable::neuf(etiquette);
    semer(j.chemin()).expect("monde jetable");
    j
}

fn attendre(m: &mut Moteur) -> Reponse {
    let debut = Instant::now();
    loop {
        if let Some(r) = m.recevoir().into_iter().next() {
            return r;
        }
        assert!(debut.elapsed() < Duration::from_secs(60), "pas de réponse");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Le pack de ces tests : `TF_PACK` s'il est donné, le codex minimal écrit à
/// la volée sinon. Le `Jetable` rendu doit vivre autant que le test : il
/// efface le codex en partant.
fn pack() -> (String, Option<Jetable>) {
    if let Ok(p) = std::env::var("TF_PACK") {
        return (p, None);
    }
    let j = Jetable::neuf("codex");
    let p = commun::codex(j.chemin(), &["glowstone"]);
    (p, Some(j))
}

#[test]
fn ce_que_le_fil_ecrit_la_coque_le_relit() {
    let (pack, _codex) = pack();
    let jetable = monde("relit");
    let dir = jetable.chemin().to_path_buf();

    let mut ouvert = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), [0, 0, 1, 1])
        .expect("le monde doit s'ouvrir");
    assert!(ouvert.editable(), "un monde ouvert est éditable");

    // Une case qu'on va écraser, et ce qu'elle vaut AVANT.
    let case = [8, -40, 8];
    let avant = ouvert.monde.grille.bloc(case[0], case[1], case[2]);

    let mut moteur = Moteur::lancer(
        ouvert.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:glowstone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    assert!(!r.echoue(), "{}", r.texte());
    assert!(r.bornes().is_some(), "l'opération doit avoir écrit");

    // **Avant de remailler, la coque voit encore le monde d'avant** : elle
    // tient une grille, pas une vue vivante sur le disque.
    assert_eq!(ouvert.monde.grille.bloc(case[0], case[1], case[2]), avant);

    ouvert.remailler(r.bornes()).expect("remaillage");
    let apres = ouvert.monde.grille.bloc(case[0], case[1], case[2]);
    assert_ne!(apres, avant, "la coque relit toujours le monde d'avant");

    // Et l'annulation revient en arrière, par le même chemin.
    assert!(moteur.envoyer(Commande::Annuler));
    let r = attendre(&mut moteur);
    assert!(matches!(r, Reponse::Defait { .. }), "{r:?}");
    ouvert.remailler(r.bornes()).expect("remaillage");
    let defait = ouvert.monde.grille.bloc(case[0], case[1], case[2]);
    assert_ne!(defait, apres, "annuler doit se voir");

    moteur.arreter();
    drop(ouvert);
}

/// **Écrire dans la save : l'ordre, et la preuve qu'il est tenu.**
///
/// Refuser si le jeu tient le monde, SAUVEGARDER, puis écrire. Une sauvegarde
/// prise après la première écriture ne sauvegarde plus rien — et c'est le
/// genre de faute qu'on ne découvre que le jour où on en a besoin.
#[test]
fn ecrire_sauvegarde_avant_d_ecrire() {
    let (pack, _codex) = pack();
    let jetable = monde("ecrire");
    let dir = jetable.chemin().to_path_buf();
    let avant = std::fs::read(dir.join("region/r.0.0.mca")).unwrap();

    let ouvert = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), [0, 0, 1, 1]).unwrap();
    let mut moteur = Moteur::lancer(
        ouvert.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );

    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:glowstone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    assert!(!attendre(&mut moteur).echoue());

    // Tant qu'on n'a pas écrit, la SAVE est intacte — invariant n° 1.
    assert_eq!(
        std::fs::read(dir.join("region/r.0.0.mca")).unwrap(),
        avant,
        "la save a été touchée avant l'ordre d'écrire"
    );

    assert!(moteur.envoyer(Commande::Ecrire {
        confirme_sans_verrou: true
    }));
    let r = attendre(&mut moteur);
    let Reponse::Ecrit {
        regions,
        sauvegarde,
    } = &r
    else {
        panic!("attendu Ecrit, reçu {r:?} — {}", r.texte());
    };
    assert!(*regions > 0);

    // La save a changé…
    assert_ne!(std::fs::read(dir.join("region/r.0.0.mca")).unwrap(), avant);
    // … et la SAUVEGARDE porte le monde d'AVANT, octet pour octet. C'est
    // toute la raison d'être de l'étape 2.
    let copie = std::path::Path::new(sauvegarde).join("region/r.0.0.mca");
    assert!(copie.exists(), "pas de sauvegarde en {sauvegarde}");
    assert_eq!(
        std::fs::read(&copie).unwrap(),
        avant,
        "la sauvegarde ne porte pas le monde d'avant"
    );

    moteur.arreter();
    drop(ouvert);
    let _ = std::fs::remove_dir_all(sauvegarde);
}

/// **Le remaillage n'en fait que ce qu'il faut — et il doit donner LA MÊME
/// image qu'un rechargement complet.**
///
/// C'est la seule propriété qui compte : une optimisation qui change l'image
/// n'est pas une optimisation. On compare bloc par bloc et lot par lot après
/// une opération, entre le chemin incrémental et le chemin complet.
/// Applique une opération, remaille par BORNES, puis recharge TOUT sur la même
/// copie de travail — et exige que les deux donnent la même scène.
///
/// Le témoin doit être le même `Ouvert` : un second aurait sa PROPRE couche de
/// travail, donc il lirait la save d'AVANT l'opération. Ma première écriture
/// faisait ça, et le test accusait le remaillage d'une différence qui venait
/// du témoin.
fn croiser_les_deux_chemins(bloc: &str, sel: BBox, etiquette: &str) {
    let (pack, _codex) = pack();
    let jetable = monde(etiquette);
    let dir = jetable.chemin().to_path_buf();

    let mut vite = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), [0, 0, 1, 1]).unwrap();
    let mut moteur = Moteur::lancer(
        vite.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte(bloc));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel,
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    assert!(!r.echoue(), "{}", r.texte());
    let bornes = r.bornes().expect("l'opération a écrit");

    vite.remailler(Some(bornes))
        .expect("remaillage incrémental");
    // Ce qui est DESSINÉ, pas la disposition : les arènes rangent chaque
    // section à une place stable et laissent des trous, donc un remplacement
    // et un rechargement ne produisent pas les mêmes tableaux — et c'est
    // voulu. Couches ramenées à un dictionnaire commun, par NOM.
    let mut mots = Vec::new();
    let c = commun::canon(&vite, &mut mots);
    let rapide = (
        vite.monde.quads,
        vite.monde.poses,
        commun::montre(&vite, &c),
        commun::montre_modeles(&vite, &c),
    );

    vite.remailler(None).expect("rechargement complet");
    assert_eq!(
        rapide.0, vite.monde.quads,
        "{etiquette} : pas les mêmes quads"
    );
    assert_eq!(
        rapide.1, vite.monde.poses,
        "{etiquette} : pas les mêmes poses"
    );
    // Sur ce qui est DESSINÉ, pas sur un compte : deux maillages du même
    // nombre de quads peuvent décrire deux images différentes.
    let c = commun::canon(&vite, &mut mots);
    assert!(
        rapide.2 == commun::montre(&vite, &c),
        "{etiquette} : pas la même géométrie"
    );
    assert!(
        rapide.3 == commun::montre_modeles(&vite, &c),
        "{etiquette} : pas les mêmes faces de modèles"
    );

    moteur.arreter();
    drop(vite);
}

/// **Le remaillage incrémental doit donner LA MÊME image qu'un rechargement
/// complet.** Une optimisation qui change l'image n'est pas une optimisation.
#[test]
fn le_remaillage_incremental_donne_la_meme_scene_que_le_complet() {
    croiser_les_deux_chemins(
        // Un état DÉJÀ dans la scène : la fixture de terrain est de la pierre.
        "minecraft:stone",
        BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        "pose",
    );
}

/// **Une section qui se VIDE ne figure plus dans la save**, donc la relecture
/// ne la rend pas : si on ne la retirait pas de la grille d'abord, son ancien
/// contenu y resterait et les blocs effacés resteraient à l'écran. Ça ne se
/// voit que sur un effacement — une pose ne l'aurait jamais montré, et c'est
/// ce que ma première série de mutations a révélé.
#[test]
fn une_section_videe_disparait_vraiment() {
    croiser_les_deux_chemins(
        "minecraft:air",
        // Exactement une section : x 0..15, y −48..−33, z 0..15.
        BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
        "vide",
    );
}

/// **Un état que l'atlas ne connaît pas force le rechargement complet.**
/// L'atlas ne monte que les textures des blocs PRÉSENTS : poser un bloc dont
/// la scène n'avait jamais vu l'état lui donnerait une texture prise au hasard
/// dans la table voisine. Ça arrive une fois par type de bloc et par séance.
#[test]
fn un_etat_inconnu_force_le_rechargement_complet() {
    croiser_les_deux_chemins(
        // Absent d'une fixture de terrain, donc absent de l'atlas.
        "minecraft:glowstone",
        BBox::new(BlockPos::new(2, -44, 2), BlockPos::new(6, -40, 6)),
        "inconnu",
    );
}

/// **L'union de deux emprises, et pas la dernière.** Plusieurs opérations
/// peuvent répondre dans la même image : ne garder que la dernière laisserait
/// les précédentes à l'écran.
#[test]
fn deux_emprises_s_unissent() {
    use tf_app::scene::unir;
    let a = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(5, 5, 5));
    let b = BBox::new(BlockPos::new(-3, 10, 2), BlockPos::new(1, 12, 9));
    assert_eq!(unir(None, a), a);
    assert_eq!(
        unir(Some(a), b),
        BBox::new(BlockPos::new(-3, 0, 0), BlockPos::new(5, 12, 9))
    );
    // Commutative, sinon l'ordre d'arrivée des réponses changerait ce qu'on
    // remaille.
    assert_eq!(unir(Some(a), b), unir(Some(b), a));
}

/// **Ce que le remaillage incrémental fait GAGNER, mesuré.**
///
/// « On ne devine pas où est le poids » : ce test ne vérifie rien, il
/// IMPRIME. Un gain annoncé sans chiffre n'est pas un gain, et un chiffre
/// sans sa commande n'est pas une mesure — celle-ci est
/// `TF_PACK=… cargo test -p tf-app --test chantier mesurer -- --nocapture`.
///
/// Il n'assert PAS le rapport : une assertion de performance à une seule
/// mesure a déjà échoué dans ce dépôt sur du code intact.
#[test]
fn mesurer_les_deux_chemins() {
    let (pack, _codex) = pack();
    let jetable = monde("mesure");
    let dir = jetable.chemin().to_path_buf();

    // Une zone plus large que 2 × 2 : c'est là que la différence se voit, le
    // chemin complet payant la ZONE pendant que l'incrémental paie ce qui a
    // bougé.
    let zone = [0, 0, 7, 7];
    let t = std::time::Instant::now();
    let mut o = Ouvert::ouvrir(&pack, Some(dir.to_str().unwrap()), zone).unwrap();
    let ouverture = t.elapsed().as_secs_f64() * 1000.0;
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(dir.clone()),
    );

    // Trois blocs, comme un coup de pinceau — le cas qui compte.
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:stone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6)),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let bornes = attendre(&mut moteur).bornes().expect("écrit");

    // MÉDIANE de cinq, alternées : un premier passage à froid suffit à
    // rendre une mesure unique trompeuse.
    let (mut rapides, mut complets) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let t = std::time::Instant::now();
        o.remailler(Some(bornes)).unwrap();
        rapides.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = std::time::Instant::now();
        o.remailler(None).unwrap();
        complets.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let med = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let (r, c) = (med(rapides), med(complets));
    println!(
        "remaillage · incrémental {r:.2} ms · complet {c:.1} ms · × {:.0} \
         (zone de 64 chunks, {} quads · ouverture {ouverture:.0} ms)",
        c / r,
        o.monde.quads
    );

    moteur.arreter();
    drop(o);
}

/// Ce que coûte la RELECTURE, découpée. Voir `mesurer_les_deux_chemins` : le
/// remaillage incrémental ne gagnait que ×1,1, et la découpe a dit pourquoi —
/// 18 ms sur 18,3 partent dans la relecture, 0,2 dans le maillage.
#[test]
fn mesurer_la_relecture() {
    let (pack, _codex) = pack();
    let jetable = monde("relire");
    let o = Ouvert::ouvrir(&pack, Some(jetable.texte()), [0, 0, 1, 1]).unwrap();
    let st = o.staging.clone().unwrap();

    let med = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    // 1. lire les octets de la région
    let mut lire = Vec::new();
    let mut octets = 0;
    for _ in 0..5 {
        let t = std::time::Instant::now();
        let b = tf_world::source::RegionSource::read_region(
            st.as_ref(),
            &tf_world::Dimension::Overworld,
            tf_world::Folder::Region,
            tf_world::coords::RegionPos { x: 0, z: 0 },
        )
        .unwrap();
        lire.push(t.elapsed().as_secs_f64() * 1000.0);
        octets = b.len();
    }
    // 2. la chaîne complète, pour UNE section
    let mut tout = Vec::new();
    for _ in 0..5 {
        let mut i = tf_anvil::Interner::new();
        let t = std::time::Instant::now();
        tf_world::sections_de(
            st.as_ref(),
            &tf_world::Dimension::Overworld,
            tf_world::Folder::Region,
            &BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(15, -33, 15)),
            &mut i,
            |_| {},
        );
        tout.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "relecture · octets de région {:.1} ms ({:.1} Mo) · une section décodée {:.1} ms",
        med(lire),
        octets as f64 / 1e6,
        med(tout)
    );
    drop(o);
}

/// Ce que coûte le RECHARGEMENT complet, découpé — c'est lui qu'un état
/// inconnu déclenche, une fois par type de bloc et par séance.
#[test]
fn mesurer_le_rechargement() {
    let (pack, _codex) = pack();
    let jetable = monde("recharge");

    let t = std::time::Instant::now();
    let assets = tf_app::scene::Assets::charger(&pack).unwrap();
    let pack_ms = t.elapsed().as_secs_f64() * 1000.0;

    let mut o = Ouvert::ouvrir(&pack, Some(jetable.texte()), [0, 0, 7, 7]).unwrap();
    let mut v = Vec::new();
    for _ in 0..3 {
        let t = std::time::Instant::now();
        o.remailler(None).unwrap();
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "chargement · pack (une fois) {pack_ms:.0} ms · zone de 64 chunks {:.0} ms",
        v[1]
    );
    drop(assets);
    drop(o);
}
