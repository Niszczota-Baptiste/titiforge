//! **Le fil de chargement : ce qu'il promet, et ce qui se casserait sans.**
//!
//! Le budget est mesuré et sans appel (`tf-app --example residence`) : une
//! région bâtie met 867 ms à venir, soit 108 images à 8 ms. Il n'existe pas de
//! version « assez rapide pour le fil principal ». Les propriétés qui rendent
//! ce fil utile se vérifient donc ici, et deux d'entre elles ne se voient
//! nulle part ailleurs : qu'il ne fait JAMAIS attendre l'hôte, et qu'il ne lit
//! un `.mca` qu'UNE fois.

use std::time::{Duration, Instant};

mod commun;

use commun::monde;
use tf_app::chargeur::{Chargeur, Reponse};
use tf_world::coords::BlockPos;
use tf_world::demande::{par_region, voulues, Lot};
use tf_world::source::Dimension;
use tf_world::Niveau;

const HAUTEUR: (i32, i32) = (-64, 319);
const EST: [f32; 3] = [1.0, 0.0, 0.0];

/// La demande d'une caméra placée à `oeil`, groupée en lectures.
fn demande(oeil: BlockPos, rayon: u32) -> Vec<Lot> {
    par_region(&voulues(oeil, EST, rayon, Niveau::Chunk, HAUTEUR))
}

/// Ramasse jusqu'à `n` réponses, en bornant l'attente.
fn ramasser(c: &mut Chargeur, n: usize, delai: Duration) -> Vec<Reponse> {
    let debut = Instant::now();
    let mut out = Vec::new();
    while out.len() < n && debut.elapsed() < delai {
        out.extend(c.recevoir(0));
        if out.len() < n {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    out
}

/// **`recevoir` ne doit JAMAIS attendre.** C'est toute la raison d'être du
/// fil, et la seule propriété dont le symptôme, quand elle casse, est
/// l'ABSENCE de symptôme : une suite qui pend ne rend ni rouge ni vert.
///
/// Elle se vérifie donc dans un fil TÉMOIN, à attente bornée — la leçon du
/// fil moteur, appliquée telle quelle.
#[test]
fn recevoir_ne_bloque_jamais() {
    let (fait, attendre) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let src = monde(1, 4);
        let mut c = Chargeur::lancer(src, Dimension::Overworld);
        // Rien n'a été demandé : `recevoir` doit rendre la main tout de suite.
        assert!(c.recevoir(0).is_empty());
        assert!(!c.occupe());
        // Et même juste après une demande, avant que le fil ait fini.
        c.demander(demande(BlockPos::new(8, 64, 8), 1));
        let _ = c.recevoir(0);
        c.arreter();
        let _ = fait.send(());
    });
    attendre
        .recv_timeout(Duration::from_secs(20))
        .expect("`recevoir` a bloqué — le fil ne sert plus à rien");
}

/// **Une réponse par cellule, et toutes les cellules.**
///
/// Une cellule sans contenu est une réponse, pas un silence : sans elle
/// l'hôte la redemanderait indéfiniment.
#[test]
fn chaque_cellule_demandee_revient_exactement_une_fois() {
    let src = monde(1, 8);
    let mut c = Chargeur::lancer(src, Dimension::Overworld);
    let lots = demande(BlockPos::new(64, 64, 64), 3);
    let attendues: Vec<(i32, i32)> = lots
        .iter()
        .flat_map(|l| l.cellules.iter().map(|v| (v.cellule.x, v.cellule.z)))
        .collect();
    let n = attendues.len();
    assert!(n > 10, "la demande doit être substantielle : {n}");

    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));
    assert_eq!(recues.len(), n, "il manque des réponses");

    let mut vues: Vec<(i32, i32)> = recues
        .iter()
        .filter_map(|r| r.cellule().map(|c| (c.x, c.z)))
        .collect();
    let mut a = attendues.clone();
    vues.sort();
    a.sort();
    assert_eq!(vues, a, "les cellules rendues ne sont pas celles demandées");
    assert!(!c.occupe(), "tout est revenu, le témoin doit s'éteindre");
    c.arreter();
}

