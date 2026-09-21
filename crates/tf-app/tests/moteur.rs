//! **Le fil moteur : il ne bloque jamais, et il répond toujours.**
//!
//! Aucune fenêtre, aucun GPU — juste un monde en mémoire et deux tuyaux. Ce
//! qui est vérifié ici est ce qu'une capture d'écran ne montrerait pas : que
//! l'interface garde la main pendant qu'on travaille, et qu'une commande qui
//! échoue revient comme un échec au lieu de disparaître.

use std::time::{Duration, Instant};

use tf_app::moteur::{Commande, Moteur, Reponse};
use tf_bench::{region, Terrain};
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn moteur() -> (Moteur, MemorySource) {
    let brut = region(&Terrain::petite());
    let src = MemorySource::new();
    src.put_region(SURFACE, Folder::Region, ZERO, brut);
    let miroir = MemorySource::new();
    miroir.put_region(
        SURFACE,
        Folder::Region,
        ZERO,
        src.read_region(&SURFACE, Folder::Region, ZERO).unwrap(),
    );
    let st = std::sync::Arc::new(Staging::new(src, MemorySource::new()));
    (Moteur::lancer(st, SURFACE, Journal::new(), None), miroir)
}

fn sel() -> BBox {
    BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(31, -33, 31))
}

fn poser(bloc: &str) -> Commande {
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte(bloc));
    Commande::Appliquer {
        op: "poser",
        params,
        sel: sel(),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }
}

