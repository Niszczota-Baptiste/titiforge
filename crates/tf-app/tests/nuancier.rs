//! **Choisir un bloc : ce qui est proposé, et dans quel ordre.**
//!
//! Sans fenêtre : l'ordre des propositions EST la fonction — la première est
//! celle qu'Entrée prend. Un ordre qui dépendrait de l'ordre d'entrée ou d'un
//! hasard de table se lirait « l'outil est instable ».

use tf_app::nuancier::{Candidat, Nuancier, Origine, MAX_RECENTS};
use tf_ops::catalogue::{descripteur, Params, Valeur};

const PACK: [&str; 7] = [
    "minecraft:stone",
    "minecraft:stone_bricks",
    "minecraft:cobblestone",
    "minecraft:oak_stairs",
    "minecraft:dark_oak_stairs",
    "minecraft:oak_planks",
    "minefield:chaise_oak",
];

/// Une table d'états de scène : l'ordre est celui où le décodeur les a
/// rencontrés, pas un ordre alphabétique.
const MONDE: [&str; 4] = [
    "minecraft:stone",
    "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=false",
    "minecraft:air",
    "minecraft:oak_stairs|facing=west,half=top,shape=straight,waterlogged=false",
];

fn nuancier() -> Nuancier {
    let mut n = Nuancier::new(PACK);
    n.suivre_monde(0, MONDE.iter().copied(), MONDE.len());
    n
}

fn cles(v: &[Candidat]) -> Vec<&str> {
    v.iter().map(|c| c.cle.as_str()).collect()
}

#[test]
fn sans_rien_tape_le_vise_puis_les_recents_puis_le_monde() {
    let mut n = nuancier();
    n.utiliser("minecraft:cobblestone");
    n.utiliser("minecraft:oak_planks");
    let v = n.chercher("", Some("minefield:chaise_oak|facing=north"), 20);
    assert_eq!(
        cles(&v)[..3],
        [
            "minefield:chaise_oak|facing=north",
            "minecraft:oak_planks",
            "minecraft:cobblestone"
        ]
    );
    assert_eq!(v[0].origine, Origine::Vise);
    assert_eq!(v[1].origine, Origine::Recent);
    assert!(v[3..].iter().all(|c| c.origine == Origine::Monde));
    assert_eq!(v.len(), 3 + MONDE.len(), "le pack entier n'est pas proposé");
    // Ce qu'on montre, et donc ce qu'on écrit dans le champ, est la syntaxe
    // du jeu — celle qu'on tape.
    assert_eq!(v[0].affiche, "minefield:chaise_oak[facing=north]");
}

#[test]
fn la_correspondance_classe_avant_l_origine() {
    let n = nuancier();
    let v = n.chercher("stone", None, 20);
    // Exact (le chemin vaut « stone »), puis préfixe, puis début de mot
    // (« cobble|stone » ne commence pas par stone : c'est un CONTENU).
    assert_eq!(
        cles(&v),
        [
            "minecraft:stone",
            "minecraft:stone_bricks",
            "minecraft:cobblestone"
        ]
    );
    assert_eq!(
        v[0].origine,
        Origine::Monde,
        "à rang égal, le monde passe avant le pack — et un bloc ne se propose \
         qu'une fois, sous sa meilleure origine"
    );
}

#[test]
fn les_etats_du_monde_passent_avant_le_nom_nu() {
    // « oak_stairs » : les états que le JEU a écrits d'abord — ce qu'un
    // //replace doit viser pour ne rien rater — puis le nom nu du pack. Entre
    // deux états, le plus court : `half=top` avant `half=bottom`, et non
    // l'ordre où la scène les a rencontrés.
    let v = nuancier().chercher("oak_stairs", None, 20);
    let c = cles(&v);
    assert_eq!(
        c[..3],
        [
            "minecraft:oak_stairs|facing=west,half=top,shape=straight,waterlogged=false",
            "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=false",
            "minecraft:oak_stairs",
        ]
    );
    // « dark_oak_stairs » ne commence pas par « oak_stairs » : il n'est même
    // pas un début de mot, seulement un contenu.
    assert_eq!(c.last(), Some(&"minecraft:dark_oak_stairs"));
}

