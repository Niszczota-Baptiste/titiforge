//! Catalogue Minefield — table ENGENDRÉE, à ne pas éditer à la main.
//!
//! Source : `titisite/public/codex/blockstates.json` croisé avec `models/` et
//! `render-models/`, sur les **1678** blocs `minefield:*` du serveur. Régénérer
//! avec le script cité dans `docs/fixtures.md`.
//!
//! Ce ne sont que des NOMS et des formes — ni textures ni modèles ne sont
//! recopiés ici. C'est ce qui permet de construire un build réaliste sans
//! redistribuer quoi que ce soit.
//!
//! ## Pourquoi une table réelle plutôt que `bloc_0 … bloc_n`
//!
//! Les fixtures de la phase 0 (pierre et minerais, cubes pleins, sections
//! homogènes) sont justes pour mesurer le chargement Anvil et **mentent** pour
//! le maillage : elles donnent 100 % de cubes pleins là où la cible en a un
//! tiers. Mesurer le rendu dessus mesurerait le mauvais chemin.
//!
//! Les longueurs de nom comptent aussi : une palette de trente entrées à
//! 31 caractères ne pèse pas ce qu'une palette de `minecraft:stone` pèse.
//!
//! ## Ce que l'échantillon préserve
//!
//! | | codex | ici |
//! |---|---:|---:|
//! | `Modele` | 1121 / 1678 (66.8 %) | 147 / 220 (66.8 %) |
//! | `Cube` | 537 / 1678 (32.0 %) | 70 / 220 (31.8 %) |
//! | `Vide` | 20 / 1678 (1.2 %) | 3 / 220 (1.4 %) |
//! | cuboïdes par modèle, moyenne | 3.58 | 3.31 |
//! | cuboïdes par modèle, médiane | 2 | 2 |
//! | le pire | 82 | 30 |
//!
//! La stratification porte sur le NOMBRE DE CUBOÏDES, proportionnellement.
//! Un premier échantillon forçait les plus lourds en tête : moyenne 7,58 contre
//! 3,58, soit un bench deux fois plus pessimiste que la cible. Une fixture
//! fausse dans le sens prudent reste fausse — elle ferait rejeter une
//! optimisation qui suffisait.

/// Ce qu'un bloc oppose au mailleur.
///
/// C'est la SEULE chose que le maillage a besoin de savoir d'un bloc. La
/// distinction n'est pas esthétique : un cube plein opaque bouche sa case, donc
/// masque les faces de ses voisins et se fond dans un quad glouton ; tout le
/// reste doit être dessiné cuboïde par cuboïde et ne masque rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Forme {
    /// Un cuboïde remplit le bloc de 0 à 16 sur les trois axes.
    Cube,
    /// Des cuboïdes partiels : escalier, dalle, chaise, tombe. Le nombre est
    /// celui du modèle réel — c'est lui qui dimensionne la passe de modèles.
    Modele,
    /// Aucun élément : fumées, effets. Rien à mailler.
    Vide,
}

impl Forme {
    /// Un bloc-modèle n'est JAMAIS opaque. Le marquer opaque supprimerait les
    /// faces de ses voisins : un escalier creuserait un trou dans le mur qu'il
    /// touche.
    pub const fn opaque(self) -> bool {
        matches!(self, Forme::Cube)
    }
}

