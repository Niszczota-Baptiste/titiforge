//! **Les composants : poser vingt fois, modifier une fois, voir les vingt
//! suivre — et qu'UN Ctrl+Z défasse la modification, pas les poses.**
//!
//! C'est la sortie de la phase 7, écrite comme un test. Ce qui est vérifié
//! est ce qu'aucune capture ne dirait : que CHAQUE instance porte la nouvelle
//! définition dans SON orientation, que seul ce qui change dans la définition
//! change autour d'elle — un coffre rempli dans une instance reste rempli —,
//! et que l'annulation rend à la fois les blocs et le document.
//!
//! L'orientation attendue est écrite À LA MAIN (`vers_monde`) : la tirer du
//! code sous test ne prouverait que sa cohérence avec lui-même.

use std::sync::Mutex;

use tf_anvil::entites::Entite;
use tf_anvil::Interner;
use tf_bench::{region, region_en, Terrain};
use tf_blocks::Transfo;
use tf_ops::composant::{
    creer, detacher, faire, mettre_a_jour, poser, renommer, Action, ErreurComposant, Instance,
    Projet, MAX_CASES,
};
use tf_ops::edition::{appliquer, coller, copier, rejouer, Pas, Sens};
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{
    Dimension, Folder, MemorySource, Overview, RegionSink, RegionSource, SourceError,
};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
type St = Staging<MemorySource, MemorySource>;

fn monde() -> St {
    let src = MemorySource::new();
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&Terrain::petite()),
    );
    Staging::new(src, MemorySource::new())
}

fn poser_bloc_en<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    dim: &Dimension,
    p: BlockPos,
    bloc: &str,
    i: &mut Interner,
) {
    let id = i.intern(bloc);
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(id));
    appliquer(st, dim, Folder::Region, &BBox::single(p), &plan, i).unwrap();
}

fn poser_bloc<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    p: BlockPos,
    bloc: &str,
    i: &mut Interner,
) {
    poser_bloc_en(st, &SURFACE, p, bloc, i);
}

fn etat_en<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    dim: &Dimension,
    p: BlockPos,
) -> String {
    let mut i = Interner::new();
    let pr = copier(st, dim, Folder::Region, &BBox::single(p), &mut i).unwrap();
    i.resolve(pr.blocs[0]).unwrap().to_string()
}

fn etat<S: RegionSource, O: RegionSource + RegionSink>(st: &Staging<S, O>, p: BlockPos) -> String {
    etat_en(st, &SURFACE, p)
}

/// La règle d'orientation, écrite à la main pour le seul bloc orienté du
/// test : `t:fleche|facing=…`. Tout état sans propriété est invariant.
fn regle(cle: &str, t: Transfo) -> Option<String> {
    let Some((nom, props)) = cle.split_once('|') else {
        return Some(cle.to_string());
    };
    if nom != "t:fleche" {
        return None;
    }
    let f = props.strip_prefix("facing=")?;
    let tourne = |f: &str| match f {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    };
    let neuf = match t {
        Transfo::Rot90 => tourne(f).to_string(),
        Transfo::Rot180 => tourne(tourne(f)).to_string(),
        Transfo::Rot270 => tourne(tourne(tourne(f))).to_string(),
        Transfo::MiroirX => match f {
            "east" => "west".into(),
            "west" => "east".into(),
            autre => autre.into(),
        },
        Transfo::MiroirZ => match f {
            "north" => "south".into(),
            "south" => "north".into(),
            autre => autre.into(),
        },
    };
    Some(format!("{nom}|facing={neuf}"))
}

/// **Où atterrit la case locale `(x, y, 0)` d'une définition 3 × 2 × 1**,
/// selon l'orientation de l'instance. Écrit à la main, depuis la convention du
/// dépôt — un quart de tour envoie +X sur +Z — et pas depuis le code.
fn vers_monde(coin: BlockPos, t: Option<Transfo>, x: i32, y: i32) -> BlockPos {
    let (dx, dz) = match t {
        None => (x, 0),
        Some(Transfo::Rot90) => (0, x),
        Some(Transfo::Rot180) => (2 - x, 0),
        Some(Transfo::Rot270) => (0, 2 - x),
        Some(Transfo::MiroirX) => (2 - x, 0),
        Some(Transfo::MiroirZ) => (x, 0),
    };
    BlockPos::new(coin.x + dx, coin.y + y, coin.z + dz)
}

const ORIENTATIONS: [Option<Transfo>; 6] = [
    None,
    Some(Transfo::Rot90),
    Some(Transfo::Rot180),
    Some(Transfo::Rot270),
    Some(Transfo::MiroirX),
    Some(Transfo::MiroirZ),
];

/// La fenêtre de départ, en coordonnées locales (x, y) : une rangée de
/// planches, et au-dessus verre / flèche / verre.
fn fenetre(x: i32, y: i32) -> &'static str {
    match (x, y) {
        (_, 0) => "minecraft:oak_planks",
        (1, 1) => "t:fleche|facing=north",
        _ => "minecraft:glass",
    }
}

fn attendu(cle: &str, t: Option<Transfo>) -> String {
    match t {
        None => cle.to_string(),
        Some(t) => regle(cle, t).unwrap(),
    }
}

const P0: BlockPos = BlockPos { x: 2, y: -60, z: 2 };

/// La clé d'état d'une case d'un contenu, en YZX.
fn cle_en(c: &tf_ops::composant::Contenu, x: u32, y: u32, z: u32) -> &str {
    let [sx, _, sz] = c.taille;
    let i = ((y * sz + z) * sx + x) as usize;
    &c.palette[c.cases[i] as usize]
}

/// Crée un composant d'une case de verre en `P0`, enregistré. Rend (définition,
/// prototype).
fn vitre<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    journal: &mut Journal,
    i: &mut Interner,
) -> (u64, u64) {
    poser_bloc(st, P0, "minecraft:glass", i);
    let (a, _) = faire(st, journal, "Créer", 0, |p| {
        creer(st, &SURFACE, &BBox::single(P0), "vitre", p, i)
    })
    .unwrap();
    (a.definition, a.instance.unwrap())
}

fn poser_enregistre<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    journal: &mut Journal,
    dim: &Dimension,
    def: u64,
    coin: BlockPos,
    t: Option<Transfo>,
    i: &mut Interner,
) -> Action {
    faire(st, journal, "Poser", 0, |p| {
        poser(st, dim, p, def, coin, t, i, Some(&regle))
    })
    .unwrap()
    .0
}

