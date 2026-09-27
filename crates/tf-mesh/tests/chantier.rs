//! La peau qui traverse les chunks, et les sections qu'on ne maille pas.

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_mesh::forme::Cuboide;
use tf_mesh::{Face, Grille, TableFormes, Voisinage, COTE};

const AIR: StateId = 0;
const PIERRE: StateId = 1;
const DALLE: StateId = 2;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    );
    t
}

/// Une section dont chaque case est décidée par `f`.
fn section(y: i8, mut f: impl FnMut(i32, i32, i32) -> StateId) -> Section {
    let mut palette: Vec<StateId> = Vec::new();
    let mut idx = vec![0u16; 4096];
    for by in 0..16i32 {
        for bz in 0..16i32 {
            for bx in 0..16i32 {
                let id = f(bx, by, bz);
                let k = match palette.iter().position(|p| *p == id) {
                    Some(k) => k,
                    None => {
                        palette.push(id);
                        palette.len() - 1
                    }
                };
                idx[(by * 256 + bz * 16 + bx) as usize] = k as u16;
            }
        }
    }
    let bits = bits_for(palette.len());
    let data = if palette.len() == 1 {
        Vec::new()
    } else {
        pack(&idx, bits as usize, Packing::NoStraddle)
    };
    Section {
        y,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing: Packing::NoStraddle,
    }
}

fn pleine(y: i8, id: StateId) -> Section {
    section(y, |_, _, _| id)
}

#[test]
fn la_peau_traverse_les_chunks() {
    // Deux chunks côte à côte, tous deux pleins de pierre. La frontière ne doit
    // produire AUCUNE face : sans peau vraie, on dessinerait un mur de faces
    // fantômes le long de chaque bord de chunk.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    g.poser(1, 0, pleine(0, PIERRE));

    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    assert_eq!(
        v.get(COTE as i32, 0, 0),
        PIERRE,
        "la case juste après le bord appartient au chunk voisin"
    );

    let c = g.mailler(&t);
    // Les quads sont LOCAUX à leur section : c'est le lot qui dit où ils sont.
    let lot = c.lots.iter().find(|l| l.adresse == (0, 0, 0)).unwrap();
    assert!(
        !lot.quads.quads.iter().any(|q| q.face == Face::PlusX),
        "la frontière entre deux chunks pleins ne montre rien"
    );
    let voisin = c.lots.iter().find(|l| l.adresse == (1, 0, 0)).unwrap();
    assert!(
        voisin.quads.quads.iter().any(|q| q.face == Face::PlusX),
        "mais la face EXTÉRIEURE du chunk voisin, oui"
    );
    assert_eq!(lot.origine(), [0, 0, 0]);
    assert_eq!(voisin.origine(), [16, 0, 0]);
}

#[test]
fn la_peau_traverse_aussi_les_sections_empilees() {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    g.poser(0, 0, pleine(1, PIERRE));

    let c = g.mailler(&t);
    let bas = c.lots.iter().find(|l| l.adresse == (0, 0, 0)).unwrap();
    assert!(
        !bas.quads.quads.iter().any(|q| q.face == Face::PlusY),
        "le plafond de la section du bas est le plancher de celle du haut"
    );
    let haut = c.lots.iter().find(|l| l.adresse == (0, 0, 1)).unwrap();
    assert!(
        !haut.quads.quads.iter().any(|q| q.face == Face::MoinsY),
        "et réciproquement"
    );
    assert_eq!(haut.origine(), [0, 16, 0]);
}

#[test]
fn ce_qui_manque_vaut_de_l_air_et_non_de_la_pierre() {
    // Au bord d'une zone chargée, supposer opaque effacerait des faces
    // RÉELLES. Entre deux erreurs, on prend celle qui se voit.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));

    let c = g.mailler(&t);
    assert_eq!(
        c.quads(),
        6,
        "une section isolée montre ses six faces, pas zéro"
    );
}

#[test]
fn une_section_d_air_n_est_pas_maillee() {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, AIR));
    g.poser(0, 0, pleine(1, PIERRE));

    let c = g.mailler(&t);
    assert_eq!(c.sautees, 1, "la section d'air se saute sur sa PALETTE");
    assert_eq!(c.maillees(), 1);
    // Et la pierre voit quand même son voisin d'air : ses faces sortent.
    let pierre = c.lots.iter().find(|l| l.adresse == (0, 0, 1)).unwrap();
    assert!(
        pierre.quads.quads.iter().any(|q| q.face == Face::MoinsY),
        "sauter une section d'air ne doit pas masquer les faces de ses voisins"
    );
}

#[test]
fn une_section_de_plantes_est_maillee_elle() {
    // Air et plantes sont indiscernables du point de vue de l'opacité. C'est
    // la palette qui tranche, et elle ne se trompe pas.
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, DALLE));

    let c = g.mailler(&t);
    assert_eq!(c.sautees, 0);
    assert_eq!(c.poses(), 4096, "4 096 dalles, 4 096 poses");
}

#[test]
fn une_section_absente_se_saute_sans_erreur() {
    let t = table();
    let g = Grille::new();
    let c = g.mailler(&t);
    assert_eq!(c.maillees(), 0);
    assert_eq!(c.quads(), 0);
}

#[test]
fn le_chantier_est_deterministe() {
    let t = table();
    let mut g = Grille::new();
    let mut n = 11u32;
    for (cx, cz) in [(0, 0), (1, 0), (0, 1)] {
        g.poser(
            cx,
            cz,
            section(0, |_, _, _| {
                n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match n % 4 {
                    0 => PIERRE,
                    1 => DALLE,
                    _ => AIR,
                }
            }),
        );
    }
    let mut a = g.mailler(&t);
    let mut b = g.mailler(&t);
    a.trier();
    b.trier();
    for (x, y) in a.lots.iter().zip(b.lots.iter()) {
        assert_eq!(x.adresse, y.adresse);
        assert_eq!(x.quads.quads, y.quads.quads);
        assert_eq!(x.poses.poses, y.poses.poses);
    }
}

#[test]
fn les_coordonnees_negatives_tombent_dans_le_bon_chunk() {
    // Division PLANCHER : le bloc −1 est dans le chunk −1, pas le chunk 0.
    // Une division entière naïve chargerait la mauvaise moitié du monde.
    let t = table();
    let mut g = Grille::new();
    g.poser(-1, -1, pleine(-1, PIERRE));
    assert_eq!(g.bloc(-1, -1, -1), PIERRE);
    assert_eq!(g.bloc(-16, -16, -16), PIERRE);
    assert_eq!(g.bloc(0, 0, 0), tf_mesh::ABSENT, "le chunk 0 n'a rien");
    assert!(
        tf_mesh::Formes::est_air(&t, g.bloc(0, 0, 0)),
        "et ce rien se lit comme de l'air"
    );

    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    assert_eq!(
        v.get(-1, -1, -1),
        PIERRE,
        "la peau du chunk 0 touche le chunk −1"
    );
    let _ = t;
}

#[cfg(feature = "parallele")]
#[test]
fn le_chantier_parallele_rend_exactement_le_meme_resultat() {
    // Une optimisation non vérifiée est une corruption silencieuse. Le chemin
    // rapide se compare au chemin lent sur la même entrée, et le compte doit
    // être exact — pas « du même ordre ».
    let t = table();
    let mut g = Grille::new();
    let mut n = 99u32;
    for cz in 0..4i32 {
        for cx in 0..4i32 {
            for sy in 0..3i8 {
                g.poser(
                    cx,
                    cz,
                    section(sy, |_, _, _| {
                        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match n % 5 {
                            0 => PIERRE,
                            1 => DALLE,
                            _ => AIR,
                        }
                    }),
                );
            }
        }
    }

    let mut sequentiel = g.mailler(&t);
    let mut parallele = g.mailler_parallele(&t);
    sequentiel.trier();
    parallele.trier();

    assert_eq!(sequentiel.sautees, parallele.sautees);
    assert_eq!(sequentiel.maillees(), parallele.maillees());
    assert_eq!(sequentiel.lots.len(), parallele.lots.len());
    for (a, b) in sequentiel.lots.iter().zip(parallele.lots.iter()) {
        assert_eq!(a.adresse, b.adresse);
        assert_eq!(a.quads.quads, b.quads.quads, "section {:?}", a.adresse);
        assert_eq!(a.poses.poses, b.poses.poses, "section {:?}", a.adresse);
    }
}

