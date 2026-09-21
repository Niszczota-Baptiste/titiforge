//! **Ce que les opérations disent d'elles-mêmes, croisé avec ce qu'elles
//! font.**
//!
//! Le catalogue a une raison d'être unique : qu'un hôte — la ligne de
//! commande, la coque, un greffon — n'ait pas à réécrire ce qu'une opération
//! prend en paramètre. Sa valeur tient donc entièrement à une chose : que la
//! description et l'opération ne puissent PAS diverger. Ces tests sont ce qui
//! le garantit.
//!
//! Ils portent sur les quatre fautes qu'`ExeWorldEdit` a payées, dans cet
//! ordre :
//!
//! 1. *déclaré, branché, testé — et inatteignable* : une opération décrite que
//!    personne ne sait construire ;
//! 2. *un normaliseur qu'aucun hôte n'appelle* : ici il est sur le chemin, donc
//!    il doit être idempotent ;
//! 3. *un normaliseur qui jette ce qu'il ne nomme pas* : la graine qui
//!    disparaît entre le descripteur et l'opération ;
//! 4. *un pourcentage serré comme un rapport* : un serrage est l'endroit où
//!    une unité fausse devient invisible.

use tf_anvil::Interner;
use tf_ops::catalogue::{
    chercher, construire, descripteur, normaliser, Composee, Defaut, Descripteur, Erreur, Params,
    Saisie, Travail, Valeur, OPS,
};
use tf_ops::{Masque, Motif};

/// De quoi remplir un paramètre OBLIGATOIRE sans écrire une seconde liste : la
/// valeur se déduit de la saisie déclarée, donc une opération ajoutée demain
/// est couverte sans toucher à ce fichier.
fn valeur_plausible(s: Saisie) -> Valeur {
    match s {
        Saisie::Bloc => Valeur::texte("minecraft:stone"),
        Saisie::Biome => Valeur::texte("minecraft:plains"),
        Saisie::Melange => Valeur::Melange(vec![(3, "minecraft:stone".into())]),
        Saisie::Entier { min, .. } => Valeur::Entier(min),
        Saisie::Vecteur => Valeur::Vecteur([1, 2, 3]),
        Saisie::Direction => Valeur::Direction(tf_world::selection::Direction::PlusZ),
        Saisie::Transformation => Valeur::Transformation(Some(tf_blocks::Transfo::Rot90)),
    }
}

/// Des paramètres complets pour une opération : ses défauts, plus de quoi
/// satisfaire ce qui est obligatoire.
fn remplir(d: &Descripteur) -> Params {
    let mut p = d.defauts();
    for decl in d.params {
        if decl.defaut.is_none() {
            p.poser(decl.nom, valeur_plausible(decl.saisie));
        }
    }
    p
}

/// **Déclaré, branché, testé — et inatteignable.** La faute la plus chère
/// d'`ExeWorldEdit` : `biome`, `copier` et `coller` existaient de bout en bout
/// côté moteur, et aucun outil ne les proposait.
#[test]
fn chaque_descripteur_se_construit() {
    assert!(!OPS.is_empty());
    for d in OPS {
        let mut i = Interner::new();
        let t = construire(d.id, &remplir(d), &mut i)
            .unwrap_or_else(|e| panic!("« {} » ne se construit pas : {e}", d.id));
        assert_eq!(
            t.id(),
            d.id,
            "« {} » construit l'opération « {} » — une ligne de match recopiée",
            d.id,
            t.id()
        );
    }
}

/// L'autre sens : on ne peut RIEN construire sans descripteur. C'est ce qui
/// met la normalisation sur le chemin au lieu de l'espérer.
#[test]
fn rien_ne_se_construit_sans_descripteur() {
    let mut i = Interner::new();
    let e = construire("pousser-tirer", &Params::new(), &mut i).unwrap_err();
    assert_eq!(e, Erreur::Inconnue("pousser-tirer".into()));
}

/// **Le normaliseur est sur le chemin, donc il sera appliqué plusieurs fois à
/// la même chose** — par l'hôte, par `construire`, au rejeu. Un normaliseur
/// qui ne serait pas idempotent déplacerait la valeur un peu plus à chaque
/// passage, sans que rien ne le dise.
#[test]
fn la_normalisation_est_idempotente() {
    for d in OPS {
        for p in [remplir(d), demesure(d)] {
            let une = normaliser(d, &p).unwrap();
            let deux = normaliser(d, &une).unwrap();
            assert_eq!(une, deux, "« {} » n'est pas idempotent", d.id);
        }
    }
}