/// **La sortie de la phase 7.** Vingt instances dans les six orientations, une
/// modifiée, toutes suivent ; un Ctrl+Z rend l'ancienne définition PARTOUT —
/// et laisse les vingt poses en place.
#[test]
fn vingt_instances_suivent_leur_definition_et_un_ctrl_z_defait_la_modification() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    for x in 0..3 {
        for y in 0..2 {
            poser_bloc(
                &st,
                BlockPos::new(P0.x + x, P0.y + y, P0.z),
                fenetre(x, y),
                &mut i,
            );
        }
    }
    let sel = BBox::new(P0, BlockPos::new(P0.x + 2, P0.y + 1, P0.z));

    let (a, r) = faire(&st, &mut journal, "Créer « fenêtre »", 0, |p| {
        creer(&st, &SURFACE, &sel, "fenêtre", p, &mut i)
    })
    .unwrap();
    assert!(a.rapport.patches.is_empty(), "créer ne touche pas au monde");
    r.expect("le document a changé : une entrée");
    let (def, prototype) = (a.definition, a.instance.unwrap());

    // Dix-neuf poses de plus, dans les six orientations.
    let mut poses: Vec<Instance> = Vec::new();
    for k in 0..19 {
        let coin = BlockPos::new(10 + (k % 10) * 12, -60, 20 + (k / 10) * 20);
        let t = ORIENTATIONS[k as usize % 6];
        let a = poser_enregistre(&st, &mut journal, &SURFACE, def, coin, t, &mut i);
        assert_eq!(a.intacts, 0);
        poses.push(a.projet.instance(a.instance.unwrap()).unwrap().clone());
    }
    // Chaque instance tient SA boîte, tournée : la case locale (2, 1) d'un
    // quart de tour est deux blocs plus au sud, pas à l'est.
    let (_, projet) = Projet::lire(&st).unwrap();
    for j in &poses {
        assert_eq!(
            projet
                .instance_en(&SURFACE, vers_monde(j.coin, j.transfo, 2, 1))
                .map(|x| x.id),
            Some(j.id),
            "{:?}",
            j.transfo
        );
    }
    for j in &poses {
        for x in 0..3 {
            for y in 0..2 {
                assert_eq!(
                    etat(&st, vers_monde(j.coin, j.transfo, x, y)),
                    attendu(fenetre(x, y), j.transfo),
                    "pose n° {} ({:?}), case ({x}, {y})",
                    j.id,
                    j.transfo
                );
            }
        }
    }
    let temoin: Vec<String> = poses
        .iter()
        .map(|j| etat(&st, BlockPos::new(j.coin.x - 1, j.coin.y, j.coin.z)))
        .collect();

    // On retouche le PROTOTYPE à la main : de l'or au centre du verre de
    // gauche, et la planche de droite retirée.
    poser_bloc(
        &st,
        BlockPos::new(P0.x, P0.y + 1, P0.z),
        "minecraft:gold_block",
        &mut i,
    );
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 2, P0.y, P0.z),
        "minecraft:air",
        &mut i,
    );
    let neuve = |x: i32, y: i32| match (x, y) {
        (0, 1) => "minecraft:gold_block",
        (2, 0) => "minecraft:air",
        _ => fenetre(x, y),
    };

    let (_, projet) = Projet::lire(&st).unwrap();
    let ancienne = projet.definition(def).unwrap().contenu.clone();
    let n = journal.entrees().len();
    let (a, r) = faire(&st, &mut journal, "Mettre à jour « fenêtre »", 0, |p| {
        mettre_a_jour(&st, p, prototype, &mut i, Some(&regle))
    })
    .unwrap();
    assert_eq!(a.reestampees, 19);
    assert_eq!(a.ailleurs, 0);
    assert_eq!((a.gagnees, a.perdues), (0, 1));
    r.unwrap();
    assert_eq!(
        journal.entrees().len(),
        n + 1,
        "UNE entrée pour les dix-neuf"
    );

    let verifier = |def_: &dyn Fn(i32, i32) -> &'static str| {
        for j in &poses {
            for x in 0..3 {
                for y in 0..2 {
                    assert_eq!(
                        etat(&st, vers_monde(j.coin, j.transfo, x, y)),
                        attendu(def_(x, y), j.transfo),
                        "instance n° {} ({:?}), case ({x}, {y})",
                        j.id,
                        j.transfo
                    );
                }
            }
        }
        // Autour, rien n'a bougé.
        for (j, t) in poses.iter().zip(&temoin) {
            assert_eq!(
                &etat(&st, BlockPos::new(j.coin.x - 1, j.coin.y, j.coin.z)),
                t
            );
        }
    };
    verifier(&neuve);
    // Le document porte la nouvelle définition : l'or en (0, 1).
    let (_, relu) = Projet::lire(&st).unwrap();
    assert_eq!(
        cle_en(&relu.definition(def).unwrap().contenu, 0, 1, 0),
        "minecraft:gold_block"
    );

    // **Un Ctrl+Z** : l'ancienne définition, partout — et dans le document.
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    verifier(&fenetre);
    let (_, relu) = Projet::lire(&st).unwrap();
    assert_eq!(relu.definition(def).unwrap().contenu, ancienne);
    assert_eq!(relu.instances.len(), 20, "les vingt poses sont toujours là");
    // Le prototype garde la retouche faite à la main : elle n'était pas dans
    // l'entrée défaite.
    assert_eq!(
        etat(&st, BlockPos::new(P0.x, P0.y + 1, P0.z)),
        "minecraft:gold_block"
    );

    // Et refaire la rend partout.
    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    verifier(&neuve);
}

/// **Une case d'air de la définition est transparente** : le terrain y reste.
/// Une matière perdue devient de l'air, une matière gagnée s'écrit.
#[test]
fn l_air_est_transparent_la_matiere_perdue_devient_de_l_air() {
    let st = monde();
    let mut i = Interner::new();
    // Planche, AIR, planche.
    for (x, b) in [
        (0, "minecraft:oak_planks"),
        (1, "minecraft:air"),
        (2, "minecraft:oak_planks"),
    ] {
        poser_bloc(&st, BlockPos::new(P0.x + x, P0.y, P0.z), b, &mut i);
    }
    let sel = BBox::new(P0, BlockPos::new(P0.x + 2, P0.y, P0.z));
    let a = creer(&st, &SURFACE, &sel, "arche", &Projet::default(), &mut i).unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    let q = BlockPos::new(40, -50, 40);
    let terrain = etat(&st, BlockPos::new(q.x + 1, q.y, q.z));
    let a = poser(&st, &SURFACE, &a.projet, def, q, None, &mut i, None).unwrap();
    assert_eq!(
        etat(&st, BlockPos::new(q.x + 1, q.y, q.z)),
        terrain,
        "l'air de la définition est transparent : le terrain reste"
    );

    // Le milieu devient du verre (gagné), la planche de gauche de l'air
    // (perdue).
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 1, P0.y, P0.z),
        "minecraft:glass",
        &mut i,
    );
    poser_bloc(&st, P0, "minecraft:air", &mut i);
    let m = mettre_a_jour(&st, &a.projet, proto, &mut i, None).unwrap();
    assert_eq!((m.gagnees, m.perdues), (1, 1));
    assert_eq!(etat(&st, q), "minecraft:air", "matière perdue : de l'air");
    assert_eq!(
        etat(&st, BlockPos::new(q.x + 1, q.y, q.z)),
        "minecraft:glass"
    );
    assert_eq!(
        etat(&st, BlockPos::new(q.x + 2, q.y, q.z)),
        "minecraft:oak_planks"
    );
}

