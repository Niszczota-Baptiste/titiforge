//! Tests de la section packée.
//!
//! Le bit packing est l'endroit du moteur où une erreur ne plante pas : elle
//! écrit des blocs faux dans la save de quelqu'un. Chaque largeur de bits est
//! donc balayée, et pas seulement « quelques-unes ».

mod common;
use common::fixture::{bits_for as fixture_bits, li, pack, Rng};

use tf_anvil::{bits_for, local_index, Section, MAX_PALETTE, VOL};

fn section_from(palette_len: usize, indices: &[u16]) -> Section {
    let mut s = Section {
        y: 0,
        palette: (0..palette_len as u32).collect(),
        bits: bits_for(palette_len),
        data: Box::new([]),
    };
    s.repack(indices);
    s
}

#[test]
fn bits_suit_la_regle_minecraft_avec_plancher_a_quatre() {
    // Plancher à 4 même pour deux entrées : c'est la règle du jeu, pas une
    // optimisation qu'on pourrait « améliorer ».
    for n in 1..=16 {
        assert_eq!(bits_for(n), 4, "palette de {n} → 4 bits");
    }
    assert_eq!(bits_for(17), 5);
    assert_eq!(bits_for(32), 5);
    assert_eq!(bits_for(33), 6);
    assert_eq!(bits_for(64), 6);
    assert_eq!(bits_for(65), 7);
    assert_eq!(bits_for(256), 8);
    assert_eq!(bits_for(257), 9);
    assert_eq!(bits_for(2048), 11);
    assert_eq!(bits_for(2049), 12);
    assert_eq!(bits_for(MAX_PALETTE), 12);
}

#[test]
fn bits_for_est_d_accord_avec_l_implementation_independante_du_producteur() {
    // Deux implémentations, écrites séparément. Si elles divergent, l'une des
    // deux est fausse — et sans cette comparaison on ne saurait pas laquelle
    // avant qu'un utilisateur ouvre sa save.
    for n in 1..=MAX_PALETTE {
        assert_eq!(bits_for(n) as usize, fixture_bits(n), "palette de {n}");
    }
}

#[test]
fn pack_puis_unpack_rend_les_memes_indices_a_toutes_les_largeurs() {
    let mut rng = Rng::new(12345);
    // Une taille de palette juste sous et juste au-dessus de chaque frontière
    // de bits : c'est là que les erreurs d'arrondi vivent.
    for &n in &[
        2usize, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257, 4095, 4096,
    ] {
        let indices: Vec<u16> = (0..VOL).map(|_| rng.below(n) as u16).collect();
        let s = section_from(n, &indices);
        assert_eq!(s.bits, bits_for(n), "palette de {n}");
        assert_eq!(
            &s.unpack()[..],
            &indices[..],
            "aller-retour sur une palette de {n}"
        );
    }
}

#[test]
fn le_packing_est_compatible_avec_celui_du_producteur_independant() {
    // Le vrai risque : que notre packing soit auto-cohérent mais différent de
    // celui de Minecraft. Le producteur du test implémente la spec de son
    // côté ; les deux suites de longs doivent coïncider à l'octet près.
    let mut rng = Rng::new(999);
    for &n in &[2usize, 16, 17, 64, 100, 256, 1000, 4096] {
        let indices: Vec<u16> = (0..VOL).map(|_| rng.below(n) as u16).collect();
        let s = section_from(n, &indices);
        let attendu = pack(&indices, fixture_bits(n));
        assert_eq!(&s.data[..], &attendu[..], "packing d'une palette de {n}");
    }
}

#[test]
fn l_index_local_est_en_ordre_yzx() {
    // i = y*256 + z*16 + x. Se tromper d'ordre fait tourner tout le build d'un
    // quart de tour, ce qui se voit — mais confondre y et z donne un build
    // couché, ce qui se voit BEAUCOUP moins sur du terrain.
    assert_eq!(local_index(0, 0, 0), 0);
    assert_eq!(local_index(1, 0, 0), 1);
    assert_eq!(local_index(0, 0, 1), 16);
    assert_eq!(local_index(0, 1, 0), 256);
    assert_eq!(local_index(15, 15, 15), 4095);
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                assert_eq!(local_index(x, y, z), li(x, y, z), "({x},{y},{z})");
            }
        }
    }
}

