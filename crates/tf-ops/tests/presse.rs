//! Le presse-papiers : déplacer des cases, et transformer des états.
//!
//! Deux choses se vérifient ici, et elles se ratent différemment. Une géométrie
//! fausse se voit tout de suite — le build sort en miroir. Un ÉTAT non
//! transformé ne se voit pas : la moitié des escaliers regarde ailleurs, et
//! rien à l'écran ne dit pourquoi.

use std::cell::RefCell;

use tf_anvil::{Interner, StateId};
use tf_blocks::{Transfo, TOUTES};
use tf_ops::presse::TransfoBoite;
use tf_ops::Presse;

/// Une règle de rotation jouet, qui ne connaît QUE `facing` — assez pour
/// distinguer un état transformé d'un état laissé tel quel, et assez petit
/// pour qu'on lise ce qu'elle fait.
///
/// Les vraies règles sont dérivées d'un pack (`tf-blocks::Table`) ; elles ne
/// sont pas rejouées ici, elles ont leurs propres contrôles.
fn regle_jouet(cle: &str, t: Transfo) -> Option<String> {
    const ROSE: [&str; 4] = ["north", "east", "south", "west"];
    let (nom, props) = cle.split_once("|facing=")?;
    let i = ROSE.iter().position(|d| *d == props)?;
    // Un quart de tour envoie `+X` (est) sur `+Z` (sud) : dans la rose, c'est
    // l'indice suivant.
    let j = match t {
        Transfo::Rot90 => (i + 1) % 4,
        Transfo::Rot180 => (i + 2) % 4,
        Transfo::Rot270 => (i + 3) % 4,
        // `x → −x` échange est et ouest, laisse nord et sud.
        Transfo::MiroirX => [0, 3, 2, 1][i],
        // `z → −z` échange nord et sud.
        Transfo::MiroirZ => [2, 1, 0, 3][i],
    };
    Some(format!("{nom}|facing={}", ROSE[j]))
}

/// Un extrait 3 × 1 × 2, chaque case distincte, pour qu'aucune symétrie ne
/// puisse faire passer une géométrie fausse pour juste.
fn extrait(interner: &mut Interner) -> Presse {
    let mut blocs = Vec::new();
    for z in 0..2u32 {
        for x in 0..3u32 {
            blocs.push(interner.intern(&format!("t:c{x}_{z}")));
        }
    }
    Presse {
        taille: [3, 1, 2],
        blocs,
        ancre: [0, 0, 0],
    }
}

#[test]
fn un_quart_de_tour_echange_la_largeur_et_la_profondeur() {
    let mut i = Interner::new();
    let p = extrait(&mut i);
    for t in [Transfo::Rot90, Transfo::Rot270] {
        let r = p.transformer(t, &mut i, &|_, _| None);
        assert_eq!(r.presse.taille, [2, 1, 3], "{t:?}");
    }
    for t in [Transfo::Rot180, Transfo::MiroirX, Transfo::MiroirZ] {
        let r = p.transformer(t, &mut i, &|_, _| None);
        assert_eq!(r.presse.taille, [3, 1, 2], "{t:?}");
    }
}

/// **Un quart de tour envoie `+X` sur `+Z`** — la convention de tout le dépôt,
/// celle que `tf-blocks` applique à la géométrie d'un modèle. La prendre à
/// l'envers ici ferait tourner le build dans un sens et ses escaliers dans
/// l'autre.
#[test]
fn un_quart_de_tour_envoie_l_est_sur_le_sud() {
    let mut i = Interner::new();
    let p = extrait(&mut i);
    // La case la plus à l'EST de la rangée nord : (x = 2, z = 0).
    let est = p.get(2, 0, 0).unwrap();
    let r = p.transformer(Transfo::Rot90, &mut i, &|_, _| None).presse;
    // Elle doit se retrouver la plus au SUD : z maximal.
    let [_, _, sz] = r.taille;
    let mut trouve = None;
    for z in 0..sz {
        for x in 0..r.taille[0] {
            if r.get(x, 0, z) == Some(est) {
                trouve = Some((x, z));
            }
        }
    }
    let (_, z) = trouve.expect("la case doit exister après rotation");
    assert_eq!(z, sz - 1, "l'est doit partir au sud");
}