/// **Une retouche faite dans une instance survit** là où la définition ne
/// change pas — et cède là où elle change : la définition l'emporte, mais
/// seulement sur ce qu'elle modifie. Vrai aussi dans une instance TOURNÉE,
/// sur un bloc orienté : l'ancienne définition doit y être tournée comme la
/// nouvelle, sinon chaque flèche paraîtrait changée et la retouche partirait.
#[test]
fn une_retouche_survit_hors_de_ce_que_la_definition_change() {
    let st = monde();
    let mut i = Interner::new();
    // Planche, flèche, planche.
    for (x, b) in [
        (0, "minecraft:oak_planks"),
        (1, "t:fleche|facing=north"),
        (2, "minecraft:oak_planks"),
    ] {
        poser_bloc(&st, BlockPos::new(P0.x + x, P0.y, P0.z), b, &mut i);
    }
    let sel = BBox::new(P0, BlockPos::new(P0.x + 2, P0.y, P0.z));
    let a = creer(&st, &SURFACE, &sel, "banc", &Projet::default(), &mut i).unwrap();
    let q = BlockPos::new(40, -60, 40);
    let b = poser(
        &st,
        &SURFACE,
        &a.projet,
        a.definition,
        q,
        None,
        &mut i,
        Some(&regle),
    )
    .unwrap();
    // Un quart de tour : la case locale x tombe en z + x.
    let r = BlockPos::new(60, -60, 40);
    let c = poser(
        &st,
        &SURFACE,
        &b.projet,
        a.definition,
        r,
        Some(Transfo::Rot90),
        &mut i,
        Some(&regle),
    )
    .unwrap();
    let dans_r = |x: i32| BlockPos::new(r.x, r.y, r.z + x);
    assert_eq!(etat(&st, dans_r(1)), "t:fleche|facing=east");

    // Retouches : la case 0 de `q` et la flèche de `r` (que la définition ne
    // changera pas), la case 2 de `q` (qu'elle changera).
    poser_bloc(&st, q, "minecraft:glass", &mut i);
    poser_bloc(&st, dans_r(1), "minecraft:glass", &mut i);
    poser_bloc(
        &st,
        BlockPos::new(q.x + 2, q.y, q.z),
        "minecraft:stone",
        &mut i,
    );
    // La définition change sa case 2.
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 2, P0.y, P0.z),
        "minecraft:gold_block",
        &mut i,
    );
    let m = mettre_a_jour(&st, &c.projet, a.instance.unwrap(), &mut i, Some(&regle)).unwrap();
    assert_eq!(m.reestampees, 2);
    assert_eq!(
        etat(&st, q),
        "minecraft:glass",
        "hors du changement : survit"
    );
    assert_eq!(
        etat(&st, BlockPos::new(q.x + 1, q.y, q.z)),
        "t:fleche|facing=north"
    );
    assert_eq!(
        etat(&st, BlockPos::new(q.x + 2, q.y, q.z)),
        "minecraft:gold_block",
        "là où la définition change, elle l'emporte"
    );
    assert_eq!(etat(&st, dans_r(0)), "minecraft:oak_planks");
    assert_eq!(
        etat(&st, dans_r(1)),
        "minecraft:glass",
        "une flèche non changée, mais tournée, a été réécrite"
    );
    assert_eq!(etat(&st, dans_r(2)), "minecraft:gold_block");
}

/// **Les variantes de l'air se valent.** Un composant pris là où le jeu a mis
/// de l'air de caverne, mis à jour depuis une instance posée en plein ciel :
/// rien n'a changé, et rien ne s'écrit.
#[test]
fn les_variantes_de_l_air_se_valent() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 1, P0.y, P0.z),
        "minecraft:cave_air",
        &mut i,
    );
    let sel = BBox::new(P0, BlockPos::new(P0.x + 1, P0.y, P0.z));
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "vitre", p, &mut i)
    })
    .unwrap();
    let haut = BlockPos::new(30, 100, 30);
    assert_eq!(
        etat(&st, BlockPos::new(haut.x + 1, haut.y, haut.z)),
        "minecraft:air"
    );
    let b = poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        a.definition,
        haut,
        None,
        &mut i,
    );
    let (m, r) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, b.instance.unwrap(), &mut i, None)
    })
    .unwrap();
    assert!(m.rapport.patches.is_empty());
    assert_eq!(m.projet, b.projet);
    assert!(
        r.is_none(),
        "l'air de caverne et l'air ont fait une définition changée"
    );
}

/// **Les états qu'une rotation ne sait pas tourner sont COMPTÉS** — à la pose
/// comme à la mise à jour : un build à moitié tourné se dit. La case d'air
/// transparente n'en est pas un.
#[test]
fn les_etats_qu_une_rotation_ne_sait_pas_tourner_sont_comptes() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    for (x, b) in [
        (0, "t:mystere|axe=x"),
        (1, "minecraft:air"),
        (2, "minecraft:glass"),
    ] {
        poser_bloc(&st, BlockPos::new(P0.x + x, P0.y, P0.z), b, &mut i);
    }
    let sel = BBox::new(P0, BlockPos::new(P0.x + 2, P0.y, P0.z));
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "mystère", p, &mut i)
    })
    .unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    // La règle ne connaît pas `t:mystere` : un état laissé tel quel.
    let b = poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        def,
        BlockPos::new(30, -60, 30),
        Some(Transfo::Rot90),
        &mut i,
    );
    assert_eq!(b.intacts, 1);
    // Sans règle du tout, tout ce qui porte de la matière reste tel quel —
    // le verre et le mystère, pas l'air transparent.
    let (c, _) = faire(&st, &mut journal, "Poser", 0, |p| {
        poser(
            &st,
            &SURFACE,
            p,
            def,
            BlockPos::new(40, -60, 30),
            Some(Transfo::Rot90),
            &mut i,
            None,
        )
    })
    .unwrap();
    assert_eq!(c.intacts, 2);
    // Détachée : elle ne compte plus dans les mises à jour qui suivent.
    faire(&st, &mut journal, "Détacher", 0, |p| {
        detacher(p, c.instance.unwrap())
    })
    .unwrap();

    // Une mise à jour qui réestampe l'instance tournée compte ce qu'elle n'a
    // pas su tourner — une fois, même vu dans plusieurs orientations.
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 2, P0.y, P0.z),
        "minecraft:gold_block",
        &mut i,
    );
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, Some(&regle))
    })
    .unwrap();
    assert_eq!(m.intacts, 1);
    // Depuis l'instance TOURNÉE, le retour dans le repère de la définition
    // compte ce qu'il ne sait pas détourner — et c'est ici la SEULE source :
    // l'autre instance, le prototype, n'est pas tournée.
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, b.instance.unwrap(), &mut i, Some(&regle))
    })
    .unwrap();
    assert_eq!(m.reestampees, 1);
    assert_eq!(m.intacts, 1);
}

