//! **Une rotation dans la coque tourne aussi les ÉTATS — ou le dit.**
//!
//! La coque passait `regle: None` au moteur : un « Copier vers » tourné
//! déplaçait les cases et laissait chaque échelle, chaque escalier dans son
//! orientation d'origine, sans un mot. Ces tests tiennent la jonction de bout
//! en bout — un pack écrit à la volée, des règles DÉRIVÉES de lui, le fil
//! moteur, la copie de travail relue — et ce qui se passe pendant que la
//! dérivation tourne encore.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tf_anvil::Interner;
use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_app::regles::Regles;
use tf_bench::{region, Terrain};
use tf_blocks::Transfo;
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

mod commun;
use commun::Jetable;

const SURFACE: Dimension = Dimension::Overworld;
const A: BlockPos = BlockPos { x: 3, y: -40, z: 3 };
const DECALAGE: [i32; 3] = [5, 0, 0];
const ECHELLE: &str = "t:echelle|facing=north";

type St = Staging<MemorySource, MemorySource>;

/// Un codex d'une échelle orientée : quatre variantes, un modèle CHIRAL —
/// un modèle symétrique laisserait la géométrie muette sur l'orientation.
fn table(dir: &Path) -> tf_blocks::Table {
    let ecrire = |chemin: &str, contenu: &str| {
        let p = dir.join(chemin);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contenu).unwrap();
    };
    ecrire(
        "blockstates.json",
        r#"{"t:echelle": {"variants": {
            "facing=north": {"model": "t:block/echelle"},
            "facing=east":  {"model": "t:block/echelle", "y": 90},
            "facing=south": {"model": "t:block/echelle", "y": 180},
            "facing=west":  {"model": "t:block/echelle", "y": 270}}}}"#,
    );
    ecrire(
        "models/block_echelle.json",
        r#"{"elements":[
            {"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"a"}}},
            {"from":[0,8,0],"to":[8,16,12],"faces":{"up":{"texture":"a"}}}]}"#,
    );
    let (cat, _, _) = tf_assets::jeu::catalogue(dir.to_str().unwrap()).unwrap();
    tf_blocks::Table::deriver(&cat)
}

fn moteur(r: Regles) -> (Moteur, Arc<St>) {
    let src = MemorySource::new();
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&Terrain::petite()),
    );
    let st = Arc::new(Staging::new(src, MemorySource::new()));
    let m = Moteur::lancer(st.clone(), SURFACE, Journal::new(), None);
    m.poser_regles(r);
    (m, st)
}

fn poser_echelle() -> Commande {
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("t:echelle[facing=north]"));
    Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::single(A),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }
}

fn copier_tourne() -> Commande {
    let mut params = Params::new();
    params.poser("decalage", Valeur::Vecteur(DECALAGE));
    params.poser(
        "transformation",
        Valeur::Transformation(Some(Transfo::Rot90)),
    );
    Commande::Appliquer {
        op: "copier-vers",
        params,
        sel: BBox::single(A),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }
}

/// L'état posé en `p`, relu dans la COPIE DE TRAVAIL.
fn etat_en(st: &St, p: BlockPos) -> String {
    let mut i = Interner::new();
    let pr =
        tf_ops::edition::copier(st, &SURFACE, Folder::Region, &BBox::single(p), &mut i).unwrap();
    i.resolve(pr.blocs[0]).unwrap().to_string()
}

fn arrivee() -> BlockPos {
    BlockPos::new(A.x + DECALAGE[0], A.y + DECALAGE[1], A.z + DECALAGE[2])
}

/// Attend une réponse, BORNÉE : un moteur qui attendrait des règles pour
/// toujours ferait pendre la suite au lieu de la faire rougir.
fn attendre(m: &mut Moteur, delai: Duration) -> Option<Reponse> {
    let debut = Instant::now();
    loop {
        if let Some(r) = m.recevoir().into_iter().next() {
            return Some(r);
        }
        if debut.elapsed() > delai {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// **Le témoin.** Le scénario tourne dans un fil à lui, et le test l'attend
/// avec une borne : un moteur qui attendrait des règles pour toujours ferait
/// sinon PENDRE la suite — au plus tard dans le `Drop` du moteur, qui joint
/// son fil — au lieu de la faire rougir. Mesuré en cassant la livraison : la
/// suite ne rendait plus la main. Un fil coincé est abandonné, et le
/// processus de test finit quand même.
fn borne(delai: Duration, scenario: impl FnOnce() + Send + 'static) {
    let (fini, attente) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(scenario));
        let _ = fini.send(r);
    });
    match attente.recv_timeout(delai) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => std::panic::resume_unwind(e),
        Err(_) => {
            panic!("le scénario n'a pas fini en {delai:?} : quelque chose attend pour toujours")
        }
    }
}

fn fait(r: Option<Reponse>) -> String {
    match r {
        Some(Reponse::Fait { resume, .. }) => resume,
        autre => panic!("attendu Fait, reçu {autre:?}"),
    }
}

