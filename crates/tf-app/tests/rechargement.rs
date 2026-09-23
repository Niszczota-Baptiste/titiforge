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

use commun::{codex, semer, Jetable};
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

/// **Une texture plus grande que l'atlas se REPLIE sur un rechargement.**
///
/// Un tableau n'a qu'une taille de couche. Accueillir une tuile plus grande
/// demanderait de réécrire tous les pixels déjà montés ; la réduire perdrait
/// la moitié des siens, ce que le dépôt s'interdit (« on agrandit les petites
/// plutôt que de réduire les grandes »). Le repli est donc la bonne réponse —
/// mais il doit être EXPLICITE, pas un silence.
///
/// Mesuré par mutation : accepter la tuile trop grande passait tous les
/// autres tests, parce qu'ils n'emploient que du 16 × 16.
#[test]
fn une_texture_trop_grande_recharge_au_lieu_de_reduire() {
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

    let sel = BBox::new(BlockPos::new(4, -40, 4), BlockPos::new(6, -40, 6));
    let bornes = poser(&mut moteur, "minecraft:emerald_block", sel).expect("écrit");
    o.remailler(Some(bornes)).expect("remaillage");

    assert_eq!(
        o.rechargements, 1,
        "une tuile plus grande que l'atlas doit forcer un rechargement"
    );
    assert!(
        o.monde.atlas.cote > cote_avant,
        "et l'atlas rebâti doit prendre le côté de la PLUS GRANDE : {} après {cote_avant}",
        o.monde.atlas.cote
    );
    println!(
        "tuile 32 dans un atlas 16 : rechargement, côté {cote_avant} → {}",
        o.monde.atlas.cote
    );

    moteur.arreter();
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

    /// Ce qu'une instance VEUT DIRE : sa géométrie, sa teinte, son lot, et le
    /// NOM de sa texture — jamais le numéro de couche, qui n'a de sens que
    /// relativement à son atlas.
    fn sens(o: &Ouvert, c: &[u32]) -> Vec<(u32, u32, u32, u32)> {
        o.monde
            .arene
            .instances
            .iter()
            .map(|i| {
                let couche = c.get(i.couche as usize).copied().unwrap_or(u32::MAX);
                (i.geo, i.teinte, i.section, couche)
            })
            .collect()
    }

    /// **Ramène les couches d'atlas d'un côté à un dictionnaire COMMUN.**
    ///
    /// Un numéro de couche n'a de sens que relativement à son atlas, et les
    /// deux chemins n'ont pas le même : l'un ÉTEND (à la fin), l'autre REBÂTIT
    /// (par nom). Comparer les numéros, c'est comparer deux systèmes de
    /// coordonnées — le piège du `StateId` relatif à son interner, et il se
    /// referme ici sur DEUX champs à la fois : `InstanceQuad::couche` et
    /// `FaceModele::couche`.
    fn canon(o: &Ouvert, mots: &mut Vec<String>) -> Vec<u32> {
        o.monde
            .atlas
            .couches
            .iter()
            .map(|c| match mots.iter().position(|m| *m == c.nom) {
                Some(i) => i as u32,
                None => {
                    mots.push(c.nom.clone());
                    mots.len() as u32 - 1
                }
            })
            .collect()
    }

    /// Ce qu'une POSE veut dire : sa case, son lot, son rang de face, et la
    /// GÉOMÉTRIE qu'elle désigne — couches ramenées au dictionnaire commun.
    fn poses(o: &Ouvert, c: &[u32]) -> Vec<(u32, u32, u32, Vec<u8>)> {
        let m = &o.monde.modeles;
        m.poses
            .iter()
            .enumerate()
            .map(|(k, p)| {
                // **Le nombre de faces d'une pose se DÉDUIT de la somme
                // préfixe**, il ne se devine pas : `debut_modele` indexe une
                // table dont l'ordre diffère entre les deux chemins.
                let fin = m
                    .poses
                    .get(k + 1)
                    .map(|q| q.debut_face)
                    .unwrap_or(m.faces_a_dessiner);
                let nombre = (fin - p.debut_face) as usize;
                let d = p.debut_modele as usize;
                let g: Vec<tf_render::FaceModele> = m.faces[d..(d + nombre).min(m.faces.len())]
                    .iter()
                    .map(|f| tf_render::FaceModele {
                        couche: c.get(f.couche as usize).copied().unwrap_or(u32::MAX),
                        ..*f
                    })
                    .collect();
                (
                    p.local,
                    p.section,
                    p.debut_face,
                    bytemuck::cast_slice(&g).to_vec(),
                )
            })
            .collect()
    }

    let mut mots = Vec::new();
    let c = canon(&o, &mut mots);
    let vite = sens(&o, &c);
    let vite_poses = poses(&o, &c);
    let faces_vite = o.monde.modeles.faces_a_dessiner;
    let tranches = o.monde.arene.tranches.clone();
    let tranches_m = o.monde.modeles.tranches.clone();
    let origines: Vec<u8> = bytemuck::cast_slice(&o.monde.arene.origines).to_vec();

    // Le témoin, sur le MÊME `Ouvert` : un second aurait sa propre copie de
    // travail et lirait le monde d'AVANT.
    o.remailler(None).expect("rechargement complet");
    let c = canon(&o, &mut mots);
    let lent = sens(&o, &c);

    assert_eq!(
        vite.len(),
        lent.len(),
        "pas le même nombre d'instances : {} contre {}",
        vite.len(),
        lent.len()
    );
    if let Some(k) = (0..vite.len()).find(|&k| vite[k] != lent[k]) {
        panic!(
            "instance {k} sur {} : remplacée {:?} contre rebâtie {:?}",
            vite.len(),
            vite[k],
            lent[k]
        );
    }
    assert_eq!(
        tranches.len(),
        o.monde.arene.tranches.len(),
        "pas le même nombre de tranches"
    );
    assert_eq!(
        origines,
        bytemuck::cast_slice::<_, u8>(&o.monde.arene.origines).to_vec(),
        "les origines de section doivent suivre les lots"
    );
    // **La passe de MODÈLES aussi.** Elle porte une somme préfixe
    // (`debut_face`) que le shader dichotomise : un seul rang faux et les
    // faces d'une pose se dessinent pour une autre.
    let lent_poses = poses(&o, &c);
    assert_eq!(
        vite_poses.len(),
        lent_poses.len(),
        "pas le même nombre de poses"
    );
    if let Some(k) = (0..vite_poses.len()).find(|&k| vite_poses[k] != lent_poses[k]) {
        let (a, b) = (&vite_poses[k], &lent_poses[k]);
        panic!(
            "pose {k} sur {} : remplacée (local {}, lot {}, rang {}) contre \
             rebâtie (local {}, lot {}, rang {}) — géométries {}",
            vite_poses.len(),
            a.0,
            a.1,
            a.2,
            b.0,
            b.1,
            b.2,
            if a.3 == b.3 {
                "identiques".to_string()
            } else {
                format!("DIFFÉRENTES ({} octets contre {})", a.3.len(), b.3.len())
            }
        );
    }
    assert_eq!(
        faces_vite, o.monde.modeles.faces_a_dessiner,
        "le total de faces à dessiner doit être le même"
    );
    assert_eq!(
        tranches_m.len(),
        o.monde.modeles.tranches.len(),
        "pas le même nombre de tranches de modèles"
    );
    println!(
        "arène : {} instances et {} poses, même sens entre remplacement et rechargement",
        lent.len(),
        lent_poses.len()
    );
    moteur.arreter();
}
