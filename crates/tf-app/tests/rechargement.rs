//! **Aucune édition ne doit recharger la zone.**
//!
//! C'est le défaut qui a coûté le plus cher aux deux applications qui
//! précèdent celle-ci. Dans `ExeWorldEdit`, `applyOperation` chauffait le
//! build ENTIER avant toute opération : mesuré chez un utilisateur, une
//! sphère de 62 blocs sur une sélection de 413 millions de cases prenait
//! 5,2 s — 47 % à décoder des chunks jamais lus, 52 % à recoller l'aperçu
//! entier. Trois coûts en O(sélection) pour trois lignes fausses.
//!
//! Ici le remaillage est incrémental et ne paie que les bornes de
//! l'opération. Mais il reste UNE porte de sortie vers le chemin complet, et
//! elle s'ouvre toute seule : un état de bloc que l'atlas ne connaît pas.
//! Comme poser un bloc neuf est exactement ce qu'un éditeur sert à faire,
//! cette porte s'ouvre au geste le plus banal qui soit.
//!
//! Le compteur `Ouvert::rechargements` existe pour que ça se VÉRIFIE plutôt
//! que se chronomètre : un seuil de temps est une opinion, un compteur
//! répond oui ou non.

mod commun;

use std::time::{Duration, Instant};

use commun::{canon, codex, montre, montre_modeles, semer, streamer, Jetable};
use tf_app::chargeur::Chargeur;
use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_app::scene::Ouvert;
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos};
use tf_world::Journal;

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

/// Pose `bloc` sur une petite boîte et rend les bornes écrites.
fn poser(m: &mut Moteur, bloc: &str, sel: BBox) -> Option<BBox> {
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte(bloc));
    assert!(m.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel,
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    attendre(m).bornes()
}

/// **Poser un bloc que la scène ne contenait pas ne recharge pas la zone.**
///
/// C'est le geste le plus banal d'un éditeur : prendre un bloc dans la
/// palette et le poser. S'il coûte la zone entière, l'outil devient
/// inutilisable exactement au moment où l'on s'en sert.
#[test]
fn poser_un_bloc_inconnu_ne_recharge_pas_la_zone() {
    let j = Jetable::neuf("codex");
    // `emerald_block` est dans le codex mais ABSENT du terrain semé : l'atlas
    // de la scène ne le connaît donc pas au premier chargement.
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    assert_eq!(o.rechargements, 0, "l'ouverture ne compte pas");

    // 1. Un bloc DÉJÀ présent : le chemin incrémental, le témoin.
    let sel = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
    let bornes = poser(&mut moteur, "minecraft:stone", sel).expect("écrit");
    let t = Instant::now();
    o.remailler(Some(bornes)).expect("remaillage");
    let connu = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(
        o.rechargements, 0,
        "un bloc déjà présent n'a aucune raison de recharger"
    );

    // 2. Un bloc NEUF, au même endroit, de la même taille. La seule chose qui
    //    change est que l'atlas ne le connaît pas.
    let sel = BBox::new(BlockPos::new(4, -39, 4), BlockPos::new(6, -39, 6));
    let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");
    let t = Instant::now();
    o.remailler(Some(bornes)).expect("remaillage");
    let neuf = t.elapsed().as_secs_f64() * 1e3;

    println!(
        "zone de 64 chunks · bloc connu {connu:.1} ms · bloc NEUF {neuf:.1} ms \
         (× {:.0}) · rechargements {}",
        neuf / connu.max(1e-6),
        o.rechargements
    );
    assert_eq!(
        o.rechargements, 0,
        "poser un bloc neuf a rechargé la ZONE ENTIÈRE — c'est le \
         `warmup(extent)` d'ExeWorldEdit, qui a coûté 5,2 s pour une sphère \
         de 62 blocs"
    );

    // 3. Et le bloc neuf est VRAIMENT à l'écran. Sans ce contrôle, la façon
    //    la plus simple de faire passer le test serait de ne rien redessiner
    //    du tout — l'absence ne se voit pas.
    assert!(o.monde.quads > 0, "la scène doit porter de la géométrie");

    moteur.arreter();
}

