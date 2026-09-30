//! **Un chunk VIDE et fini** — celui que le jeu génère dans un monde plat
//! dont les couches ne sont que de l'air (le préréglage « The Void »).
//!
//! C'est le seul chunk que ce dépôt s'autorise à CRÉER. Partout ailleurs, une
//! opération n'engendre pas de chunk : on ne sait pas générer le terrain, et
//! un chunk vide posé au milieu d'un monde serait un trou dans le paysage.
//! Dans un monde vide, le terrain EST du vide : le chunk écrit ici est celui
//! que le jeu aurait écrit, à nos blocs près.
//!
//! Ce qu'on n'y met pas, le jeu le refait au chargement, par les mêmes
//! mécanismes que pour un chunk édité : `isLightOn` à 0 pour l'éclairage, pas
//! de `Heightmaps` pour les cartes de hauteur.

use tf_nbt::{tag, PaletteEntryRef, Writer};

/// Le premier `DataVersion` du format 1.18 : les sections à la racine, la
/// surface de −64 à 319.
pub const DV_1_18: i32 = 2860;

/// Les sections d'un chunk de SURFACE depuis 1.18 : de −4 à 19.
pub const SECTIONS_SURFACE: std::ops::RangeInclusive<i8> = -4..=19;

/// Un chunk vide et FINI, inflaté, aux coordonnées `(cx, cz)`.
///
/// `biome` est celui du monde plat — le jeu remplirait ses sections de ce
/// biome-là, et une teinte d'eau ou d'herbe en dépend. `None` avant 1.18 :
/// l'ancien format range tout sous `Level` et ne descend pas sous zéro, et
/// aucun monde de ces versions n'a été relu pour vérifier ce qu'on écrirait.
pub fn chunk_vide(cx: i32, cz: i32, data_version: i32, biome: &str) -> Option<Vec<u8>> {
    if data_version < DV_1_18 {
        return None;
    }
    let air = [PaletteEntryRef {
        name: "minecraft:air",
        props: &[],
    }];
    let blocs = tf_nbt::block_states_payload(&air, &[]);
    let biomes = tf_nbt::biomes_payload(&[biome], &[]);

    let mut w = Writer::with_capacity(96 + 24 * (blocs.len() + biomes.len() + 32));
    w.field(tag::COMPOUND, "");
    w.field(tag::INT, "DataVersion").i32_payload(data_version);
    w.field(tag::INT, "xPos").i32_payload(cx);
    w.field(tag::INT, "yPos")
        .i32_payload(*SECTIONS_SURFACE.start() as i32);
    w.field(tag::INT, "zPos").i32_payload(cz);
    // FINI : c'est ce qui dit au jeu de ne rien générer par-dessus.
    w.field(tag::STRING, "Status").raw_str("minecraft:full");
    w.field(tag::LONG, "LastUpdate").i64_payload(0);
    w.field(tag::LONG, "InhabitedTime").i64_payload(0);
    w.field(tag::BYTE, "isLightOn").i8_payload(0);
    w.field(tag::LIST, "sections");
    w.list_header(tag::COMPOUND, SECTIONS_SURFACE.count());
    for y in SECTIONS_SURFACE {
        w.field(tag::BYTE, "Y").i8_payload(y);
        w.field(tag::COMPOUND, "block_states").raw(&blocs);
        w.field(tag::COMPOUND, "biomes").raw(&biomes);
        w.end();
    }
    // Une liste vide porte le type `TAG_End` : c'est ce que le jeu écrit.
    w.field(tag::LIST, "block_entities")
        .list_header(tag::END, 0);
    w.field(tag::COMPOUND, "structures");
    w.field(tag::COMPOUND, "References").end();
    w.field(tag::COMPOUND, "starts").end();
    w.end();
    w.end();
    Some(w.into_bytes())
}
