//! **La jonction entre le fil de chargement et la scène.**
//!
//! Chacune des deux moitiés est juste de son côté : le fil rend des sections,
//! la scène sait mailler. Ça ne dit rien de leur raccord, et c'est là que les
//! fautes se paient — la fusion des tables d'états, le retrait de ce qui
//! n'existe plus, la marge du remaillage. Ce dépôt a déjà payé deux fois « une
//! jonction que personne n'écrit est une jonction que chaque hôte réécrira ».
//!
//! La propriété porteuse est celle du dépôt : *toute stratégie rapide se
//! compare au résultat de la stratégie lente*. Charger cellule par cellule
//! doit donner exactement la scène qu'un chargement d'un bloc donne.

mod commun;

use std::time::{Duration, Instant};

use commun::{codex, semer, semer_build, Jetable};
use tf_app::chargeur::{Chargeur, Reponse};
use tf_app::scene::{Arrivee, Ouvert};
use tf_world::coords::BlockPos;
use tf_world::demande::{par_region, voulues};
use tf_world::Niveau;

const EST: [f32; 3] = [1.0, 0.0, 0.0];
const HAUTEUR: (i32, i32) = (-64, 319);

/// Ramène les couches d'atlas d'un côté à un dictionnaire COMMUN.
///
/// Un numéro de couche n'a de sens que relativement à son atlas, et les deux
/// chemins n'ont pas le même : celui qui charge d'un bloc bâtit son atlas par
/// nom, celui qui streame l'ÉTEND à mesure. Comparer les numéros, c'est
/// comparer deux systèmes de coordonnées.
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

/// Ce que la scène MONTRE : chaque quad par sa géométrie, sa teinte, son
/// origine de section et le NOM de sa texture.
///
/// L'origine et non l'indice de lot : l'ordre des lots dépend de l'ordre
/// d'arrivée des cellules, qui est justement ce qui diffère entre les deux
/// chemins. Ce qu'on veut savoir est si les mêmes quads sont au même ENDROIT.
fn montre(o: &Ouvert, c: &[u32]) -> Vec<(u32, u32, [u32; 4], String)> {
    let mut v: Vec<(u32, u32, [u32; 4], String)> = o
        .monde
        .arene
        .instances
        .iter()
        .map(|i| {
            let nom = o
                .monde
                .atlas
                .couches
                .get(i.couche as usize)
                .map(|x| x.nom.clone())
                .unwrap_or_default();
            let org = o
                .monde
                .arene
                .origines
                .get(i.section as usize)
                .map(|p| p.position.map(|f| f.to_bits()))
                .unwrap_or([0; 4]);
            (
                i.geo,
                i.teinte,
                org,
                c.get(i.couche as usize).map(|_| nom).unwrap_or_default(),
            )
        })
        .collect();
    // L'ORDRE des instances suit l'ordre des lots, donc l'ordre d'arrivée.
    // Ce qui doit être identique est l'ENSEMBLE, pas la suite.
    v.sort();
    v
}