/// **Étendre doit donner EXACTEMENT la même scène que rebâtir.**
///
/// Sans ce croisement, la façon la plus simple de faire passer le test
/// précédent serait de ne rien redessiner du tout : une absence ne se voit
/// pas. Et l'erreur la plus probable n'est pas l'absence mais le DÉCALAGE —
/// une table indexée par `StateId` prolongée d'un cran de travers donne à
/// chaque bloc la texture de son voisin, ce qui produit une image
/// parfaitement plausible et fausse.
#[test]
fn l_extension_rend_la_meme_scene_qu_un_rechargement() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block", "lapis_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );

    // Deux blocs neufs d'un coup : c'est le cas où un décalage d'un cran se
    // voit, puisque les deux se suivent dans la table.
    let a = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
    let bornes = poser(&mut moteur, "minecraft:emerald_block", a).expect("écrit");
    o.remailler(Some(bornes)).expect("remaillage");
    let b = BBox::new(BlockPos::new(8, -40, 8), BlockPos::new(10, -40, 10));
    let bornes = poser(&mut moteur, "minecraft:lapis_block", b).expect("écrit");
    o.remailler(Some(bornes)).expect("remaillage");

    assert_eq!(
        o.rechargements, 0,
        "aucun rechargement ne devait avoir lieu"
    );
    let (quads, poses) = (o.monde.quads, o.monde.poses);
    let couches = o.monde.atlas.len();

    // Le TÉMOIN se prend sur le MÊME `Ouvert`, donc sur la même copie de
    // travail : un second `Ouvert` aurait sa propre couche et lirait le monde
    // d'AVANT — un témoin qui ne voit pas le même monde accuse le mauvais
    // coupable.
    o.remailler(None).expect("rechargement complet");
    assert_eq!(o.rechargements, 1, "le témoin, lui, recharge bien");

    assert_eq!(
        (quads, poses),
        (o.monde.quads, o.monde.poses),
        "l'atlas étendu doit produire le même maillage qu'un atlas rebâti"
    );
    assert_eq!(
        couches,
        o.monde.atlas.len(),
        "et le même nombre de couches d'atlas"
    );
    println!(
        "extension contre rechargement : {quads} quads, {poses} poses, {couches} couches — \
         identiques"
    );

    moteur.arreter();
}

/// **Le coût du rechargement est en O(ZONE), celui de l'incrémental non.**
///
/// C'est ce qui rend le défaut insidieux : sur la petite zone d'un test il ne
/// coûte que quelques millisecondes, et chez l'utilisateur qui regarde une
/// région entière il fige la fenêtre.
///
/// **La propriété se COMPTE, elle ne se chronomètre pas.** Premier jet : un
/// rapport de TEMPS entre deux tailles de zone, avec un seuil de 3. Il passait
/// seul (rapport 2,5) et TOMBAIT quand la suite entière tournait en
/// parallèle — les temps absolus dérivent avec la charge, ce que ce dépôt a
/// mesuré à un facteur 2,4 à code identique. Un test rouge une fois sur trois
/// fait douter du code au lieu du test, ce qui est pire que pas de test. Le
/// nombre de sections REFAITES, lui, ne dépend d'aucune machine — et il dit
/// exactement la propriété qu'on veut.
#[test]
fn le_rechargement_paie_la_zone_pas_l_edition() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 16);

    let mesure = |zone: [i32; 4]| -> (usize, usize, f64, f64) {
        let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), zone).expect("monde ouvert");
        let mut moteur = Moteur::lancer(
            o.staging.clone().unwrap(),
            tf_world::Dimension::Overworld,
            Journal::new(),
            Some(m.chemin().to_path_buf()),
        );
        let sel = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
        let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");

        let t = Instant::now();
        o.remailler(Some(bornes)).expect("incrémental");
        let t_inc = t.elapsed().as_secs_f64() * 1e3;
        let inc = o.sections_remaillees;

        let t = Instant::now();
        o.remailler(None).expect("complet");
        let t_plein = t.elapsed().as_secs_f64() * 1e3;
        let plein = o.sections_remaillees;

        moteur.arreter();
        (inc, plein, t_inc, t_plein)
    };

    let (petit_inc, petit_plein, tpi, tpp) = mesure([0, 0, 3, 3]);
    let (grand_inc, grand_plein, tgi, tgp) = mesure([0, 0, 15, 15]);
    println!(
        "16 chunks : {petit_inc} sections refaites ({tpi:.2} ms) contre {petit_plein} \
au rechargement ({tpp:.1} ms)"
    );
    println!(
        "256 chunks : {grand_inc} sections refaites ({tgi:.2} ms) contre {grand_plein} \
au rechargement ({tgp:.1} ms)"
    );

    // Seize fois plus de chunks. Le rechargement le paie…
    assert!(
        grand_plein > petit_plein * 8,
        "un rechargement doit refaire la ZONE : {petit_plein} sections contre \
         {grand_plein} pour seize fois plus de chunks"
    );
    // …et l'édition ne doit RIEN devoir à la taille de la zone. Exactement
    // rien : les visées sont les sections que l'opération a touchées, plus
    // leur marge d'une case.
    assert_eq!(
        petit_inc, grand_inc,
        "une édition de trois blocs refait {petit_inc} sections sur une petite \
         zone et {grand_inc} sur une grande — elle paie donc la zone"
    );
}

