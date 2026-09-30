//! **Un chunk vide et fini** — le seul que ce dépôt s'autorise à créer, dans
//! un monde plat dont les couches ne sont que de l'air.
//!
//! Relu par le décodeur GELÉ : un chunk qu'on écrit de toutes pièces, sans
//! modèle que le jeu aurait produit, ne se vérifie pas par le code qui l'a
//! écrit.

mod common;
use common::frozen::{self, Tag};

use tf_anvil::{chunk_vide, decode_section, scan, Interner, DV_1_18, SECTIONS_SURFACE};

const DV_1_18_2: i32 = 2975;

/// Ce que le jeu lit pour ACCEPTER le chunk tel quel : fini, à sa place, de
/// la bonne version, sans lumière (il la refera) et sans cartes de hauteur
/// (il les refera) — et vingt-quatre sections d'air, du bon biome.
#[test]
fn un_chunk_vide_se_relit_par_un_decodeur_independant() {
    let nbt = chunk_vide(-3, 7, DV_1_18_2, "minecraft:the_void").unwrap();
    let (_, racine) = frozen::parse_nbt(&nbt);
    assert_eq!(
        racine.get("DataVersion").and_then(Tag::as_i32),
        Some(DV_1_18_2)
    );
    assert_eq!(racine.get("xPos").and_then(Tag::as_i32), Some(-3));
    assert_eq!(racine.get("zPos").and_then(Tag::as_i32), Some(7));
    assert_eq!(racine.get("yPos").and_then(Tag::as_i32), Some(-4));
    assert_eq!(
        racine.get("Status").and_then(Tag::as_str),
        Some("minecraft:full"),
        "FINI : sinon le jeu générerait par-dessus"
    );
    assert_eq!(racine.get("isLightOn").and_then(Tag::as_i8), Some(0));
    assert!(racine.get("Heightmaps").is_none(), "le jeu les refera");
    assert_eq!(
        racine
            .get("block_entities")
            .and_then(Tag::as_list)
            .map(Vec::len),
        Some(0)
    );

    let sections = racine.get("sections").and_then(Tag::as_list).unwrap();
    let ys: Vec<i8> = sections
        .iter()
        .map(|s| s.get("Y").and_then(Tag::as_i8).unwrap())
        .collect();
    assert_eq!(ys, SECTIONS_SURFACE.collect::<Vec<_>>());
    for s in sections {
        let etats = frozen::section_states(s).unwrap();
        assert!(etats.iter().all(|e| e == "minecraft:air"));
        let biomes = s
            .get("biomes")
            .and_then(|b| b.get("palette"))
            .and_then(Tag::as_list)
            .unwrap();
        assert_eq!(biomes, &vec![Tag::Str("minecraft:the_void".into())]);
    }
}

/// Et le moteur le lit comme un chunk FINI de 1.18, section par section.
#[test]
fn un_chunk_vide_se_balaye_comme_un_chunk_fini() {
    let nbt = chunk_vide(0, 0, DV_1_18_2, "minecraft:plains").unwrap();
    let sc = scan(&nbt).unwrap();
    assert!(!sc.incomplet);
    assert_eq!(
        (sc.x_pos, sc.z_pos, sc.data_version),
        (Some(0), Some(0), DV_1_18_2)
    );
    assert_eq!(sc.sections.len(), 24);
    let mut i = Interner::new();
    for s in &sc.sections {
        let sec = decode_section(&nbt, &sc, s, &mut i).unwrap().unwrap();
        assert_eq!(sec.palette.len(), 1);
        assert_eq!(i.resolve(sec.palette[0]), Some("minecraft:air"));
    }
}

/// Avant 1.18, pas de chunk vide : l'ancien format range tout sous `Level`,
/// et ne descend pas sous zéro — on n'écrit pas ce qu'on n'a jamais relu.
#[test]
fn avant_1_18_on_ne_cree_rien() {
    assert!(chunk_vide(0, 0, DV_1_18 - 1, "minecraft:the_void").is_none());
    assert!(chunk_vide(0, 0, DV_1_18, "minecraft:the_void").is_some());
}