#[test]
fn get_lit_le_bloc_qu_on_a_ecrit() {
    let mut indices = vec![0u16; VOL];
    indices[local_index(3, 7, 11)] = 5;
    indices[local_index(15, 15, 15)] = 9;
    let s = section_from(10, &indices);
    assert_eq!(s.get(3, 7, 11), Some(5));
    assert_eq!(s.get(15, 15, 15), Some(9));
    assert_eq!(s.get(0, 0, 0), Some(0));
}

#[test]
fn une_section_homogene_n_a_pas_de_data() {
    let s = Section::uniform(3, 42);
    assert!(s.is_uniform());
    assert!(s.data.is_empty());
    assert_eq!(s.get(5, 5, 5), Some(42));
    assert_eq!(s.count_of(42), VOL);
    assert_eq!(s.count_of(1), 0);
    assert_eq!(&s.unpack()[..], &[0u16; VOL][..]);
}

#[test]
fn set_uniform_efface_les_indices() {
    let mut rng = Rng::new(7);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(8) as u16).collect();
    let mut s = section_from(8, &indices);
    assert!(!s.data.is_empty());

    s.set_uniform(3);
    assert!(
        s.data.is_empty(),
        "l'étage section supprime le tableau d'indices"
    );
    assert_eq!(s.palette, vec![3]);
    assert_eq!(s.count_of(3), VOL);
}

// ── l'étage palette, et son piège ───────────────────────────────────────────

#[test]
fn replace_state_ne_touche_aucun_indice() {
    let mut rng = Rng::new(4242);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(10) as u16).collect();
    let mut s = section_from(10, &indices);
    let avant = s.data.clone();
    let bits_avant = s.bits;

    assert_eq!(s.replace_state(3, 99), 1);

    assert_eq!(s.data, avant, "les indices ne doivent pas bouger");
    assert_eq!(s.bits, bits_avant, "la largeur de bits ne doit pas bouger");
    assert_eq!(s.palette[3], 99);
}

#[test]
fn replace_state_vers_un_etat_deja_present_cree_un_doublon_assume() {
    // Le cœur de la décision mesurée : on ne fusionne PAS. Fusionner
    // obligerait à remapper 4096 indices, et sur du vrai terrain ça se
    // déclenche sur quasiment toutes les sections.
    let mut indices = vec![0u16; VOL];
    indices[0] = 1; // une case de « terre »
    indices[1] = 2; // une case de « pierre »
    let mut s = section_from(3, &indices); // palette [0, 1, 2]
    let avant = s.data.clone();

    // pierre (id 2) → terre (id 1), alors que la terre est déjà là.
    assert_eq!(s.replace_state(2, 1), 1);

    assert_eq!(s.data, avant, "aucun indice touché malgré la collision");
    assert_eq!(s.palette, vec![0, 1, 1], "le doublon est assumé");
    // Et le comptage doit voir les DEUX entrées.
    assert_eq!(
        s.count_of(1),
        2,
        "count_of doit compter toutes les occurrences"
    );
}

#[test]
fn count_of_compte_toutes_les_occurrences_pas_la_premiere() {
    // La régression la plus dangereuse du dépôt : un `position()` au lieu d'un
    // filtre ferait rater la moitié d'un //replace suivant, en silence.
    let mut indices = vec![0u16; VOL];
    for (i, v) in indices.iter_mut().enumerate().take(300) {
        *v = (i % 3) as u16;
    }
    let mut s = section_from(3, &indices);
    s.replace_state(1, 7);
    s.replace_state(2, 7); // 7 figure maintenant DEUX fois dans la palette

    assert_eq!(s.palette, vec![0, 7, 7]);
    let attendu = indices.iter().filter(|&&v| v == 1 || v == 2).count();
    assert_eq!(s.count_of(7), attendu);
    assert!(attendu > 0);
}