/// **Les couches doivent porter les mêmes noms, étendues ou rebâties.**
///
/// Le croisement précédent compare des quads : c'est de la GÉOMÉTRIE, et un
/// indice de couche décalé d'un cran n'en change pas un seul. Mesuré par
/// mutation — décaler l'indice de couche passait les trois tests — alors que
/// c'est le défaut le plus probable de toute table indexée, et qu'il donne à
/// chaque bloc la texture de son VOISIN. Ce dépôt l'a déjà payé sous le nom
/// « un identifiant de voxel n'est pas un indice de palette ».
///
/// La propriété se dit donc sur l'APPARENCE : chaque nom doit désigner la
/// couche qui le porte, et l'atlas étendu doit être le même que l'atlas
/// rebâti — nom par nom, dans l'ordre.
#[test]
fn les_couches_de_l_atlas_etendu_sont_celles_de_l_atlas_rebati() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block", "lapis_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let avant: Vec<String> = o
        .monde
        .atlas
        .couches
        .iter()
        .map(|c| c.nom.clone())
        .collect();

    for (bloc, y) in [
        ("minecraft:emerald_block", -40),
        ("minecraft:lapis_block", -39),
    ] {
        let sel = BBox::new(BlockPos::new(4, y, 4), BlockPos::new(6, y, 6));
        let bornes = poser(&mut moteur, bloc, sel).expect("écrit");
        o.remailler(Some(bornes)).expect("remaillage");
    }
    assert_eq!(o.rechargements, 0);

    let etendu: Vec<String> = o
        .monde
        .atlas
        .couches
        .iter()
        .map(|c| c.nom.clone())
        .collect();
    // 1. Ce qui était là n'a pas BOUGÉ. C'est la propriété qui rend
    //    l'extension sûre : le maillage déjà produit porte ces indices.
    assert_eq!(
        &etendu[..avant.len()],
        &avant[..],
        "les couches déjà montées ont changé de place — tout ce qui est déjà \
         maillé afficherait la mauvaise texture"
    );
    assert!(
        etendu.len() > avant.len(),
        "les blocs neufs ont amené leurs textures"
    );

    // 2. Chaque nom désigne SA couche. Un cran de décalage et chaque bloc
    //    prend la texture de son voisin.
    for (i, c) in o.monde.atlas.couches.iter().enumerate() {
        assert_eq!(
            o.monde.atlas.couche(&c.nom),
            Some(i as u32),
            "la couche « {} » ne se retrouve pas à sa place",
            c.nom
        );
    }

    // 3. Le rechargement monte le même ENSEMBLE de textures — mais pas dans
    //    le même ordre, et c'est voulu. `batir` les range par nom ; étendre
    //    ajoute à la fin, parce que renuméroter est précisément ce qu'il ne
    //    faut pas faire : tout le maillage déjà produit porte les anciens
    //    indices. L'invariant est donc la CORRESPONDANCE nom → couche, jamais
    //    l'ordre du tableau. Premier jet de ce test : j'exigeais deux listes
    //    identiques, et il rougissait sur du code juste — ce qui envoie
    //    chercher un bug là où il n'y en a pas.
    o.remailler(None).expect("rechargement complet");
    let mut rebati: Vec<String> = o
        .monde
        .atlas
        .couches
        .iter()
        .map(|c| c.nom.clone())
        .collect();
    let mut trie = etendu.clone();
    trie.sort();
    rebati.sort();
    assert_eq!(
        trie, rebati,
        "étendre et rebâtir doivent monter les mêmes textures"
    );

    moteur.arreter();
}