/// Fait tourner le chargeur jusqu'à ce que `n` cellules soient intégrées.
fn streamer(o: &mut Ouvert, c: &mut Chargeur, n: usize) -> usize {
    let debut = Instant::now();
    let mut faites = 0;
    while faites < n && debut.elapsed() < Duration::from_secs(60) {
        let lot = c.recevoir(0);
        if lot.is_empty() {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        let mut arrivees = Vec::new();
        for r in lot {
            match r {
                Reponse::Prete {
                    cellule,
                    sections,
                    interner,
                } => arrivees.push(Arrivee {
                    cellule,
                    sections,
                    interner,
                }),
                Reponse::Echec(e) => panic!("le chargeur a échoué : {e}"),
            }
        }
        faites += arrivees.len();
        o.integrer(arrivees).expect("intégration");
    }
    faites
}

/// **Charger cellule par cellule donne la MÊME scène qu'un chargement d'un
/// bloc.**
///
/// Le monde semé n'occupe que les chunks 0..7 de la région (0, 0) : les
/// cellules du disque qui tombent au-delà sont vides et n'ajoutent rien. Les
/// deux chemins couvrent donc exactement le même contenu, et la comparaison a
/// un sens.
///
/// Les deux `Ouvert` lisent la même SOURCE et personne n'édite : contrairement
/// au piège du témoin qui voyait le monde d'avant, il n'y a rien ici qui
/// puisse diverger entre les deux copies de travail.
#[test]
fn charger_cellule_par_cellule_donne_la_meme_scene() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 8);

    // Le TÉMOIN : les 64 chunks d'un coup.
    let temoin = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("témoin");
    assert!(
        temoin.monde.quads > 0,
        "le témoin doit porter de la géométrie"
    );

    // Le chemin streamé : on part d'UN chunk, le reste arrive par le fil.
    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);

    // Un œil au milieu des 8 × 8 chunks, un disque qui les couvre tous.
    let lots = par_region(&voulues(
        BlockPos::new(56, 64, 56),
        EST,
        8,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    assert_eq!(
        streamer(&mut o, &mut c, n),
        n,
        "toutes les cellules doivent arriver"
    );
    assert_eq!(
        o.rechargements, 0,
        "streamer ne doit JAMAIS recharger la zone"
    );

    let mut mots = Vec::new();
    let ct = canon(&temoin, &mut mots);
    let attendu = montre(&temoin, &ct);
    let cs = canon(&o, &mut mots);
    let obtenu = montre(&o, &cs);

    assert_eq!(
        obtenu.len(),
        attendu.len(),
        "pas le même nombre de quads : {} streamés contre {} d'un bloc",
        obtenu.len(),
        attendu.len()
    );
    if let Some(k) = (0..obtenu.len()).find(|&k| obtenu[k] != attendu[k]) {
        panic!(
            "quad {k} sur {} : streamé {:?} contre d'un bloc {:?}",
            obtenu.len(),
            obtenu[k],
            attendu[k]
        );
    }
    assert_eq!(
        o.monde.poses, temoin.monde.poses,
        "pas le même nombre de poses de blocs-modèles"
    );
    println!(
        "streamé en {n} cellules : {} quads, identiques au chargement d'un bloc",
        o.monde.quads
    );
    c.arreter();
}

/// La même chose sur du BÂTI, où le maillage porte vraiment quelque chose.
///
/// `Terrain` se maille en presque rien — une section homogène rend six quads —
/// donc un test qui n'emploie que lui pourrait passer en ne posant presque
/// rien. C'est la leçon du codex qui ne couvrait pas `minefield:*` : une
/// fixture qui ne mesure pas ce qu'elle prétend mesurer est un test vert qui
/// ne dit rien.
#[test]
fn le_streaming_tient_aussi_sur_du_bati() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), 8);

    let temoin = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 7, 7]).expect("témoin");
    assert!(
        temoin.monde.quads > 10_000,
        "la prémisse : du bâti doit porter beaucoup de quads, pas {}",
        temoin.monde.quads
    );
    assert!(temoin.monde.poses > 0, "et des blocs-modèles");

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    let lots = par_region(&voulues(
        BlockPos::new(56, 64, 56),
        EST,
        8,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    assert_eq!(streamer(&mut o, &mut c, n), n);
    assert_eq!(o.rechargements, 0);

    let mut mots = Vec::new();
    let ct = canon(&temoin, &mut mots);
    let attendu = montre(&temoin, &ct);
    let cs = canon(&o, &mut mots);
    let obtenu = montre(&o, &cs);
    assert_eq!(obtenu.len(), attendu.len(), "pas le même nombre de quads");
    assert_eq!(
        obtenu, attendu,
        "le bâti streamé diffère du bâti chargé d'un bloc"
    );
    assert_eq!(o.monde.poses, temoin.monde.poses);
    println!(
        "bâti streamé : {} quads et {} poses, identiques",
        o.monde.quads, o.monde.poses
    );
    c.arreter();
}

/// **Ce qu'intégrer coûte, une cellule à la fois contre par LOT.**
///
/// Le remplacement de tranches recopie les deux arènes : son coût est en
/// O(scène), pas en O(cellules intégrées). Une par une, c'est donc cette
/// recopie qu'on paie N fois. Le test IMPRIME les deux, parce qu'un seuil
/// absolu dépendrait de la machine, et vérifie ce qui ne dépend de rien :
/// qu'aucun rechargement ne s'y glisse.
#[test]
fn integrer_par_lot_amortit_la_recopie_des_arenes() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer_build(m.chemin(), 16);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    let lots = par_region(&voulues(
        BlockPos::new(128, 64, 128),
        EST,
        8,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);

    // `budget` = combien de cellules l'hôte prend dans une image.
    let mesurer = |o: &mut Ouvert, c: &mut Chargeur, budget: usize| -> (f64, usize) {
        let mut total = 0.0;
        let mut faites = 0;
        let debut = Instant::now();
        while faites < n && debut.elapsed() < Duration::from_secs(120) {
            let lot = c.recevoir(budget);
            if lot.is_empty() {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let arrivees: Vec<Arrivee> = lot
                .into_iter()
                .filter_map(|r| match r {
                    Reponse::Prete {
                        cellule,
                        sections,
                        interner,
                    } => Some(Arrivee {
                        cellule,
                        sections,
                        interner,
                    }),
                    Reponse::Echec(_) => None,
                })
                .collect();
            faites += arrivees.len();
            let t = Instant::now();
            o.integrer(arrivees).expect("intégration");
            total += t.elapsed().as_secs_f64() * 1e3;
        }
        (total, faites)
    };

    let (une_a_une, faites) = mesurer(&mut o, &mut c, 1);
    assert_eq!(faites, n);
    assert_eq!(
        o.rechargements, 0,
        "une cellule intégrée ne doit jamais recharger la zone"
    );
    let quads = o.monde.quads;
    c.arreter();

    // Le même chargement, mais par lots — ce que l'hôte fera.
    let mut o2 = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("second monde");
    let mut c2 = Chargeur::lancer(o2.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    c2.demander(par_region(&voulues(
        BlockPos::new(128, 64, 128),
        EST,
        8,
        Niveau::Chunk,
        HAUTEUR,
    )));
    let (par_lot, faites2) = mesurer(&mut o2, &mut c2, 0);
    assert_eq!(faites2, n);
    assert_eq!(o2.rechargements, 0);
    assert_eq!(
        o2.monde.quads, quads,
        "le lot et l'unité doivent donner la même scène"
    );
    println!(
        "intégrer {n} cellules ({quads} quads) : une par une {une_a_une:.0} ms, \
         par lot {par_lot:.0} ms — × {:.1}",
        une_a_une / par_lot.max(1e-6)
    );
    c2.arreter();
}

/// **Une cellule qui revient VIDE efface ce qu'elle portait.**
///
/// C'est ce que le retrait achète, et aucun autre test ne pouvait le dire :
/// tous chargent chaque cellule une seule fois, et `Grille::poser` remplace
/// l'adresse qu'on lui donne. Le cas se produit dès qu'une cellule est
/// évincée puis rechargée alors que son contenu a changé — c'est-à-dire tout
/// le temps, quand la résidence pilotera la caméra. Sans le retrait, les
/// blocs effacés resteraient à l'écran.
///
/// Mesuré par mutation : sans ce test, retirer le retrait passait au vert.
#[test]
fn une_cellule_qui_revient_vide_efface_ce_qu_elle_portait() {
    let j = Jetable::neuf("codex");
    let pack = codex(j.chemin(), &[]);
    let m = Jetable::neuf("monde");
    semer(m.chemin(), 1, 4);

    let mut o = Ouvert::ouvrir(&pack, Some(m.texte()), [0, 0, 0, 0]).expect("monde ouvert");
    let mut c = Chargeur::lancer(o.staging.clone().unwrap(), tf_world::Dimension::Overworld);
    let lots = par_region(&voulues(
        BlockPos::new(24, 64, 24),
        EST,
        2,
        Niveau::Chunk,
        HAUTEUR,
    ));
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    // On garde les cellules pour pouvoir en REDEMANDER une.
    let cellules: Vec<tf_world::Cellule> = lots
        .iter()
        .flat_map(|l| l.cellules.iter().map(|v| v.cellule.clone()))
        .collect();
    c.demander(lots);
    assert_eq!(streamer(&mut o, &mut c, n), n);
    let plein = o.monde.quads;
    assert!(plein > 0, "la prémisse : la scène doit porter des quads");
    c.arreter();

    // La cellule du milieu revient SANS contenu, comme si la save ne portait
    // plus rien là — ou comme si on l'avait évincée et que le monde avait
    // changé.
    let cible = cellules
        .iter()
        .find(|c| c.x == 1 && c.z == 1)
        .expect("une cellule au milieu")
        .clone();
    o.integrer(vec![Arrivee {
        cellule: cible,
        sections: Vec::new(),
        interner: tf_anvil::Interner::new(),
    }])
    .expect("intégration du vide");

    assert!(
        o.monde.quads < plein,
        "la cellule vidée doit avoir retiré des quads : {} contre {plein}",
        o.monde.quads
    );
    assert_eq!(o.rechargements, 0);
    println!("cellule vidée : {plein} quads → {}", o.monde.quads);
}
