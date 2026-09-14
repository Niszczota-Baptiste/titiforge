//! Les trois étages doivent rendre le MÊME monde.
//!
//! C'est l'invariant du dépôt, et celui dont le coût d'une erreur est le plus
//! élevé : une optimisation non vérifiée ne plante pas, elle écrit des blocs
//! faux dans la sauvegarde de quelqu'un. Chaque test rapide se compare donc au
//! chemin lent sur les mêmes données, et le compte doit être exact au bloc
//! près — pas « du même ordre ».

use tf_anvil::{bits_for, Packing, Section, StateId, VOL};
use tf_ops::plan::{Etage, Plan};
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, SectionPos};

const AIR: StateId = 0;
const PIERRE: StateId = 1;
const TERRE: StateId = 2;
const ROCHE: StateId = 3;
const HERBE: StateId = 4;

fn section(indices: &[u16], palette: &[StateId]) -> Section {
    let mut s = Section {
        y: 0,
        palette: palette.to_vec(),
        bits: bits_for(palette.len()),
        data: Box::new([]),
        packing: Packing::NoStraddle,
    };
    s.repack(indices);
    s
}

/// Une section bariolée, déterministe.
fn bariolee(palette: &[StateId]) -> Section {
    let mut n = 0x1234_5678u32;
    let idx: Vec<u16> = (0..VOL)
        .map(|_| {
            n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((n >> 16) as usize % palette.len()) as u16
        })
        .collect();
    section(&idx, palette)
}

/// Tous les états d'une section, case par case — la vérité de référence.
fn etats(s: &Section) -> Vec<StateId> {
    let idx = s.unpack();
    idx.iter().map(|&i| s.palette[i as usize]).collect()
}

/// Le chemin LENT : on relit et on réécrit chaque case, sans malice.
fn lent(s: &Section, sel: &BBox, pos: SectionPos, plan: &Plan) -> (Vec<StateId>, u64) {
    let avant = etats(s);
    let mut apres = avant.clone();
    let mut n = 0;
    for y in 0..16usize {
        for z in 0..16usize {
            for x in 0..16usize {
                let p = BlockPos {
                    x: pos.x * 16 + x as i32,
                    y: pos.y * 16 + y as i32,
                    z: pos.z * 16 + z as i32,
                };
                if !sel.contains(p) {
                    continue;
                }
                let i = (y << 8) | (z << 4) | x;
                if !plan.masque.accepte(avant[i]) {
                    continue;
                }
                let neuf = plan.motif.choisir(p.x, p.y, p.z, plan.seed, avant[i]);
                if neuf != avant[i] {
                    n += 1;
                }
                apres[i] = neuf;
            }
        }
    }
    (apres, n)
}

/// Joue le plan des deux façons et exige le même monde.
fn croiser(mut s: Section, sel: BBox, pos: SectionPos, plan: &Plan) -> Etage {
    let (attendu, n_attendu) = lent(&s, &sel, pos, plan);
    let r = plan.appliquer(&mut s, &sel, pos);
    assert_eq!(
        etats(&s),
        attendu,
        "l'étage {:?} ne rend pas le même monde que le chemin lent",
        r.etage
    );
    if let Some(n) = r.blocs {
        assert_eq!(n, n_attendu, "l'étage {:?} ne compte pas juste", r.etage);
    }
    r.etage
}

fn toute_la_section() -> BBox {
    BBox::new(
        BlockPos { x: 0, y: 0, z: 0 },
        BlockPos {
            x: 15,
            y: 15,
            z: 15,
        },
    )
}

const ORIGINE: SectionPos = SectionPos { x: 0, y: 0, z: 0 };

// ── le choix de l'étage ─────────────────────────────────────────────────────

#[test]
fn une_section_couverte_et_un_resultat_uniforme_coutent_o_de_1() {
    let s = bariolee(&[PIERRE, TERRE]);
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(ROCHE)).en_comptant();
    assert_eq!(
        croiser(s, toute_la_section(), ORIGINE, &plan),
        Etage::Section
    );
}