/// **Le remaillage PARTIEL en parallèle rend les mêmes lots, dans le même
/// ORDRE, que le séquentiel.** L'ordre compte ici plus que pour le chantier
/// complet : les lots partiels vont droit aux arènes, dont les places se
/// décident dans l'ordre d'arrivée — un ordre qui dépendrait du nombre de
/// cœurs donnerait deux dispositions différentes de la même scène.
#[cfg(feature = "parallele")]
#[test]
fn le_remaillage_partiel_parallele_rend_la_meme_chose_dans_le_meme_ordre() {
    let t = table();
    let mut g = Grille::new();
    let mut n = 7u32;
    for cz in 0..5i32 {
        for cx in 0..5i32 {
            for sy in 0..3i8 {
                g.poser(
                    cx,
                    cz,
                    section(sy, |_, _, _| {
                        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match n % 4 {
                            0 => PIERRE,
                            1 => DALLE,
                            _ => AIR,
                        }
                    }),
                );
            }
        }
    }
    // Plus de seize visées, donc plusieurs paquets pour rayon, plus des
    // adresses sans rien — sautées des deux côtés.
    let mut visees = Grille::sections_autour([16, 0, 16], [47, 47, 47]);
    visees.push((40, 40, 0));
    visees.sort_unstable();
    assert!(visees.len() > 32, "la prémisse : plusieurs paquets");

    let seq = g.mailler_ces(&t, &visees);
    let par = g.mailler_ces_parallele(&t, &visees);
    assert_eq!(seq.sautees, par.sautees);
    assert_eq!(
        seq.lots.iter().map(|l| l.adresse).collect::<Vec<_>>(),
        par.lots.iter().map(|l| l.adresse).collect::<Vec<_>>(),
        "les lots doivent sortir dans l'ordre des visées"
    );
    for (a, b) in seq.lots.iter().zip(par.lots.iter()) {
        assert_eq!(a.quads.quads, b.quads.quads, "section {:?}", a.adresse);
        assert_eq!(a.poses.poses, b.poses.poses, "section {:?}", a.adresse);
    }
}

#[test]
fn un_indice_hors_palette_ne_tue_pas_le_mailleur() {
    // `bits` se déduit de la longueur de palette : deux entrées se lisent sur
    // quatre bits, donc seize valeurs sont représentables pour deux valides. Un
    // `.mca` corrompu en porte, et un mailleur qui panique dessus fait mourir
    // l'application à l'AFFICHAGE — avant même que l'utilisateur ait touché à
    // quoi que ce soit.
    let mut s = tf_anvil::Section {
        y: 0,
        palette: vec![0, 1],
        bits: tf_anvil::bits_for(2),
        data: Box::new([]),
        packing: tf_anvil::Packing::NoStraddle,
    };
    let mut idx = vec![0u16; tf_anvil::VOL];
    idx[100] = 9;
    idx[2000] = 15;
    s.repack(&idx);

    let mut g = Grille::new();
    g.poser(0, 0, s);
    // Ce qui compte est de ne pas mourir ; le maillage doit rester cohérent.
    let c = g.mailler(&table());
    for lot in &c.lots {
        for q in &lot.quads.quads {
            assert!(q.aire() > 0.0, "un quad d'aire nulle");
        }
    }
}

// ── le remaillage PARTIEL ───────────────────────────────────────────────────

/// Quelques sections pleines côte à côte et empilées : de quoi qu'un
/// remaillage partiel ait des VOISINS à lire, ce qui est tout l'enjeu.
fn petit_monde() -> (Grille, TableFormes) {
    let f = table();
    let mut g = Grille::new();
    for cz in 0..2 {
        for cx in 0..2 {
            for y in 0..2 {
                g.poser(cx, cz, pleine(y, PIERRE));
            }
        }
    }
    (g, f)
}

/// **Un remaillage partiel doit rendre EXACTEMENT les mêmes quads** que le
/// maillage complet des mêmes sections. Une optimisation qui change l'image
/// n'est pas une optimisation, c'est un bug qu'on a choisi.
#[test]
fn mailler_ces_rend_la_meme_chose_que_tout_mailler() {
    let (g, f) = petit_monde();
    let complet = g.mailler(&f);

    // On remaille une section sur deux, et on compare lot par lot.
    let toutes = g.adresses();
    let moitie: Vec<_> = toutes.iter().copied().step_by(2).collect();
    assert!(moitie.len() >= 2, "il faut de quoi comparer");
    let partiel = g.mailler_ces(&f, &moitie);

    for lot in &partiel.lots {
        let attendu = complet
            .lots
            .iter()
            .find(|l| l.adresse == lot.adresse)
            .unwrap_or_else(|| panic!("{:?} absent du maillage complet", lot.adresse));
        // Sur les QUADS eux-mêmes, pas sur un compte : deux maillages du même
        // nombre de quads peuvent décrire deux images différentes.
        assert_eq!(lot.quads.quads, attendu.quads.quads, "{:?}", lot.adresse);
        assert_eq!(
            lot.quads.quads_glouton, attendu.quads.quads_glouton,
            "{:?}",
            lot.adresse
        );
        assert_eq!(lot.poses.poses, attendu.poses.poses, "{:?}", lot.adresse);
    }
}

/// **La marge d'UNE case n'est pas une précaution.** Le mailleur travaille
/// avec une couche de padding : poser un bloc au bord d'une section change
/// les faces visibles de la section d'à côté. Sans la marge, un trait au bord
/// laisse un mur de faces fantômes le long de la frontière.
#[test]
fn les_sections_a_remailler_debordent_d_une_case() {
    use tf_mesh::Grille;

    // **La marge est d'un BLOC, pas d'une section** — et c'est exactement ce
    // qu'il faut : le padding de la section S couvre les blocs
    // `S·16 − 1 .. S·16 + 16`, donc un bloc B ne concerne que les sections
    // dont le padding le contient. Déborder d'une SECTION entière remaillerait
    // vingt-six voisines pour un bloc posé au milieu — un facteur 26 pour rien.
    //
    // Au coin bas (bloc 0), les deux sections de chaque axe : celle du bloc, et
    // celle d'avant, dont le padding touche le bloc 0.
    let a = Grille::sections_autour([0, 0, 0], [0, 0, 0]);
    assert!(a.contains(&(0, 0, 0)), "la sienne");
    assert!(a.contains(&(-1, 0, 0)), "la voisine en −X");
    assert!(a.contains(&(0, -1, 0)), "la voisine en −Z");
    assert!(a.contains(&(0, 0, -1)), "la voisine en −Y");
    assert!(a.contains(&(-1, -1, -1)), "et leur coin commun");
    assert_eq!(a.len(), 8, "2 × 2 × 2 au coin BAS d'une section : {a:?}");

    // Au coin HAUT (bloc 15), c'est l'autre côté : la section suivante.
    let h = Grille::sections_autour([15, 15, 15], [15, 15, 15]);
    assert!(h.contains(&(0, 0, 0)) && h.contains(&(1, 1, 1)), "{h:?}");
    assert_eq!(h.len(), 8);

    // Un bloc au MILIEU ne touche aucune voisine : son padding ne sort pas.
    let b = Grille::sections_autour([8, 8, 8], [8, 8, 8]);
    assert_eq!(b, vec![(0, 0, 0)], "{b:?}");
}