/// **Un `.mca` n'est lu qu'UNE fois par lot.**
///
/// C'est la conclusion qui a décidé l'unité de lecture, et elle se vérifie
/// par un COMPTEUR : servir 1 024 chunks un par un gaspillerait 4,4 s par
/// région en relectures pures. Un chronomètre dirait la même chose en moins
/// sûr.
#[test]
fn une_region_n_est_lue_qu_une_fois() {
    let src = monde(2, 8);
    let compteur = src.clone();
    let mut c = Chargeur::lancer(src, Dimension::Overworld);

    // Un œil au coin de quatre régions : la demande en traverse plusieurs.
    let lots = demande(BlockPos::new(512, 64, 512), 4);
    let regions = lots.len();
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    assert!(regions >= 2, "la demande doit couvrir plusieurs régions");
    assert!(n > regions, "et plusieurs cellules par région");

    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));
    assert_eq!(recues.len(), n);
    assert_eq!(
        compteur.lectures(),
        regions,
        "{n} cellules dans {regions} régions ont demandé {} lectures — \
         l'unité de LECTURE n'est pas celle de l'affichage",
        compteur.lectures()
    );
    c.arreter();
}

/// **Les cellules reviennent par URGENCE**, la plus pressée d'abord.
///
/// L'ordre EST la fonctionnalité : une région bâtie pèse 186 Mo résidents,
/// donc on n'ira jamais au bout de la liste. Ce qui arrive en premier est ce
/// qu'on verra.
#[test]
fn les_cellules_reviennent_par_urgence() {
    let src = monde(1, 8);
    let mut c = Chargeur::lancer(src, Dimension::Overworld);
    let lots = demande(BlockPos::new(64, 64, 64), 3);
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    // Le premier lot est le plus urgent, et sa première cellule est celle de
    // l'œil.
    let premiere = (lots[0].cellules[0].cellule.x, lots[0].cellules[0].cellule.z);

    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));
    assert!(!recues.is_empty());
    assert_eq!(
        recues[0].cellule().map(|c| (c.x, c.z)),
        Some(premiere),
        "la cellule sous nos pieds doit arriver la première"
    );
    c.arreter();
}

/// **Une nouvelle demande REMPLACE l'ancienne.**
///
/// Dès que la caméra bouge, ce qui n'a pas encore été lu n'est plus ce qu'il
/// faut lire. Une file qui s'accumulerait ferait charger le passé pendant
/// qu'on vole vers l'avenir — et sur un monde de 800 régions, elle ne
/// rattraperait jamais son retard.
#[test]
fn une_demande_neuve_remplace_la_precedente() {
    let src = monde(2, 8);
    let compteur = src.clone();
    let mut c = Chargeur::lancer(src, Dimension::Overworld);

    // Une grosse demande, puis tout de suite une autre ailleurs. La première
    // ne doit pas être servie en entier.
    let grosse = demande(BlockPos::new(64, 64, 64), 8);
    let grosse_n: usize = grosse.iter().map(|l| l.cellules.len()).sum();
    c.demander(grosse);
    let petite = demande(BlockPos::new(600, 64, 600), 1);
    let petite_n: usize = petite.iter().map(|l| l.cellules.len()).sum();
    let voulues: Vec<(i32, i32)> = petite
        .iter()
        .flat_map(|l| l.cellules.iter().map(|v| (v.cellule.x, v.cellule.z)))
        .collect();
    c.demander(petite);

    let recues = ramasser(&mut c, petite_n, Duration::from_secs(30));
    let vues: Vec<(i32, i32)> = recues
        .iter()
        .filter_map(|r| r.cellule().map(|c| (c.x, c.z)))
        .collect();
    // Les cellules de la petite demande sont arrivées…
    for v in &voulues {
        assert!(
            vues.contains(v),
            "la cellule {v:?} de la demande COURANTE n'est pas venue"
        );
    }
    // …et la grosse n'a pas été servie en entier : le fil a jeté sa file.
    assert!(
        vues.len() < grosse_n,
        "la demande périmée a été servie en entier ({} réponses pour une file \
         de {grosse_n} qui devait être abandonnée)",
        vues.len()
    );
    assert!(
        compteur.lectures() <= 4,
        "une file abandonnée a quand même coûté {} lectures",
        compteur.lectures()
    );
    c.arreter();
}

/// **Les sections d'une cellule sont bien les SIENNES.**
///
/// Le fil lit un rectangle et range par cellule : une section mal classée se
/// dessinerait au mauvais endroit, ce qu'aucune image ne montrerait comme une
/// erreur.
#[test]
fn les_sections_appartiennent_a_leur_cellule() {
    let src = monde(1, 8);
    let mut c = Chargeur::lancer(src, Dimension::Overworld);
    let lots = demande(BlockPos::new(64, 64, 64), 2);
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));

    let mut avec_contenu = 0;
    for r in &recues {
        let Reponse::Prete {
            cellule, sections, ..
        } = r
        else {
            continue;
        };
        if !sections.is_empty() {
            avec_contenu += 1;
        }
        for s in sections {
            assert_eq!(
                (s.chunk.x, s.chunk.z),
                (cellule.x, cellule.z),
                "une section du chunk ({}, {}) est rangée dans la cellule ({}, {})",
                s.chunk.x,
                s.chunk.z,
                cellule.x,
                cellule.z
            );
        }
    }
    assert!(
        avec_contenu > 0,
        "aucune cellule n'a de contenu — le test ne prouverait rien"
    );
    c.arreter();
}