/// **Une texture plus grande que l'atlas le fait GRANDIR — sans recharger.**
///
/// Un tableau n'a qu'une taille de couche. Ce cas RECHARGEAIT la zone : on
/// refusait de réécrire les pixels déjà montés, et on payait la zone entière
/// pour ne pas le faire — puis, sous le streaming, on la payait en boucle
/// (`une_texture_plus_grande_qui_arrive_en_volant_ne_recharge_pas`).
///
/// L'atlas agrandi doit être CELUI qu'un bâti direct aurait donné : mêmes
/// pixels, couche par couche, que le rechargement complet qui le rebâtit.
/// C'est ce qui prouve qu'agrandir sur place ne perd rien — ni la tuile neuve
/// (rien n'est réduit), ni les anciennes (agrandies au plus proche voisin).
#[test]
fn une_texture_plus_grande_agrandit_l_atlas_sans_recharger() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    // Le pack du serveur mélange du 16 et du 32 : c'est un cas réel.
    commun::texture_hd(j.chemin(), "emerald_block", 32);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 3, 3]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let cote_avant = o.monde.atlas.cote;
    let noms_avant: Vec<String> = o
        .monde
        .atlas
        .couches
        .iter()
        .map(|c| c.nom.clone())
        .collect();

    let sel = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
    let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");
    o.remailler(Some(bornes)).expect("remaillage");
    moteur.arreter();

    assert_eq!(
        o.rechargements, 0,
        "une tuile plus grande que l'atlas l'agrandit ; elle ne recharge pas la zone"
    );
    assert_eq!(
        o.monde.atlas.cote, 32,
        "l'atlas a pris le côté de la PLUS GRANDE ({cote_avant} avant)"
    );
    assert!(cote_avant < 32, "la prémisse : l'atlas partait plus petit");
    let noms: Vec<String> = o
        .monde
        .atlas
        .couches
        .iter()
        .map(|c| c.nom.clone())
        .collect();
    assert_eq!(
        &noms[..noms_avant.len()],
        &noms_avant[..],
        "agrandir ne déplace AUCUNE couche : le maillage déjà produit porte ces indices"
    );

    let par_nom = |o: &Ouvert| -> std::collections::BTreeMap<String, Vec<u8>> {
        let a = &o.monde.atlas;
        let n = (a.cote * a.cote * 4) as usize;
        a.couches
            .iter()
            .enumerate()
            .map(|(i, c)| (c.nom.clone(), a.pixels[i * n..(i + 1) * n].to_vec()))
            .collect()
    };
    let agrandi = par_nom(&o);
    o.remailler(None).expect("rechargement complet");
    let rebati = par_nom(&o);
    assert_eq!(o.monde.atlas.cote, 32);
    assert_eq!(
        agrandi.keys().collect::<Vec<_>>(),
        rebati.keys().collect::<Vec<_>>(),
        "les mêmes textures"
    );
    for (nom, px) in &agrandi {
        assert!(
            px == &rebati[nom],
            "la couche « {nom} » agrandie sur place diffère de celle d'un bâti direct"
        );
    }
    println!(
        "tuile 32 dans un atlas {cote_avant} : agrandi sur place, {} couches, aucun rechargement",
        agrandi.len()
    );
}