#[test]
fn plusieurs_mots_et_la_syntaxe_du_jeu_se_comprennent() {
    let n = nuancier();
    let v = n.chercher("oak stairs", None, 20);
    assert!(cles(&v).contains(&"minecraft:dark_oak_stairs"));
    assert!(
        !cles(&v).contains(&"minecraft:oak_planks"),
        "« stairs » manque"
    );
    // Chaque mot compte : « oak » COMMENCE le premier nom et n'est qu'un mot
    // du second. Ne garder que le pire des deux mots les mettait à égalité,
    // et la longueur départageait alors en faveur du plus court — le moins
    // bon. Les noms sont choisis pour que la longueur tire dans le MAUVAIS
    // sens, sinon le test ne distinguerait pas les deux règles.
    let m = Nuancier::new([
        "minecraft:oak_stairs_bien_longues",
        "minecraft:x_oak_stairs",
    ]);
    assert_eq!(
        cles(&m.chercher("oak stairs", None, 5)),
        [
            "minecraft:oak_stairs_bien_longues",
            "minecraft:x_oak_stairs"
        ]
    );

    // Un début d'état tapé à la main trouve les états qui le portent.
    let v = n.chercher("minecraft:oak_stairs[facing=west", None, 20);
    assert_eq!(
        cles(&v),
        ["minecraft:oak_stairs|facing=west,half=top,shape=straight,waterlogged=false"]
    );
    // Un espace de noms seul liste ses blocs.
    let v = n.chercher("minefield:", None, 20);
    assert_eq!(cles(&v), ["minefield:chaise_oak"]);
    // Les majuscules ne comptent pas : les identifiants du jeu n'en ont pas.
    assert_eq!(
        cles(&n.chercher("STONE_B", None, 20)),
        ["minecraft:stone_bricks"]
    );
    assert!(n.chercher("rien_de_tel", None, 20).is_empty());
}

#[test]
fn l_ordre_est_total_et_borne() {
    let n = nuancier();
    let a = n.chercher("o", None, 50);
    // Un nuancier construit dans l'ordre inverse propose la même liste.
    let mut inverse = Nuancier::new(PACK.iter().rev());
    inverse.suivre_monde(0, MONDE.iter().rev().copied(), MONDE.len());
    assert_eq!(a, inverse.chercher("o", None, 50));
    assert_eq!(n.chercher("o", None, 2), a[..2]);
}

#[test]
fn les_recents_montent_en_tete_sans_doublon_et_restent_bornes() {
    let mut n = Nuancier::new(PACK);
    assert!(
        n.utiliser("Stone"),
        "canonique : « Stone » est minecraft:stone"
    );
    assert!(
        !n.utiliser("minecraft:stone"),
        "déjà en tête : rien ne change"
    );
    assert!(!n.utiliser("minecraft:air"), "l'air est toujours proposé");
    assert!(
        !n.utiliser("oak_stairs[facing"),
        "illisible : pas un récent"
    );
    for i in 0..MAX_RECENTS + 2 {
        n.utiliser(&format!("minefield:bloc_{i}"));
    }
    assert_eq!(n.recents().len(), MAX_RECENTS);
    assert_eq!(
        n.recents()[0],
        format!("minefield:bloc_{}", MAX_RECENTS + 1)
    );
    assert!(!n.recents().contains(&"minecraft:stone".to_string()));
    n.utiliser("minefield:bloc_5");
    assert_eq!(n.recents()[0], "minefield:bloc_5");
    assert_eq!(
        n.recents()
            .iter()
            .filter(|r| *r == "minefield:bloc_5")
            .count(),
        1
    );

    // Ils survivent à un aller-retour par le texte, et un fichier retouché à
    // la main n'empêche rien.
    let mut m = Nuancier::new(PACK);
    m.relire_recents(&(n.recents_en_texte() + "n'importe quoi[\nminecraft:air\n"));
    assert_eq!(m.recents(), n.recents());
    // « stone » et « minecraft:stone » sont le même bloc : une seule place.
    m.relire_recents("minecraft:stone\nStone\nminecraft:dirt\n");
    assert_eq!(m.recents(), ["minecraft:stone", "minecraft:dirt"]);
}