/// **Renommer ne change que l'étiquette**, s'annule, et un nom inchangé ne
/// remplit pas le journal. Les espaces autour d'un nom ne comptent pas.
#[test]
fn renommer_ne_change_que_l_etiquette() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &BBox::single(P0), "  vitre  ", p, &mut i)
    })
    .unwrap();
    let def = a.definition;
    assert_eq!(a.projet.definition(def).unwrap().nom, "vitre");
    let (b, r) = faire(&st, &mut journal, "Renommer", 0, |p| {
        renommer(p, def, " hublot ")
    })
    .unwrap();
    assert!(r.is_some());
    assert!(b.rapport.patches.is_empty());
    assert_eq!(
        Projet::lire(&st).unwrap().1.definition(def).unwrap().nom,
        "hublot"
    );
    let (_, r) = faire(&st, &mut journal, "Renommer", 0, |p| {
        renommer(p, def, "hublot")
    })
    .unwrap();
    assert!(r.is_none(), "un nom inchangé a rempli le journal");
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(
        Projet::lire(&st).unwrap().1.definition(def).unwrap().nom,
        "vitre"
    );
    assert!(matches!(
        renommer(&a.projet, 999, "x"),
        Err(ErreurComposant::DefinitionInconnue(999))
    ));
}

/// **Une mise à jour ne relit que ce qui change.** Un mât de quarante blocs
/// traverse trois sections ; n'en changer que le sommet ne doit en visiter
/// qu'UNE sous chaque instance. Un compteur, pas un chronomètre : relire la
/// boîte entière donnerait le même monde, trois fois plus lentement, et
/// aucun autre test ne le verrait.
#[test]
fn une_mise_a_jour_ne_relit_que_ce_qui_change() {
    let st = monde();
    let mut i = Interner::new();
    let sel = BBox::new(
        BlockPos::new(P0.x, -60, P0.z),
        BlockPos::new(P0.x, -21, P0.z),
    );
    let bois = i.intern("minecraft:oak_log");
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(bois));
    appliquer(&st, &SURFACE, Folder::Region, &sel, &plan, &i).unwrap();
    let a = creer(&st, &SURFACE, &sel, "mât", &Projet::default(), &mut i).unwrap();
    let mut p = a.projet;
    for k in 0..3 {
        let coin = BlockPos::new(40 + 4 * k, -60, 40);
        p = poser(&st, &SURFACE, &p, a.definition, coin, None, &mut i, None)
            .unwrap()
            .projet;
    }
    poser_bloc(&st, sel.max, "minecraft:gold_block", &mut i);
    let m = mettre_a_jour(&st, &p, a.instance.unwrap(), &mut i, None).unwrap();
    assert_eq!(m.reestampees, 3);
    assert_eq!(
        m.rapport.etages.iter().sum::<usize>(),
        3,
        "sections visitées : {:?}",
        m.rapport.etages
    );
    assert_eq!(
        etat(&st, BlockPos::new(44, -21, 40)),
        "minecraft:gold_block"
    );
}

// ── les coffres ─────────────────────────────────────────────────────────────

/// Ces octets figurent-ils dans ceux-là ?
fn contient(foin: &[u8], aiguille: &[u8]) -> bool {
    foin.windows(aiguille.len()).any(|w| w == aiguille)
}

/// Le coffre de la fixture du chunk (cx, 0) : sa case, et de quoi le
/// reconnaître.
fn coffre(cx: i32) -> (BlockPos, String) {
    let [x, y, z] = Terrain::case_coffre(cx, 0, 0);
    (BlockPos::new(x, y, z), format!("Coffre {cx}/0#0"))
}

/// Ce que porte le coffre de cette case.
fn contenu_du_coffre<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    p: BlockPos,
) -> Vec<u8> {
    let mut i = Interner::new();
    let pr = copier(st, &SURFACE, Folder::Region, &BBox::single(p), &mut i).unwrap();
    assert_eq!(pr.entites.len(), 1, "un coffre en {p:?}");
    pr.entites[0].nbt.clone()
}

/// **Remplit** le coffre de `case` avec le contenu de celui de `depuis`, sans
/// changer l'état de la case — ce que fait un joueur qui range des objets.
fn remplir<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    case: BlockPos,
    depuis: BlockPos,
    i: &mut Interner,
) {
    let etat = copier(st, &SURFACE, Folder::Region, &BBox::single(case), i)
        .unwrap()
        .blocs[0];
    let mut p = copier(st, &SURFACE, Folder::Region, &BBox::single(depuis), i).unwrap();
    p.blocs[0] = etat;
    let air = i.intern("minecraft:air");
    let pas = Pas {
        d: [0, 0, 0],
        avec_air: false,
        air,
        compter: false,
    };
    coller(st, &SURFACE, Folder::Region, &p, case, pas, i).unwrap();
}

fn monde_peuple() -> St {
    let src = MemorySource::new();
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&Terrain::peuplee(1)),
    );
    Staging::new(src, MemorySource::new())
}

/// **Un coffre rempli dans une instance garde son contenu** quand la
/// définition change AILLEURS — la réestamper en entier l'aurait vidé dans
/// les vingt maisons. Et quand c'est le contenu du coffre de la définition
/// qui change, lui, il suit.
#[test]
fn un_coffre_rempli_dans_une_instance_garde_son_contenu() {
    let st = monde_peuple();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    // Le composant : le coffre du chunk (0, 0), et du verre au-dessus.
    let (c0, nom0) = coffre(0);
    let haut = BlockPos::new(c0.x, c0.y + 1, c0.z);
    poser_bloc(&st, haut, "minecraft:glass", &mut i);
    let sel = BBox::new(c0, haut);
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "cellier", p, &mut i)
    })
    .unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    assert_eq!(a.projet.definition(def).unwrap().contenu.entites.len(), 1);

    // Une instance, loin des coffres de la fixture.
    let q = BlockPos::new(5, c0.y, 40);
    poser_enregistre(&st, &mut journal, &SURFACE, def, q, None, &mut i);
    assert!(contient(&contenu_du_coffre(&st, q), nom0.as_bytes()));

    // Un joueur y range autre chose.
    let (c2, nom2) = coffre(2);
    remplir(&st, q, c2, &mut i);
    assert!(contient(&contenu_du_coffre(&st, q), nom2.as_bytes()));

    // La définition change AILLEURS que le coffre : le contenu rangé reste.
    poser_bloc(&st, haut, "minecraft:gold_block", &mut i);
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert_eq!(m.reestampees, 1);
    assert_eq!(
        etat(&st, BlockPos::new(q.x, q.y + 1, q.z)),
        "minecraft:gold_block"
    );
    assert!(
        contient(&contenu_du_coffre(&st, q), nom2.as_bytes()),
        "le coffre rangé a été vidé par une mise à jour qui ne le touchait pas"
    );

    // Le COFFRE de la définition change : l'instance suit.
    let (c3, nom3) = coffre(3);
    remplir(&st, c0, c3, &mut i);
    faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert!(contient(&contenu_du_coffre(&st, q), nom3.as_bytes()));
}