#[test]
fn quatre_quarts_de_tour_rendent_l_extrait_de_depart() {
    let mut i = Interner::new();
    let depart = extrait(&mut i);
    let mut p = depart.clone();
    for _ in 0..4 {
        p = p.transformer(Transfo::Rot90, &mut i, &regle_jouet).presse;
    }
    assert_eq!(p, depart);
}

#[test]
fn un_miroir_deux_fois_ne_fait_rien() {
    let mut i = Interner::new();
    let depart = extrait(&mut i);
    for t in [Transfo::MiroirX, Transfo::MiroirZ] {
        let a = depart.transformer(t, &mut i, &regle_jouet).presse;
        let b = a.transformer(t, &mut i, &regle_jouet).presse;
        assert_eq!(b, depart, "{t:?} deux fois");
    }
}

/// **Les deux miroirs ne sont pas indépendants** : `z → −z` est `x → −x` suivi
/// d'un demi-tour. `tf-blocks` en dérive UN et compose l'autre ; la géométrie
/// de la boîte doit suivre la même loi, sinon les deux moitiés du travail se
/// contrediraient.
#[test]
fn le_miroir_nord_sud_est_le_miroir_est_ouest_puis_un_demi_tour() {
    let mut i = Interner::new();
    let p = extrait(&mut i);
    let direct = p.transformer(Transfo::MiroirZ, &mut i, &regle_jouet).presse;
    let compose = p
        .transformer(Transfo::MiroirX, &mut i, &regle_jouet)
        .presse
        .transformer(Transfo::Rot180, &mut i, &regle_jouet)
        .presse;
    assert_eq!(direct, compose);
}

/// Les états suivent la géométrie : un escalier qui regardait l'est regarde le
/// sud après un quart de tour.
#[test]
fn les_etats_sont_transformes_avec_les_cases() {
    let mut i = Interner::new();
    let est = i.intern("t:escalier|facing=east");
    let p = Presse::uniforme([2, 1, 2], est);
    let r = p.transformer(Transfo::Rot90, &mut i, &regle_jouet);
    assert!(r.intacts.is_empty(), "la règle connaît cet état");
    let sud = i.get("t:escalier|facing=south").expect("interné");
    assert!(
        r.presse.blocs.iter().all(|&b| b == sud),
        "toutes les cases doivent porter l'état tourné"
    );
}

/// **Ce qu'on ne sait pas transformer, on n'y touche pas — et on le DIT.**
///
/// Le supposer symétrique produirait un build à moitié tourné, faux d'une
/// façon qu'aucune capture d'écran ne montre.
#[test]
fn un_etat_inconnu_reste_intact_et_est_signale() {
    let mut i = Interner::new();
    let connu = i.intern("t:escalier|facing=east");
    let mystere = i.intern("t:bidule_sans_regle");
    let p = Presse {
        taille: [2, 1, 1],
        blocs: vec![connu, mystere],
        ancre: [0, 0, 0],
    };
    let r = p.transformer(Transfo::Rot90, &mut i, &regle_jouet);
    assert_eq!(r.intacts, vec![mystere], "l'état inconnu doit être nommé");
    assert!(
        r.presse.blocs.contains(&mystere),
        "et laissé TEL QUEL, pas remplacé par de l'air"
    );
}