/// Un Y hors de l'intervalle d'un `i8` n'a pas de section : on ne l'invente
/// pas. Le monde va de −64 à 320, mais un `//expand` peut sortir de tout.
#[test]
fn un_y_demesure_ne_donne_pas_de_section() {
    use tf_mesh::Grille;
    let a = Grille::sections_autour([0, i32::MAX - 1, 0], [0, i32::MAX, 0]);
    assert!(a.is_empty(), "{a:?}");
    // Et près de i32::MIN, la marge ne doit pas s'enrouler.
    let b = Grille::sections_autour([i32::MIN, 0, i32::MIN], [i32::MIN, 0, i32::MIN]);
    assert!(b
        .iter()
        .all(|(x, z, _)| *x <= i32::MIN / 16 + 1 && *z <= i32::MIN / 16 + 1));
}

/// Une adresse qui n'a pas de contenu est SAUTÉE, pas maillée à vide — mais
/// elle reste dans la liste des VISÉES, parce que c'est ce qui permet de
/// retirer le maillage d'une section qui vient de se vider.
#[test]
fn une_section_absente_est_sautee_sans_disparaitre_de_la_liste() {
    let (g, f) = petit_monde();
    let nulle_part = (9999, 9999, 0);
    assert!(g.section(nulle_part).is_none());

    let c = g.mailler_ces(&f, &[nulle_part]);
    assert!(c.lots.is_empty());
    assert_eq!(c.sautees, 1);

    // Et `sections_autour` la rend quand même : c'est une liste de CIBLES.
    let vise =
        tf_mesh::Grille::sections_autour([9999 * 16, 0, 9999 * 16], [9999 * 16, 0, 9999 * 16]);
    assert!(vise.contains(&nulle_part));
}

/// **Remplacer par un remaillage partiel doit donner le MÊME chantier qu'un
/// maillage complet.** C'est la seule propriété qui compte : si les deux
/// divergent, l'image dépend de l'historique des opérations, et personne ne
/// sait plus ce qu'il regarde.
#[test]
fn remplacer_par_un_remaillage_partiel_rend_le_chantier_complet() {
    let (mut g, f) = petit_monde();
    let mut courant = g.mailler(&f);

    // On change une section — de la pierre pleine à de l'air.
    let vise = tf_mesh::Grille::sections_autour([16, 0, 0], [31, 15, 15]);
    g.poser(1, 0, pleine(0, AIR));
    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    let complet = g.mailler(&f);
    assert_eq!(courant.lots.len(), complet.lots.len());
    for (a, b) in courant.lots.iter().zip(complet.lots.iter()) {
        assert_eq!(a.adresse, b.adresse, "l'ordre doit rester trié");
        assert_eq!(a.quads.quads, b.quads.quads, "{:?}", a.adresse);
        assert_eq!(a.poses.poses, b.poses.poses, "{:?}", a.adresse);
    }
}

/// **Un chunk qui se VIDE ne figure plus dans la liste des chunks.** Si la
/// fusion se contentait d'insérer ce que le remaillage produit, le maillage
/// d'une section effacée resterait à l'écran — les blocs supprimés resteraient
/// visibles. Ça ne se voit que sur un effacement, jamais sur une pose.
#[test]
fn une_section_videe_perd_son_maillage() {
    let (mut g, f) = petit_monde();
    let mut courant = g.mailler(&f);
    let avant = courant.lots.len();
    assert!(courant.lots.iter().any(|l| l.adresse == (1, 1, 0)));

    // Effacée pour de bon : la section n'est plus dans la grille du tout.
    let vise = tf_mesh::Grille::sections_autour([16, 0, 16], [31, 15, 31]);
    g.poser(1, 1, pleine(0, AIR));
    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    assert!(
        !courant.lots.iter().any(|l| l.adresse == (1, 1, 0)),
        "le maillage d'une section vidée est resté"
    );
    assert!(courant.lots.len() < avant);
}

/// **L'ordre reste trié même quand on ne remaille qu'une section du MILIEU.**
///
/// Le premier test de fusion ne le voyait pas : sa liste visée couvrait tout
/// le monde, donc `retain` vidait la liste et l'ordre se retrouvait trié par
/// accident. Il faut une section BASSE remaillée seule — retirée du début,
/// rajoutée à la fin — pour que l'ordre se dérange. Le chantier est
/// déterministe, et `Arene::origines` est indexée comme ses lots : deux ordres
/// donneraient deux dispositions d'arène, donc deux images qu'on ne peut plus
/// comparer.
#[test]
fn remplacer_une_seule_section_garde_l_ordre_trie() {
    let (g, f) = petit_monde();
    let mut courant = g.mailler(&f);
    assert!(courant.lots.len() > 2);

    // Le bloc (8, 8, 8) est au MILIEU de la section (0, 0, 0) : son padding
    // ne sort pas, donc la liste visée ne contient qu'elle.
    let vise = tf_mesh::Grille::sections_autour([8, 8, 8], [8, 8, 8]);
    assert_eq!(vise, vec![(0, 0, 0)], "la fixture doit isoler UNE section");
    assert_eq!(
        courant.lots[0].adresse,
        (0, 0, 0),
        "et ce doit être la PREMIÈRE, sinon le retrait ne dérange rien"
    );

    courant.remplacer(&vise, g.mailler_ces(&f, &vise));

    let ordre: Vec<_> = courant.lots.iter().map(|l| l.adresse).collect();
    let mut trie = ordre.clone();
    trie.sort_unstable();
    assert_eq!(ordre, trie, "l'ordre des lots s'est dérangé");
}

/// `retirer` est le pendant de `poser`, et il manquait. Sans lui, rien ne peut
/// enlever une section d'une grille : ce qui a été lu une fois y reste pour
/// toujours, même quand la save ne le porte plus.
#[test]
fn retirer_enleve_la_section_et_ses_biomes() {
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, PIERRE));
    assert!(g.poser_biomes(0, 0, 0, vec![7; tf_mesh::voisinage::VOL_BIOME]));
    assert!(g.section((0, 0, 0)).is_some());

    assert!(g.retirer((0, 0, 0)), "il y en avait une");
    assert!(g.section((0, 0, 0)).is_none());
    // Et le biome part avec : le laisser donnerait la couleur d'une section
    // qui n'existe plus à celle qui prendra sa place.
    assert!(g.poser_biomes(0, 0, 0, vec![9; tf_mesh::voisinage::VOL_BIOME]));

    assert!(!g.retirer((0, 0, 0)), "deux fois ne fait rien");
    assert!(!g.retirer((42, 42, 0)), "et ce qui n'existe pas non plus");
}

// ── la marge de remaillage : une CROIX, pas une boîte ─────────────────────

/// **Une colonne qui arrive en fait remailler CINQ, pas neuf.** C'est tout
/// le gain, et il se compte : les colonnes diagonales n'ont aucune face
/// commune avec celle qui arrive.
#[test]
fn une_colonne_touche_cinq_colonnes_pas_neuf() {
    let (min, max) = ([32, -64, 48], [47, 319, 63]);
    let colonnes = |v: &[(i32, i32, i8)]| {
        let mut c: Vec<(i32, i32)> = v.iter().map(|a| (a.0, a.1)).collect();
        c.sort_unstable();
        c.dedup();
        c
    };
    assert_eq!(colonnes(&Grille::sections_autour(min, max)).len(), 9);
    assert_eq!(
        colonnes(&Grille::sections_touchees(min, max)),
        vec![(1, 3), (2, 2), (2, 3), (2, 4), (3, 3)],
        "la colonne et ses quatre voisines par face"
    );
}