/// **Deux copies du même composant sont le même contenu**, d'où qu'on les
/// prenne et quelle que soit l'orientation de l'instance : mettre à jour
/// depuis une instance tournée qu'on n'a pas touchée n'écrit rien.
///
/// Deux pièges, un test : les octets d'une block entity portent les
/// coordonnées MONDE d'où on l'a prise, et l'ordre du ramassage suit le
/// découpage en chunks. L'un ou l'autre, et la maison se croit changée — ses
/// vingt instances réécrites pour rien.
#[test]
fn une_instance_tournee_intacte_ne_change_pas_sa_definition() {
    let st = monde_peuple();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    // La boîte de (1, −30, 2) à (17, −30, 18) : les coffres de fixture de
    // quatre chunks, à ses quatre coins.
    let [ax, ay, az] = Terrain::case_coffre(0, 0, 0);
    let [bx, by, bz] = Terrain::case_coffre(1, 1, 0);
    let sel = BBox::new(BlockPos::new(ax, ay, az), BlockPos::new(bx, by, bz));
    let (c, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "cour", p, &mut i)
    })
    .unwrap();
    assert_eq!(
        c.projet
            .definition(c.definition)
            .unwrap()
            .contenu
            .entites
            .len(),
        4
    );
    // Un quart de tour à l'envers, à une hauteur sans coffre de fixture.
    let q = BlockPos::new(40, -40, 40);
    let posee = poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        c.definition,
        q,
        Some(Transfo::Rot270),
        &mut i,
    );
    let n = journal.entrees().len();
    let (m, r) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, posee.instance.unwrap(), &mut i, Some(&regle))
    })
    .unwrap();
    assert!(m.rapport.patches.is_empty(), "réécrit pour rien");
    assert_eq!(m.projet, posee.projet);
    assert!(r.is_none());
    assert_eq!(journal.entrees().len(), n, "une entrée qui ne défait rien");
}

/// Pose en `case` un bloc d'AIR qui porte le coffre de `depuis` — ce qu'un
/// monde abîmé ou moddé peut contenir, et que rien ne doit transformer en
/// panique ni en coffre fantôme.
fn coffre_dans_l_air<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    case: BlockPos,
    depuis: BlockPos,
    i: &mut Interner,
) {
    let mut p = copier(st, &SURFACE, Folder::Region, &BBox::single(depuis), i).unwrap();
    let air = i.intern("minecraft:air");
    p.blocs[0] = air;
    let pas = Pas {
        d: [0, 0, 0],
        avec_air: true,
        air,
        compter: false,
    };
    coller(st, &SURFACE, Folder::Region, &p, case, pas, i).unwrap();
}

/// Les block entities d'une case.
fn entites_en<S: RegionSource, O: RegionSource + RegionSink>(
    st: &Staging<S, O>,
    p: BlockPos,
) -> usize {
    let mut i = Interner::new();
    copier(st, &SURFACE, Folder::Region, &BBox::single(p), &mut i)
        .unwrap()
        .entites
        .len()
}

/// **Une block entity sur une case d'air** ne se pose nulle part : ni quand
/// seule elle change — la mise à jour n'a alors RIEN à écrire, ce qui ne doit
/// pas paniquer —, ni quand sa case perd sa matière : l'air qu'on y écrit ne
/// porte pas de coffre fantôme.
#[test]
fn une_block_entity_sur_de_l_air_ne_se_pose_pas() {
    let st = monde_peuple();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    // Verre, puis pierre.
    let p1 = BlockPos::new(P0.x + 1, P0.y, P0.z);
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    poser_bloc(&st, p1, "minecraft:stone", &mut i);
    let sel = BBox::new(P0, p1);
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "socle", p, &mut i)
    })
    .unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    let q = BlockPos::new(40, -60, 40);
    poser_enregistre(&st, &mut journal, &SURFACE, def, q, None, &mut i);

    // La pierre du prototype devient de l'air qui porte un coffre : la
    // matière est perdue, l'instance reçoit de l'AIR — sans coffre.
    let (c2, _) = coffre(2);
    coffre_dans_l_air(&st, p1, c2, &mut i);
    assert_eq!(entites_en(&st, p1), 1);
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert_eq!(m.perdues, 1);
    let q1 = BlockPos::new(q.x + 1, q.y, q.z);
    assert_eq!(etat(&st, q1), "minecraft:air");
    assert_eq!(
        entites_en(&st, q1),
        0,
        "un coffre fantôme posé sur de l'air"
    );

    // Seul le coffre posé sur l'air change : rien à écrire, et surtout pas
    // de panique.
    let (c3, _) = coffre(3);
    coffre_dans_l_air(&st, p1, c3, &mut i);
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert_eq!(m.reestampees, 0);
    assert!(m.rapport.patches.is_empty());
}

// ── partout, et tout ou rien ────────────────────────────────────────────────

/// **« Partout » veut dire toutes les dimensions.** Une instance du Nether
/// laissée à l'ancienne définition ne la porterait plus, et la mise à jour
/// suivante calculerait ses pertes contre une définition qu'elle n'a jamais
/// eue.
#[test]
fn une_mise_a_jour_reestampe_aussi_les_autres_dimensions() {
    let src = MemorySource::new();
    for d in [SURFACE, Dimension::Nether] {
        src.put_region(
            d,
            Folder::Region,
            RegionPos { x: 0, z: 0 },
            region(&Terrain::petite()),
        );
    }
    let st: St = Staging::new(src, MemorySource::new());
    let mut i = Interner::new();
    let mut journal = Journal::new();
    let (def, proto) = vitre(&st, &mut journal, &mut i);
    let n = BlockPos::new(40, -60, 40);
    poser_enregistre(&st, &mut journal, &Dimension::Nether, def, n, None, &mut i);
    let o = BlockPos::new(30, -60, 30);
    poser_enregistre(&st, &mut journal, &SURFACE, def, o, None, &mut i);

    poser_bloc(&st, P0, "minecraft:gold_block", &mut i);
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert_eq!((m.reestampees, m.ailleurs), (2, 1));
    assert_eq!(etat_en(&st, &Dimension::Nether, n), "minecraft:gold_block");
    assert_eq!(etat(&st, o), "minecraft:gold_block");

    // La case d'une instance n'appartient qu'à SA dimension.
    let du_nether = m.projet.instance_en(&Dimension::Nether, n).map(|x| x.id);
    assert!(du_nether.is_some());
    assert_eq!(m.projet.instance_en(&SURFACE, n), None);

    // Et UN Ctrl+Z défait les deux dimensions.
    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(etat_en(&st, &Dimension::Nether, n), "minecraft:glass");
    assert_eq!(etat(&st, o), "minecraft:glass");

    // Mettre à jour DEPUIS le Nether : c'est là qu'on lit la définition.
    poser_bloc_en(
        &st,
        &Dimension::Nether,
        n,
        "minecraft:diamond_block",
        &mut i,
    );
    let (m, _) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, du_nether.unwrap(), &mut i, None)
    })
    .unwrap();
    assert_eq!((m.reestampees, m.ailleurs), (2, 2));
    assert_eq!(etat(&st, o), "minecraft:diamond_block");
    assert_eq!(etat(&st, P0), "minecraft:diamond_block");
}

