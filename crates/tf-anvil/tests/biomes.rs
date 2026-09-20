//! Les biomes — la SECONDE palette d'une section.
//!
//! Depuis 1.18 un biome est une propriété de la section, rangée exactement
//! comme les blocs : palette plus indices packés. Trois différences, et les
//! trois cassent quelque chose si on copie le code des blocs :
//!
//! 1. la palette est une liste de **chaînes**, pas de compounds ;
//! 2. il n'y a que **64 cellules**, une pour 4 × 4 × 4 blocs ;
//! 3. il n'y a **pas de plancher à 4 bits** — cinq biomes se lisent sur trois.
//!
//! La troisième est la plus vicieuse : un plancher à 4 ferait écrire quatre
//! longs au lieu de trois, ce qui est un tableau valide pour personne, et le
//! chunk ne se charge plus. Rien ne le signale à l'écriture.

mod common;
use common::fixture::{self, SectionSpec};
use common::frozen;

use tf_anvil::{biome_edits, bits_biome, decode_biomes, scan, splice, Biomes, Interner, VOL_BIOME};

fn chunk(y: i8) -> Vec<u8> {
    fixture::chunk_nbt(0, 0, &[SectionSpec::uniform(y, "minecraft:stone")])
}

fn lire(brut: &[u8], i: &mut Interner) -> Biomes {
    let s = scan(brut).unwrap();
    decode_biomes(brut, &s.sections[0], i)
        .unwrap()
        .expect("la section porte des biomes")
}

// ── la largeur d'indice ─────────────────────────────────────────────────────

#[test]
fn un_biome_n_a_pas_de_plancher_a_quatre_bits() {
    assert_eq!(bits_biome(1), 1, "monobiome : un bit, et aucun `data`");
    assert_eq!(bits_biome(2), 1);
    assert_eq!(bits_biome(3), 2);
    assert_eq!(bits_biome(5), 3, "cinq biomes tiennent sur trois bits");
    assert_eq!(bits_biome(16), 4);
    assert_eq!(bits_biome(17), 5);
    // 64 cellules, donc au plus 64 biomes distincts.
    assert_eq!(bits_biome(64), 6);
    assert_eq!(bits_biome(1000), 6, "plafonné, pas débordé");
}

#[test]
fn la_cellule_couvre_quatre_blocs_par_axe() {
    assert_eq!(Biomes::cellule_de_bloc(0, 0, 0), (0, 0, 0));
    assert_eq!(Biomes::cellule_de_bloc(3, 3, 3), (0, 0, 0));
    assert_eq!(Biomes::cellule_de_bloc(4, 0, 0), (1, 0, 0));
    assert_eq!(Biomes::cellule_de_bloc(15, 15, 15), (3, 3, 3));
    // L'ordre est YZX, comme partout, mais sur quatre.
    assert_eq!(Biomes::index(0, 0, 0), 0);
    assert_eq!(Biomes::index(1, 0, 0), 1);
    assert_eq!(Biomes::index(0, 0, 1), 4);
    assert_eq!(Biomes::index(0, 1, 0), 16);
    assert_eq!(Biomes::index(3, 3, 3), VOL_BIOME - 1);
}

// ── la lecture ──────────────────────────────────────────────────────────────

#[test]
fn une_section_multibiome_se_relit_cellule_par_cellule() {
    let brut = chunk(1); // 1 % 3 != 0 → cinq biomes
    let mut i = Interner::new();
    let b = lire(&brut, &mut i);
    assert_eq!(b.palette.len(), 5);
    assert_eq!(b.bits, 3, "cinq entrées, trois bits");
    assert!(!b.est_uniforme());

    // La fixture pose la cellule `n` sur le biome `n % 5`.
    let noms = b.noms(&i).unwrap();
    for c in 0..VOL_BIOME {
        let (x, y, z) = (c & 3, c >> 4, (c >> 2) & 3);
        let vu = b.get(x, y, z).unwrap();
        let rang = b.palette.iter().position(|&p| p == vu).unwrap();
        assert_eq!(rang, c % 5, "cellule {c} ({x},{y},{z})");
    }
    assert_eq!(noms[0], "minecraft:plains");
}

#[test]
fn une_section_monobiome_n_a_pas_de_data() {
    let brut = chunk(0); // 0 % 3 == 0 → un seul biome
    let mut i = Interner::new();
    let b = lire(&brut, &mut i);
    assert_eq!(b.palette.len(), 1);
    assert!(b.est_uniforme());
    assert!(b.data.is_empty());
    assert_eq!(b.get(2, 2, 2), b.palette.first().copied());
}