#[test]
fn un_masque_qui_couvre_toute_la_palette_passe_aussi_par_l_etage_section() {
    // `//replace pierre,terre roche` sur une section qui ne contient que de la
    // pierre et de la terre donne un résultat UNIFORME. Il n'y a aucune raison
    // de le payer plus cher qu'un `//set` — et sans ce cas, l'opération la plus
    // courante d'un éditeur retomberait d'un étage.
    let s = bariolee(&[PIERRE, TERRE]);
    let plan = Plan::nouveau(Masque::parmi(vec![PIERRE, TERRE]), Motif::Bloc(ROCHE)).en_comptant();
    assert_eq!(
        croiser(s, toute_la_section(), ORIGINE, &plan),
        Etage::Section
    );
}

#[test]
fn un_remplacement_partiel_sur_section_couverte_reste_a_l_etage_palette() {
    let s = bariolee(&[PIERRE, TERRE, HERBE]);
    let plan = Plan::nouveau(Masque::Etat(TERRE), Motif::Bloc(ROCHE)).en_comptant();
    assert_eq!(
        croiser(s, toute_la_section(), ORIGINE, &plan),
        Etage::Palette
    );
}

#[test]
fn une_bordure_de_selection_descend_a_l_etage_bloc() {
    let s = bariolee(&[PIERRE, TERRE]);
    let sel = BBox::new(BlockPos { x: 2, y: 3, z: 4 }, BlockPos { x: 9, y: 9, z: 9 });
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(ROCHE)).en_comptant();
    assert_eq!(croiser(s, sel, ORIGINE, &plan), Etage::Bloc);
}

#[test]
fn un_motif_qui_depend_de_la_position_descend_a_l_etage_bloc() {
    let s = bariolee(&[PIERRE, TERRE]);
    let plan = Plan::nouveau(Masque::Tout, Motif::melange(vec![(3, PIERRE), (1, ROCHE)]))
        .avec_seed(42)
        .en_comptant();
    assert_eq!(croiser(s, toute_la_section(), ORIGINE, &plan), Etage::Bloc);
}

#[test]
fn une_section_que_le_masque_ne_touche_pas_ne_coute_que_sa_palette() {
    // Le cas le PLUS FRÉQUENT sur un vrai monde : un `//replace` visant un bloc
    // rare ne concerne qu'une section sur mille. S'il coûtait un parcours, le
    // reste ne servirait à rien.
    let mut s = bariolee(&[PIERRE, TERRE]);
    let avant = etats(&s);
    let plan = Plan::nouveau(Masque::Etat(HERBE), Motif::Bloc(ROCHE)).en_comptant();
    let r = plan.appliquer(&mut s, &toute_la_section(), ORIGINE);
    assert_eq!(r.etage, Etage::Rien);
    assert_eq!(r.blocs, Some(0));
    assert_eq!(
        r.bornes, None,
        "rien fait, donc rien à annuler ni à remailler"
    );
    assert_eq!(etats(&s), avant);
}

// ── l'invariant du dédoublonnage ────────────────────────────────────────────

#[test]
fn l_etage_palette_ne_dedoublonne_pas() {
    // C'est ce qui fait tout le gain : en laissant l'entrée redondante, la
    // longueur de la palette ne bouge pas, donc `bits` non plus, donc aucun des
    // 4 096 indices n'est touché. Mesuré dans le prototype : 0,26 ms contre
    // 23,3 quand on dédoublonne.
    let mut s = bariolee(&[PIERRE, TERRE, HERBE]);
    let data_avant = s.data.clone();
    let bits_avant = s.bits;
    let plan = Plan::nouveau(Masque::Etat(TERRE), Motif::Bloc(PIERRE));
    plan.appliquer(&mut s, &toute_la_section(), ORIGINE);

    assert_eq!(s.palette.len(), 3, "la palette ne raccourcit pas");
    assert_eq!(s.bits, bits_avant, "donc `bits` ne bouge pas");
    assert_eq!(&s.data, &data_avant, "donc AUCUN indice n'est réécrit");
    assert_eq!(
        s.palette.iter().filter(|&&e| e == PIERRE).count(),
        2,
        "la pierre est bien dans la palette DEUX fois"
    );
}