/// Une instance qui tomberait dans du terrain jamais généré est refusée AVANT
/// d'écrire : le document dirait sinon une instance que le monde ne porte
/// qu'à moitié.
#[test]
fn une_instance_hors_du_terrain_est_refusee_avant_d_ecrire() {
    let st = monde();
    let mut i = Interner::new();
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    let a = creer(
        &st,
        &SURFACE,
        &BBox::single(P0),
        "vitre",
        &Projet::default(),
        &mut i,
    )
    .unwrap();
    let ecrites_avant = st.touched();
    let loin = BlockPos::new(5000, -60, 5000);
    let r = poser(
        &st,
        &SURFACE,
        &a.projet,
        a.definition,
        loin,
        None,
        &mut i,
        None,
    );
    assert!(
        matches!(r, Err(ErreurComposant::HorsTerrain { instance: None, .. })),
        "{r:?}"
    );
    assert_eq!(st.touched(), ecrites_avant, "rien d'écrit");
}

/// **Une mise à jour vérifie le terrain sous TOUTES les instances avant la
/// première écriture.** Une instance posée au bord du monde généré n'y a
/// écrit que sa matière ; que la définition en gagne là où le terrain manque,
/// et la mise à jour est refusée — sans avoir touché aux instances d'avant.
#[test]
fn une_mise_a_jour_sous_du_terrain_absent_est_refusee_avant_d_ecrire() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    // Verre, puis air (transparent).
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 1, P0.y, P0.z),
        "minecraft:air",
        &mut i,
    );
    let sel = BBox::new(P0, BlockPos::new(P0.x + 1, P0.y, P0.z));
    let (a, _) = faire(&st, &mut journal, "Créer", 0, |p| {
        creer(&st, &SURFACE, &sel, "demi", p, &mut i)
    })
    .unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    // `c`, en plein terrain, PUIS `b` au bord : sa case d'air tombe en
    // x = 256, dans un chunk jamais généré.
    let c = BlockPos::new(100, -60, 2);
    poser_enregistre(&st, &mut journal, &SURFACE, def, c, None, &mut i);
    let b = BlockPos::new(255, -60, 2);
    let posee = poser_enregistre(&st, &mut journal, &SURFACE, def, b, None, &mut i);
    let sous_c = etat(&st, BlockPos::new(c.x + 1, c.y, c.z));

    // La définition gagne de la matière dans sa case d'air.
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 1, P0.y, P0.z),
        "minecraft:gold_block",
        &mut i,
    );
    let (_, avant) = Projet::lire(&st).unwrap();
    let n = journal.entrees().len();
    let r = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    });
    match r {
        Err(ErreurComposant::HorsTerrain {
            instance: Some(id), ..
        }) => assert_eq!(Some(id), posee.instance),
        autre => panic!("{autre:?}"),
    }
    assert_eq!(
        etat(&st, BlockPos::new(c.x + 1, c.y, c.z)),
        sous_c,
        "l'instance d'avant a été écrite"
    );
    assert_eq!(Projet::lire(&st).unwrap().1, avant);
    assert_eq!(journal.entrees().len(), n);
}

/// Une copie de travail qui REFUSE d'écrire là où on le lui dit : de quoi
/// faire échouer une action en route, à l'endroit voulu.
#[derive(Default)]
struct Capricieuse {
    m: MemorySource,
    refus: Mutex<Refus>,
}

#[derive(Default, Clone)]
struct Refus {
    /// Les écritures de cette région échouent.
    region: Option<RegionPos>,
    /// Au-delà de ce nombre d'écritures de région, toutes échouent.
    budget: Option<usize>,
    /// Ce fichier du monde ne s'écrit pas.
    meta: Option<String>,
}

impl Capricieuse {
    fn refuser(&self, r: Refus) {
        *self.refus.lock().unwrap() = r;
    }
}

impl RegionSource for Capricieuse {
    fn dimensions(&self) -> Result<Vec<Dimension>, SourceError> {
        self.m.dimensions()
    }
    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview, SourceError> {
        self.m.overview(dim, folder)
    }
    fn read_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
    ) -> Result<Vec<u8>, SourceError> {
        self.m.read_region(dim, folder, pos)
    }
    fn read_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
    ) -> Result<Vec<u8>, SourceError> {
        self.m.read_external(dim, folder, name)
    }
    fn external_names(&self, dim: &Dimension, folder: Folder) -> Result<Vec<String>, SourceError> {
        self.m.external_names(dim, folder)
    }
    fn read_meta(&self, nom: &str) -> Result<Vec<u8>, SourceError> {
        self.m.read_meta(nom)
    }
    fn meta_names(&self) -> Result<Vec<String>, SourceError> {
        self.m.meta_names()
    }
}

impl RegionSink for Capricieuse {
    fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<(), SourceError> {
        {
            let mut r = self.refus.lock().unwrap();
            if r.region == Some(pos) {
                return Err(SourceError::Io("disque plein (région)".into()));
            }
            if let Some(b) = r.budget.as_mut() {
                if *b == 0 {
                    return Err(SourceError::Io("disque plein (budget)".into()));
                }
                *b -= 1;
            }
        }
        self.m.write_region(dim, folder, pos, bytes)
    }
    fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), SourceError> {
        self.m.write_external(dim, folder, name, bytes)
    }
    fn remove_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
    ) -> Result<(), SourceError> {
        self.m.remove_external(dim, folder, name)
    }
    fn remove_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
    ) -> Result<(), SourceError> {
        self.m.remove_region(dim, folder, pos)
    }
    fn write_meta(&self, nom: &str, bytes: &[u8]) -> Result<(), SourceError> {
        if self.refus.lock().unwrap().meta.as_deref() == Some(nom) {
            return Err(SourceError::Io("disque plein (fichier)".into()));
        }
        self.m.write_meta(nom, bytes)
    }
    fn remove_meta(&self, nom: &str) -> Result<(), SourceError> {
        self.m.remove_meta(nom)
    }
}

type Fragile = Staging<MemorySource, Capricieuse>;

/// Deux régions : (0, 0) et (1, 0).
fn monde_fragile() -> Fragile {
    let src = MemorySource::new();
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&Terrain::petite()),
    );
    src.put_region(
        SURFACE,
        Folder::Region,
        RegionPos { x: 1, z: 0 },
        region_en(&Terrain::petite(), 1, 0),
    );
    Staging::new(src, Capricieuse::default())
}