/// Des valeurs volontairement hors bornes : c'est là que le serrage se voit.
fn demesure(d: &Descripteur) -> Params {
    let mut p = remplir(d);
    for decl in d.params {
        if let Saisie::Entier { .. } = decl.saisie {
            p.poser(decl.nom, Valeur::Entier(i64::MAX));
        }
    }
    p
}

/// **Un serrage est un endroit où une unité fausse devient invisible.** Un
/// lissage de rayon dix mille ne rend pas d'erreur : il rend un monde plat et
/// dix minutes d'attente.
#[test]
fn les_bornes_serrent_dans_les_deux_sens() {
    let mut vues = 0;
    for d in OPS {
        for decl in d.params {
            let Saisie::Entier { min, max } = decl.saisie else {
                continue;
            };
            vues += 1;
            assert!(min <= max, "« {} » / {} : bornes inversées", d.id, decl.nom);

            let mut p = remplir(d);
            p.poser(decl.nom, Valeur::Entier(max + 1000));
            let haut = normaliser(d, &p).unwrap();
            assert_eq!(haut.get(decl.nom), Some(&Valeur::Entier(max)));

            p.poser(decl.nom, Valeur::Entier(min - 1000));
            let bas = normaliser(d, &p).unwrap();
            assert_eq!(bas.get(decl.nom), Some(&Valeur::Entier(min)));
        }
    }
    assert!(vues >= 5, "seulement {vues} paramètres bornés — suspect");
}

/// **Un normaliseur qui jette ce qu'il ne nomme pas.** Dans `ExeWorldEdit`,
/// ajouter `seed` au descripteur ET à l'opération n'aurait rien changé : le
/// normaliseur ne recopiait que ce qu'il listait, et la graine disparaissait
/// entre les deux, sans erreur. Ici l'inconnu est REFUSÉ, donc il se voit.
#[test]
fn un_parametre_inconnu_est_refuse_pas_ignore() {
    let d = descripteur("poser").unwrap();
    let mut p = remplir(d);
    p.poser("seed", Valeur::Entier(7));
    assert_eq!(
        normaliser(d, &p).unwrap_err(),
        Erreur::ParamInconnu {
            op: "poser".into(),
            nom: "seed".into()
        }
    );
}

#[test]
fn un_parametre_obligatoire_absent_est_une_erreur() {
    let d = descripteur("remplacer").unwrap();
    let mut p = Params::new();
    p.poser("de", Valeur::texte("minecraft:stone"));
    assert_eq!(
        normaliser(d, &p).unwrap_err(),
        Erreur::ParamManquant {
            op: "remplacer".into(),
            nom: "vers".into()
        }
    );
}

/// **Une valeur du mauvais genre est refusée, jamais convertie.** C'est le
/// plantage de « Naturaliser → Personnalisé » : l'inspecteur envoyait un objet
/// là où l'opération attendait une chaîne, et ça sortait en
/// `s.includes is not a function` — une erreur qui ne nomme ni l'opération ni
/// le paramètre.
#[test]
fn une_valeur_du_mauvais_genre_est_refusee() {
    let d = descripteur("poser").unwrap();
    let mut p = Params::new();
    p.poser("bloc", Valeur::Entier(42));
    match normaliser(d, &p).unwrap_err() {
        Erreur::MauvaisGenre {
            op, nom, attendu, ..
        } => {
            assert_eq!(
                (op.as_str(), nom.as_str(), attendu),
                ("poser", "bloc", "texte")
            );
        }
        autre => panic!("erreur inattendue : {autre}"),
    }
}