/// **Une texture plus grande qui arrive EN VOLANT ne recharge pas la zone.**
///
/// Trouvé en ouvrant la fenêtre sur le vrai codex : les crânes d'oiseau du
/// serveur sont en 32 × 32, la zone d'ouverture n'en contenait pas, et chaque
/// cellule qui en amenait un faisait RECHARGER la zone. Le rechargement
/// rebâtit l'atlas depuis la zone seule — sans le crâne, donc au même côté —
/// la caméra redemande la cellule, et tout recommence : cinquante-trois
/// rechargements en trente secondes, chacun en O(zone). C'est le défaut que
/// les deux applications précédentes avaient déjà payé.
#[test]
fn une_texture_plus_grande_qui_arrive_en_volant_ne_recharge_pas() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    commun::texture_hd(j.chemin(), "emerald_block", 32);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    // Le bloc en haute définition est posé LOIN de la zone d'ouverture, dans
    // la copie de travail — c'est elle que le fil lit.
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let sel = BBox::new(BlockPos::new(84, -40, 4), BlockPos::new(86, -40, 6));
    poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");
    moteur.arreter();
    let cote_avant = o.monde.atlas.cote;
    assert!(
        o.monde
            .atlas
            .couches
            .iter()
            .all(|c| !c.nom.contains("emerald")),
        "la prémisse : la zone d'ouverture ne contient pas le bloc"
    );

    let mut p = tf_app::pilote::Pilote::pour(
        &o,
        tf_world::Dimension::Overworld,
        tf_world::Niveau::Chunk,
        6,
        (-64, 319),
    );
    let debut = Instant::now();
    let mut vu = None;
    for i in 0..4000 {
        p.image(&mut o, BlockPos::new(8, 64, 8), [1.0, 0.0, 0.0], 2)
            .expect("une image");
        let arrive = o
            .monde
            .atlas
            .couches
            .iter()
            .any(|c| c.nom.contains("emerald"));
        if arrive && vu.is_none() {
            vu = Some(i);
        }
        // Deux cents images APRÈS l'arrivée : une boucle de rechargements a
        // le temps de se montrer.
        if vu.is_some_and(|v| i > v + 200) || o.rechargements > 3 {
            break;
        }
        assert!(
            debut.elapsed() < Duration::from_secs(120),
            "le vol n'avance plus"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    p.arreter();

    assert_eq!(
        o.rechargements, 0,
        "une texture plus grande que l'atlas l'AGRANDIT ; elle ne recharge pas la zone"
    );
    assert!(vu.is_some(), "la prémisse : le bloc est arrivé par le vol");
    assert_eq!(
        o.monde.atlas.cote, 32,
        "l'atlas a pris le côté de la plus grande ({cote_avant} avant)"
    );
}

/// **Éditer une cellule STREAMÉE ne la fait pas disparaître.**
///
/// Le remaillage d'une édition relisait ses sections en serrant la lecture
/// sur la ZONE d'ouverture — une règle d'avant le streaming, quand la zone
/// était toute la scène. Il retirait d'abord toutes les sections visées, puis
/// ne relisait que ce qui tombait dans la zone : éditer là où l'on avait volé
/// effaçait la cellule de la scène. Et comme elle restait inscrite à la
/// fenêtre de résidence, personne ne la redemandait — un trou définitif, au
/// premier geste d'édition loin du point de départ.
#[test]
fn editer_une_cellule_streamee_ne_la_fait_pas_disparaitre() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    // Trois cellules autour d'une diagonale (1, 1) qui, elle, n'est PAS
    // chargée : une lecture qui déborderait des résidentes la ferait naître.
    for (bx, bz) in [(40, 40), (24, 40), (40, 24)] {
        c.demander(tf_world::demande::par_region(&tf_world::demande::voulues(
            BlockPos::new(bx, 64, bz),
            [1.0, 0.0, 0.0],
            0,
            tf_world::Niveau::Chunk,
            (-64, 319),
        )));
        assert_eq!(
            streamer(&mut o, &mut c, 1),
            1,
            "la cellule de ({bx}, {bz}) arrive"
        );
    }
    c.arreter();
    let colonne = |o: &Ouvert| {
        (-4..20)
            .filter(|sy| o.monde.grille.section((2, 2, *sy)).is_some())
            .count()
    };
    let avant = colonne(&o);
    assert!(
        avant > 0,
        "la prémisse : la cellule streamée porte des sections"
    );

    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    // Au milieu de la cellule, puis dans son COIN : la croix de ce coin
    // touche (1, 2) et (2, 1), résidentes, et leur boîte couvre (1, 1), qui
    // ne l'est pas. Puis contre son bord +X, qui donne sur (3, 2), absente.
    for (x0, z0, x1, z1) in [(36, 36, 38, 38), (32, 32, 32, 32), (47, 40, 47, 40)] {
        let sel = BBox::new(BlockPos::new(x0, -40, z0), BlockPos::new(x1, -40, z1));
        let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");
        o.remailler(Some(bornes)).expect("remaillage");
    }
    moteur.arreter();

    assert_eq!(
        colonne(&o),
        avant,
        "éditer la cellule streamée l'a retirée de la scène"
    );
    for (x, z) in [(37, 37), (32, 32), (47, 40)] {
        assert_eq!(
            o.monde.etat_en(x, -40, z),
            "minecraft:emerald_block",
            "l'édition en ({x}, {z}) s'y voit"
        );
    }
    for (cx, cz) in [(1, 1), (3, 2)] {
        assert!(
            (-4..20).all(|sy| o.monde.grille.section((cx, cz, sy)).is_none()),
            "le chunk ({cx}, {cz}) n'est pas chargé : l'édition ne doit pas l'y faire naître"
        );
    }
    assert_eq!(o.rechargements, 0, "sans recharger la zone");
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "la fenêtre de résidence a suivi le poids de ce qu'on vient d'éditer"
    );
}