/// **Une mise à jour qui échoue en route DÉFAIT ce qu'elle a écrit.** Sans
/// ça, l'instance réestampée avant l'échec restait dans la copie de travail
/// sans entrée de journal : aucun Ctrl+Z ne l'atteignait, et celui de sa pose
/// DIVERGEAIT — on ne pouvait plus rien annuler.
#[test]
fn une_mise_a_jour_qui_echoue_en_route_defait_ce_qu_elle_a_ecrit() {
    let st = monde_fragile();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    let (def, proto) = vitre(&st, &mut journal, &mut i);
    // `c` et `c2` dans le MÊME chunk : défaire doit remonter leurs deux
    // correctifs à l'envers, sinon le second ne s'applique plus.
    let c = BlockPos::new(30, -60, 30);
    let c2 = BlockPos::new(28, -60, 30);
    let terrain_c = etat(&st, c);
    let terrain_c2 = etat(&st, c2);
    poser_enregistre(&st, &mut journal, &SURFACE, def, c, None, &mut i);
    poser_enregistre(&st, &mut journal, &SURFACE, def, c2, None, &mut i);
    // `b` dans la région (1, 0), qui refusera d'être écrite.
    let b = BlockPos::new(600, -60, 30);
    let terrain_b = etat(&st, b);
    poser_enregistre(&st, &mut journal, &SURFACE, def, b, None, &mut i);

    poser_bloc(&st, P0, "minecraft:gold_block", &mut i);
    let (_, avant) = Projet::lire(&st).unwrap();
    let n = journal.entrees().len();
    st.overlay().refuser(Refus {
        region: Some(RegionPos { x: 1, z: 0 }),
        ..Default::default()
    });
    let r = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    });
    assert!(matches!(r, Err(ErreurComposant::Edition(_))), "{r:?}");
    for p in [c, c2] {
        assert_eq!(
            etat(&st, p),
            "minecraft:glass",
            "une instance écrite avant l'échec est restée"
        );
    }
    assert_eq!(Projet::lire(&st).unwrap().1, avant);
    assert_eq!(journal.entrees().len(), n);

    // La preuve que tout est rentré dans l'ordre : les trois poses se
    // défont.
    st.overlay().refuser(Refus::default());
    for _ in 0..3 {
        let (e, _) = journal.annuler().unwrap();
        rejouer(&st, e, Sens::Annuler).expect("une pose ne se défait plus");
    }
    assert_eq!(etat(&st, c), terrain_c);
    assert_eq!(etat(&st, c2), terrain_c2);
    assert_eq!(etat(&st, b), terrain_b);
}

/// Et si même DÉFAIRE échoue, l'erreur le dit — le seul cas où la copie de
/// travail garde une action à moitié faite.
#[test]
fn un_echec_qu_on_ne_peut_pas_defaire_se_dit() {
    let st = monde_fragile();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    let (def, proto) = vitre(&st, &mut journal, &mut i);
    poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        def,
        BlockPos::new(30, -60, 30),
        None,
        &mut i,
    );
    poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        def,
        BlockPos::new(600, -60, 30),
        None,
        &mut i,
    );
    poser_bloc(&st, P0, "minecraft:gold_block", &mut i);
    // Une seule écriture de région passe : la première instance.
    st.overlay().refuser(Refus {
        budget: Some(1),
        ..Default::default()
    });
    let r = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    });
    match r {
        Err(e @ ErreurComposant::AMoitie { .. }) => {
            assert!(e.to_string().contains("n'a pas pu être défait"), "{e}")
        }
        autre => panic!("{autre:?}"),
    }
}

/// **Un document qui ne s'écrit pas défait les blocs.** Une instance posée que
/// le document ne connaîtrait pas ne suivrait plus jamais sa définition — et
/// ses blocs seraient hors de tout journal.
#[test]
fn un_document_qui_ne_s_ecrit_pas_defait_les_blocs() {
    let st = monde_fragile();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    let (def, _) = vitre(&st, &mut journal, &mut i);
    let q = BlockPos::new(30, -60, 30);
    let terrain = etat(&st, q);
    let (_, avant) = Projet::lire(&st).unwrap();
    let n = journal.entrees().len();
    st.overlay().refuser(Refus {
        meta: Some("projet".into()),
        ..Default::default()
    });
    let r = faire(&st, &mut journal, "Poser", 0, |p| {
        poser(&st, &SURFACE, p, def, q, None, &mut i, None)
    });
    assert!(matches!(r, Err(ErreurComposant::Edition(_))), "{r:?}");
    assert_eq!(etat(&st, q), terrain, "les blocs de la pose sont restés");
    assert_eq!(Projet::lire(&st).unwrap().1, avant);
    assert_eq!(journal.entrees().len(), n);
}

// ── les refus ───────────────────────────────────────────────────────────────

/// Un composant fait d'air n'estamperait rien : refusé, en le disant.
#[test]
fn un_composant_d_air_est_refuse() {
    let st = monde();
    let mut i = Interner::new();
    poser_bloc(&st, P0, "minecraft:air", &mut i);
    let r = creer(
        &st,
        &SURFACE,
        &BBox::single(P0),
        "rien",
        &Projet::default(),
        &mut i,
    );
    assert!(matches!(r, Err(ErreurComposant::Vide)), "{r:?}");
}

/// **Un composant que le document ne relirait pas est refusé à la création**
/// — sinon le premier geste suivant écrirait un document illisible, et TOUS
/// les composants du monde deviendraient inaccessibles. Refusé avant de
/// copier : la copie d'une sélection démesurée coûterait pour rien.
#[test]
fn un_composant_trop_grand_est_refuse_a_la_creation() {
    let st = monde();
    let mut i = Interner::new();
    // 97 × 257 × 673 = 2²⁴ + 1 : le plafond, plus UNE case.
    let sel = BBox::new(P0, BlockPos::new(P0.x + 96, P0.y + 256, P0.z + 672));
    let (sx, sy, sz) = sel.size();
    assert_eq!(sx as u64 * sy as u64 * sz as u64, MAX_CASES as u64 + 1);
    let r = creer(&st, &SURFACE, &sel, "tout", &Projet::default(), &mut i);
    assert!(matches!(r, Err(ErreurComposant::TropGrand { .. })), "{r:?}");
    // Au plafond exactement, on copie.
    let sel = BBox::new(P0, BlockPos::new(P0.x + 255, P0.y + 255, P0.z + 255));
    let r = creer(&st, &SURFACE, &sel, "tout", &Projet::default(), &mut i);
    assert!(r.is_ok(), "{r:?}");
}

/// Mettre à jour depuis une instance qu'on n'a pas touchée ne change rien —
/// et ne remplit pas le journal d'une entrée qui ne défait rien.
#[test]
fn une_mise_a_jour_sans_changement_n_ecrit_rien() {
    let st = monde();
    let mut i = Interner::new();
    let mut journal = Journal::new();
    let (def, proto) = vitre(&st, &mut journal, &mut i);
    let b = poser_enregistre(
        &st,
        &mut journal,
        &SURFACE,
        def,
        BlockPos::new(30, -60, 30),
        None,
        &mut i,
    );
    let n = journal.entrees().len();
    let (m, r) = faire(&st, &mut journal, "Mettre à jour", 0, |p| {
        mettre_a_jour(&st, p, proto, &mut i, None)
    })
    .unwrap();
    assert!(m.rapport.patches.is_empty());
    assert_eq!(m.projet, b.projet);
    assert!(r.is_none());
    assert_eq!(journal.entrees().len(), n);
}