/// Les trois opérations DIRECTES portent un identifiant recopié, pas déduit :
/// c'est donc leur masque et leur motif qu'il faut regarder. Une ligne de
/// `match` copiée-collée y perdrait le masque — et `//replace` remplirait
/// toute la sélection.
#[test]
fn les_operations_directes_posent_le_bon_masque_et_le_bon_motif() {
    let mut i = Interner::new();

    let d = descripteur("poser").unwrap();
    let Travail::Direct { plan, .. } = construire("poser", &remplir(d), &mut i).unwrap() else {
        panic!("« poser » doit être directe");
    };
    assert_eq!(plan.masque, Masque::Tout);
    assert!(matches!(plan.motif, Motif::Bloc(_)));

    let d = descripteur("remplacer").unwrap();
    let Travail::Direct { plan, .. } = construire("remplacer", &remplir(d), &mut i).unwrap() else {
        panic!("« remplacer » doit être directe");
    };
    assert!(
        matches!(plan.masque, Masque::Etat(_)),
        "sans masque, //replace remplit tout"
    );

    let d = descripteur("melanger").unwrap();
    let mut p = d.defauts();
    p.poser(
        "melange",
        Valeur::Melange(vec![
            (3, "minecraft:stone".into()),
            (1, "minecraft:dirt".into()),
        ]),
    );
    let Travail::Direct { plan, .. } = construire("melanger", &p, &mut i).unwrap() else {
        panic!("« mélange » doit être directe");
    };
    match plan.motif {
        Motif::Melange(v) => assert_eq!(v.len(), 2),
        autre => panic!("un mélange de deux entrées doit rester un mélange : {autre:?}"),
    }
}

/// Les composées portent leurs paramètres jusqu'au bout — c'est le trajet
/// exact où `ExeWorldEdit` perdait sa graine.
#[test]
fn les_composees_gardent_leurs_parametres() {
    let mut i = Interner::new();
    let d = descripteur("deplacer").unwrap();
    let mut p = d.defauts();
    p.poser("decalage", Valeur::Vecteur([64, 0, -16]));
    p.poser("remplir", Valeur::texte("minecraft:dirt"));
    let Travail::Composee(Composee::Deplacer { decalage, remplir }) =
        construire("deplacer", &p, &mut i).unwrap()
    else {
        panic!("« déplacer » doit être composée");
    };
    assert_eq!(decalage, [64, 0, -16]);
    assert_eq!(remplir, "minecraft:dirt");

    let d = descripteur("naturaliser").unwrap();
    let mut p = d.defauts();
    p.poser("profondeur", Valeur::Entier(7));
    let Travail::Composee(Composee::Naturaliser {
        surface,
        profondeur,
        ..
    }) = construire("naturaliser", &p, &mut i).unwrap()
    else {
        panic!("« naturaliser » doit être composée");
    };
    assert_eq!(profondeur, 7);
    assert_eq!(surface, "minecraft:grass_block");
}

/// **Un identifiant ne doit jamais tomber sur le nom WorldEdit d'une autre
/// opération.** `//set` nomme légitimement deux opérations — c'est une même
/// commande du jeu à un motif près — donc les noms WorldEdit ne sont PAS des
/// clés, et la recherche prend l'identifiant d'abord. Mais si un identifiant
/// se mettait à désigner autre chose par la seconde passe, la réponse
/// dépendrait de l'ordre du tableau.
#[test]
fn les_identifiants_sont_uniques_et_prioritaires() {
    let mut ids: Vec<&str> = OPS.iter().map(|d| d.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "deux opérations partagent un identifiant");

    for d in OPS {
        assert_eq!(descripteur(d.id).map(|x| x.id), Some(d.id));
        for autre in OPS {
            if autre.id != d.id {
                assert!(
                    !autre.we.contains(&d.id),
                    "« {} » est aussi un nom WorldEdit de « {} »",
                    d.id,
                    autre.id
                );
            }
        }
        assert!(!d.we.is_empty(), "« {} » n'a aucun nom WorldEdit", d.id);
        assert!(!d.label.is_empty() && !d.resume.is_empty());
    }
}

/// La palette doit RENDRE les candidats, pas en choisir un à la place de
/// l'utilisateur.
#[test]
fn la_recherche_rend_tous_les_candidats() {
    let set: Vec<&str> = chercher("//set").map(|d| d.id).collect();
    assert!(
        set.contains(&"poser") && set.contains(&"melanger"),
        "« //set » nomme les deux : {set:?}"
    );
    assert_eq!(
        chercher("//hollow").map(|d| d.id).collect::<Vec<_>>(),
        ["creuser"]
    );
    assert_eq!(chercher("").count(), OPS.len());
    assert_eq!(
        chercher("REMPLAC").map(|d| d.id).collect::<Vec<_>>(),
        ["remplacer"]
    );
}