/// **Remailler la croix donne EXACTEMENT le maillage complet**, après des
/// éditions tirées au hasard — et surtout aux ARÊTES et aux COINS des
/// sections, le seul endroit où la croix et la boîte diffèrent.
///
/// C'est le test qui tombera le jour où le mailleur lira ses voisins d'arête
/// ou de coin — l'occlusion ambiante le fera. Ce jour-là il faut revenir à
/// `sections_autour`, pas faire taire le test.
#[test]
fn remailler_la_croix_donne_le_maillage_complet() {
    let t = table();
    let mut g = Grille::new();
    let mut n = 1234u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            for sy in 0..3i8 {
                g.poser(cx, cz, pleine(sy, PIERRE));
            }
        }
    }
    let mut courant = g.mailler(&t);
    for pas in 0..400 {
        // Une coordonnée par axe, tirée aux bords d'une section plus souvent
        // qu'au milieu : 0 et 15 sont là où la croix et la boîte divergent.
        let mut axe = |nb: i32| {
            let s = tirer(nb as u32) as i32;
            let l = match tirer(4) {
                0 => 0,
                1 => 15,
                _ => tirer(16) as i32,
            };
            s * 16 + l
        };
        let (x, y, z) = (axe(3), axe(3), axe(3));
        let (cx, cz, sy) = (x.div_euclid(16), z.div_euclid(16), y.div_euclid(16) as i8);
        // On réécrit la section avec le bloc changé.
        let id = [AIR, PIERRE, DALLE][tirer(3) as usize];
        let ancienne = g.section((cx, cz, sy)).cloned();
        let (lx, ly, lz) = (x.rem_euclid(16), y.rem_euclid(16), z.rem_euclid(16));
        g.poser(
            cx,
            cz,
            section(sy, |bx, by, bz| {
                if (bx, by, bz) == (lx, ly, lz) {
                    id
                } else {
                    ancienne
                        .as_ref()
                        .and_then(|s| s.get(bx as usize, by as usize, bz as usize))
                        .unwrap_or(AIR)
                }
            }),
        );
        let vise = Grille::sections_touchees([x, y, z], [x, y, z]);
        courant.remplacer(&vise, g.mailler_ces(&t, &vise));
        let mut complet = g.mailler(&t);
        complet.trier();
        assert_eq!(
            courant.lots.len(),
            complet.lots.len(),
            "pas {pas} : nombre de lots"
        );
        for (a, b) in courant.lots.iter().zip(complet.lots.iter()) {
            assert_eq!(a.adresse, b.adresse, "pas {pas}");
            assert_eq!(
                a.quads.quads, b.quads.quads,
                "pas {pas}, bloc ({x}, {y}, {z}) : la section {:?} garde des faces \
                 d'avant — elle n'était pas dans la croix",
                a.adresse
            );
            assert_eq!(
                a.poses.poses, b.poses.poses,
                "pas {pas} : poses de {:?}",
                a.adresse
            );
        }
    }
}

// --- Ce qu'un changement de CONTENU oblige à remailler ----------------------

#[test]
fn bords_opaques_dit_quelles_couches_touchent_une_voisine() {
    let t = table();
    let mut g = Grille::new();
    let un_bloc = |x: i32, y: i32, z: i32| {
        section(0, move |bx, by, bz| {
            if (bx, by, bz) == (x, y, z) {
                PIERRE
            } else {
                AIR
            }
        })
    };
    let cas: [((i32, i32, i32), u8); 5] = [
        ((15, 7, 3), Face::PlusX.bit()),
        (
            (0, 0, 0),
            Face::MoinsX.bit() | Face::MoinsY.bit() | Face::MoinsZ.bit(),
        ),
        ((4, 15, 9), Face::PlusY.bit()),
        ((9, 4, 15), Face::PlusZ.bit()),
        ((7, 7, 7), 0),
    ];
    for ((x, y, z), attendu) in cas {
        g.poser(0, 0, un_bloc(x, y, z));
        assert_eq!(
            g.bords_opaques((0, 0, 0), &t),
            attendu,
            "une pierre en ({x}, {y}, {z})"
        );
    }
    g.poser(0, 0, pleine(0, PIERRE));
    assert_eq!(g.bords_opaques((0, 0, 0), &t), 0x3F, "pleine : les six");
    // Une dalle n'est PAS opaque : elle ne cache rien à sa voisine.
    g.poser(0, 0, pleine(0, DALLE));
    assert_eq!(g.bords_opaques((0, 0, 0), &t), 0, "des dalles partout");
    assert_eq!(g.bords_opaques((5, 5, 0), &t), 0, "absente : de l'air");
}

#[test]
fn une_cellule_sans_rien_d_opaque_au_bord_ne_fait_remailler_aucune_voisine() {
    // La croix remaillait les quatre colonnes voisines quoi que la cellule
    // porte à leur contact. Une cellule dont les couches bordières n'ont rien
    // d'opaque ne leur change pourtant rien : pour elles, elle vaut de l'air,
    // comme avant son arrivée.
    let t = table();
    let mut g = Grille::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            for sy in 0..3i8 {
                g.poser(cx, cz, pleine(sy, PIERRE));
            }
        }
    }
    for sy in 0..3i8 {
        g.poser(
            0,
            0,
            section(sy, |x, y, z| {
                let dedans = |v: i32| (1..15).contains(&v);
                if dedans(x) && dedans(z) {
                    PIERRE
                } else if y == 3 {
                    DALLE
                } else {
                    AIR
                }
            }),
        );
    }
    // La colonne du chunk (0, 0), sections 0 à 2.
    let touchees = g.touchees_par_le_contenu([0, 0, 0], [15, 47, 15], &t);
    assert_eq!(
        touchees,
        vec![(0, 0, 0), (0, 0, 1), (0, 0, 2)],
        "rien d'opaque au bord en X ni en Z : aucune voisine — mais le haut et le \
         bas de la colonne sont de la pierre, et leurs voisines hors de la boîte \
         n'existent pas"
    );
    // La même avec UNE pierre contre le bord −X de la section du milieu :
    // exactement la voisine qu'elle touche.
    let milieu = g.section((0, 0, 1)).cloned().expect("posée");
    g.poser(
        0,
        0,
        section(1, |x, y, z| {
            if (x, y, z) == (0, 8, 8) {
                PIERRE
            } else {
                milieu
                    .get(x as usize, y as usize, z as usize)
                    .unwrap_or(AIR)
            }
        }),
    );
    let touchees = g.touchees_par_le_contenu([0, 0, 0], [15, 47, 15], &t);
    assert_eq!(
        touchees,
        vec![(-1, 0, 1), (0, 0, 0), (0, 0, 1), (0, 0, 2)],
        "la seule voisine touchée est celle de −X, à la hauteur de la pierre"
    );
}

