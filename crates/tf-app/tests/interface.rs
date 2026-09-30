//! **Que l'interface sache dessiner ce que le moteur déclare.**
//!
//! Aucune fenêtre : egui tourne sans surface, et c'est ce qui rend ces
//! propriétés vérifiables. Elles portent toutes sur la MÊME couture — le
//! formulaire se génère depuis le descripteur — et sur la faute qui la ruine :
//! *un type de paramètre déclaré sans champ pour le saisir*. Dans
//! `ExeWorldEdit`, `blocklist`, `pattern` et `mask` retombaient sur la case de
//! texte par défaut ; « Remplacer » et « Mélange » ne pouvaient pas
//! fonctionner, et rien à l'écran ne le disait.

use tf_app::etat::{Atelier, Etat, Note};
use tf_app::interface::{champ, Aide};
use tf_app::nuancier::Nuancier;
use tf_ops::catalogue::{descripteur, Param, Saisie, Valeur, OPS};
use tf_world::coords::BlockPos;

/// Fait tourner une image d'interface sans surface ni GPU.
fn dessiner(f: impl FnOnce(&mut egui::Ui)) {
    dessiner_avec(egui::RawInput::default(), f);
}

/// La même, avec une entrée donnée — des touches, du texte tapé.
fn dessiner_avec(entree: egui::RawInput, f: impl FnOnce(&mut egui::Ui)) {
    // `run` prend un `FnMut` — il peut rendre plusieurs images. La fermeture
    // de l'appelant, elle, ne sert qu'une fois : on la met dans une `Option`
    // plutôt que de la lui faire cloner.
    let mut une_fois = Some(f);
    let ctx = egui::Context::default();
    // La sortie décrit ce qu'il faudrait afficher ; ici on ne dessine pas.
    let _ = ctx.run(entree, |ctx| {
        if let Some(g) = une_fois.take() {
            egui::CentralPanel::default().show(ctx, g);
        }
    });
}

/// Une valeur du bon genre pour chaque saisie.
///
/// Le `match` est EXHAUSTIF : ajouter une variante à `Saisie` casse la
/// compilation de ce fichier, donc personne ne peut en ajouter une sans
/// passer par le test qui exige son champ.
fn valeur_neutre(s: Saisie) -> Valeur {
    match s {
        Saisie::Bloc | Saisie::Biome => Valeur::Texte(String::new()),
        Saisie::Melange => Valeur::Melange(vec![(1, "minecraft:stone".into())]),
        Saisie::Entier { min, .. } => Valeur::Entier(min.max(0)),
        Saisie::Vecteur => Valeur::Vecteur([0; 3]),
        Saisie::Direction => Valeur::Direction(tf_world::selection::Direction::PlusX),
        Saisie::Transformation => Valeur::Transformation(None),
    }
}

/// **Chaque genre de paramètre a son champ.** Un repli muet sur une case de
/// texte est pire qu'un refus visible : la chaîne part telle quelle vers une
/// opération qui attend autre chose.
#[test]
fn chaque_saisie_a_son_champ() {
    for s in Saisie::TOUTES {
        let p = Param {
            nom: "essai",
            label: "Essai",
            saisie: s,
            defaut: None,
        };
        let mut v = valeur_neutre(s);
        let mut dessine = false;
        let n = Nuancier::default();
        let aide = Aide {
            nuancier: &n,
            vise: None,
        };
        dessiner(|ui| dessine = champ(ui, &p, &mut v, aide));
        assert!(dessine, "la saisie {s:?} n'a pas de champ");
    }
}

/// Et le même contrôle sur les paramètres RÉELS du catalogue, avec la valeur
/// que l'atelier leur donne : c'est le chemin que l'utilisateur emprunte.
#[test]
fn chaque_parametre_du_catalogue_se_dessine() {
    let mut a = Atelier::default();
    for d in OPS {
        assert!(a.choisir(d.id));
        for p in d.params {
            let mut v = a.valeur(p.nom);
            let mut dessine = false;
            let n = Nuancier::default();
            let aide = Aide {
                nuancier: &n,
                vise: None,
            };
            dessiner(|ui| dessine = champ(ui, p, &mut v, aide));
            assert!(dessine, "« {} » / {} ne se dessine pas", d.id, p.nom);
        }
    }
}

