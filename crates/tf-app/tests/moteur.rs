//! **Le fil moteur : il ne bloque jamais, et il répond toujours.**
//!
//! Aucune fenêtre, aucun GPU — juste un monde en mémoire et deux tuyaux. Ce
//! qui est vérifié ici est ce qu'une capture d'écran ne montrerait pas : que
//! l'interface garde la main pendant qu'on travaille, et qu'une commande qui
//! échoue revient comme un échec au lieu de disparaître.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tf_app::moteur::{ActionComposant, Carnet, Commande, Moteur, Reponse};
use tf_bench::{region, Terrain};
use tf_ops::catalogue::{Params, Valeur};
use tf_ops::Forme;
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::{Journal, Record};
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

// ── le carnet : ce qui survit à la fermeture ────────────────────────────────

/// Un carnet qui RETIENT ce qu'on lui confie, dans l'ordre.
#[derive(Clone, Default)]
struct Temoin(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl Temoin {
    fn lignes(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn dire(&self, l: String) {
        self.0.lock().unwrap().push(l);
    }
}

impl Carnet for Temoin {
    fn commencer(&mut self, label: &str) {
        self.dire(format!("commencer {label}"));
    }
    fn terminer(&mut self) {
        self.dire("terminer".into());
    }
    fn noter(&mut self, _: &mut Journal, records: &[Record]) -> Result<(), String> {
        for r in records {
            self.dire(match r {
                Record::Entree(e) => format!("entrée {}", e.label),
                Record::Curseur(c) => format!("curseur {c}"),
                Record::Troncature(t) => format!("troncature {t}"),
            });
        }
        Ok(())
    }
    fn fermer(self: Box<Self>) -> String {
        self.dire("fermer".into());
        "fermé".into()
    }
}

fn en_seance() -> (Moteur, Temoin, Arc<Staging<MemorySource, MemorySource>>) {
    let src = MemorySource::new();
    src.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    let st = Arc::new(Staging::new(src, MemorySource::new()));
    let t = Temoin::default();
    let m = Moteur::lancer_en_seance(
        st.clone(),
        SURFACE,
        Journal::new(),
        None,
        Box::new(t.clone()),
    );
    (m, t, st)
}

/// **Chaque action est rangée**, dans l'ordre, et la séance se ferme quand le
/// fil s'arrête — après la dernière action, jamais pendant.
#[test]
fn chaque_action_est_rangee_dans_le_carnet() {
    let (mut m, t, _) = en_seance();
    let label = tf_ops::catalogue::descripteur("poser").unwrap().label;
    for c in [
        poser("minecraft:dirt"),
        Commande::Annuler,
        Commande::Refaire,
    ] {
        assert!(m.envoyer(c));
        let r = attendre(&mut m);
        assert!(!r.echoue(), "{}", r.texte());
    }
    m.arreter();
    assert_eq!(
        t.lignes(),
        [
            format!("commencer {label}"),
            format!("entrée {label}"),
            "terminer".into(),
            "commencer Annuler".into(),
            "curseur 0".into(),
            "terminer".into(),
            "commencer Refaire".into(),
            "curseur 1".into(),
            "terminer".into(),
            "fermer".into(),
        ]
    );
}

/// **Une annulation refusée rend le curseur.** Resté avancé, il désignait
/// comme défaite une entrée toujours appliquée : « refaire » la réappliquait
/// alors par-dessus elle-même, et le journal sur disque disait autre chose que
/// la copie de travail.
#[test]
fn une_annulation_refusee_ne_bouge_pas_le_curseur() {
    let (mut m, t, st) = en_seance();
    assert!(m.envoyer(poser("minecraft:dirt")));
    assert!(!attendre(&mut m).echoue());
    // La région change sous le journal — relue depuis la save, par exemple.
    let brut = st
        .source()
        .read_region(&SURFACE, Folder::Region, ZERO)
        .unwrap();
    st.write_region(&SURFACE, Folder::Region, ZERO, &brut)
        .unwrap();

    assert!(m.envoyer(Commande::Annuler));
    let r = attendre(&mut m);
    assert!(r.echoue(), "{r:?}");
    assert!(
        !t.lignes().iter().any(|l| l.starts_with("curseur")),
        "rien à ranger : rien n'a bougé — {:?}",
        t.lignes()
    );

    assert!(m.envoyer(Commande::Refaire));
    let r = attendre(&mut m);
    assert!(
        matches!(r, Reponse::Rien(_)),
        "l'entrée est toujours appliquée : rien à refaire — {r:?}"
    );
}

/// Un carnet qui ne sait pas écrire ne bloque pas l'édition, mais se DIT : un
/// historique qui ne survivra pas à la fermeture n'est pas un détail.
#[test]
fn un_carnet_qui_echoue_se_dit() {
    struct Plein;
    impl Carnet for Plein {
        fn commencer(&mut self, _: &str) {}
        fn terminer(&mut self) {}
        fn noter(&mut self, _: &mut Journal, _: &[Record]) -> Result<(), String> {
            Err("disque plein".into())
        }
        fn fermer(self: Box<Self>) -> String {
            String::new()
        }
    }
    let src = MemorySource::new();
    src.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
    let st = Arc::new(Staging::new(src, MemorySource::new()));
    let mut m = Moteur::lancer_en_seance(st, SURFACE, Journal::new(), None, Box::new(Plein));
    assert!(m.envoyer(poser("minecraft:dirt")));
    let r = attendre(&mut m);
    assert!(
        matches!(r, Reponse::Fait { .. }),
        "l'édition a eu lieu : {r:?}"
    );
    assert!(
        r.texte().contains("disque plein") && r.texte().contains("fermeture"),
        "{}",
        r.texte()
    );
}

/// **Les zones à remailler** ne gardent que les chunks de BLOCS de la
/// dimension regardée — ni points d'intérêt, ni entités, ni le Nether —, un
/// chunk une fois, chacun borné par ce que l'opération a écrit ; sans bornes,
/// la colonne entière.
#[test]
fn les_zones_ne_gardent_que_les_blocs_de_la_dimension_regardee() {
    use tf_app::moteur::zones_de;
    use tf_world::journal::Cible;
    let c = |dim: Dimension, folder: Folder, x: i32, z: i32, chunk: u16| Cible {
        dim,
        folder,
        region: RegionPos { x, z },
        chunk,
    };
    let cibles = vec![
        // Le chunk (3, 1) : index z × 32 + x.
        c(SURFACE, Folder::Region, 0, 0, 32 + 3),
        // Dans les bornes, mais pas des blocs, ou pas d'ici : aucune zone.
        c(SURFACE, Folder::Poi, 0, 0, 1),
        c(SURFACE, Folder::Entities, 0, 0, 2),
        c(Dimension::Nether, Folder::Region, 0, 0, 0),
        // Des blocs d'ici, mais hors des bornes : aucune zone.
        c(SURFACE, Folder::Region, 0, 0, 10),
        // Le chunk (−1, 0), dans la région −1.
        c(SURFACE, Folder::Region, -1, 0, 31),
        // Deux passes sur le même chunk : une zone.
        c(SURFACE, Folder::Region, 0, 0, 32 + 3),
    ];
    let bornes = BBox::new(BlockPos::new(-10, 5, 0), BlockPos::new(60, 9, 20));
    assert_eq!(
        zones_de(&cibles, Some(bornes), &SURFACE),
        vec![
            BBox::new(BlockPos::new(-10, 5, 0), BlockPos::new(-1, 9, 15)),
            BBox::new(BlockPos::new(48, 5, 16), BlockPos::new(60, 9, 20)),
        ]
    );
    assert_eq!(
        zones_de(&cibles[..1], None, &SURFACE),
        vec![BBox::new(
            BlockPos::new(48, -2048, 16),
            BlockPos::new(63, 2047, 31)
        )]
    );
}

// ── les composants ──────────────────────────────────────────────────────────

fn une_case(p: BlockPos, bloc: &str) -> Commande {
    let mut params = Params::new();
    params.poser("bloc", Valeur::texte(bloc));
    Commande::Appliquer {
        op: "poser",
        params,
        sel: BBox::single(p),
        forme: Forme::Boite,
        compter: false,
        seed: 0,
    }
}

fn faire(m: &mut Moteur, c: Commande) -> Reponse {
    assert!(m.envoyer(c));
    attendre(m)
}

/// La palette de la première définition, pour voir ce qu'elle porte.
fn palette(m: &Moteur) -> Vec<String> {
    m.composants().projet.definitions[0].contenu.palette.clone()
}

/// **Les composants passent par le fil, et le document PUBLIÉ suit chaque
/// pas** — créer, poser, mettre à jour, annuler, refaire, détacher, renommer.
/// L'interface ne décode rien : elle lit ce que le fil a publié, et une
/// annulation doit s'y voir comme une action.
#[test]
fn les_composants_passent_par_le_fil_et_leur_document_suit() {
    let (mut m, _) = moteur();
    let c = m.composants();
    assert!(c.projet.definitions.is_empty() && c.erreur.is_none());
    let p0 = BlockPos::new(2, -60, 2);
    assert!(!faire(&mut m, une_case(p0, "minecraft:glass")).echoue());

    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::Creer {
            sel: BBox::single(p0),
            nom: " vitre ".into(),
        }),
    );
    assert!(matches!(r, Reponse::Fait { .. }), "{r:?}");
    assert!(r.texte().contains("Créer « vitre »"), "{}", r.texte());
    assert!(r.texte().contains("1 × 1 × 1 · 1 bloc(s)"), "{}", r.texte());
    let c = m.composants();
    let (def, proto) = (c.projet.definitions[0].id, c.projet.instances[0].id);
    assert_eq!(c.projet.definitions[0].nom, "vitre");

    let q = BlockPos::new(30, -60, 30);
    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::Poser {
            definition: def,
            coin: q,
            transfo: None,
        }),
    );
    assert!(r.texte().contains("Poser « vitre »"), "{}", r.texte());
    assert!(r.texte().contains("instance n° 3"), "{}", r.texte());
    assert!(
        !r.zones().is_empty(),
        "une pose écrit : la coque doit remailler"
    );
    assert_eq!(m.composants().projet.instances.len(), 2);

    // Le prototype retouché, puis la mise à jour.
    assert!(!faire(&mut m, une_case(p0, "minecraft:gold_block")).echoue());
    let v = m.composants().version;
    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::MettreAJour { instance: proto }),
    );
    for attendu in [
        "Mettre à jour « vitre »",
        "1 instance(s) réestampée(s)",
        "0 case(s) entrée(s)",
    ] {
        assert!(r.texte().contains(attendu), "{}", r.texte());
    }
    assert!(m.composants().version > v);
    assert_eq!(palette(&m), vec!["minecraft:gold_block".to_string()]);

    // UN Ctrl+Z : le document publié revient aussi.
    let r = faire(&mut m, Commande::Annuler);
    assert!(matches!(r, Reponse::Defait { .. }), "{r:?}");
    assert!(!r.zones().is_empty());
    assert_eq!(palette(&m), vec!["minecraft:glass".to_string()]);
    let r = faire(&mut m, Commande::Refaire);
    assert!(matches!(r, Reponse::Refait { .. }), "{r:?}");
    assert_eq!(palette(&m), vec!["minecraft:gold_block".to_string()]);

    let posee = m.composants().projet.instances[1].id;
    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::Detacher { instance: posee }),
    );
    assert!(matches!(r, Reponse::Fait { .. }), "{r:?}");
    assert!(
        r.texte().contains("Détacher l'instance n° 3"),
        "{}",
        r.texte()
    );
    assert_eq!(m.composants().projet.instances.len(), 1);
    // Le document ne change pas : sa version non plus, même quand on défait
    // une opération qui n'y touchait pas.
    let v = m.composants().version;
    assert!(!faire(&mut m, une_case(p0, "minecraft:stone")).echoue());
    assert!(matches!(
        faire(&mut m, Commande::Annuler),
        Reponse::Defait { .. }
    ));
    assert_eq!(m.composants().version, v);

    let renommer = |nom: &str| {
        Commande::Composant(ActionComposant::Renommer {
            definition: def,
            nom: nom.into(),
        })
    };
    let r = faire(&mut m, renommer("hublot"));
    assert!(
        r.texte().contains("« vitre » en « hublot »"),
        "{}",
        r.texte()
    );
    assert_eq!(m.composants().projet.definitions[0].nom, "hublot");
    // Un nom inchangé : rien, et surtout pas une entrée qui ne défait rien.
    assert!(matches!(
        faire(&mut m, renommer("hublot")),
        Reponse::Rien(_)
    ));

    // Une action refusée revient en échec, et le fil continue.
    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::MettreAJour { instance: 999 }),
    );
    assert!(r.echoue() && r.texte().contains("999"), "{}", r.texte());
    let r = faire(
        &mut m,
        Commande::Composant(ActionComposant::Creer {
            sel: BBox::single(BlockPos::new(2, 100, 2)),
            nom: "rien".into(),
        }),
    );
    assert!(
        r.echoue() && r.texte().contains("aucun bloc"),
        "{}",
        r.texte()
    );
    assert!(!faire(&mut m, une_case(p0, "minecraft:dirt")).echoue());
}

