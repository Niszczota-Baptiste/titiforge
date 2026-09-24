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