/// **La table d'états d'une réponse suffit à la relire.**
///
/// Un `StateId` n'a de sens que relativement à SON interner. Si la table
/// rendue ne couvre pas les palettes qu'elle accompagne, l'hôte fusionnerait
/// des identifiants qui ne veulent rien dire — et chaque bloc prendrait celui
/// d'un autre, sans la moindre erreur.
#[test]
fn la_table_rendue_couvre_les_palettes_rendues() {
    let src = monde(1, 8);
    let mut c = Chargeur::lancer(src, Dimension::Overworld);
    let lots = demande(BlockPos::new(64, 64, 64), 2);
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));

    let mut etats = 0;
    for r in &recues {
        let Reponse::Prete {
            sections, interner, ..
        } = r
        else {
            continue;
        };
        for s in sections {
            for id in &s.section.palette {
                assert!(
                    interner.resolve(*id).is_some(),
                    "l'état {id} d'une palette n'est pas dans la table rendue"
                );
                etats += 1;
            }
            if let Some(b) = &s.biomes {
                for id in b {
                    assert!(
                        interner.resolve(*id).is_some(),
                        "le biome {id} n'est pas dans la table rendue"
                    );
                }
            }
        }
    }
    assert!(etats > 0, "aucun état relu — le test ne prouverait rien");
    c.arreter();
}

/// Une région ABSENTE n'est pas une anomalie : le monde est un semis, pas une
/// grille pleine. Ses cellules reviennent vides, et elles reviennent.
#[test]
fn une_region_absente_rend_des_cellules_vides() {
    let src = monde(1, 4);
    let mut c = Chargeur::lancer(src, Dimension::Overworld);
    // Très loin de la seule région écrite.
    let lots = demande(BlockPos::new(100_000, 64, 100_000), 1);
    let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
    c.demander(lots);
    let recues = ramasser(&mut c, n, Duration::from_secs(30));
    assert_eq!(recues.len(), n, "une cellule sans région doit répondre");
    for r in &recues {
        match r {
            Reponse::Prete { sections, .. } => assert!(sections.is_empty()),
            Reponse::Echec(e) => panic!("une région absente n'est pas une erreur : {e}"),
        }
    }
    c.arreter();
}

#[test]
fn une_region_corrompue_se_dit_et_ne_libere_pas_le_fil_trop_tot() {
    // La région (0, 0) : des octets qui ne sont pas une région. Ses cellules
    // reviennent vides — c'est ce qui les empêche d'être relues en boucle —
    // et l'échec se DIT, une fois. Mais le message n'est pas une cellule :
    // le compter comme telle ferait croire le fil libre alors que la
    // cellule est encore en route, et la demande repartirait la chercher.
    let src = commun::Comptee::neuve();
    let src = std::sync::Arc::new(src);
    let mut garbage = vec![0u8; 16_384];
    for (i, o) in garbage.iter_mut().enumerate() {
        *o = (i as u32).wrapping_mul(2_654_435_761) as u8;
    }
    src.poser_region(0, 0, garbage);
    let mut c = Chargeur::lancer(src.clone(), Dimension::Overworld);
    c.demander(demande(BlockPos::new(8, 64, 8), 0));

    // Une réponse à la fois : le message d'abord.
    let debut = Instant::now();
    let premiere = loop {
        if let Some(r) = c.recevoir(1).into_iter().next() {
            break r;
        }
        assert!(
            debut.elapsed() < Duration::from_secs(20),
            "rien n'est revenu"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    let texte = match &premiere {
        Reponse::Echec(e) => e.clone(),
        Reponse::Prete { .. } => "une cellule".into(),
    };
    assert!(
        texte.contains("illisible"),
        "la corruption doit se DIRE, et d'abord : {texte}"
    );
    assert!(
        c.occupe(),
        "la cellule n'est pas encore rendue : le fil n'est pas libre"
    );
    let reste = ramasser(&mut c, 1, Duration::from_secs(20));
    c.arreter();
    assert!(
        matches!(&reste[..], [Reponse::Prete { sections, .. }] if sections.is_empty()),
        "puis la cellule, vide : {} réponse(s)",
        reste.len()
    );
    assert_eq!(src.lectures(), 1, "lue une fois");
}