#[test]
fn une_operation_note_tous_ses_blocs() {
    let mut n = Nuancier::new(PACK);
    let d = descripteur("remplacer").unwrap();
    let mut p = Params::new();
    p.poser("de", Valeur::texte("minecraft:stone"));
    p.poser("vers", Valeur::texte("oak_stairs[facing=east]"));
    assert!(n.utiliser_params(d, &p));
    assert_eq!(
        n.recents(),
        ["minecraft:oak_stairs|facing=east", "minecraft:stone"]
    );

    let d = descripteur("melanger").unwrap();
    let mut p = Params::new();
    p.poser(
        "melange",
        Valeur::Melange(vec![(3, "cobblestone".into()), (1, "stone".into())]),
    );
    n.utiliser_params(d, &p);
    assert_eq!(
        n.recents()[..2],
        ["minecraft:stone", "minecraft:cobblestone"]
    );
}

#[test]
fn un_bloc_inconnu_du_pack_et_du_monde_se_signale() {
    let n = nuancier();
    assert!(n.connu("minecraft:stone"));
    assert!(
        n.connu("minecraft:oak_stairs|facing=north,half=bottom,shape=straight"),
        "le NOM est dans le pack : l'état se juge au jeu, pas ici"
    );
    assert!(n.connu("minecraft:air"));
    assert!(!n.connu("minecraft:stonee"), "une faute de frappe");
    assert!(!n.connu("minefield:chaise"));
    // Sans pack chargé, on ne juge rien : tout serait « inconnu ».
    assert!(Nuancier::default().connu("minecraft:stonee"));
}

#[test]
fn le_monde_se_suit_par_la_fin_et_se_reprend_quand_la_table_change() {
    let mut n = Nuancier::new(PACK);
    n.suivre_monde(0, MONDE[..2].iter().copied(), 2);
    assert_eq!(n.monde_vus(), 2);
    assert_eq!(n.chercher("", None, 50).len(), 2);
    // La table a grandi : seuls les nouveaux sont lus — les deux premiers
    // ne se dédoublent pas.
    n.suivre_monde(0, MONDE.iter().copied(), MONDE.len());
    assert_eq!(n.chercher("", None, 50).len(), MONDE.len());
    // Rien n'a bougé : rien n'est lu — l'itérateur n'est même pas parcouru.
    n.suivre_monde(
        0,
        std::iter::from_fn(|| panic!("relue sans avoir bougé")),
        MONDE.len(),
    );

    // Une table RECHARGÉE, même plus longue, n'est pas la suite de
    // l'ancienne : ses premiers états ne sont pas ceux qu'on a vus.
    let neuve = [
        "minefield:chaise_oak",
        "minecraft:dirt",
        "minecraft:gravel",
        "minecraft:sand",
        "minecraft:clay",
    ];
    n.suivre_monde(1, neuve.into_iter(), neuve.len());
    let mut attendu = neuve.to_vec();
    attendu.sort_unstable();
    assert_eq!(cles(&n.chercher("", None, 50)), attendu);

    // Et une table plus COURTE que ce qu'on a vu repart de zéro, même sous
    // le même numéro.
    n.suivre_monde(1, ["minecraft:stone"].into_iter(), 1);
    assert_eq!(cles(&n.chercher("", None, 50)), ["minecraft:stone"]);

    // Changer de pack garde les récents : c'est une habitude, pas un monde.
    n.utiliser("minecraft:cobblestone");
    n.repartir(["minecraft:dirt"]);
    assert_eq!(n.recents(), ["minecraft:cobblestone"]);
    assert_eq!(n.monde_vus(), 0);
    assert!(!n.connu("minecraft:stone"));
}