#[test]
fn une_copie_tournee_tourne_aussi_les_echelles() {
    borne(Duration::from_secs(60), move || {
        let j = Jetable::neuf("regles-pack");
        let t = table(j.chemin());
        let attendu = t.transformer(ECHELLE, Transfo::Rot90).unwrap();
        assert_ne!(attendu, ECHELLE, "sinon le test ne prouve rien");

        let (mut m, st) = moteur(Regles::pretes(t));
        m.envoyer(poser_echelle());
        fait(attendre(&mut m, Duration::from_secs(20)));
        assert_eq!(etat_en(&st, A), ECHELLE);
        m.envoyer(copier_tourne());
        let resume = fait(attendre(&mut m, Duration::from_secs(20)));
        assert_eq!(etat_en(&st, arrivee()), attendu, "{resume}");
        assert!(!resume.contains("NON réécrites"), "{resume}");
    });
}

#[test]
fn sans_regles_la_rotation_le_dit() {
    borne(Duration::from_secs(60), move || {
        let (mut m, st) = moteur(Regles::absentes());
        m.envoyer(poser_echelle());
        let pose = fait(attendre(&mut m, Duration::from_secs(20)));
        assert!(
            !pose.contains("NON réécrites"),
            "un //set ne tourne rien : il n'a rien à avouer — {pose}"
        );
        m.envoyer(copier_tourne());
        let resume = fait(attendre(&mut m, Duration::from_secs(20)));
        assert_eq!(
            etat_en(&st, arrivee()),
            ECHELLE,
            "la case a bougé, l'orientation non"
        );
        assert!(resume.contains("NON réécrites"), "{resume}");
    });
}

/// **Un `//set` n'attend pas une dérivation en cours ; une rotation, si.**
/// Et elle repart dès que les règles arrivent.
#[test]
fn seule_une_rotation_attend_les_regles() {
    borne(Duration::from_secs(60), move || {
        let j = Jetable::neuf("regles-attente");
        let t = table(j.chemin());
        let attendu = t.transformer(ECHELLE, Transfo::Rot90).unwrap();
        let (r, livraison) = Regles::en_attente();
        let (mut m, st) = moteur(r);

        m.envoyer(poser_echelle());
        fait(attendre(&mut m, Duration::from_secs(20)));

        m.envoyer(copier_tourne());
        assert_eq!(
            attendre(&mut m, Duration::from_millis(300)),
            None,
            "la rotation a répondu sans règles, alors qu'elles allaient venir"
        );
        livraison.livrer(Some(t));
        fait(attendre(&mut m, Duration::from_secs(20)));
        assert_eq!(etat_en(&st, arrivee()), attendu);
    });
}

/// Une dérivation qui MEURT — le fil panique, la livraison est lâchée sans
/// rien remettre — vaut « aucune règle » : le moteur répond, et le dit.
#[test]
fn une_derivation_morte_ne_fait_pas_attendre_pour_toujours() {
    borne(Duration::from_secs(60), move || {
        let (r, livraison) = Regles::en_attente();
        let (mut m, st) = moteur(r);
        std::thread::spawn(move || {
            let _gardee = livraison;
            panic!("dérivation morte, pour l'essai");
        })
        .join()
        .unwrap_err();
        m.envoyer(poser_echelle());
        fait(attendre(&mut m, Duration::from_secs(20)));
        m.envoyer(copier_tourne());
        let resume = fait(attendre(&mut m, Duration::from_secs(20)));
        assert!(resume.contains("NON réécrites"), "{resume}");
        assert_eq!(etat_en(&st, arrivee()), ECHELLE);
    });
}

/// Le chemin de la coque : un catalogue PARTAGÉ, dérivé dans un fil à lui.
#[test]
fn la_derivation_en_fond_rend_les_regles_du_pack() {
    borne(Duration::from_secs(60), move || {
        let j = Jetable::neuf("regles-fond");
        let dir = j.chemin();
        let direct = table(dir);
        let (cat, _, _) = tf_assets::jeu::catalogue(dir.to_str().unwrap()).unwrap();
        let r = Regles::deriver_en_fond(Arc::new(cat));
        let t = r.table().expect("dérivées");
        assert_eq!(
            t.transformer(ECHELLE, Transfo::Rot90),
            direct.transformer(ECHELLE, Transfo::Rot90)
        );
        assert!(r.connues());
        // Le défaut n'est surtout pas « en attente » : un moteur lancé sans
        // qu'on lui donne de règles doit répondre, pas attendre.
        assert!(Regles::default().connues());
        assert!(Regles::default().table().is_none());
    });
}

/// Des règles là, mais pas pour CE bloc : il reste tel quel, et la réponse le
/// dit — autrement que « il manque les règles », qui enverrait chercher un
/// pack qu'on a déjà.
#[test]
fn un_bloc_que_le_pack_ne_sait_pas_tourner_se_signale() {
    borne(Duration::from_secs(60), move || {
        let j = Jetable::neuf("regles-inconnu");
        let (mut m, st) = moteur(Regles::pretes(table(j.chemin())));
        let mut params = Params::new();
        params.poser("bloc", Valeur::texte("t:mystere[sens=nord]"));
        m.envoyer(Commande::Appliquer {
            op: "poser",
            params,
            sel: BBox::single(A),
            forme: Forme::Boite,
            compter: false,
            seed: 0,
        });
        fait(attendre(&mut m, Duration::from_secs(20)));
        m.envoyer(copier_tourne());
        let resume = fait(attendre(&mut m, Duration::from_secs(20)));
        assert_eq!(etat_en(&st, arrivee()), "t:mystere|sens=nord");
        assert!(resume.contains("ne sait pas tourner"), "{resume}");
        assert!(!resume.contains("NON réécrites"), "{resume}");
    });
}
