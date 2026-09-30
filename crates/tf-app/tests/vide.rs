//! **Un monde VIDE, de l'ouverture à l'écran.**
//!
//! Le moteur sait créer les chunks d'un monde vide (`tf-ops/tests/vide.rs`),
//! et la coque sait viser le plan de référence (`tests/etat.rs`). Chaque
//! moitié passe ses tests ; ce fichier vérifie leur JONCTION, là où ce dépôt
//! a déjà payé : une save sans aucune région s'ouvre, une opération y crée ses
//! chunks, le remaillage les MONTRE, et l'annulation les retire de l'écran
//! comme du disque.

mod commun;

use std::time::{Duration, Instant};

use commun::{codex, Jetable};
use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_app::scene::Ouvert;
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::Journal;

/// Le `level.dat` du préréglage « The Void » de 1.18.2 : un monde plat d'une
/// couche d'air, apparition en y = −60. Écrit octet par octet — c'est le
/// format que le jeu lit, et la fixture reste lisible en revue.
fn level_dat_du_vide() -> Vec<u8> {
    fn nom(v: &mut Vec<u8>, t: u8, n: &str) {
        v.push(t);
        v.extend_from_slice(&(n.len() as u16).to_be_bytes());
        v.extend_from_slice(n.as_bytes());
    }
    fn texte(v: &mut Vec<u8>, n: &str, s: &str) {
        nom(v, 8, n);
        v.extend_from_slice(&(s.len() as u16).to_be_bytes());
        v.extend_from_slice(s.as_bytes());
    }
    fn entier(v: &mut Vec<u8>, n: &str, i: i32) {
        nom(v, 3, n);
        v.extend_from_slice(&i.to_be_bytes());
    }
    let mut v = Vec::new();
    nom(&mut v, 10, "");
    nom(&mut v, 10, "Data");
    entier(&mut v, "DataVersion", 2975);
    texte(&mut v, "LevelName", "Vide");
    entier(&mut v, "SpawnX", 0);
    entier(&mut v, "SpawnY", -60);
    entier(&mut v, "SpawnZ", 0);
    nom(&mut v, 10, "WorldGenSettings");
    nom(&mut v, 10, "dimensions");
    nom(&mut v, 10, "minecraft:overworld");
    texte(&mut v, "type", "minecraft:overworld");
    nom(&mut v, 10, "generator");
    texte(&mut v, "type", "minecraft:flat");
    nom(&mut v, 10, "settings");
    // layers : une liste d'UN composé { block: air, height: 1 }.
    nom(&mut v, 9, "layers");
    v.push(10);
    v.extend_from_slice(&1i32.to_be_bytes());
    texte(&mut v, "block", "minecraft:air");
    entier(&mut v, "height", 1);
    v.push(0);
    texte(&mut v, "biome", "minecraft:the_void");
    // settings, generator, overworld, dimensions, WorldGenSettings, Data,
    // racine.
    v.extend_from_slice(&[0; 7]);
    tf_anvil::deflate(&v, tf_anvil::Compression::Gzip).expect("gzip")
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

/// **Une save vide, sans une seule région : on l'ouvre, on y remplit, on le
/// voit, on l'annule.**
#[test]
fn un_monde_vide_se_remplit_a_l_ecran_et_s_annule() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("vide");
    std::fs::write(m.chemin().join("level.dat"), level_dat_du_vide()).unwrap();
    let niveau = tf_world::niveau::lire_fichier(m.chemin()).expect("level.dat lisible");
    assert!(niveau.monde_vide().is_some(), "la prémisse : un monde vide");

    let zone = tf_app::accueil::zone_d_ouverture(Some(&niveau));
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), zone).expect("une save vide s'ouvre");
    assert_eq!(o.monde.quads, 0, "rien à montrer");
    assert_eq!(o.monde.bornes_d_ouverture(), None, "et donc rien à cadrer");
    // Le découpage se pose SUR le plan : sans lui, ses boîtes flotteraient
    // autour du cube par défaut, à soixante blocs du seul repère qu'il y ait.
    assert_eq!(o.monde.hauteurs_du_decoupage(Some(-60)), (-61, -61));
    assert_eq!(
        o.monde.hauteurs_du_decoupage(None),
        (0, 64),
        "le défaut d'avant"
    );

    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    // Un sol de pierre SUR le plan de l'apparition, à cheval sur quatre
    // chunks de la zone ouverte.
    let sel = BBox::new(BlockPos::new(-4, -61, -4), BlockPos::new(3, -61, 3));
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:stone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel,
        forme: Forme::Boite,
        compter: true,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    let Reponse::Fait { resume, .. } = &r else {
        panic!("attendu Fait, reçu {r:?}");
    };
    assert!(resume.contains("4 chunk(s) créé(s)"), "{resume}");
    o.remailler(r.bornes()).expect("remaillage");
    assert_eq!(
        o.monde.etat_en(0, -61, 0),
        "minecraft:stone",
        "le sol se voit"
    );
    assert_eq!(o.monde.etat_en(-4, -61, 3), "minecraft:stone");
    assert!(o.monde.quads > 0, "la scène porte de la géométrie");

    // L'annulation retire le sol de l'ÉCRAN — et, en dessous, les chunks et
    // la région qu'elle avait créés.
    assert!(moteur.envoyer(Commande::Annuler));
    let r = attendre(&mut moteur);
    assert!(matches!(r, Reponse::Defait { .. }), "{r:?}");
    o.remailler(r.bornes()).expect("remaillage");
    assert_eq!(o.monde.etat_en(0, -61, 0), "minecraft:air");
    assert_eq!(o.monde.quads, 0, "le sol annulé est parti de l'écran");

    // Au-delà du plafond de création, rien n'est créé — et la réponse le dit
    // en termes de monde VIDE : « allez-y en jeu » n'y changerait rien.
    let cote = 16 * 131 - 1;
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:stone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::new(BlockPos::new(0, -61, 0), BlockPos::new(cote, -61, cote)),
        forme: Forme::Boite,
        compter: true,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    // Rien n'existe sous la zone : l'opération n'écrit rien, et dit pourquoi.
    assert!(matches!(r, Reponse::Rien(_)), "{r:?}");
    let resume = r.texte();
    assert!(
        resume.contains("17161 chunk(s) non créés") && resume.contains("plafond"),
        "{resume}"
    );
    assert!(!resume.contains("en jeu"), "{resume}");
    moteur.arreter();
    assert_eq!(o.rechargements, 0, "sans recharger la zone");
}