/// **L'identité est stable.** Un identifiant n'est jamais redonné, même après
/// un détachement ; une instance détachée ne suit plus sa définition ; et la
/// case d'un chevauchement appartient à la plus récente.
#[test]
fn l_identite_est_stable_et_une_instance_detachee_ne_suit_plus() {
    let st = monde();
    let mut i = Interner::new();
    poser_bloc(&st, P0, "minecraft:glass", &mut i);
    let a = creer(
        &st,
        &SURFACE,
        &BBox::single(P0),
        "vitre",
        &Projet::default(),
        &mut i,
    )
    .unwrap();
    let (def, proto) = (a.definition, a.instance.unwrap());
    let q = BlockPos::new(30, -60, 30);
    let b = poser(&st, &SURFACE, &a.projet, def, q, None, &mut i, None).unwrap();
    let posee = b.instance.unwrap();
    let c = detacher(&b.projet, posee).unwrap();
    assert!(c.projet.instance(posee).is_none());
    let d = poser(
        &st,
        &SURFACE,
        &c.projet,
        def,
        BlockPos::new(31, -60, 30),
        None,
        &mut i,
        None,
    )
    .unwrap();
    let neuve = d.instance.unwrap();
    assert!(neuve > posee, "{neuve} ≤ {posee} : un identifiant redonné");

    // Détachée, elle ne suit plus.
    poser_bloc(&st, P0, "minecraft:gold_block", &mut i);
    let m = mettre_a_jour(&st, &d.projet, proto, &mut i, None).unwrap();
    assert_eq!(m.reestampees, 1);
    assert_eq!(
        etat(&st, q),
        "minecraft:glass",
        "la détachée garde ses blocs"
    );
    assert_eq!(
        etat(&st, BlockPos::new(31, -60, 30)),
        "minecraft:gold_block"
    );

    // Deux instances sur la même case : la plus récente la tient.
    let e = poser(
        &st,
        &SURFACE,
        &m.projet,
        def,
        BlockPos::new(31, -60, 30),
        None,
        &mut i,
        None,
    )
    .unwrap();
    assert_eq!(
        e.projet
            .instance_en(&SURFACE, BlockPos::new(31, -60, 30))
            .map(|x| x.id),
        e.instance
    );
    assert_eq!(e.projet.instance_en(&SURFACE, BlockPos::new(0, 0, 0)), None);
}

/// **Le document se relit à l'identique, et refuse ce qu'il ne comprend pas**
/// — jamais un document vide à la place d'un illisible : le geste suivant
/// l'écraserait, et les définitions partiraient avec.
#[test]
fn le_document_se_relit_a_l_identique_et_refuse_ce_qu_il_ne_comprend_pas() {
    let st = monde();
    let mut i = Interner::new();
    poser_bloc(&st, P0, "t:fleche|facing=north", &mut i);
    poser_bloc(
        &st,
        BlockPos::new(P0.x + 1, P0.y, P0.z),
        "minecraft:glass",
        &mut i,
    );
    let sel = BBox::new(P0, BlockPos::new(P0.x + 1, P0.y, P0.z));
    let mut p = creer(
        &st,
        &SURFACE,
        &sel,
        "flèche — « é »",
        &Projet::default(),
        &mut i,
    )
    .unwrap()
    .projet;
    for (k, t) in ORIENTATIONS.iter().enumerate() {
        p = poser(
            &st,
            &SURFACE,
            &p,
            1,
            BlockPos::new(10 + 4 * k as i32, -60, 10),
            *t,
            &mut i,
            Some(&regle),
        )
        .unwrap()
        .projet;
    }
    p.instances.push(Instance {
        id: p.prochain,
        definition: 1,
        dim: Dimension::Custom {
            namespace: "minefield".into(),
            path: "donjon".into(),
        },
        coin: BlockPos::new(-7, 300, -9),
        transfo: Some(Transfo::MiroirZ),
    });
    p.prochain += 1;
    // Toutes les dimensions de base, qui ont chacune leur nom dans le format.
    for dim in [Dimension::Nether, Dimension::End] {
        p.instances.push(Instance {
            id: p.prochain,
            definition: 1,
            dim,
            coin: BlockPos::new(1, 2, 3),
            transfo: None,
        });
        p.prochain += 1;
    }
    // Une block entity, ses octets et ses trois décalages.
    p.definitions[0].contenu.entites.push(Entite {
        case: [1, 0, 0],
        nbt: (0u8..13).collect(),
        champs: [0, 4, usize::MAX],
    });
    let octets = p.encoder();
    assert_eq!(Projet::decoder(&octets).unwrap(), p);
    assert_eq!(Projet::decoder(&[]).unwrap(), Projet::default());
    assert!(Projet::default().encoder().is_empty());

    for n in 1..octets.len() {
        assert!(
            Projet::decoder(&octets[..n]).is_err(),
            "tronqué à {n} octets, et lu quand même"
        );
    }
    let mut autre = octets.clone();
    autre[0] = b'X';
    assert!(Projet::decoder(&autre).is_err());
    // La plus petite version plus récente.
    let mut futur = octets.clone();
    futur[4] = 2;
    assert!(Projet::decoder(&futur)
        .unwrap_err()
        .contains("plus récente"));
    // Un octet de trop : refusé, pas ignoré.
    let mut long = octets.clone();
    long.push(0);
    assert!(Projet::decoder(&long).is_err());
    // Un code de transformation inconnu — le dernier octet est celui de la
    // dernière instance.
    let mut code = octets.clone();
    *code.last_mut().unwrap() = 6;
    assert!(Projet::decoder(&code).is_err());
    // Un identifiant déjà donné qui pourrait revenir — le plus grand est égal
    // au prochain : refusé.
    let mut p2 = p.clone();
    p2.prochain -= 1;
    assert!(Projet::decoder(&p2.encoder()).is_err());
    // Un identifiant qui désigne deux choses : refusé.
    let mut p3 = p.clone();
    let dernier = p3.instances.len() - 1;
    p3.instances[dernier].id = p3.instances[0].id;
    assert!(Projet::decoder(&p3.encoder()).is_err());
    // Une case qui désigne une entrée de palette absente : refusée — relue,
    // elle ferait paniquer le premier compte de matière.
    let mut p4 = p.clone();
    let hors = p4.definitions[0].contenu.palette.len() as u32;
    p4.definitions[0].contenu.cases[0] = hors;
    assert!(Projet::decoder(&p4.encoder()).is_err());
    // Un document sans composant mais qui a déjà donné des identifiants les
    // garde : les oublier, ce serait les redonner.
    let vide = Projet {
        prochain: 3,
        ..Default::default()
    };
    assert_eq!(Projet::decoder(&vide.encoder()).unwrap(), vide);
}

/// **Plus de 65 535 états distincts** : le document passe à quatre octets par
/// case au lieu de deux, et se relit à l'identique.
#[test]
fn une_palette_immense_se_relit_a_l_identique() {
    let n = u16::MAX as u32 + 2;
    let palette: Vec<String> = (0..n).map(|k| format!("t:bloc{k:06}")).collect();
    let p = Projet {
        prochain: 2,
        definitions: vec![tf_ops::composant::Definition {
            id: 1,
            nom: "immense".into(),
            contenu: tf_ops::composant::Contenu {
                taille: [n, 1, 1],
                palette,
                cases: (0..n).rev().collect(),
                entites: Vec::new(),
            },
        }],
        instances: Vec::new(),
    };
    assert_eq!(Projet::decoder(&p.encoder()).unwrap(), p);
}