/// **Un état JAMAIS VU au bord d'une cellule qui arrive cache bien la face
/// de sa voisine.**
///
/// Une voisine n'est remaillée que si la couche qui la touche a quelque chose
/// d'opaque (`touchees_par_le_contenu`). Mais un état que la table ne connaît
/// pas encore s'y lit « pas opaque » : relevé avant que la table accueille
/// les états neufs, il laisse la voisine avec sa face — une face cachée, donc
/// invisible, mais une scène qui n'est plus celle d'un chargement direct.
///
/// Le cas est construit pour que rien d'autre ne sauve la mise : en plein
/// ciel, une pierre contre le bord +X du chunk (0, 0) et une émeraude — que
/// la zone d'ouverture ne connaît pas — contre le bord −X du chunk (1, 0).
#[test]
fn un_etat_neuf_au_bord_d_une_cellule_cache_la_face_de_sa_voisine() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);
    {
        let o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 1, 0]).expect("monde ouvert");
        let mut moteur = Moteur::lancer(
            o.staging.clone().unwrap(),
            tf_world::Dimension::Overworld,
            Journal::new(),
            Some(m.chemin().to_path_buf()),
        );
        for (x, bloc) in [(15, "minecraft:stone"), (16, "minecraft:emerald_block")] {
            let p = BlockPos::new(x, 150, 8);
            poser(&mut moteur, bloc, BBox::new(p, p)).expect("écrit");
        }
        assert!(moteur.envoyer(Commande::Ecrire {
            confirme_sans_verrou: true
        }));
        let r = attendre(&mut moteur);
        let Reponse::Ecrit { sauvegarde, .. } = &r else {
            panic!("attendu Ecrit, reçu {r:?} — {}", r.texte());
        };
        let _ = std::fs::remove_dir_all(sauvegarde);
        moteur.arreter();
    }

    // Le témoin : les deux chunks chargés d'un bloc.
    let temoin = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 1, 0]).expect("témoin");
    // Le streamé : le chunk (0, 0) d'abord, puis (1, 0) arrive.
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    assert!(
        o.monde
            .atlas
            .couches
            .iter()
            .all(|c| !c.nom.contains("emerald")),
        "la prémisse : la zone d'ouverture ne connaît pas l'émeraude"
    );
    assert_eq!(o.monde.etat_en(15, 150, 8), "minecraft:stone");
    // Ce qui n'est pas chargé est de l'AIR — pas l'état n° 0 de la table, qui
    // est ici du deepslate. Sans ça, le réticule s'arrêtait sur un mur
    // invisible au bord de ce qui est chargé.
    assert_eq!(o.monde.etat_en(16, 150, 8), "minecraft:air");
    assert!(
        !(o.monde.solide())([16, 150, 8]),
        "une case non chargée n'arrête pas le réticule"
    );
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    c.demander(tf_world::demande::par_region(&tf_world::demande::voulues(
        BlockPos::new(24, 64, 8),
        [1.0, 0.0, 0.0],
        0,
        tf_world::Niveau::Chunk,
        (-64, 319),
    )));
    assert_eq!(streamer(&mut o, &mut c, 1), 1, "le chunk (1, 0) arrive");
    c.arreter();
    assert_eq!(o.monde.etat_en(16, 150, 8), "minecraft:emerald_block");

    let mut mots = Vec::new();
    let ct = canon(&temoin, &mut mots);
    let cs = canon(&o, &mut mots);
    let (attendu, obtenu) = (montre(&temoin, &ct), montre(&o, &cs));
    assert_eq!(
        obtenu.len(),
        attendu.len(),
        "le streamé porte {} quads, le chargement direct {} : la face de la pierre \
         contre l'émeraude est restée",
        obtenu.len(),
        attendu.len()
    );
    assert!(obtenu == attendu, "la même scène que le chargement direct");
}