/// **Construire LOIN de l'ouverture, là où l'on est arrivé en volant.** La
/// cellule y est arrivée VIDE — aucune région, aucun chunk. Une opération
/// y crée ses chunks : le remaillage doit les montrer, comme il montre une
/// édition dans une cellule qui avait du terrain. Une cellule arrivée vide
/// que la scène oublierait resterait vide à l'écran pour toujours, puisque
/// personne ne la redemande.
#[test]
fn construire_dans_une_cellule_arrivee_vide_se_voit() {
    use tf_app::chargeur::Chargeur;
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("vide-loin");
    std::fs::write(m.chemin().join("level.dat"), level_dat_du_vide()).unwrap();

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [-1, -1, 1, 1]).expect("ouvert");
    // On vole jusqu'au chunk (12, 12) : la cellule arrive, vide.
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    c.demander(tf_world::demande::par_region(&tf_world::demande::voulues(
        BlockPos::new(200, -60, 200),
        [1.0, 0.0, 0.0],
        0,
        tf_world::Niveau::Chunk,
        (-64, 319),
    )));
    assert_eq!(commun::streamer(&mut o, &mut c, 1), 1, "la cellule arrive");
    c.arreter();

    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let sel = BBox::new(BlockPos::new(198, -61, 198), BlockPos::new(201, -61, 201));
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:stone"));
    assert!(moteur.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel,
        forme: Forme::Boite,
        compter: true,
        seed: 0,
    }));
    let r = attendre(&mut moteur);
    let Reponse::Fait { resume, .. } = &r else {
        panic!("attendu Fait, reçu {r:?}");
    };
    assert!(resume.contains("chunk(s) créé(s)"), "{resume}");
    o.remailler(r.bornes()).expect("remaillage");
    moteur.arreter();
    assert_eq!(
        o.monde.etat_en(200, -61, 200),
        "minecraft:stone",
        "ce qu'on a construit dans la cellule arrivée vide ne se voit pas"
    );
    assert_eq!(o.rechargements, 0);
}

/// Dans un monde qui a du contenu, le plan ÉTEND les hauteurs du découpage
/// jusqu'à lui, sans les remplacer : le découpage doit border le terrain ET
/// le plan où l'on construit au-dessus.
#[test]
fn le_plan_etend_les_hauteurs_du_decoupage_sans_les_remplacer() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("terrain");
    commun::semer(m.chemin(), 1, 2);
    let o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 1, 1]).expect("ouvert");
    let (a, b) = o.monde.bornes_d_ouverture().expect("du terrain");
    let (y0, y1) = (a[1] as i32, b[1] as i32);
    assert!(y0 < y1, "la prémisse : du relief");
    assert_eq!(o.monde.hauteurs_du_decoupage(None), (y0, y1));
    assert_eq!(o.monde.hauteurs_du_decoupage(Some(y1 + 20)), (y0, y1 + 19));
    assert_eq!(o.monde.hauteurs_du_decoupage(Some(y0 - 5)), (y0 - 6, y1));
}
