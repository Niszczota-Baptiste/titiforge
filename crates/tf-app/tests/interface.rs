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
use tf_app::interface::champ;
use tf_ops::catalogue::{descripteur, Param, Saisie, Valeur, OPS};
use tf_world::coords::BlockPos;

/// Fait tourner une image d'interface sans surface ni GPU.
fn dessiner(f: impl FnOnce(&mut egui::Ui)) {
    // `run` prend un `FnMut` — il peut rendre plusieurs images. La fermeture
    // de l'appelant, elle, ne sert qu'une fois : on la met dans une `Option`
    // plutôt que de la lui faire cloner.
    let mut une_fois = Some(f);
    let ctx = egui::Context::default();
    // La sortie décrit ce qu'il faudrait afficher ; ici on ne dessine pas.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
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
        dessiner(|ui| dessine = champ(ui, &p, &mut v));
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
            dessiner(|ui| dessine = champ(ui, p, &mut v));
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
    dessiner(|ui| dessine = champ(ui, &p, &mut v));
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