/// `(nom, forme, nombre de cuboïdes)`.
pub const BLOCS: &[(&str, Forme, u8)] = &[
    ("minefield:acacia_carved_log_3", Forme::Cube, 1),
    ("minefield:acacia_carved_log_5", Forme::Cube, 1),
    ("minefield:acacia_corner_leg_table", Forme::Modele, 2),
    ("minefield:acacia_crate_cross_diagonal", Forme::Cube, 1),
    ("minefield:acacia_strut", Forme::Modele, 3),
    ("minefield:algae_birch_log", Forme::Cube, 1),
    ("minefield:algae_dark_oak_planks", Forme::Cube, 1),
    ("minefield:algae_snow", Forme::Cube, 1),
    ("minefield:andesite_ladder", Forme::Modele, 23),
    ("minefield:armorer_stone_sign", Forme::Cube, 1),
    ("minefield:big_candle_with_cup", Forme::Modele, 9),
    ("minefield:birch_carved_log_6", Forme::Cube, 1),
    ("minefield:birch_door", Forme::Modele, 1),
    ("minefield:birch_shelf", Forme::Modele, 4),
    ("minefield:black_bird_skull", Forme::Modele, 2),
    ("minefield:black_colored_glass_slab", Forme::Modele, 1),
    ("minefield:black_copper_lantern", Forme::Modele, 10),
    ("minefield:black_cross_colored_glass_pane", Forme::Modele, 1),
    ("minefield:black_cross_colored_glass_slab", Forme::Modele, 1),
    ("minefield:black_paper_lantern", Forme::Modele, 1),
    ("minefield:blank_stone_sign", Forme::Cube, 1),
    ("minefield:blue_brass_lantern", Forme::Modele, 10),
    ("minefield:blue_colored_glass_pane", Forme::Modele, 1),
    ("minefield:blue_cross_colored_glass_pane", Forme::Modele, 1),
    ("minefield:blue_wool_stairs", Forme::Modele, 2),
    ("minefield:brass_brick_quarter_block", Forme::Modele, 1),
    ("minefield:brass_slab", Forme::Modele, 1),
    ("minefield:brass_strut", Forme::Modele, 3),
    ("minefield:brick_flat_stairs", Forme::Modele, 8),
    (
        "minefield:brown_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:brown_cross_colored_glass_pane", Forme::Modele, 1),
    ("minefield:brown_terracotta_bricks", Forme::Cube, 1),
    ("minefield:cast_obsidian", Forme::Cube, 1),
    (
        "minefield:cast_obsidian_large_brick_stairs",
        Forme::Modele,
        2,
    ),
    ("minefield:cast_obsidian_low_vase", Forme::Modele, 2),
    (
        "minefield:chiseled_red_sandstone_double_cross",
        Forme::Cube,
        1,
    ),
    (
        "minefield:chiseled_red_sandstone_hieroglyphics",
        Forme::Cube,
        1,
    ),
    ("minefield:chiseled_sandstone_flat", Forme::Cube, 1),
    ("minefield:chiseled_sandstone_hieroglyphics", Forme::Cube, 1),
    ("minefield:chiseled_sandstone_nine_dots", Forme::Cube, 1),
    ("minefield:chiseled_stone_bricks_flat", Forme::Cube, 1),
    (
        "minefield:chiseled_white_marble_double_cross",
        Forme::Cube,
        1,
    ),
    ("minefield:clover_7", Forme::Modele, 1),
    ("minefield:copper_brick_slab", Forme::Modele, 1),
    ("minefield:copper_coins_4", Forme::Modele, 1),
    ("minefield:copper_fence_gate", Forme::Modele, 7),
    ("minefield:copper_large_brick_slab", Forme::Modele, 1),
    ("minefield:copper_large_vase", Forme::Modele, 2),
    ("minefield:copper_trapdoor", Forme::Modele, 1),
    ("minefield:cyan_colored_glass", Forme::Cube, 1),
    ("minefield:cyan_colored_glass_slab", Forme::Modele, 1),
    ("minefield:cyan_terracotta_brick_stairs", Forme::Modele, 2),
    ("minefield:dark_oak_carved_log_6", Forme::Cube, 1),
    ("minefield:dark_oak_low_table", Forme::Modele, 5),
    ("minefield:dark_oak_structure_slab", Forme::Modele, 1),
    ("minefield:dark_oak_strut", Forme::Modele, 3),
    ("minefield:dark_oak_wine_barrel", Forme::Modele, 30),
    ("minefield:dipteris", Forme::Modele, 2),
    ("minefield:edelweiss", Forme::Modele, 2),
    ("minefield:fallen_magenta_leaves_1", Forme::Modele, 1),
    ("minefield:fallen_magenta_leaves_3", Forme::Modele, 1),
    ("minefield:fallen_magenta_leaves_4", Forme::Modele, 1),
    ("minefield:fallen_pink_leaves_4", Forme::Modele, 1),
    ("minefield:fallen_red_leaves_1", Forme::Modele, 1),
    ("minefield:fallen_red_leaves_3", Forme::Modele, 1),
    ("minefield:fallen_yellow_leaves_1", Forme::Modele, 1),
    ("minefield:fireworks_5", Forme::Cube, 1),
    ("minefield:gold_armor_rack", Forme::Vide, 0),
    ("minefield:gold_trapdoor", Forme::Modele, 1),
    ("minefield:gray_steel_lantern", Forme::Modele, 10),
    ("minefield:gray_wood_lantern", Forme::Modele, 10),
    ("minefield:green_colored_glass", Forme::Cube, 1),
    (
        "minefield:green_terracotta_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:green_terracotta_quarter_block", Forme::Modele, 1),
    ("minefield:ice_slab", Forme::Modele, 1),
    (
        "minefield:inlaid_black_marble_circle_sapphire",
        Forme::Cube,
        1,
    ),
    (
        "minefield:inlaid_black_marble_cross_lozenge_sapphire",
        Forme::Cube,
        1,
    ),
    (
        "minefield:inlaid_sandstone_cross_lozenge_ruby",
        Forme::Cube,
        1,
    ),
    ("minefield:inlaid_sandstone_square_jade", Forme::Cube, 1),
    ("minefield:inlaid_sandstone_square_sapphire", Forme::Cube, 1),
    ("minefield:inlaid_stone_bricks_circle_jade", Forme::Cube, 1),
    (
        "minefield:inlaid_stone_bricks_cross_lozenge_ruby",
        Forme::Cube,
        1,
    ),
    ("minefield:inlaid_white_marble_circle_jade", Forme::Cube, 1),
    (
        "minefield:inlaid_white_marble_cross_lozenge_jade",
        Forme::Cube,
        1,
    ),
    ("minefield:iron_quarter_block", Forme::Modele, 1),
    ("minefield:iron_strut", Forme::Modele, 3),
    ("minefield:jeweler_wood_sign", Forme::Cube, 1),
    ("minefield:jungle_carved_log_7", Forme::Cube, 1),
    ("minefield:jungle_leaves_quarter_block", Forme::Modele, 1),
    ("minefield:jungle_quarter_block", Forme::Modele, 1),
    ("minefield:jungle_structure_quarter_block", Forme::Modele, 1),
    ("minefield:leaves_3", Forme::Modele, 1),
    ("minefield:light_blue_brass_lantern", Forme::Modele, 10),
    ("minefield:light_blue_coral", Forme::Modele, 2),
    (
        "minefield:light_blue_cross_colored_glass_slab",
        Forme::Modele,
        1,
    ),
    ("minefield:light_blue_smoke", Forme::Vide, 0),
    ("minefield:light_blue_wool_quarter_block", Forme::Modele, 1),
    ("minefield:light_blue_wool_stairs", Forme::Modele, 2),
    ("minefield:light_gray_colored_glass_slab", Forme::Modele, 1),
    ("minefield:light_gray_gold_lantern", Forme::Modele, 10),
    (
        "minefield:light_gray_terracotta_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:lime_copper_lantern", Forme::Modele, 10),
    ("minefield:lime_terracotta_quarter_block", Forme::Modele, 1),
    ("minefield:lumberjack_wood_sign", Forme::Cube, 1),
    ("minefield:magenta_steel_lantern", Forme::Modele, 10),
    ("minefield:metro_d", Forme::Cube, 1),
    ("minefield:metro_f", Forme::Cube, 1),
    ("minefield:minetoy_neolinkp6", Forme::Cube, 1),
    ("minefield:minetoy_neymir", Forme::Cube, 1),
    ("minefield:minetoy_yann291", Forme::Cube, 1),
    ("minefield:minetoy_yuna_moon", Forme::Cube, 1),
    ("minefield:mob_eater_squid", Forme::Modele, 3),
    ("minefield:mob_eater_wolf", Forme::Modele, 3),
    ("minefield:mob_eater_zombified_piglin", Forme::Modele, 3),
    ("minefield:mycelium_slab", Forme::Modele, 1),
    ("minefield:oak_barrel", Forme::Cube, 1),
    ("minefield:oak_chair", Forme::Modele, 7),
    ("minefield:oak_corner_leg_table", Forme::Modele, 2),
    ("minefield:oak_empty_bookcase", Forme::Modele, 7),
    ("minefield:oak_log_slab", Forme::Modele, 1),
    ("minefield:oak_stool", Forme::Modele, 5),
    ("minefield:oak_table_top", Forme::Modele, 1),
    ("minefield:oak_wood_stairs", Forme::Modele, 2),
    ("minefield:orange_coral", Forme::Modele, 2),
    ("minefield:orange_leaves_quarter_block", Forme::Modele, 1),
    ("minefield:oxidized_copper_door", Forme::Modele, 1),
    (
        "minefield:oxidized_copper_large_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:oxidized_copper_large_brick_slab",
        Forme::Modele,
        1,
    ),
    ("minefield:oxidized_copper_slab", Forme::Modele, 1),
    ("minefield:oxidized_copper_wall", Forme::Modele, 1),
    ("minefield:packed_ice_quarter_block", Forme::Modele, 1),
    ("minefield:paper_block_1", Forme::Cube, 1),
    ("minefield:paper_block_10", Forme::Cube, 1),
    ("minefield:paper_block_12", Forme::Cube, 1),
    ("minefield:paper_block_3", Forme::Cube, 1),
    (
        "minefield:patterned_marble_brick_6_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:patterned_marble_brick_slab_4", Forme::Modele, 1),
    ("minefield:patterned_marble_bricks_2", Forme::Cube, 1),
    ("minefield:patterned_marble_bricks_3", Forme::Cube, 1),
    ("minefield:pebble_4", Forme::Modele, 1),
    ("minefield:pebble_7", Forme::Modele, 1),
    ("minefield:pebble_8", Forme::Modele, 1),
    ("minefield:pine_cone_3", Forme::Modele, 1),
    ("minefield:pink_cross_colored_glass", Forme::Cube, 1),
    ("minefield:pink_gold_lantern", Forme::Modele, 10),
    ("minefield:pink_leaves", Forme::Cube, 1),
    ("minefield:pink_rose", Forme::Modele, 2),
    ("minefield:pink_terracotta_slab", Forme::Modele, 1),
    ("minefield:placeable_blue_closed_book", Forme::Modele, 4),
    ("minefield:placeable_bow", Forme::Modele, 8),
    ("minefield:placeable_brown_closed_book", Forme::Modele, 4),
    ("minefield:placeable_brown_open_book", Forme::Modele, 4),
    ("minefield:placeable_diamond_axe", Forme::Modele, 8),
    (
        "minefield:placeable_diamond_hammer_and_chisel",
        Forme::Modele,
        10,
    ),
    ("minefield:placeable_golden_axe", Forme::Modele, 8),
    ("minefield:placeable_golden_hoe", Forme::Modele, 5),
    ("minefield:placeable_golden_shovel", Forme::Modele, 6),
    ("minefield:placeable_green_closed_book", Forme::Modele, 4),
    ("minefield:placeable_obsidian_chisel", Forme::Modele, 4),
    ("minefield:placeable_white_closed_book", Forme::Modele, 4),
    ("minefield:placeable_wooden_chisel", Forme::Modele, 4),
    ("minefield:plate", Forme::Modele, 12),
    ("minefield:podzol_slab", Forme::Modele, 1),
    ("minefield:portal_diagonal", Forme::Modele, 1),
    ("minefield:ptedirium", Forme::Modele, 2),
    ("minefield:pulley", Forme::Cube, 1),
    ("minefield:purple_terracotta_brick_stairs", Forme::Modele, 2),
    ("minefield:purple_terracotta_stairs", Forme::Modele, 2),
    ("minefield:purple_wool_quarter_block", Forme::Modele, 1),
    ("minefield:raw_citrine_block", Forme::Cube, 1),
    ("minefield:red_colored_glass_slab", Forme::Modele, 1),
    ("minefield:red_coral", Forme::Modele, 2),
    (
        "minefield:red_cross_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:red_easter_egg", Forme::Modele, 18),
    ("minefield:red_leaves_slab", Forme::Modele, 1),
    ("minefield:smooth_stone_low_vase", Forme::Modele, 2),
    ("minefield:snow_brick_stairs", Forme::Modele, 2),
    ("minefield:spruce_barrel", Forme::Cube, 1),
    ("minefield:spruce_carved_log_9", Forme::Cube, 1),
    ("minefield:spruce_ladder", Forme::Modele, 1),
    ("minefield:spruce_low_table", Forme::Modele, 5),
    ("minefield:spruce_low_vase", Forme::Modele, 2),
    ("minefield:spruce_structure_stairs", Forme::Modele, 2),
    ("minefield:steel_armor_rack", Forme::Vide, 0),
    (
        "minefield:steel_large_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:steel_large_brick_stairs", Forme::Modele, 2),
    ("minefield:steel_large_bricks", Forme::Cube, 1),
    ("minefield:steel_large_vase", Forme::Modele, 2),
    ("minefield:steel_low_vase", Forme::Modele, 2),
    ("minefield:steel_structure", Forme::Cube, 1),
    ("minefield:steel_wall", Forme::Modele, 1),
    ("minefield:stone_grave_1", Forme::Modele, 11),
    ("minefield:storage_acacia", Forme::Cube, 1),
    ("minefield:storage_brass", Forme::Cube, 1),
    ("minefield:storage_copper", Forme::Cube, 1),
    ("minefield:storage_crate_acacia", Forme::Cube, 1),
    ("minefield:storage_crate_birch", Forme::Cube, 1),
    ("minefield:storage_diamond_ore", Forme::Cube, 1),
    ("minefield:storage_iron_block", Forme::Cube, 1),
    ("minefield:storage_iron_ore", Forme::Cube, 1),
    ("minefield:storage_log_jungle", Forme::Cube, 1),
    ("minefield:storage_log_spruce", Forme::Cube, 1),
    ("minefield:storage_oxidized_copper", Forme::Cube, 1),
    ("minefield:storage_workbench", Forme::Cube, 1),
    ("minefield:thick_chain", Forme::Modele, 2),
    ("minefield:thorns_3", Forme::Modele, 1),
    ("minefield:vertical_rope", Forme::Modele, 1),
    ("minefield:very_oxidized_brass_fence", Forme::Modele, 1),
    ("minefield:very_oxidized_brass_vase", Forme::Modele, 2),
    (
        "minefield:very_oxidized_copper_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:very_oxidized_copper_brick_wall",
        Forme::Modele,
        1,
    ),
    ("minefield:very_oxidized_copper_stairs", Forme::Modele, 2),
    ("minefield:white_colored_glass", Forme::Cube, 1),
    ("minefield:white_cross_colored_glass", Forme::Cube, 1),
    ("minefield:white_terracotta_bricks", Forme::Cube, 1),
    ("minefield:white_wood_lantern", Forme::Modele, 10),
    (
        "minefield:yellow_cross_colored_glass_pane",
        Forme::Modele,
        1,
    ),
    ("minefield:yellow_leaves_slab", Forme::Modele, 1),
    ("minefield:yellow_leaves_stairs", Forme::Modele, 2),
    ("minefield:yellow_wood_lantern", Forme::Modele, 10),
];

/// Le bloc le plus lourd du serveur : **82 cuboïdes** dans une seule case.
///
/// Il n'est PAS dans `BLOCS`, et c'est délibéré. Un sur 1 121, l'y glisser
/// tirerait la moyenne de 3,58 à 4,12 et ferait porter le pire cas par tous les
/// chiffres — alors qu'il mérite sa propre mesure. Les benchs moyens utilisent
/// `BLOCS` ; le bench du pire cas utilise celui-ci.
pub const PIRE_CAS: (&str, Forme, u8) = ("minefield:red_pumpkin_treat_bag", Forme::Modele, 82);

/// Nombre moyen de cuboïdes d'un bloc-modèle DANS LE CODEX, × 100.
///
/// Figé ici pour qu'un test puisse refuser un échantillon qui dérive. Une
/// fixture dont la forme s'éloigne de la cible mesure autre chose que la cible,
/// et ne le dit pas.
pub const CUBOIDES_MOYENS_CODEX: u32 = 358;