#[test]
fn un_chunk_1_17_ne_rend_aucun_biome_plutot_que_de_mauvais() {
    // 1.13–1.17 range ses biomes sous `Level`, en tableau d'entiers sans
    // palette. Les lire avec ce décodeur écrirait la carte des biomes de
    // travers sur toute une save : on préfère ne rien rendre.
    let brut =
        fixture::legacy_chunk_nbt(0, 0, &[SectionSpec::uniform(0, "minecraft:stone")], false);
    let s = scan(&brut).unwrap();
    assert!(s.sections[0].biomes.is_none());
    let mut i = Interner::new();
    assert!(decode_biomes(&brut, &s.sections[0], &mut i)
        .unwrap()
        .is_none());
}

// ── l'écriture ──────────────────────────────────────────────────────────────

#[test]
fn reecrire_les_memes_biomes_ne_produit_aucune_edition() {
    let brut = chunk(1);
    let mut i = Interner::new();
    let b = lire(&brut, &mut i);
    let s = scan(&brut).unwrap();
    assert!(biome_edits(&brut, &b, &s.sections[0], &i)
        .unwrap()
        .is_empty());
}

#[test]
fn passer_a_un_seul_biome_fait_disparaitre_le_data() {
    // Le jeu n'écrit pas de `data` pour une palette d'une entrée, et en
    // laisser un de la mauvaise longueur casse le chargement du chunk.
    let brut = chunk(1);
    let mut i = Interner::new();
    let mut b = lire(&brut, &mut i);
    let desert = i.intern("minecraft:desert");
    b.set_uniforme(desert);

    let s = scan(&brut).unwrap();
    let mut edits = biome_edits(&brut, &b, &s.sections[0], &i).unwrap();
    let apres = splice(&brut, &mut edits).unwrap();

    let (_, arbre) = frozen::parse_nbt(&apres);
    let sec = &arbre.get("sections").unwrap().as_list().unwrap()[0];
    let bio = sec.get("biomes").unwrap();
    let pal = bio.get("palette").unwrap().as_list().unwrap();
    assert_eq!(pal.len(), 1);
    assert_eq!(pal[0].as_str(), Some("minecraft:desert"));
    assert!(
        bio.get("data").is_none(),
        "le champ `data` doit disparaître"
    );
    // Et les blocs d'à côté n'ont pas bougé.
    assert!(sec.get("block_states").unwrap().get("palette").is_some());
    assert_eq!(
        sec.get("modtest:donnees_de_section").unwrap().as_bytes(),
        Some(&vec![0xDE, 0xAD, 0xBE, 0xEF])
    );
}

#[test]
fn ajouter_un_biome_a_une_section_monobiome_cree_le_data() {
    let brut = chunk(0); // monobiome : pas de `data` à remplacer
    let mut i = Interner::new();
    let mut b = lire(&brut, &mut i);
    assert!(b.data.is_empty());

    let neige = i.intern("minecraft:snowy_plains");
    b.palette.push(neige);
    let mut idx = vec![0u16; VOL_BIOME];
    idx[Biomes::index(1, 2, 3)] = 1;
    b.repack(&idx);
    assert_eq!(b.bits, 1, "deux entrées : un bit");
    assert!(!b.data.is_empty());

    let s = scan(&brut).unwrap();
    let mut edits = biome_edits(&brut, &b, &s.sections[0], &i).unwrap();
    let apres = splice(&brut, &mut edits).unwrap();

    // On relit avec NOTRE décodeur : la cellule doit revenir où on l'a mise.
    let mut j = Interner::new();
    let relu = lire(&apres, &mut j);
    assert_eq!(relu.palette.len(), 2);
    assert_eq!(relu.noms(&j).unwrap()[1], "minecraft:snowy_plains");
    assert_eq!(relu.get(1, 2, 3), relu.palette.get(1).copied());
    assert_eq!(relu.get(0, 0, 0), relu.palette.first().copied());

    // Et avec le décodeur INDÉPENDANT : la longueur du tableau est celle que
    // trois bits — pardon, UN bit — exigent, pas celle d'un plancher à 4.
    let (_, arbre) = frozen::parse_nbt(&apres);
    let bio = arbre.get("sections").unwrap().as_list().unwrap()[0]
        .get("biomes")
        .unwrap();
    assert_eq!(bio.get("palette").unwrap().as_list().unwrap().len(), 2);
    assert_eq!(
        bio.get("data").unwrap().as_longs().unwrap().len(),
        1,
        "64 cellules sur 1 bit tiennent dans UN long ; un plancher à 4 bits en écrirait quatre"
    );
}