/// **Une valeur du mauvais genre ne se dessine pas, et surtout ne se réécrit
/// pas.** Convertir en silence est ce qui a fait planter « Naturaliser →
/// Personnalisé » : l'inspecteur envoyait un objet là où l'opération attendait
/// une chaîne.
#[test]
fn une_valeur_du_mauvais_genre_ne_se_dessine_pas() {
    let p = Param {
        nom: "bloc",
        label: "Bloc",
        saisie: Saisie::Bloc,
        defaut: None,
    };
    let mut v = Valeur::Entier(42);
    let mut dessine = true;
    let n = Nuancier::default();
    let aide = Aide {
        nuancier: &n,
        vise: None,
    };
    dessiner(|ui| dessine = champ(ui, &p, &mut v, aide));
    assert!(!dessine);
    assert_eq!(v, Valeur::Entier(42), "la valeur a été réécrite en silence");
}

/// **Une opération qui n'appartient pas à l'outil affiché.** Dans
/// `ExeWorldEdit`, l'état de départ était `tool: 'select'` et
/// `operation: 'set'`, deux constantes indépendantes : la liste a fini par
/// afficher « Copier » pendant que le bouton disait « Remplir ». L'atelier
/// s'ouvre donc sur une opération DU catalogue, jamais sur un nom recopié.
#[test]
fn l_atelier_s_ouvre_sur_une_operation_du_catalogue() {
    let a = Atelier::default();
    assert_eq!(a.op(), OPS[0].id);
    assert_eq!(a.descripteur().id, a.op());
    // Et ses paramètres sont ceux de CETTE opération.
    for (nom, _) in a.params.iter() {
        assert!(a.descripteur().param(nom).is_some());
    }
}

/// Changer d'opération repart des défauts : garder les valeurs de la
/// précédente donnerait un formulaire qui ment, et `normaliser` refuserait de
/// toute façon ce qui ne lui appartient pas.
#[test]
fn choisir_une_operation_repart_de_ses_defauts() {
    let mut a = Atelier::default();
    assert!(a.choisir("lisser"));
    a.params.poser("rayon", Valeur::Entier(9));
    assert!(a.choisir("creuser"));
    assert_eq!(a.op(), "creuser");
    assert!(
        a.params.get("rayon").is_none(),
        "un paramètre étranger a survécu"
    );
    assert_eq!(a.params.get("epaisseur"), Some(&Valeur::Entier(1)));
    // Un identifiant inconnu ne change RIEN — l'atelier reste valide.
    assert!(!a.choisir("pousser-tirer"));
    assert_eq!(a.op(), "creuser");
}

/// Le verdict dit ce qui manque plutôt que de griser un bouton sans raison.
#[test]
fn le_verdict_nomme_ce_qui_bloque() {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    assert!(e.atelier.choisir("remplacer"));

    // Pas de sélection, et deux paramètres obligatoires vides.
    let notes = e.atelier.verdict(None);
    assert!(notes.iter().any(|n| n.bloque()));
    assert!(
        notes.iter().any(|n| n.texte().contains("sélection")),
        "{notes:?}"
    );

    // Avec la sélection et les blocs, plus rien ne bloque.
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(15, 15, 15));
    e.atelier
        .params
        .poser("de", Valeur::texte("minecraft:stone"));
    e.atelier
        .params
        .poser("vers", Valeur::texte("minecraft:dirt"));
    let resume = e.resume_selection();
    let notes = e.atelier.verdict(resume.as_ref());
    assert!(!notes.iter().any(|n| n.bloque()), "{notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.texte().contains("sections entières")),
        "le coût doit être annoncé : {notes:?}"
    );
}

/// **Ce qui matérialise toute la sélection l'ANNONCE avant de commencer.**
/// `vec![]` n'échoue pas gentiment : une allocation refusée ABANDONNE le
/// processus, et l'éditeur disparaîtrait avec le travail en cours.
#[test]
fn creuser_annonce_ce_qu_il_materialise_et_refuse_l_impossible() {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    assert!(e.atelier.choisir("//hollow"));
    assert_eq!(e.atelier.op(), "creuser");

    // Gros mais tenable : 128³ cases à 12 octets.
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(127, 127, 127));
    let r = e.resume_selection();
    let notes = e.atelier.verdict(r.as_ref());
    assert!(!notes.iter().any(|n| n.bloque()), "{notes:?}");
    let att = notes
        .iter()
        .find(|n| matches!(n, Note::Attention(_)))
        .expect("le coût doit être annoncé");
    assert!(att.texte().contains("matérialise"), "{}", att.texte());
    assert!(att.texte().contains("Mo") || att.texte().contains("Go"));

    // Démesuré : refusé, en disant le plafond — jamais tenté.
    e.selection.poser_coin2(BlockPos::new(2047, 2047, 2047));
    let r = e.resume_selection();
    let notes = e.atelier.verdict(r.as_ref());
    assert!(
        notes.iter().any(|n| n.bloque()),
        "une sélection de 8 milliards de cases doit être refusée : {notes:?}"
    );
}