/// **Ce qui matérialise toute la sélection doit l'ANNONCER.** `//hollow` est la
/// seule : c'est assumé, c'est borné, et ça se découvrirait autrement quand
/// l'éditeur disparaît avec le travail en cours. En ajouter une est une
/// décision, pas un effet de bord — d'où cette liste, qui la rend délibérée.
#[test]
fn seul_creuser_annonce_materialiser_toute_la_selection() {
    let m: Vec<&str> = OPS
        .iter()
        .filter(|d| d.cout.materialise)
        .map(|d| d.id)
        .collect();
    assert_eq!(m, ["creuser"]);
}

/// **Le coût annoncé se croise avec ce que l'opération DÉCLARE au moteur.**
/// `Portee::Colonne` ferme les étages `Section` et `Palette` : l'annoncer à
/// l'écran et l'oublier dans le code, ou l'inverse, ferait mentir l'un des
/// deux. Les deux seules opérations dont l'objet existe sans staging sont
/// croisées ici ; les autres n'ont rien à croiser aujourd'hui.
#[test]
fn le_cout_declare_correspond_a_la_portee_du_moteur() {
    use tf_ops::plan::Operation;
    use tf_ops::Portee;

    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let nat = tf_ops::Naturaliser::nouveau(
        i.intern("minecraft:grass_block"),
        i.intern("minecraft:dirt"),
        i.intern("minecraft:stone"),
        air,
    );
    assert_eq!(nat.portee(), Portee::Colonne);
    assert!(descripteur("naturaliser").unwrap().cout.colonne);

    let carte = tf_ops::Carte::vide(0, 0, 4, 4);
    let lis = tf_ops::Lissage {
        carte: &carte,
        vide: air,
        compter: false,
    };
    assert_eq!(lis.portee(), Portee::Colonne);
    assert!(descripteur("lisser").unwrap().cout.colonne);

    // Et les directes, elles, gardent la portée qui donne l'étage palette.
    for id in ["poser", "remplacer", "melanger"] {
        assert!(!descripteur(id).unwrap().cout.colonne);
        assert!(
            descripteur(id).unwrap().cout.forme,
            "« {id} » accepte une forme"
        );
    }
}

/// Un défaut doit répondre à la saisie qu'il accompagne. Un `Defaut::Entier`
/// sur un paramètre de type bloc passerait la compilation et sortirait en
/// `MauvaisGenre` chez l'utilisateur, à la première ouverture du formulaire.
#[test]
fn chaque_defaut_repond_a_sa_saisie() {
    for d in OPS {
        for decl in d.params {
            let Some(def) = decl.defaut else { continue };
            let v = def.valeur();
            let mut p = remplir(d);
            p.poser(decl.nom, v.clone());
            normaliser(d, &p).unwrap_or_else(|e| {
                panic!(
                    "« {} » / {} : le défaut {def:?} est refusé — {e}",
                    d.id, decl.nom
                )
            });
        }
    }
    // Et le témoin : un défaut du mauvais genre serait bien refusé.
    let d = descripteur("poser").unwrap();
    let mut p = remplir(d);
    p.poser("bloc", Defaut::Entier(3).valeur());
    assert!(normaliser(d, &p).is_err());
}

/// Ce que le formulaire montre À L'OUVERTURE : les défauts seuls. Ils doivent
/// suffire, SAUF pour ce qu'on ne peut pas deviner — « remplacer quoi ? » n'a
/// pas de réponse raisonnable, et en inventer une ferait remplacer l'air par
/// de la pierre au premier clic distrait.
#[test]
fn les_defauts_seuls_suffisent_sauf_pour_ce_qui_ne_se_devine_pas() {
    let mut sans_defaut = Vec::new();
    for d in OPS {
        let complet = d.params.iter().all(|p| p.defaut.is_some());
        let ok = normaliser(d, &d.defauts()).is_ok();
        assert_eq!(
            ok,
            complet,
            "« {} » : défauts {}, normalisation {}",
            d.id,
            if complet { "complets" } else { "incomplets" },
            if ok { "réussie" } else { "refusée" }
        );
        if !complet {
            sans_defaut.push(d.id);
        }
    }
    assert_eq!(
        sans_defaut,
        ["remplacer"],
        "une opération qui exige une saisie est une décision"
    );
}