#[test]
fn un_replace_en_chaine_reste_juste() {
    let mut rng = Rng::new(31337);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(6) as u16).collect();
    let mut s = section_from(6, &indices);

    // Tout devient 0, un état à la fois.
    for from in 1..6u32 {
        s.replace_state(from, 0);
    }
    assert_eq!(
        s.count_of(0),
        VOL,
        "après la chaîne, toute la section vaut 0"
    );
    assert_eq!(s.palette, vec![0; 6]);
    assert!(!s.data.is_empty(), "les indices n'ont jamais été touchés");
}

#[test]
fn compact_palette_fusionne_et_retire_l_inutilise() {
    let mut indices = vec![0u16; VOL];
    indices[0] = 1;
    indices[1] = 2;
    // l'entrée 3 n'est référencée par AUCUN indice
    let mut s = section_from(4, &indices);
    s.replace_state(2, 1); // doublon

    let avant_len = s.palette.len();
    let retires = s.compact_palette();
    assert_eq!(avant_len, 4);
    assert_eq!(retires, 2, "un doublon fusionné + une entrée morte retirée");
    assert_eq!(s.palette, vec![0, 1]);

    // Et le contenu est inchangé.
    assert_eq!(s.count_of(1), 2);
    assert_eq!(s.count_of(0), VOL - 2);
}

#[test]
fn compact_palette_ne_reecrit_rien_si_la_palette_est_deja_compacte() {
    let mut rng = Rng::new(2024);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(9) as u16).collect();
    let mut s = section_from(9, &indices);
    let avant = s.data.clone();
    assert_eq!(s.compact_palette(), 0);
    assert_eq!(
        s.data, avant,
        "une palette compacte ne doit pas être repackée"
    );
}

#[test]
fn compact_palette_peut_ramener_une_section_a_l_uniforme() {
    let mut rng = Rng::new(5);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(5) as u16).collect();
    let mut s = section_from(5, &indices);
    for from in 1..5u32 {
        s.replace_state(from, 0);
    }
    s.compact_palette();
    assert!(
        s.is_uniform(),
        "tout vaut le même état : la section redevient homogène"
    );
    assert!(s.data.is_empty());
    assert_eq!(s.palette, vec![0]);
}

#[test]
fn map_blocks_compte_exactement_les_blocs_changes() {
    let mut rng = Rng::new(8080);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(4) as u16).collect();
    let mut s = section_from(4, &indices);

    let attendu = indices.iter().filter(|&&v| v == 2).count();
    let changes = s.map_blocks(|id| if id == 2 { 42 } else { id });
    assert_eq!(changes, attendu);
    assert_eq!(s.count_of(42), attendu);
}

#[test]
fn map_blocks_sur_une_section_homogene() {
    let mut s = Section::uniform(0, 5);
    assert_eq!(s.map_blocks(|id| if id == 5 { 6 } else { id }), VOL);
    assert_eq!(s.count_of(6), VOL);
    assert_eq!(
        s.map_blocks(|id| id),
        0,
        "sans changement, zéro bloc compté"
    );
}

#[test]
fn l_empreinte_packee_reste_sous_un_octet_par_bloc() {
    // Le chiffre qui justifie toute la réécriture. Une palette réaliste de
    // terrain fait une dizaine d'entrées → 4 bits → 2048 octets pour 4096
    // blocs, soit un demi-octet par bloc.
    let mut rng = Rng::new(1);
    let indices: Vec<u16> = (0..VOL).map(|_| rng.below(12) as u16).collect();
    let s = section_from(12, &indices);
    let par_bloc = s.packed_bytes() as f64 / VOL as f64;
    assert!(par_bloc < 1.0, "{par_bloc:.2} o/bloc — on vise moins de 1");
}