/// Un monde qui porte DÉJÀ des composants les montre dès le lancement — et un
/// document illisible est dit, et jamais réécrit : le premier geste suivant
/// l'aurait remplacé par ce qu'on en a compris, c'est-à-dire rien.
#[test]
fn le_document_est_publie_des_le_lancement_et_jamais_ecrase_s_il_est_illisible() {
    use tf_world::source::RegionSink;
    let monde = |doc: &[u8]| {
        let src = MemorySource::new();
        src.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::petite()));
        src.write_meta("projet", doc).unwrap();
        let st = Arc::new(Staging::new(src, MemorySource::new()));
        (
            Moteur::lancer(st.clone(), SURFACE, Journal::new(), None),
            st,
        )
    };

    let (mut m, _) = moteur();
    faire(
        &mut m,
        une_case(BlockPos::new(2, -60, 2), "minecraft:glass"),
    );
    faire(
        &mut m,
        Commande::Composant(ActionComposant::Creer {
            sel: BBox::single(BlockPos::new(2, -60, 2)),
            nom: "vitre".into(),
        }),
    );
    let doc = m.composants().projet.encoder();
    let (m2, _) = monde(&doc);
    let c = m2.composants();
    assert_eq!(
        c.projet.definitions.len(),
        1,
        "le document du monde n'est pas publié"
    );
    assert_eq!(c.version, 1);

    let (mut m3, st) = monde(b"TFP1 abime");
    assert!(m3.composants().erreur.is_some());
    let r = faire(
        &mut m3,
        Commande::Composant(ActionComposant::Creer {
            sel: BBox::single(BlockPos::new(2, -60, 2)),
            nom: "vitre".into(),
        }),
    );
    assert!(
        r.echoue() && r.texte().contains("rien n'a été écrit"),
        "{}",
        r.texte()
    );
    assert_eq!(st.lire_fichier("projet").unwrap(), b"TFP1 abime");
}