/// Attend une réponse en interrogeant, comme la boucle d'images le fera.
/// **Jamais un `recv` bloquant** : c'est justement ce qu'on veut interdire.
fn attendre(m: &mut Moteur) -> Reponse {
    let debut = Instant::now();
    loop {
        if let Some(r) = m.recevoir().into_iter().next() {
            return r;
        }
        assert!(
            debut.elapsed() < Duration::from_secs(20),
            "le moteur n'a pas répondu"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// **Envoyer et ramasser ne bloquent jamais.** C'est toute la raison d'être du
/// fil : une opération de trois secondes sur la boucle d'images fige la
/// fenêtre, et une fenêtre figée est indistinguable d'une fenêtre plantée.
///
/// **Le test tourne dans un fil témoin**, et pas par commodité : un `recevoir`
/// qui bloquerait ferait PENDRE la suite au lieu de la faire rougir, et un
/// test qui pend ne dit pas ce qui ne va pas — il dit seulement qu'on a
/// attendu. Mesuré en cassant la pièce : la suite ne rendait plus la main du
/// tout. Avec le témoin, la même faute sort une phrase.
#[test]
fn l_interface_ne_bloque_jamais() {
    let (fini, attente) = std::sync::mpsc::channel::<Duration>();
    let temoin = std::thread::spawn(move || {
        let (mut m, _) = moteur();
        let t = Instant::now();
        assert!(m.envoyer(poser("minecraft:dirt")));
        // Une centaine de tours de boucle à vide pendant que le fil travaille :
        // aucun ne doit coûter quoi que ce soit.
        for _ in 0..100 {
            let _ = m.recevoir();
        }
        let tours = t.elapsed();
        let _ = fini.send(tours);
        let r = attendre(&mut m);
        assert!(!r.echoue(), "{}", r.texte());
        assert!(!m.occupe(), "le compteur d'en-vol doit retomber");
    });

    let tours = attente
        .recv_timeout(Duration::from_secs(10))
        .expect("cent relevés ont BLOQUÉ : l'interface se figerait");
    assert!(
        tours < Duration::from_millis(200),
        "envoyer + 100 relevés ont pris {tours:?}"
    );
    temoin.join().expect("le fil témoin a paniqué");
}

/// Une commande, une réponse — toujours. Sans ça le compteur d'en-vol dérive
/// et le bouton reste grisé pour toujours.
#[test]
fn chaque_commande_rend_exactement_une_reponse() {
    let (mut m, _) = moteur();
    for i in 0..4 {
        let bloc = if i % 2 == 0 {
            "minecraft:dirt"
        } else {
            "minecraft:stone"
        };
        assert!(m.envoyer(poser(bloc)));
    }
    let mut recues = Vec::new();
    let debut = Instant::now();
    while recues.len() < 4 {
        recues.extend(m.recevoir());
        assert!(debut.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(recues.len(), 4);
    assert!(!m.occupe());
}

/// La boucle complète, par le fil : appliquer, annuler, refaire.
#[test]
fn appliquer_annuler_refaire_par_le_fil() {
    let (mut m, _) = moteur();
    assert!(m.envoyer(poser("minecraft:dirt")));
    let r = attendre(&mut m);
    let Reponse::Fait { bornes, .. } = &r else {
        panic!("attendu Fait, reçu {r:?}");
    };
    assert!(bornes.is_some(), "les bornes portent le remaillage");

    assert!(m.envoyer(Commande::Annuler));
    let r = attendre(&mut m);
    assert!(matches!(r, Reponse::Defait { .. }), "{r:?}");

    assert!(m.envoyer(Commande::Refaire));
    let r = attendre(&mut m);
    assert!(matches!(r, Reponse::Refait { .. }), "{r:?}");

    // Et au bout de la pile, on le DIT plutôt que de ne rien faire.
    assert!(m.envoyer(Commande::Refaire));
    let r = attendre(&mut m);
    assert!(matches!(r, Reponse::Rien(_)), "{r:?}");
}

/// **Une erreur revient comme une erreur.** Un échec avalé laisse l'interface
/// croire que ça travaille encore, et le bouton grisé pour toujours.
#[test]
fn une_commande_fautive_revient_en_echec_et_le_fil_survit() {
    let (mut m, _) = moteur();
    // « remplacer » exige `de` et `vers` : sans eux, `normaliser` refuse.
    assert!(m.envoyer(Commande::Appliquer {
        op: "remplacer",
        params: Params::new(),
        sel: sel(),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }));
    let r = attendre(&mut m);
    assert!(r.echoue(), "{r:?}");
    assert!(r.texte().contains("remplacer"), "{}", r.texte());

    // Le fil DOIT survivre : sinon l'interface resterait vivante et le moteur
    // mort, ce qui se lit « les boutons ne font plus rien ».
    assert!(m.vivant());
    assert!(m.envoyer(poser("minecraft:dirt")));
    let r = attendre(&mut m);
    assert!(!r.echoue(), "{}", r.texte());
}

/// Une opération qui ne change rien n'est pas un échec — et le taire ferait
/// croire à un bouton mort.
#[test]
fn une_operation_sans_effet_se_dit() {
    let (mut m, _) = moteur();
    assert!(m.envoyer(poser("minecraft:dirt")));
    assert!(!attendre(&mut m).echoue());
    // La même, une seconde fois : tout est déjà de la terre.
    assert!(m.envoyer(poser("minecraft:dirt")));
    let r = attendre(&mut m);
    assert!(matches!(r, Reponse::Rien(_)), "{r:?}");
    assert!(r.texte().contains("rien n'a changé"), "{}", r.texte());
}

/// **La SOURCE reste intacte** — invariant n° 1, vérifié à travers le fil,
/// parce que c'est le chemin que la coque prendra.
#[test]
fn la_source_reste_intacte_a_travers_le_fil() {
    let (mut m, miroir) = moteur();
    let avant = miroir.read_region(&SURFACE, Folder::Region, ZERO).unwrap();
    assert!(m.envoyer(poser("minecraft:dirt")));
    assert!(!attendre(&mut m).echoue());
    m.arreter();
    assert_eq!(
        miroir.read_region(&SURFACE, Folder::Region, ZERO).unwrap(),
        avant
    );
}

/// Un moteur arrêté le DIT, au lieu d'avaler les commandes suivantes.
#[test]
fn un_moteur_arrete_refuse_les_commandes() {
    let (mut m, _) = moteur();
    m.arreter();
    assert!(!m.vivant());
    assert!(!m.envoyer(poser("minecraft:dirt")));
    assert!(!m.occupe());
}

/// La forme passe par le fil et resserre ce qui est écrit. Elle vient des
/// gestes, pas du descripteur : si elle se perdait en route, une sphère
/// remplirait toute la sélection.
#[test]
fn la_forme_traverse_le_fil() {
    let (mut m, _) = moteur();
    let s = sel();
    let centre = [
        (s.min.x + s.max.x).div_euclid(2),
        (s.min.y + s.max.y).div_euclid(2),
        (s.min.z + s.max.z).div_euclid(2),
    ];
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte("minecraft:dirt"));
    assert!(m.envoyer(Commande::Appliquer {
        op: "poser",
        params,
        sel: s,
        forme: Forme::sphere(centre, 4.0),
        compter: false,
        seed: 0,
    }));
    let r = attendre(&mut m);
    let b = r.bornes().expect("l'opération a écrit");
    let (bx, by, bz) = b.size();
    let (sx, sy, sz) = s.size();
    assert!(
        (bx as u64 * by as u64 * bz as u64) < (sx as u64 * sy as u64 * sz as u64),
        "la sphère doit écrire moins que la sélection : {bx}×{by}×{bz}"
    );
}

/// **Un monde sans save derrière lui ne s'écrit pas**, et il le DIT. Un
/// bouton qui ne répond rien se lit « ça n'a pas marché » sans qu'on sache
/// pourquoi.
#[test]
fn ecrire_sans_save_est_un_echec_nomme() {
    let (mut m, _) = moteur();
    assert!(m.envoyer(Commande::Ecrire {
        confirme_sans_verrou: true
    }));
    let r = attendre(&mut m);
    assert!(r.echoue(), "{r:?}");
    assert!(r.texte().contains("save"), "{}", r.texte());
    // Et le fil survit : on peut continuer à éditer.
    assert!(m.vivant());
    assert!(m.envoyer(poser("minecraft:dirt")));
    assert!(!attendre(&mut m).echoue());
}