/// Une opération à portée `Colonne` ne peut pas promettre l'étage palette. Le
/// panneau doit le dire au lieu d'afficher un compte de sections qui ne
/// s'appliquera pas.
#[test]
fn une_operation_par_colonne_ne_promet_pas_l_etage_palette() {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    e.selection.poser_coin1(BlockPos::new(0, 0, 0));
    e.selection.poser_coin2(BlockPos::new(15, 15, 15));
    let r = e.resume_selection();

    assert!(e.atelier.choisir("naturaliser"));
    let notes = e.atelier.verdict(r.as_ref());
    assert!(
        notes.iter().any(|n| n.texte().contains("COLONNE")),
        "{notes:?}"
    );
    // Et surtout, PAS le compte de sections : il ne s'appliquera pas, et un
    // chiffre juste au mauvais endroit est un chiffre faux.
    assert!(
        !notes
            .iter()
            .any(|n| n.texte().contains("sections entières")),
        "promesse intenable : {notes:?}"
    );

    assert!(e.atelier.choisir("poser"));
    let notes = e.atelier.verdict(r.as_ref());
    assert!(
        notes
            .iter()
            .any(|n| n.texte().contains("sections entières")),
        "{notes:?}"
    );
}

/// La palette montre bien quelque chose, et le descripteur porte de quoi
/// l'afficher. Un bouton sans texte est un bouton qu'on ne clique pas.
#[test]
fn la_palette_a_de_quoi_s_afficher() {
    for d in OPS {
        assert!(!d.label.is_empty());
        assert!(!d.resume.is_empty());
        assert!(d.resume.len() > 20, "« {} » : résumé trop court", d.id);
        assert!(descripteur(d.id).is_some());
    }
}

/// Un champ de bloc qui a le focus, puis « Entrée » : ce qu'il contient après.
fn taper_entree(texte: &str, n: &Nuancier, vise: Option<&str>) -> String {
    use tf_app::interface::champ_de_bloc;
    let ctx = egui::Context::default();
    let id = egui::Id::new("essai-bloc");
    let mut s = texte.to_string();
    let aide = Aide { nuancier: n, vise };
    let image = |entree: egui::RawInput, s: &mut String| {
        let _ = ctx.run(entree, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                champ_de_bloc(ui, id, s, aide, 200.0);
            });
        });
    };
    ctx.memory_mut(|m| m.request_focus(id));
    image(egui::RawInput::default(), &mut s);
    let mut entree = egui::RawInput::default();
    entree.events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    image(entree, &mut s);
    s
}

/// **Entrée complète ce qui n'est pas encore un bloc** — et ne remplace pas
/// un identifiant exact tapé à la main par un voisin mieux classé.
#[test]
fn entree_complete_sans_remplacer_ce_qui_est_deja_un_bloc() {
    let mut n = Nuancier::new([
        "minecraft:stone",
        "minecraft:oak_stairs",
        "minecraft:stone_bricks",
    ]);
    n.suivre_monde(
        0,
        ["minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=false"]
            .into_iter(),
        1,
    );
    assert_eq!(
        taper_entree("oak_st", &n, None),
        "minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]",
        "l'état que le jeu a écrit d'abord"
    );
    assert_eq!(
        taper_entree("stone", &n, None),
        "stone",
        "déjà un bloc connu"
    );
    assert_eq!(
        taper_entree("", &n, Some("minecraft:stone_bricks")),
        "minecraft:stone_bricks",
        "un champ vide prend le bloc visé"
    );
    assert_eq!(
        taper_entree("rien_de_tel", &n, None),
        "rien_de_tel",
        "sans proposition, le texte reste — et le champ dit qu'il est inconnu"
    );
}