#[test]
fn remailler_ce_que_le_contenu_touche_donne_le_maillage_complet() {
    // Le croisement qui rend la réduction VÉRIFIABLE : des cellules entières
    // arrivent, partent et changent au hasard, et remailler l'union de ce que
    // leur contenu touchait avant et touche après doit rendre exactement le
    // maillage complet. Le jour où l'occlusion ambiante lira les voisines
    // d'arête et de coin, ce test rougira : c'est pour ça qu'il existe.
    let t = table();
    let mut g = Grille::new();
    let mut n = 4321u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            for sy in 0..3i8 {
                g.poser(cx, cz, pleine(sy, PIERRE));
            }
        }
    }
    let mut courant = g.mailler(&t);
    let mut voisines_epargnees = 0usize;
    for pas in 0..300 {
        // Une boîte de sections : une section, une colonne, ou deux colonnes.
        let (cx, cz) = (tirer(3) as i32, tirer(3) as i32);
        let (min, max) = match tirer(3) {
            0 => {
                let sy = tirer(3) as i32;
                (
                    [cx * 16, sy * 16, cz * 16],
                    [cx * 16 + 15, sy * 16 + 15, cz * 16 + 15],
                )
            }
            1 => ([cx * 16, 0, cz * 16], [cx * 16 + 15, 47, cz * 16 + 15]),
            _ => ([cx * 16, 0, cz * 16], [cx * 16 + 31, 47, cz * 16 + 15]),
        };
        let avant = g.touchees_par_le_contenu(min, max, &t);
        // Le nouveau contenu, section par section : absente, de l'air, un
        // cœur sans bord, des dalles au bord, ou un tirage qui touche tout.
        for scz in min[2].div_euclid(16)..=max[2].div_euclid(16) {
            for scx in min[0].div_euclid(16)..=max[0].div_euclid(16) {
                for sy in min[1].div_euclid(16)..=max[1].div_euclid(16) {
                    let sy = sy as i8;
                    let mode = tirer(6);
                    let densite = 1 + tirer(8);
                    let graine = tirer(1 << 20);
                    if mode == 0 {
                        g.retirer((scx, scz, sy));
                        continue;
                    }
                    let mut m = graine;
                    g.poser(
                        scx,
                        scz,
                        section(sy, |x, y, z| {
                            m = m.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                            let hasard = (m >> 16) % 10;
                            let bord = [x, y, z].iter().any(|v| *v == 0 || *v == 15);
                            match mode {
                                1 => AIR,
                                2 if bord => AIR,
                                3 if bord => DALLE,
                                2 | 3 => [AIR, PIERRE, DALLE][(hasard % 3) as usize],
                                4 if hasard < densite => PIERRE,
                                4 => AIR,
                                _ => [AIR, PIERRE, DALLE][(hasard % 3) as usize],
                            }
                        }),
                    );
                }
            }
        }
        let apres = g.touchees_par_le_contenu(min, max, &t);
        let mut vise = avant.clone();
        vise.extend(apres.iter().copied());
        vise.sort_unstable();
        vise.dedup();
        let croix = Grille::sections_touchees(min, max);
        voisines_epargnees += croix.len() - vise.len();
        assert!(
            vise.iter().all(|a| croix.contains(a)),
            "pas {pas} : la réduction ne sort jamais de la croix"
        );
        courant.remplacer(&vise, g.mailler_ces(&t, &vise));
        let mut complet = g.mailler(&t);
        complet.trier();
        assert_eq!(
            courant.lots.len(),
            complet.lots.len(),
            "pas {pas} : nombre de lots"
        );
        for (a, b) in courant.lots.iter().zip(complet.lots.iter()) {
            assert_eq!(a.adresse, b.adresse, "pas {pas}");
            assert_eq!(
                a.quads.quads, b.quads.quads,
                "pas {pas}, boîte {min:?}..{max:?} : la section {:?} garde des faces \
                 d'avant — son bord a changé sans qu'on la remaille",
                a.adresse
            );
            assert_eq!(
                a.poses.poses, b.poses.poses,
                "pas {pas} : poses de {:?}",
                a.adresse
            );
        }
    }
    assert!(
        voisines_epargnees > 300,
        "la prémisse : la réduction a vraiment épargné des voisines ({voisines_epargnees})"
    );
}

#[test]
fn ce_qui_manque_vaut_de_l_air_meme_quand_l_etat_zero_est_un_bloc_plein() {
    // Dans l'application, l'identifiant 0 est le premier état DÉCODÉ — du
    // deepslate sur un monde 1.18 — et non l'air : toutes les tables de ces
    // tests posent l'air en 0, et c'est ce qui cachait le défaut. Un
    // remplissage à zéro faisait d'un chunk non chargé un mur opaque et
    // invisible : les faces qui le regardent disparaissaient.
    let mut t = TableFormes::new();
    t.pousser(false, true, Vec::new()); // 0 : un bloc PLEIN
    t.pousser(true, false, Vec::new()); // 1 : l'air
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |x, y, z| if (x, y, z) == (15, 8, 8) { 0 } else { 1 }),
    );
    let c = g.mailler(&t);
    assert_eq!(
        c.quads(),
        6,
        "un bloc seul au bord du chunk, à côté d'un chunk absent : ses SIX faces"
    );
    assert!(
        tf_mesh::Formes::est_air(&t, g.bloc(16, 8, 8)),
        "une case non chargée n'est pas un bloc plein"
    );
    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    assert!(
        !tf_mesh::Formes::opaque(&t, v.get(16, 8, 8)),
        "la peau d'un voisin absent n'est pas opaque"
    );
    // Et le voisinage d'une section ABSENTE elle-même : son intérieur est de
    // l'air, pas un cube plein de l'état 0.
    g.voisinage((5, 5, 0), &mut v);
    assert!(
        tf_mesh::Formes::est_air(&t, v.get(8, 8, 8)),
        "l'intérieur d'une section absente"
    );
}

#[test]
fn des_maillages_tenus_valent_le_chantier_retrie_et_ses_totaux() {
    // `Maillages` remplace la liste triée que la scène filtrait et retriait à
    // chaque remaillage : il doit tenir EXACTEMENT les mêmes lots, dans le
    // même ordre, et des totaux tenus à chaque retrait et chaque ajout égaux à
    // ceux qu'on resommerait — un total qui dérive d'un lot par image finit
    // par mentir de plusieurs mégaoctets à la fenêtre de résidence.
    let t = table();
    let mut g = Grille::new();
    let mut n = 99u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            for sy in 0..2i8 {
                g.poser(cx, cz, pleine(sy, PIERRE));
            }
        }
    }
    let mut liste = g.mailler(&t);
    liste.trier();
    let mut tenus = tf_mesh::Maillages::depuis(g.mailler(&t));
    for pas in 0..200 {
        let (cx, cz, sy) = (tirer(3) as i32, tirer(3) as i32, tirer(2) as i8);
        match tirer(4) {
            0 => {
                g.retirer((cx, cz, sy));
            }
            mode => {
                let graine = tirer(1 << 16);
                let mut m = graine;
                g.poser(
                    cx,
                    cz,
                    section(sy, |_, _, _| {
                        m = m.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                        match ((m >> 16) % 7, mode) {
                            (0, _) | (_, 1) => AIR,
                            (1 | 2, _) => DALLE,
                            _ => PIERRE,
                        }
                    }),
                );
            }
        }
        let p = (cx * 16 + 8, sy as i32 * 16 + 8, cz * 16 + 8);
        let vise = Grille::sections_touchees([p.0, p.1, p.2], [p.0, p.1, p.2]);
        liste.remplacer(&vise, g.mailler_ces(&t, &vise));
        tenus.remplacer(&vise, g.mailler_ces(&t, &vise));
        assert_eq!(
            tenus.maillees(),
            liste.lots.len(),
            "pas {pas} : nombre de lots"
        );
        for (a, b) in tenus.lots().zip(liste.lots.iter()) {
            assert_eq!(a.adresse, b.adresse, "pas {pas} : ordre");
            assert_eq!(
                a.quads.quads, b.quads.quads,
                "pas {pas} : quads de {:?}",
                a.adresse
            );
            assert_eq!(
                a.poses.poses, b.poses.poses,
                "pas {pas} : poses de {:?}",
                a.adresse
            );
        }
        assert_eq!(
            (
                tenus.quads(),
                tenus.poses(),
                tenus.octets(),
                tenus.octets_vive()
            ),
            (
                liste.quads(),
                liste.poses(),
                liste.octets(),
                liste.octets_vive()
            ),
            "pas {pas} : les totaux tenus ont dérivé des totaux resommés"
        );
        let a = (cx, cz, sy);
        assert_eq!(
            tenus.lot(&a).map(|l| l.quads.len()),
            liste
                .lots
                .iter()
                .find(|l| l.adresse == a)
                .map(|l| l.quads.len()),
            "pas {pas} : la recherche par adresse"
        );
    }
    // Un lot neuf pour une adresse qu'on n'a PAS visée : il remplace
    // l'ancien, et l'ancien ne compte plus dans les totaux.
    // (La boucle ne remaille que la section éditée : ses voisines peuvent
    // être en retard, dans les deux structures à la fois. On met d'abord
    // celle-ci à jour.)
    let a = tenus.lots().next().expect("des lots").adresse;
    tenus.remplacer(&[a], g.mailler_ces(&t, &[a]));
    let avant = (tenus.maillees(), tenus.quads(), tenus.octets());
    tenus.remplacer(&[], g.mailler_ces(&t, &[a]));
    assert_eq!(
        (tenus.maillees(), tenus.quads(), tenus.octets()),
        avant,
        "remettre le même lot sans le viser ne doit rien compter deux fois"
    );
}