/// **Ce qu'une édition doit encore à la taille de la zone**, sur du BÂTI.
///
/// Le remaillage ne relit et ne maille que ses bornes ; mais les arènes GPU,
/// elles, se reconstruisent ENTIÈREMENT à chaque édition. Sur du sous-sol ça
/// ne se voit pas — une section homogène rend six quads — et c'est
/// exactement pourquoi la mesure se prend sur `Build` : une région bâtie
/// porte sept millions de quads et de poses là où du terrain en porte
/// trente-neuf mille.
///
/// Ce test ne fixe pas un seuil : il IMPRIME la découpe, pour qu'on sache où
/// va le prochain travail plutôt que de le deviner. Il vérifie seulement
/// qu'aucun rechargement ne s'est glissé là-dedans.
#[test]
fn ce_qu_une_edition_doit_encore_a_la_zone() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    commun::semer_build(m.chemin(), 16);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 15, 15]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );
    let sel = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
    let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");

    let mut v = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        o.remailler(Some(bornes)).expect("remaillage");
        v.push(t.elapsed().as_secs_f64() * 1e3);
    }
    v.sort_by(f64::total_cmp);
    println!(
        "BÂTI, 256 chunks, {} quads + {} poses : édition de 3 blocs en {:.2} ms \
         (médiane de 5) · rechargements {}",
        o.monde.quads,
        o.monde.poses,
        v[v.len() / 2],
        o.rechargements
    );
    assert_eq!(
        o.rechargements, 0,
        "une édition ne recharge jamais la zone, bâti ou pas"
    );
    moteur.arreter();
}

/// **L'arène remplacée doit dire la même chose que l'arène rebâtie.**
///
/// C'est l'invariant porteur du dépôt appliqué au rendu : *toute stratégie
/// rapide se compare au résultat de la stratégie lente, et le compte doit
/// être exact — pas « du même ordre »*. Une instance recopiée qui garde
/// l'indice de lot d'AVANT se dessine à la place d'une autre : un pan de
/// build posé ailleurs, sans la moindre erreur à l'écran.
///
/// **Mais pas au bit près, et c'est le piège que ce test a trouvé.** Premier
/// jet : je comparais les octets. Écart sur 276 328 instances sur 276 328 —
/// et uniquement sur le champ `couche`. La cause n'était pas un bug : un
/// atlas ÉTENDU ajoute ses couches à la fin, un atlas REBÂTI les range par
/// nom, donc les deux numérotent différemment. Comparer des numéros de couche
/// entre deux atlas, c'est comparer deux systèmes de coordonnées — exactement
/// ce que `StateId` interdit entre deux interners, sous une troisième forme.
/// Ce qui traverse les deux se compare par NOM.
#[test]
fn l_arene_remplacee_dit_la_meme_chose_qu_un_rebati() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &["emerald_block"]);
    let m = Jetable::neuf("monde");
    commun::semer_build(m.chemin(), 8);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("monde ouvert");
    let mut moteur = Moteur::lancer(
        o.staging.clone().unwrap(),
        tf_world::Dimension::Overworld,
        Journal::new(),
        Some(m.chemin().to_path_buf()),
    );

    // Trois éditions de suite, dont une avec un bloc NEUF : c'est
    // l'empilement qui fait dériver un état incrémental, pas la première
    // passe.
    for (bloc, y) in [
        ("minecraft:stone", -40),
        ("minecraft:emerald_block", -39),
        ("minecraft:stone", -38),
    ] {
        let sel = BBox::new(BlockPos::new(4, y, 4), BlockPos::new(6, y, 6));
        let bornes = poser(&mut moteur, bloc, sel).expect("écrit");
        o.remailler(Some(bornes)).expect("remaillage");
    }
    assert_eq!(o.rechargements, 0);

    // **Ce qui est DESSINÉ, pas la disposition.** Les arènes rangent chaque
    // section à une place stable et laissent des trous : un remplacement et
    // un rechargement ne produisent donc ni les mêmes tableaux, ni les mêmes
    // emplacements, ni les mêmes numéros de couche — et c'est voulu. Ce qui
    // doit être identique est l'image : chaque quad et chaque face de modèle,
    // à la même ORIGINE, avec la même texture par NOM. La passe de modèles se
    // rejoue comme le shader la dessine (`AreneModeles::dessinees`).
    let mut mots = Vec::new();
    let c = canon(&o, &mut mots);
    let vite = montre(&o, &c);
    let vite_faces = montre_modeles(&o, &c);

    // Le témoin, sur le MÊME `Ouvert` : un second aurait sa propre copie de
    // travail et lirait le monde d'AVANT.
    o.remailler(None).expect("rechargement complet");
    let c = canon(&o, &mut mots);
    let lent = montre(&o, &c);
    let lent_faces = montre_modeles(&o, &c);

    assert_eq!(
        vite.len(),
        lent.len(),
        "pas le même nombre de quads dessinés : {} contre {}",
        vite.len(),
        lent.len()
    );
    if let Some(k) = (0..vite.len()).find(|&k| vite[k] != lent[k]) {
        panic!(
            "quad {k} sur {} : remplacé {:?} contre rebâti {:?}",
            vite.len(),
            vite[k],
            lent[k]
        );
    }
    assert_eq!(
        vite_faces.len(),
        lent_faces.len(),
        "pas le même nombre de faces de modèles dessinées : {} contre {}",
        vite_faces.len(),
        lent_faces.len()
    );
    if let Some(k) = (0..vite_faces.len()).find(|&k| vite_faces[k] != lent_faces[k]) {
        panic!(
            "face {k} sur {} : remplacée {:?} contre rebâtie {:?}",
            vite_faces.len(),
            vite_faces[k],
            lent_faces[k]
        );
    }
    let lent_poses = lent_faces;
    println!(
        "arène : {} quads et {} faces de modèles, la même image par remplacement et par rechargement",
        lent.len(),
        lent_poses.len()
    );
    moteur.arreter();
}