/// **La transformation se fait sur la PALETTE, jamais par bloc.**
///
/// C'est le même raisonnement que l'étage palette des opérations : un extrait
/// d'un million de cases porte quelques centaines d'états. Le test compte les
/// appels — une régression qui transformerait case par case passerait tous les
/// autres tests sans que rien ne le dise, et coûterait quatre ordres de
/// grandeur.
#[test]
fn la_regle_n_est_appelee_qu_une_fois_par_etat() {
    let mut i = Interner::new();
    let a = i.intern("t:escalier|facing=east");
    let b = i.intern("t:escalier|facing=north");
    let c = i.intern("t:pierre");
    // 60 000 cases, trois états.
    let mut blocs = Vec::with_capacity(60_000);
    for k in 0..60_000usize {
        blocs.push([a, b, c][k % 3]);
    }
    let p = Presse {
        taille: [100, 6, 100],
        blocs,
        ancre: [0, 0, 0],
    };
    let appels = RefCell::new(0usize);
    let r = p.transformer(Transfo::Rot90, &mut i, &|cle, t| {
        *appels.borrow_mut() += 1;
        regle_jouet(cle, t)
    });
    assert_eq!(
        *appels.borrow(),
        3,
        "trois états distincts, trois appels — pas 60 000"
    );
    assert_eq!(r.presse.blocs.len(), 60_000);
    assert_eq!(r.presse.taille, [100, 6, 100]);
}

/// L'ancre suit la transformation, et peut SORTIR de la boîte : on copie
/// souvent depuis l'endroit où l'on se tient. Sans elle, un build tourné se
/// collerait décalé de sa propre largeur.
#[test]
fn l_ancre_suit_la_transformation_meme_hors_de_la_boite() {
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let p = Presse {
        ancre: [-3, 2, 1],
        ..Presse::uniforme([4, 3, 8], air)
    };
    // Quatre quarts de tour la ramènent où elle était.
    let mut q = p.clone();
    for _ in 0..4 {
        q = q.transformer(Transfo::Rot90, &mut i, &|_, _| None).presse;
    }
    assert_eq!(q.ancre, p.ancre);
    // Et elle ne se fait pas rogner dans la boîte au passage.
    let r = p.transformer(Transfo::Rot90, &mut i, &|_, _| None).presse;
    assert_eq!(r.ancre[1], 2, "la hauteur ne bouge pas sous une rotation");
    assert!(
        r.ancre[0] < 0 || r.ancre[2] < 0 || r.ancre[0] >= 8 || r.ancre[2] >= 4,
        "une ancre hors boîte le reste : {:?}",
        r.ancre
    );
}

/// Toutes les transformations conservent le VOLUME et le contenu : une
/// rotation déplace, elle ne crée ni ne perd de case.
#[test]
fn aucune_transformation_ne_perd_ni_n_invente_de_case() {
    let mut i = Interner::new();
    let p = extrait(&mut i);
    let mut depart: Vec<StateId> = p.blocs.clone();
    depart.sort_unstable();
    for t in TOUTES {
        let r = p.transformer(t, &mut i, &|_, _| None).presse;
        assert_eq!(r.blocs.len(), p.blocs.len(), "{t:?}");
        let mut apres = r.blocs.clone();
        apres.sort_unstable();
        assert_eq!(apres, depart, "{t:?} doit être une permutation");
    }
}

/// La géométrie de la BOÎTE et celle d'un POINT doivent s'accorder : une case
/// prise comme point doit atterrir au même endroit. Deux formules écrites côte
/// à côte finissent par diverger — celle-là est vérifiée.
#[test]
fn la_case_et_le_point_atterrissent_au_meme_endroit() {
    let taille = [5u32, 2, 3];
    for t in TOUTES {
        for z in 0..taille[2] {
            for x in 0..taille[0] {
                let (cx, cz) = t.case_apres((x, z), (taille[0], taille[2]));
                let p = t.point_apres([x as i32, 0, z as i32], taille);
                assert_eq!((cx as i32, cz as i32), (p[0], p[2]), "{t:?} en ({x}, {z})");
            }
        }
    }
}