#[test]
fn mailler_dans_un_extrait_rend_ce_que_rend_la_grille_entiere() {
    // Le maillage part hors du fil principal avec un EXTRAIT de la grille :
    // les sections visées, leurs vingt-six voisines, leurs biomes. Il doit
    // rendre exactement ce que la grille entière rendrait — une voisine
    // oubliée, et un mur de faces fantômes apparaît au bord de l'extrait.
    let mut t = table();
    t.marquer_teinte(PIERRE);
    let mut g = Grille::new();
    let mut n = 7u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..4i32 {
        for cx in 0..4i32 {
            for sy in 0..3i8 {
                let graine = tirer(1 << 16);
                let mut m = graine;
                g.poser(
                    cx,
                    cz,
                    section(sy, |_, _, _| {
                        m = m.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                        [AIR, AIR, PIERRE, DALLE][((m >> 16) % 4) as usize]
                    }),
                );
                // Deux biomes en damier : la teinte coupe les quads, donc un
                // biome perdu en route se verrait.
                let biomes: Vec<StateId> = (0..64u32).map(|i| 10 + (i + graine) % 2).collect();
                assert!(g.poser_biomes(cx, cz, sy, biomes));
            }
        }
    }
    for essai in 0..20 {
        let k = 1 + tirer(6) as usize;
        let mut visees: Vec<(i32, i32, i8)> = (0..k)
            .map(|_| (tirer(4) as i32, tirer(4) as i32, tirer(3) as i8))
            .collect();
        visees.sort_unstable();
        visees.dedup();
        let dans_la_grille = g.mailler_ces(&t, &visees);
        let dans_l_extrait = g.extrait(&visees).mailler_ces(&t, &visees);
        assert_eq!(
            dans_l_extrait.lots.len(),
            dans_la_grille.lots.len(),
            "essai {essai} : nombre de lots"
        );
        for (a, b) in dans_l_extrait.lots.iter().zip(dans_la_grille.lots.iter()) {
            assert_eq!(a.adresse, b.adresse, "essai {essai}");
            assert_eq!(
                a.quads.quads, b.quads.quads,
                "essai {essai} : quads de {:?}",
                a.adresse
            );
            assert_eq!(
                a.poses.poses, b.poses.poses,
                "essai {essai} : poses de {:?}",
                a.adresse
            );
        }
    }
}

// ── Les fluides : ce qu'une eau voisine oblige à remailler ─────────────────

const EAU: StateId = 3;
const EAU_COURANTE: StateId = 4;
const EAU_CHUTE: StateId = 5;
const LAVE: StateId = 6;

/// La table des tests ci-dessus, plus de l'eau et de la lave.
fn table_eau() -> TableFormes {
    let mut t = table();
    for (niveau, genre) in [
        (0, tf_mesh::GenreFluide::Eau),
        (3, tf_mesh::GenreFluide::Eau),
        (8, tf_mesh::GenreFluide::Eau),
        (0, tf_mesh::GenreFluide::Lave),
    ] {
        let id = t.pousser(true, false, Vec::new());
        t.marquer_fluide(id, tf_mesh::Fluide { genre, niveau });
    }
    t
}

#[test]
fn une_section_d_eau_n_est_pas_de_l_air() {
    // Sa palette n'a que de l'eau, que les passes de blocs prennent pour de
    // l'air : la sauter perdrait la surface de la mer.
    let t = table_eau();
    let mut g = Grille::new();
    g.poser(0, 0, pleine(0, EAU));
    g.poser(0, 0, pleine(1, AIR));
    g.poser(0, 0, pleine(-1, PIERRE));
    // Des berges de pierre tout autour : sans elles, les coins du bord
    // plongeraient vers l'air des colonnes voisines — ce que le jeu fait
    // aussi — et la surface ne serait plus plate au bord.
    for dz in -1..=1 {
        for dx in -1..=1 {
            if (dx, dz) != (0, 0) {
                g.poser(dx, dz, pleine(0, PIERRE));
            }
        }
    }
    assert!(!g.sans_contenu((0, 0, 0), &t));
    assert!(g.porte_du_fluide((0, 0, 0), &t));
    assert!(!g.porte_du_fluide((0, 0, 1), &t));
    assert!(g.sans_contenu((0, 0, 1), &t), "l'air reste sauté");
    let c = g.mailler_ces(&t, &[(0, 0, 0)]);
    assert_eq!(c.lots.len(), 1);
    let l = &c.lots[0];
    assert!(l.quads.is_empty() && l.poses.is_empty());
    assert_eq!(l.fluides.len(), 1, "ni côté ni fond : de la pierre partout");
    let surface: Vec<_> = l.fluides.iter().filter(|f| f.face == Face::PlusY).collect();
    assert_eq!(surface.len(), 1, "la surface, d'un seul tenant");
    assert_eq!(surface[0].taille, [16, 16]);
    // Et le lot le compte, au GPU comme en mémoire vive.
    assert_eq!(
        l.octets(),
        l.fluides.len() * tf_mesh::OCTETS_FACE_FLUIDE,
        "vingt octets par face au GPU"
    );
    assert!(l.octets_vive() >= l.fluides.len() * 20);
}

#[test]
fn les_voisines_fluides_sont_la_coquille_qui_porte_de_l_eau() {
    let t = table_eau();
    let mut g = Grille::new();
    // Une coquille de sections autour de (0, 0, 0) : de l'eau en DIAGONALE
    // (arête et coin), juste AU-DESSUS, et de la pierre ailleurs.
    for dz in -1..=1 {
        for dx in -1..=1 {
            for dy in -1..=1i8 {
                let id = match (dx, dy, dz) {
                    (1, 0, 1) | (-1, -1, -1) | (0, 1, 1) | (0, 1, 0) => EAU,
                    _ => PIERRE,
                };
                g.poser(dx, dz, pleine(dy, id));
            }
        }
    }
    let v = g.voisines_fluides([0, 0, 0], [15, 15, 15], &t);
    assert_eq!(v, vec![(-1, -1, -1), (0, 0, 1), (0, 1, 1), (1, 1, 0)]);
    // La section de la boîte n'y est pas, même avec de l'eau : elle est
    // remaillée de toute façon.
    g.poser(0, 0, pleine(0, EAU));
    assert!(!g
        .voisines_fluides([0, 0, 0], [15, 15, 15], &t)
        .contains(&(0, 0, 0)));
    // Et le contenu les ajoute à ce qu'il touche.
    let touchees = g.touchees_par_le_contenu([0, 0, 0], [15, 15, 15], &t);
    for a in [(-1, -1, -1), (0, 1, 1), (1, 1, 0), (0, 0, 0)] {
        assert!(touchees.contains(&a), "{a:?} dans {touchees:?}");
    }
}