/// **Un rechargement pendant le streaming ne laisse pas de cellule fantôme.**
///
/// Un rechargement remplace le monde par la seule ZONE : les cellules
/// streamées n'y sont plus. Les garder inscrites à la fenêtre de résidence
/// les ferait compter pour zéro octet — donc invisibles à la comptabilité —
/// tout en les déclarant résidentes. Une cellule déclarée résidente et jamais
/// chargée est un trou dans le monde que rien ne vient combler : la demande
/// la croit là et ne la redemande plus.
///
/// Rien d'autre ne le voit. Le contenu affiché est juste (c'est celui de la
/// zone rechargée), la mémoire est bornée, la comptabilité reste exacte —
/// seule la LISTE des résidentes ment.
///
/// Le rechargement est ici DEMANDÉ (`remailler(None)`) : une arrivée n'en
/// déclenche plus aucun — l'atlas grandit sur place.
#[test]
fn un_rechargement_pendant_le_streaming_ne_laisse_pas_de_cellule_fantome() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);

    // La zone d'ouverture : UN chunk. C'est tout ce qui doit rester résident.
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    assert_eq!(o.residentes(), 1, "la zone, et elle seule");

    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    let cellule = |bx: i32, bz: i32| {
        tf_world::demande::par_region(&tf_world::demande::voulues(
            BlockPos::new(bx, 64, bz),
            [1.0, 0.0, 0.0],
            0,
            tf_world::Niveau::Chunk,
            (-64, 319),
        ))
    };
    for (bx, bz) in [(24, 24), (40, 40)] {
        let lots = cellule(bx, bz);
        assert_eq!(
            lots.iter().map(|l| l.cellules.len()).sum::<usize>(),
            1,
            "un rayon de zéro demande la cellule où l'on est"
        );
        c.demander(lots);
        assert_eq!(streamer(&mut o, &mut c, 1), 1);
    }
    c.arreter();
    assert_eq!(o.rechargements, 0, "streamer ne recharge jamais");
    assert_eq!(
        o.residentes(),
        3,
        "la zone, plus les deux cellules streamées"
    );

    o.remailler(None).expect("rechargement complet");
    assert_eq!(o.rechargements, 1);
    assert_eq!(
        o.residentes(),
        1,
        "le rechargement remet la scène à la ZONE : les cellules streamées n'en \
         font plus partie et ne doivent pas rester inscrites"
    );
    assert!(
        (-4..20).all(|sy| o.monde.grille.section((2, 2, sy)).is_none()),
        "et elles ne sont effectivement plus dans la grille"
    );
    assert_eq!(
        o.octets_comptes(),
        o.octets_residents(),
        "la comptabilité reste exacte après le rechargement"
    );
}