/// Une palette de biomes qui n'est pas une liste de CHAÎNES est un format
/// qu'on ne comprend pas. On n'y touche pas : la lire comme si de rien
/// n'était rendrait des noms pris au hasard dans les octets voisins, et
/// l'écriture les figerait dans la save.
#[test]
fn une_palette_de_biomes_qui_n_est_pas_des_chaines_est_ignoree() {
    let mut n = fixture::Nbt::new();
    n.field(common::fixture::t::COMPOUND, "");
    n.field(common::fixture::t::LIST, "sections")
        .list(common::fixture::t::COMPOUND, 1);
    n.field(common::fixture::t::BYTE, "Y").i8v(0);
    n.field(common::fixture::t::COMPOUND, "biomes");
    // Des COMPOUNDS, comme une palette de blocs — pas ce qu'Anvil met ici.
    n.field(common::fixture::t::LIST, "palette")
        .list(common::fixture::t::COMPOUND, 1);
    n.field(common::fixture::t::STRING, "Name")
        .strv("minecraft:plains");
    n.end();
    n.end(); // biomes
    n.end(); // la section
    n.end(); // racine

    let s = scan(&n.b).unwrap();
    let mut i = Interner::new();
    assert_eq!(
        decode_biomes(&n.b, &s.sections[0], &mut i).unwrap(),
        None,
        "un type de palette inattendu ne se devine pas"
    );
}

#[test]
fn un_tableau_de_la_mauvaise_longueur_est_refuse() {
    // Un `.mca` tronqué ou forgé en porte. Deviner écrirait la carte des
    // biomes de travers, et rien ne le signalerait.
    let mut n = fixture::Nbt::new();
    n.field(common::fixture::t::COMPOUND, "");
    n.field(common::fixture::t::LIST, "sections")
        .list(common::fixture::t::COMPOUND, 1);
    n.field(common::fixture::t::BYTE, "Y").i8v(0);
    n.field(common::fixture::t::COMPOUND, "biomes");
    n.field(common::fixture::t::LIST, "palette")
        .list(common::fixture::t::STRING, 2);
    n.strv("minecraft:plains");
    n.strv("minecraft:desert");
    // Deux entrées → 1 bit → 1 long attendu. On en écrit sept.
    n.field(common::fixture::t::LONG_ARRAY, "data")
        .longs(&[0; 7]);
    n.end(); // biomes
    n.end(); // la section
    n.end(); // racine

    let s = scan(&n.b).unwrap();
    let mut i = Interner::new();
    assert!(
        decode_biomes(&n.b, &s.sections[0], &mut i).is_err(),
        "une longueur qui ne correspond à rien doit être refusée, pas devinée"
    );
}

#[test]
fn les_biomes_traversent_un_aller_retour_de_region() {
    // La chaîne complète : région → chunk → biomes → édition → splice →
    // région, relue par le décodeur indépendant.
    let mut i = Interner::new();
    let brut = chunk(1);
    let mut b = lire(&brut, &mut i);
    let ocean = i.intern("minecraft:ocean");
    let mut idx = b.unpack().to_vec();
    b.palette.push(ocean);
    let rang = (b.palette.len() - 1) as u16;
    for c in idx.iter_mut().take(8) {
        *c = rang;
    }
    b.repack(&idx);

    let s = scan(&brut).unwrap();
    let mut edits = biome_edits(&brut, &b, &s.sections[0], &i).unwrap();
    let apres = splice(&brut, &mut edits).unwrap();
    let chunks = vec![(0u32, 0u32, apres.clone())];
    let region = fixture::region_file(&chunks, 3);

    let lu = frozen::decode_region(&region);
    let c = lu.get(&(0, 0)).expect("le chunk est là");
    let bio = c.root.get("sections").unwrap().as_list().unwrap()[0]
        .get("biomes")
        .unwrap();
    let pal: Vec<&str> = bio
        .get("palette")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(pal.len(), 6);
    assert_eq!(pal[5], "minecraft:ocean");
    // Six entrées → 3 bits → 4 longs (21 cellules par long, 64/21 = 4).
    assert_eq!(bio.get("data").unwrap().as_longs().unwrap().len(), 4);

    // Et notre propre décodeur retrouve les huit premières cellules.
    let mut j = Interner::new();
    let relu = lire(&apres, &mut j);
    let ocean_relu = relu.palette[5];
    for cell in 0..8usize {
        let (x, y, z) = (cell & 3, cell >> 4, (cell >> 2) & 3);
        assert_eq!(relu.get(x, y, z), Some(ocean_relu), "cellule {cell}");
    }
}