/// **À la CASE, pas à la palette.** Une voisine dont l'eau est loin de la
/// boîte n'a aucune surface qui change : la remailler, c'est payer pour rien —
/// et sur un sous-sol 1.18, où presque toute section porte une poche d'eau
/// quelque part, c'était remailler les vingt-six voisines de chaque édition.
#[test]
fn les_voisines_fluides_se_jugent_a_la_case_pas_a_la_palette() {
    let t = table_eau();
    let mut g = Grille::new();
    // La boîte : la section (0, 0, 0) entière. Autour, six voisines dont la
    // palette porte TOUTES de l'eau — mais pas toutes à un bloc d'elle.
    // (1, 0, 1), arête : l'eau au coin OPPOSÉ, à seize blocs.
    g.poser(
        1,
        1,
        section(0, |x, _, z| if (x, z) == (15, 15) { EAU } else { PIERRE }),
    );
    // (−1, 0, 0), face : l'eau contre la boîte.
    g.poser(
        -1,
        0,
        section(0, |x, _, _| if x == 15 { EAU } else { PIERRE }),
    );
    // (0, 0, −1), face : l'eau à DEUX blocs — une case de trop.
    g.poser(0, -1, section(0, |_, _, z| if z == 14 { EAU } else { AIR }));
    // (1, 1, 1), coin : une seule case d'eau, pile au coin.
    g.poser(
        1,
        1,
        section(
            1,
            |x, y, z| if (x, y, z) == (0, 0, 0) { EAU } else { PIERRE },
        ),
    );
    // (0, 1, 0), au-dessus : l'eau sur la couche du bas.
    g.poser(0, 0, section(1, |_, y, _| if y == 0 { EAU } else { AIR }));
    // (0, −1, 0), au-dessous : l'eau loin du plafond.
    g.poser(0, 0, section(-1, |_, y, _| if y == 5 { EAU } else { AIR }));
    for (dx, dz) in [(1, 1), (-1, 0), (0, -1)] {
        assert!(g.porte_du_fluide((dx, dz, 0), &t), "la palette dit « eau »");
    }
    let v = g.voisines_fluides([0, 0, 0], [15, 15, 15], &t);
    assert_eq!(v, vec![(-1, 0, 0), (0, 0, 1), (1, 1, 1)]);
    // Une boîte qui ne touche PAS le bord de sa section : la bande n'atteint
    // aucune voisine, et rien n'est remaillé.
    assert!(g.voisines_fluides([4, 4, 4], [11, 11, 11], &t).is_empty());
}

/// Une table qui COMPTE ce qu'on lui demande d'opacité.
struct Compteur<'a> {
    t: &'a TableFormes,
    lectures: std::cell::Cell<usize>,
}

impl tf_mesh::Formes for Compteur<'_> {
    fn est_air(&self, id: StateId) -> bool {
        self.t.est_air(id)
    }
    fn opaque(&self, id: StateId) -> bool {
        self.lectures.set(self.lectures.get() + 1);
        self.t.opaque(id)
    }
    fn cuboides(&self, id: StateId) -> &[Cuboide] {
        self.t.cuboides(id)
    }
    fn fluide(&self, id: StateId) -> Option<tf_mesh::Fluide> {
        self.t.fluide(id)
    }
    fn solide(&self, id: StateId) -> bool {
        self.t.solide(id)
    }
}

/// **Une section NOYÉE se saute — et elle n'avait rien à dire.** Le raccourci
/// se vérifie contre ce qu'il épargne : on maille quand même le centre, à la
/// main, et tout doit sortir vide. Puis une seule voisine d'une autre sorte,
/// et la section doit reparler.
#[test]
fn une_section_noyee_se_saute_et_n_avait_rien_a_dire() {
    let mut t = table_eau();
    let inonde = t.pousser(false, false, tf_mesh::Formes::cuboides(&t, DALLE).to_vec());
    t.marquer_fluide(
        inonde,
        tf_mesh::Fluide {
            genre: tf_mesh::GenreFluide::Eau,
            niveau: 0,
        },
    );
    // Un cube de 3 × 3 × 3 sections : `voisine` sur les six faces du centre,
    // de l'eau sur les arêtes et les coins.
    let cube = |centre: StateId, voisine: StateId| {
        let mut g = Grille::new();
        for dy in -1..=1i8 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let par_face = (dx != 0) as u8 + (dy != 0) as u8 + (dz != 0) as u8 == 1;
                    let id = match ((dx, dy, dz), par_face) {
                        ((0, 0, 0), _) => centre,
                        (_, true) => voisine,
                        _ => EAU,
                    };
                    g.poser(dx, dz, pleine(dy, id));
                }
            }
        }
        g
    };
    // Le fond de la mer — et de l'eau qui COULE autour : le même fluide.
    let g = cube(EAU, EAU_COURANTE);
    assert!(g.noyee((0, 0, 0), &t));
    // Son lot est VIDE — celui que les passes auraient rendu — mais il
    // existe : sa présence ne dépend que de sa propre palette.
    let ch = g.mailler(&t);
    let lot = ch
        .lots
        .iter()
        .find(|l| l.adresse == (0, 0, 0))
        .expect("la section noyée garde son lot");
    assert!(lot.quads.is_empty() && lot.poses.is_empty() && lot.fluides.is_empty());
    let mut v = Voisinage::new();
    g.voisinage((0, 0, 0), &mut v);
    let mut faces = Vec::new();
    tf_mesh::fluides::mailler(&v, &t, &mut faces);
    assert!(
        faces.is_empty(),
        "{} faces de fluide épargnées à tort",
        faces.len()
    );
    let (quads, poses) = tf_mesh::mailler_pour_gpu(&v, &t);
    assert!(quads.quads.is_empty() && poses.poses.is_empty());
    // Et le raccourci est PRIS : rien ne le montre dans le résultat, qui est
    // le même avec ou sans. Ce qui le montre est ce qu'il n'a pas LU — les
    // passes relèvent l'opacité des 5 832 cases du voisinage, la noyade ne
    // lit que des palettes.
    let compte = Compteur {
        t: &t,
        lectures: std::cell::Cell::new(0),
    };
    g.mailler_ces(&compte, &[(0, 0, 0)]);
    assert!(
        compte.lectures.get() < 64,
        "{} lectures d'opacité pour une section noyée",
        compte.lectures.get()
    );

    for (centre, voisine, quoi) in [
        (EAU, AIR, "de l'air sur une face"),
        (EAU, LAVE, "de la lave sur une face"),
        (
            EAU,
            PIERRE,
            "de la pierre : sous elle, les coins du dessus lisent à côté",
        ),
        (inonde, EAU, "un modèle inondé dedans"),
        (EAU, inonde, "un modèle inondé à côté"),
        (LAVE, EAU, "de la lave dans l'eau"),
    ] {
        assert!(!cube(centre, voisine).noyee((0, 0, 0), &t), "{quoi}");
    }
    // Une voisine ABSENTE vaut de l'air.
    let mut g = cube(EAU, EAU);
    assert!(g.noyee((0, 0, 0), &t));
    g.retirer((0, 0, 1));
    assert!(!g.noyee((0, 0, 0), &t), "le ciel non chargé au-dessus");

    // **Une voisine éditée LOIN de leur frontière** : sa palette change, donc
    // la noyade du centre — mais la croix de l'édition ne le contient pas, et
    // il garde le lot d'avant. Ce lot doit être celui d'un maillage complet :
    // c'est ce qui interdit de rendre `None` pour une section noyée.
    let mut g = cube(EAU, EAU);
    let avant = g.mailler(&t);
    g.poser(
        0,
        0,
        section(1, |x, y, z| if (x, y, z) == (8, 8, 8) { AIR } else { EAU }),
    );
    let visees = Grille::sections_touchees([8, 24, 8], [8, 24, 8]);
    assert!(
        !visees.contains(&(0, 0, 0)),
        "la prémisse : le centre n'est pas remaillé"
    );
    assert!(!g.noyee((0, 0, 0), &t), "et il n'est plus noyé");
    let refait = g.mailler_ces(&t, &visees);
    let resume = |l: &tf_mesh::Lot| (l.adresse, l.quads.len(), l.poses.len(), l.fluides.len());
    let mut partiel: Vec<_> = avant
        .lots
        .iter()
        .filter(|l| !visees.contains(&l.adresse))
        .chain(refait.lots.iter())
        .map(resume)
        .collect();
    partiel.sort();
    let mut complet: Vec<_> = g.mailler(&t).lots.iter().map(resume).collect();
    complet.sort();
    assert_eq!(partiel, complet);
}