/// **La fiche de l'outil Composant se dessine** — avec un document, une
/// définition choisie et une instance sous le réticule, et avec un document
/// illisible. Sans fenêtre : c'est l'interface entière qui passe.
#[test]
fn la_fiche_des_composants_se_dessine() {
    use tf_app::etat::{Etat, Outil};
    use tf_ops::composant::{Contenu, Definition, Instance, Projet};
    use tf_world::coords::BlockPos;
    let p = Projet {
        prochain: 3,
        definitions: vec![Definition {
            id: 1,
            nom: "fenêtre".into(),
            contenu: Contenu {
                taille: [3, 2, 1],
                palette: vec!["minecraft:glass".into()],
                cases: vec![0; 6],
                entites: Vec::new(),
            },
        }],
        instances: vec![Instance {
            id: 2,
            definition: 1,
            dim: tf_world::source::Dimension::Overworld,
            coin: BlockPos::new(0, 0, 0),
            transfo: Some(tf_blocks::Transfo::Rot90),
        }],
    };
    for erreur in [None, Some("document abîmé".to_string())] {
        let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
        e.mode = tf_render::controles::Mode::Conception;
        e.outil = Outil::Composant;
        e.editable = true;
        e.composant_choisi = Some(1);
        e.vise.case = Some(BlockPos::new(0, 0, 1));
        e.suivre_composants(tf_app::moteur::Composants {
            projet: std::sync::Arc::new(p.clone()),
            erreur,
            version: 1,
        });
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            tf_app::interface::dessiner(ctx, &mut e);
        });
        assert!(e.demande.is_none(), "dessiner n'envoie rien tout seul");
    }
}

/// **Les échanges et la fiche de l'outil Coller se dessinent** — vide, puis
/// avec un presse-papiers dont la lecture a des remarques, venu d'un
/// Minecraft plus récent que le monde, et une liste de fichiers trouvés.
#[test]
fn les_echanges_et_la_fiche_coller_se_dessinent() {
    use tf_app::etat::{Etat, Outil, Trouve};
    use tf_app::moteur::PressePapiers;
    for plein in [false, true] {
        let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
        e.mode = tf_render::controles::Mode::Conception;
        e.outil = Outil::Coller;
        e.editable = true;
        e.selection.poser_coin1(BlockPos::new(0, 0, 0));
        e.selection.poser_coin2(BlockPos::new(4, 4, 4));
        let i = std::path::PathBuf::from("/jeu");
        e.situer_echanges(Some(i.clone()), None, Some(2975), Some("Ville"));
        e.echanges.trouves = vec![Trouve {
            chemin: i.join("schematics").join("porte.litematic"),
            nom: "porte.litematic".into(),
            octets: 12_345,
        }];
        if plein {
            e.suivre_presse(PressePapiers {
                taille: Some([5, 5, 5]),
                ancre: [0, 0, 0],
                source: "porte.litematic".into(),
                data_version: Some(3465),
                remarques: vec!["3 régions fusionnées en une boîte".into()],
                version: 1,
            });
        }
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            tf_app::interface::dessiner(ctx, &mut e);
        });
        assert!(e.demande.is_none(), "dessiner n'envoie rien tout seul");
        assert!(e.presse_plus_recente().is_some() == plein);
    }
}

/// Le TEXTE que l'interface affiche, image faite — ce que l'œil lirait.
fn texte_affiche(e: &mut Etat) -> String {
    fn recueillir(s: &egui::Shape, t: &mut String) {
        match s {
            egui::Shape::Text(x) => {
                t.push_str(x.galley.text());
                t.push('\n');
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| recueillir(s, t)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    let sortie = ctx.run(egui::RawInput::default(), |ctx| {
        tf_app::interface::dessiner(ctx, e);
    });
    let mut t = String::new();
    for s in &sortie.shapes {
        recueillir(&s.shape, &mut t);
    }
    t
}

/// **Sur le plan de référence, l'inspecteur dit la hauteur du PLAN** — pas une
/// hauteur déduite de la case visée : vue d'en dessous, la case est AU-DESSUS
/// du plan, et la déduire donnerait un plan d'un bloc trop haut. Il ne
/// propose pas non plus de casser : sous le plan, il n'y a que de l'air.
#[test]
fn l_inspecteur_dit_la_hauteur_du_plan_vu_d_en_dessous() {
    let mut e = Etat::cadre([0.0; 3], [32.0; 3], 1.0);
    e.plan = tf_app::etat::PlanDeReference { actif: true, y: 64 };
    let d_en_dessous = tf_render::Camera {
        oeil: [0.5, 40.0, 0.5],
        cible: [1.5, 41.0, 0.5],
        fov: 1.0,
        proche: 0.1,
        loin: 1000.0,
    };
    e.relever_vise(&d_en_dessous, 1.0, Some([0.0, 0.0]), 256.0, &|_| false);
    assert!(e.vise.sur_le_plan);
    assert_eq!(
        e.vise.case.map(|c| c.y),
        Some(64),
        "d'en dessous, la case est au-dessus"
    );
    let t = texte_affiche(&mut e);
    assert!(t.contains("plan de référence, y = 64"), "{t}");
    assert!(!t.contains("casser :"), "{t}");
    assert!(
        t.contains("PLAN DE RÉFÉRENCE"),
        "la section de réglage : {t}"
    );
}