#[test]
fn une_palette_a_doublons_se_relit_correctement() {
    // Le corollaire dangereux : après un premier remplacement, la palette porte
    // le même état deux fois. Un second remplacement qui ne chercherait que la
    // PREMIÈRE occurrence en raterait la moitié — sans la moindre erreur.
    let mut s = bariolee(&[PIERRE, TERRE, HERBE]);
    let plan1 = Plan::nouveau(Masque::Etat(TERRE), Motif::Bloc(PIERRE));
    plan1.appliquer(&mut s, &toute_la_section(), ORIGINE);

    let attendu = etats(&s).iter().filter(|&&e| e == PIERRE).count();
    let plan2 = Plan::nouveau(Masque::Etat(PIERRE), Motif::Bloc(ROCHE)).en_comptant();
    let r = plan2.appliquer(&mut s, &toute_la_section(), ORIGINE);

    assert_eq!(r.blocs, Some(attendu as u64));
    assert_eq!(
        etats(&s).iter().filter(|&&e| e == PIERRE).count(),
        0,
        "toutes les occurrences, jamais la première"
    );
}

// ── le tirage ───────────────────────────────────────────────────────────────

#[test]
fn un_melange_est_rejouable_et_independant_de_l_ordre() {
    // Le tirage se hache sur la POSITION : refaire l'opération sur la même
    // graine redonne exactement le même monde, et refaire un COIN redonne les
    // mêmes blocs que le mélange entier. Sans ça, retoucher un bout d'une zone
    // laisserait une couture visible.
    let palette = [AIR, PIERRE, TERRE];
    let plan = Plan::nouveau(
        Masque::Tout,
        Motif::melange(vec![(2, PIERRE), (1, TERRE), (1, HERBE)]),
    )
    .avec_seed(1234);

    let mut entiere = bariolee(&palette);
    plan.appliquer(&mut entiere, &toute_la_section(), ORIGINE);

    let mut bis = bariolee(&palette);
    plan.appliquer(&mut bis, &toute_la_section(), ORIGINE);
    assert_eq!(etats(&entiere), etats(&bis), "même graine, même monde");

    // Le même mélange, mais restreint à un coin.
    let coin = BBox::new(BlockPos { x: 4, y: 4, z: 4 }, BlockPos { x: 7, y: 7, z: 7 });
    let mut morceau = bariolee(&palette);
    plan.appliquer(&mut morceau, &coin, ORIGINE);

    let a = etats(&entiere);
    let b = etats(&morceau);
    for y in 4..=7usize {
        for z in 4..=7usize {
            for x in 4..=7usize {
                let i = (y << 8) | (z << 4) | x;
                assert_eq!(a[i], b[i], "le coin doit être identique en ({x},{y},{z})");
            }
        }
    }
}

#[test]
fn les_proportions_d_un_melange_sont_celles_qu_on_demande() {
    // Un tirage haché doit rester un tirage : si les poids ne se retrouvent pas
    // dans le résultat, c'est que le modulo mord sur un motif.
    let plan =
        Plan::nouveau(Masque::Tout, Motif::melange(vec![(3, PIERRE), (1, TERRE)])).avec_seed(7);
    let mut s = bariolee(&[AIR]);
    plan.appliquer(&mut s, &toute_la_section(), ORIGINE);
    let v = etats(&s);
    let pierres = v.iter().filter(|&&e| e == PIERRE).count();
    let part = pierres as f64 / VOL as f64;
    assert!(
        (part - 0.75).abs() < 0.02,
        "trois quarts attendus, {part:.3} obtenu"
    );
}