/// Une section tirée au hasard, avec de l'eau de toutes sortes — par pavés
/// de 2 × 2 × 2 : case par case, l'eau produirait des milliers de faces par
/// section et le test passerait son temps à les comparer, alors que ce qu'il
/// vérifie est ce qui traverse les FRONTIÈRES. Le détail case par case est
/// l'affaire de `tests/fluides.rs`, contre la référence du jeu.
fn section_humide(sy: i8, graine: u32) -> Section {
    section(sy, |x, y, z| {
        let mut m = graine ^ (((x >> 1) * 73 + (y >> 1) * 179 + (z >> 1) * 283) as u32);
        m = m.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        m = m.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        match (m >> 16) % 20 {
            0..=5 => AIR,
            6..=8 => PIERRE,
            9 => DALLE,
            10..=14 => EAU,
            15 => EAU_COURANTE,
            16 => EAU_CHUTE,
            17 => LAVE,
            _ if y < 8 => EAU,
            _ => AIR,
        }
    })
}

#[test]
fn remailler_la_croix_et_les_voisines_fluides_donne_le_maillage_complet() {
    // Le test de la croix, avec de l'EAU. La croix seule ne suffit plus : un
    // coin de surface lit quatre colonnes et ce qu'il y a au-dessus, donc une
    // section en diagonale change quand un bloc change au coin d'une autre.
    // Avec les voisines fluides, le remaillage partiel est EXACT — et le
    // témoin, la croix seule, doit bel et bien se tromper, sinon le test ne
    // prouverait rien.
    let t = table_eau();
    let mut g = Grille::new();
    let mut n = 97u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            for sy in 0..3i8 {
                g.poser(cx, cz, section_humide(sy, tirer(1 << 20)));
            }
        }
    }
    let mut courant = g.mailler(&t);
    let mut temoin = g.mailler(&t);
    let mut temoin_faux = 0usize;
    let comparer = |c: &tf_mesh::Chantier, complet: &tf_mesh::Chantier| -> Option<String> {
        if c.lots.len() != complet.lots.len() {
            return Some(format!(
                "{} lots contre {}",
                c.lots.len(),
                complet.lots.len()
            ));
        }
        for (a, b) in c.lots.iter().zip(complet.lots.iter()) {
            if a.adresse != b.adresse
                || a.quads.quads != b.quads.quads
                || a.poses.poses != b.poses.poses
                || a.fluides != b.fluides
            {
                return Some(format!("la section {:?}", b.adresse));
            }
        }
        None
    };
    // 150 pas : chacun remaille les 27 sections en entier pour comparer, et
    // des sections d'eau tirée au hasard sont le pire cas de la passe —
    // des milliers de faces chacune.
    for pas in 0..150 {
        // Aux BORDS de section trois fois sur quatre par axe : la dépendance
        // en diagonale ne se voit que sur une arête ou un coin.
        let mut axe = |nb: i32| {
            let s = tirer(nb as u32) as i32;
            let l = match tirer(8) {
                0..=2 => 0,
                3..=5 => 15,
                _ => tirer(16) as i32,
            };
            s * 16 + l
        };
        let (x, y, z) = (axe(3), axe(3), axe(3));
        let (cx, cz, sy) = (x.div_euclid(16), z.div_euclid(16), y.div_euclid(16) as i8);
        let id = [AIR, PIERRE, DALLE, EAU, EAU_COURANTE, EAU_CHUTE, LAVE][tirer(7) as usize];
        let ancienne = g.section((cx, cz, sy)).cloned();
        let (lx, ly, lz) = (x.rem_euclid(16), y.rem_euclid(16), z.rem_euclid(16));
        g.poser(
            cx,
            cz,
            section(sy, |bx, by, bz| {
                if (bx, by, bz) == (lx, ly, lz) {
                    id
                } else {
                    ancienne
                        .as_ref()
                        .and_then(|s| s.get(bx as usize, by as usize, bz as usize))
                        .unwrap_or(AIR)
                }
            }),
        );
        let croix = Grille::sections_touchees([x, y, z], [x, y, z]);
        let mut vise = croix.clone();
        vise.extend(g.voisines_fluides([x, y, z], [x, y, z], &t));
        vise.sort_unstable();
        vise.dedup();
        courant.remplacer(&vise, g.mailler_ces(&t, &vise));
        temoin.remplacer(&croix, g.mailler_ces(&t, &croix));
        let mut complet = g.mailler(&t);
        complet.trier();
        if let Some(e) = comparer(&courant, &complet) {
            panic!("pas {pas}, bloc ({x}, {y}, {z}) : {e} garde un maillage d'avant");
        }
        if comparer(&temoin, &complet).is_some() {
            temoin_faux += 1;
            // Le témoin repart juste, pour que chaque pas compte seul.
            temoin = g.mailler(&t);
            temoin.trier();
        }
    }
    // Mesuré : la croix seule se trompe 14 fois sur 150.
    assert!(
        temoin_faux >= 7,
        "la prémisse : la croix seule se trompe avec de l'eau ({temoin_faux} fois)"
    );
}

#[test]
fn remailler_ce_que_le_contenu_touche_avec_de_l_eau_donne_le_maillage_complet() {
    // Le croisement des cellules qui arrivent et partent, avec de l'eau : la
    // réduction par le contenu garde ses voisines fluides.
    let t = table_eau();
    let mut g = Grille::new();
    let mut n = 2024u32;
    let mut tirer = |borne: u32| {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) % borne
    };
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            for sy in 0..3i8 {
                g.poser(cx, cz, section_humide(sy, tirer(1 << 20)));
            }
        }
    }
    let mut courant = g.mailler(&t);
    for pas in 0..80 {
        let (cx, cz) = (tirer(3) as i32, tirer(3) as i32);
        let sy = tirer(3) as i32;
        let (min, max) = (
            [cx * 16, sy * 16, cz * 16],
            [cx * 16 + 15, sy * 16 + 15, cz * 16 + 15],
        );
        let avant = g.touchees_par_le_contenu(min, max, &t);
        match tirer(4) {
            0 => {
                g.retirer((cx, cz, sy as i8));
            }
            1 => g.poser(cx, cz, pleine(sy as i8, EAU)),
            _ => g.poser(cx, cz, section_humide(sy as i8, tirer(1 << 20))),
        }
        let apres = g.touchees_par_le_contenu(min, max, &t);
        let mut vise = avant;
        vise.extend(apres);
        vise.sort_unstable();
        vise.dedup();
        courant.remplacer(&vise, g.mailler_ces(&t, &vise));
        let mut complet = g.mailler(&t);
        complet.trier();
        assert_eq!(courant.lots.len(), complet.lots.len(), "pas {pas}");
        for (a, b) in courant.lots.iter().zip(complet.lots.iter()) {
            assert_eq!(a.adresse, b.adresse, "pas {pas}");
            assert_eq!(a.quads.quads, b.quads.quads, "pas {pas} : {:?}", a.adresse);
            assert_eq!(a.poses.poses, b.poses.poses, "pas {pas} : {:?}", a.adresse);
            assert_eq!(
                a.fluides, b.fluides,
                "pas {pas} : la surface de {:?} date d'avant",
                a.adresse
            );
        }
    }
}