/// **Le normaliseur est SUR LE CHEMIN, pas à côté.**
///
/// C'est la forme forte du remède : plutôt qu'espérer que chaque hôte pense à
/// valider, on rend la validation inévitable. Le test le prouve là où ça se
/// voit — une valeur hors bornes donnée à `construire` doit ressortir SERRÉE
/// dans le travail, sans que l'appelant ait rien fait.
#[test]
fn construire_normalise_meme_si_l_hote_ne_l_a_pas_fait() {
    let mut i = Interner::new();
    let mut p = Params::new();
    p.poser("epaisseur", Valeur::Entier(1_000_000));
    let Travail::Composee(Composee::Creuser { epaisseur }) =
        construire("//hollow", &p, &mut i).unwrap()
    else {
        panic!("« creuser » doit être composée");
    };
    assert_eq!(
        epaisseur, 64,
        "la borne du descripteur n'a pas été appliquée"
    );

    // Et un paramètre inconnu ne passe pas davantage par `construire`.
    let mut p = Params::new();
    p.poser("epaisseur", Valeur::Entier(2));
    p.poser("graine", Valeur::Entier(7));
    assert!(construire("creuser", &p, &mut i).is_err());
}

/// `Saisie::TOUTES` doit être complète : c'est elle qui permettra d'EXIGER
/// d'un hôte qu'il sache saisir tous les genres de paramètres. Incomplète,
/// elle laisserait passer exactement la faute qu'elle sert à empêcher.
#[test]
fn toutes_les_saisies_sont_enumerees() {
    let mut rangs: Vec<usize> = Saisie::TOUTES.iter().map(|s| s.rang()).collect();
    rangs.sort_unstable();
    assert_eq!(
        rangs,
        (0..Saisie::TOUTES.len()).collect::<Vec<_>>(),
        "`TOUTES` ne couvre pas toutes les variantes"
    );
    // Et chaque saisie employée par une opération y figure.
    for d in OPS {
        for decl in d.params {
            assert!(
                Saisie::TOUTES
                    .iter()
                    .any(|s| s.rang() == decl.saisie.rang()),
                "« {} » / {} emploie une saisie absente de TOUTES",
                d.id,
                decl.nom
            );
        }
    }
}

/// **Une valeur s'affiche comme on l'écrirait**, pas comme Rust la nomme.
/// L'aide de la ligne de commande et le champ de la coque lisent la même
/// table ; sans elle, l'aide montrait `Texte("minecraft:stone")` et
/// `Direction(PlusX)` à quelqu'un qui tape une commande.
#[test]
fn une_valeur_s_affiche_comme_on_l_ecrirait() {
    use tf_world::selection::Direction;
    let cas = [
        (Valeur::texte("minecraft:stone"), "minecraft:stone"),
        (Valeur::Entier(-64), "-64"),
        (Valeur::Vecteur([1, -2, 3]), "1,-2,3"),
        (Valeur::Melange(vec![]), "aucun"),
        (
            Valeur::Melange(vec![
                (3, "minecraft:stone".into()),
                (1, "minecraft:dirt".into()),
            ]),
            "3:minecraft:stone,1:minecraft:dirt",
        ),
        (Valeur::Direction(Direction::MoinsZ), "nord"),
        (Valeur::Transformation(None), "aucune"),
        (
            Valeur::Transformation(Some(tf_blocks::Transfo::MiroirX)),
            "miroir est-ouest",
        ),
    ];
    for (v, attendu) in cas {
        assert_eq!(v.to_string(), attendu);
    }
    // Aucun défaut du catalogue ne doit s'afficher avec un nom de type Rust.
    for d in OPS {
        for p in d.params {
            if let Some(def) = p.defaut {
                let t = def.valeur().to_string();
                assert!(
                    !t.contains('(') && !t.contains('"'),
                    "« {} » / {} affiche du Rust : {t}",
                    d.id,
                    p.nom
                );
            }
        }
    }
}