#[test]
fn un_melange_ne_depend_pas_de_la_section_mais_de_la_position_monde() {
    // Deux sections voisines doivent se raccorder : le tirage se hache sur les
    // coordonnées monde, pas sur l'indice local. Sinon chaque section
    // répéterait le même motif, et un grand mélange sortirait en damier de
    // 16 × 16.
    let plan =
        Plan::nouveau(Masque::Tout, Motif::melange(vec![(1, PIERRE), (1, TERRE)])).avec_seed(9);
    let mut a = bariolee(&[AIR]);
    let mut b = bariolee(&[AIR]);
    plan.appliquer(
        &mut a,
        &BBox::new(
            BlockPos { x: 0, y: 0, z: 0 },
            BlockPos {
                x: 15,
                y: 15,
                z: 15,
            },
        ),
        ORIGINE,
    );
    plan.appliquer(
        &mut b,
        &BBox::new(
            BlockPos { x: 16, y: 0, z: 0 },
            BlockPos {
                x: 31,
                y: 15,
                z: 15,
            },
        ),
        SectionPos { x: 1, y: 0, z: 0 },
    );
    assert_ne!(
        etats(&a),
        etats(&b),
        "deux sections ne doivent pas se répéter"
    );
}

// ── les bornes ──────────────────────────────────────────────────────────────

#[test]
fn les_bornes_ne_debordent_pas_de_la_selection() {
    // L'instantané d'annulation et le remaillage s'y fient. Trop larges, on
    // paie ; trop étroites, on perd de quoi annuler.
    let s = bariolee(&[PIERRE]);
    let sel = BBox::new(
        BlockPos { x: 3, y: 5, z: 7 },
        BlockPos { x: 9, y: 11, z: 13 },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(ROCHE));
    let mut s = s;
    let r = plan.appliquer(&mut s, &sel, ORIGINE);
    let b = r
        .bornes
        .expect("l'opération a écrit, elle a donc des bornes");
    assert_eq!(b.min, sel.min);
    assert_eq!(b.max, sel.max);
}

#[test]
fn une_selection_qui_manque_la_section_ne_fait_rien() {
    let mut s = bariolee(&[PIERRE]);
    let avant = etats(&s);
    let loin = BBox::new(
        BlockPos {
            x: 1000,
            y: 0,
            z: 0,
        },
        BlockPos {
            x: 1015,
            y: 15,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(ROCHE)).en_comptant();
    let r = plan.appliquer(&mut s, &loin, ORIGINE);
    assert_eq!(r.blocs, Some(0));
    assert_eq!(r.bornes, None);
    assert_eq!(etats(&s), avant);
}
#[test]
fn un_indice_hors_palette_ne_tue_pas_le_processus() {
    // Un `.mca` corrompu ou forgé peut porter un indice que la palette ne
    // contient pas : `bits` se DÉDUIT de la longueur de palette, donc une
    // palette de deux entrées se lit sur quatre bits et seize valeurs sont
    // représentables pour deux valides. Le moteur doit le refuser ou l'ignorer,
    // jamais paniquer — c'est la sauvegarde de quelqu'un.
    let mut s = section(&[0; VOL], &[PIERRE, TERRE]);
    // On force un indice de 7 dans une palette de 2.
    let mut idx = vec![0u16; VOL];
    idx[100] = 7;
    idx[4000] = 3;
    s.repack(&idx);

    // L'étage BLOC, celui qui indexe la palette directement : une sélection
    // qui ne couvre pas la section l'y force.
    let bordee = BBox::new(
        BlockPos { x: 0, y: 0, z: 0 },
        BlockPos {
            x: 14,
            y: 15,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(ROCHE)).en_comptant();
    let mut a = s.clone();
    let r = plan.appliquer(&mut a, &bordee, ORIGINE);
    assert_eq!(r.etage, Etage::Bloc);

    // Et la case incomprise ressort À L'IDENTIQUE : ne pas paniquer ne suffit
    // pas, il ne faut pas non plus la réécrire au hasard.
    let apres = a.unpack();
    assert_eq!(apres[100], 7, "l'indice incompris doit survivre tel quel");
    assert_eq!(apres[4000], 3, "et celui-là aussi");

    // Et l'étage PALETTE, qui construit sa table sur la palette.
    let plan = Plan::nouveau(Masque::Etat(TERRE), Motif::Bloc(ROCHE)).en_comptant();
    let mut b = s.clone();
    plan.appliquer(&mut b, &toute_la_section(), ORIGINE);
    assert_eq!(b.unpack()[100], 7, "l'étage palette ne touche aucun indice");
}
