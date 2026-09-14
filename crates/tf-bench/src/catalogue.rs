//! Catalogue Minefield — table ENGENDRÉE, à ne pas éditer à la main.
//!
//! Produite par `cargo run -p tf-assets --example engendrer_catalogue`,
//! qui lit le pack du serveur et le classe avec
//! `tf_assets::catalogue::classer` — le MÊME code que l'application. Un
//! script qui rejouerait la règle de son côté finirait par diverger, et
//! c'est arrivé : 16 désaccords sur 220 blocs, dont onze cubes que le pack
//! dit translucides.
//!
//! Ce ne sont que des NOMS et des formes — ni textures ni modèles ne sont
//! recopiés ici. C'est ce qui permet de construire un build réaliste sans
//! redistribuer quoi que ce soit.
//!
//! ## Pourquoi une table réelle plutôt que `bloc_0 … bloc_n`
//!
//! Les fixtures de la phase 0 (pierre et minerais, cubes pleins, sections
//! homogènes) sont justes pour mesurer le chargement Anvil et **mentent**
//! pour le maillage : elles donnent 100 % de cubes pleins là où la cible en
//! a un tiers. Les longueurs de nom comptent aussi — une palette de trente
//! entrées à 31 caractères ne pèse pas ce qu'une palette de
//! `minecraft:stone` pèse.
//!
//! ## Ce que l'échantillon préserve
//!
//! | | pack | ici |
//! |---|---:|---:|
//! | `Modele` | 1236 / 1678 (73.7 %) | 162 / 220 (73.6 %) |
//! | `Cube` | 422 / 1678 (25.1 %) | 55 / 220 (25.0 %) |
//! | `Vide` | 20 / 1678 (1.2 %) | 3 / 220 (1.4 %) |
//! | cuboïdes par modèle, moyenne | 3.52 | 3.31 |
//! | cuboïdes par modèle, médiane | 1 | 1 |
//! | le pire | 82 | 30 |
//!
//! Un `Cube` que ses textures TROUENT compte comme modèle : le verre remplit
//! son bloc et ne masque personne. Seules les faces du cuboïde qui remplit
//! la case entrent dans ce jugement — la couche d'herbe transparente de
//! `grass_block` ne rend pas le terrain translucide.

/// Ce qu'un bloc oppose au mailleur.
///
/// C'est la SEULE chose que le maillage a besoin de savoir d'un bloc. Un cube
/// plein opaque bouche sa case, donc masque les faces de ses voisins et se
/// fond dans un quad glouton ; tout le reste doit être dessiné cuboïde par
/// cuboïde et ne masque rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Forme {
    /// Un cuboïde remplit le bloc, et ses textures ne le trouent pas.
    Cube,
    /// Des cuboïdes partiels, ou un cube que ses textures trouent.
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
    ("minefield:acacia_barrel", Forme::Cube, 1),
    ("minefield:acacia_central_leg_table", Forme::Modele, 2),
    ("minefield:algae_acacia_planks", Forme::Cube, 1),
    ("minefield:algae_dark_oak_planks", Forme::Cube, 1),
    ("minefield:algae_oak_log", Forme::Cube, 1),
    ("minefield:armorer_stone_sign", Forme::Modele, 1),
    ("minefield:aspidistra", Forme::Modele, 2),
    ("minefield:birch_carved_log_0", Forme::Cube, 1),
    ("minefield:birch_carved_log_1", Forme::Cube, 1),
    ("minefield:birch_corner_leg_table", Forme::Modele, 2),
    ("minefield:birch_large_vase", Forme::Modele, 2),
    ("minefield:birch_log_quarter_block", Forme::Modele, 1),
    ("minefield:birch_log_slab", Forme::Modele, 1),
    ("minefield:black_colored_glass", Forme::Modele, 1),
    (
        "minefield:black_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:black_marble_flat_stairs", Forme::Modele, 15),
    ("minefield:black_marble_large_bricks", Forme::Cube, 1),
    ("minefield:black_smoke", Forme::Vide, 0),
    ("minefield:blue_cross_colored_glass_pane", Forme::Modele, 1),
    ("minefield:blue_terracotta_brick_stairs", Forme::Modele, 3),
    ("minefield:blue_wood_lantern", Forme::Modele, 10),
    ("minefield:brass_fence", Forme::Modele, 1),
    ("minefield:brass_quarter_block", Forme::Modele, 1),
    ("minefield:brown_colored_glass", Forme::Modele, 1),
    (
        "minefield:brown_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:brown_leaves", Forme::Modele, 1),
    ("minefield:brown_terracotta_slab", Forme::Modele, 1),
    ("minefield:cast_obsidian_large_bricks", Forme::Cube, 1),
    ("minefield:cast_obsidian_large_vase", Forme::Modele, 2),
    (
        "minefield:chiseled_black_marble_double_cross",
        Forme::Cube,
        1,
    ),
    ("minefield:chiseled_red_sandstone_lozenge", Forme::Cube, 1),
    ("minefield:chiseled_ruby_block", Forme::Cube, 1),
    ("minefield:chiseled_sandstone_double_cross", Forme::Cube, 1),
    (
        "minefield:chiseled_sandstone_double_lozenge",
        Forme::Cube,
        1,
    ),
    ("minefield:chiseled_sandstone_lozenge", Forme::Cube, 1),
    ("minefield:copper_block", Forme::Cube, 1),
    ("minefield:copper_low_vase", Forme::Modele, 2),
    ("minefield:copper_structure", Forme::Modele, 1),
    ("minefield:cracked_stone_brick_stairs", Forme::Modele, 3),
    ("minefield:cyan_colored_glass_slab", Forme::Modele, 1),
    ("minefield:cyan_copper_lantern", Forme::Modele, 10),
    ("minefield:cyan_terracotta_brick_slab", Forme::Modele, 1),
    ("minefield:cyan_terracotta_brick_stairs", Forme::Modele, 3),
    ("minefield:cyan_wool_slab", Forme::Modele, 1),
    ("minefield:dark_oak_carved_log_8", Forme::Cube, 1),
    ("minefield:dark_oak_log_slab", Forme::Modele, 1),
    ("minefield:dark_oak_low_table", Forme::Modele, 5),
    (
        "minefield:dark_oak_structure_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:dark_oak_trapdoor", Forme::Modele, 1),
    ("minefield:egyptian_grave_1", Forme::Modele, 3),
    ("minefield:engineer_stone_sign", Forme::Modele, 1),
    ("minefield:engineer_wood_sign", Forme::Modele, 1),
    ("minefield:fallen_orange_leaves_2", Forme::Modele, 1),
    ("minefield:farmer_stone_sign", Forme::Modele, 1),
    ("minefield:farmer_wood_sign", Forme::Modele, 1),
    ("minefield:fireworks_15", Forme::Cube, 1),
    ("minefield:fisherman_stone_sign", Forme::Modele, 1),
    ("minefield:gold_armor_rack", Forme::Vide, 0),
    ("minefield:gold_fence_gate", Forme::Modele, 7),
    ("minefield:gold_strut", Forme::Modele, 3),
    ("minefield:gray_colored_glass_slab", Forme::Modele, 1),
    ("minefield:gray_cross_colored_glass_pane", Forme::Modele, 1),
    ("minefield:green_bird_skull", Forme::Modele, 2),
    ("minefield:green_colored_glass", Forme::Modele, 1),
    ("minefield:green_cross_colored_glass", Forme::Modele, 1),
    ("minefield:inkwell_with_feather", Forme::Modele, 9),
    (
        "minefield:inlaid_black_marble_circle_sapphire",
        Forme::Cube,
        1,
    ),
    (
        "minefield:inlaid_black_marble_cross_lozenge_jade",
        Forme::Cube,
        1,
    ),
    ("minefield:inlaid_black_marble_square_empty", Forme::Cube, 1),
    (
        "minefield:inlaid_red_sandstone_circle_empty",
        Forme::Cube,
        1,
    ),
    (
        "minefield:inlaid_stone_bricks_cross_lozenge_empty",
        Forme::Cube,
        1,
    ),
    (
        "minefield:inlaid_stone_bricks_cross_lozenge_ruby",
        Forme::Cube,
        1,
    ),
    ("minefield:inlaid_white_marble_circle_jade", Forme::Cube, 1),
    ("minefield:inlaid_white_marble_circle_ruby", Forme::Cube, 1),
    (
        "minefield:inlaid_white_marble_cross_lozenge_jade",
        Forme::Cube,
        1,
    ),
    ("minefield:inlaid_white_marble_square_empty", Forme::Cube, 1),
    ("minefield:inlaid_white_marble_square_jade", Forme::Cube, 1),
    (
        "minefield:inlaid_white_marble_square_sapphire",
        Forme::Cube,
        1,
    ),
    ("minefield:iron_structure", Forme::Modele, 1),
    ("minefield:iron_strut", Forme::Modele, 3),
    ("minefield:jungle_barrel", Forme::Cube, 1),
    ("minefield:jungle_carved_log_7", Forme::Cube, 1),
    ("minefield:jungle_door", Forme::Modele, 1),
    ("minefield:jungle_low_vase", Forme::Modele, 2),
    ("minefield:jungle_stool", Forme::Modele, 5),
    ("minefield:jungle_wine_barrel", Forme::Modele, 30),
    ("minefield:leaves_1", Forme::Modele, 1),
    ("minefield:leaves_3", Forme::Modele, 1),
    ("minefield:light_blue_brass_lantern", Forme::Modele, 10),
    (
        "minefield:light_blue_cross_colored_glass_slab",
        Forme::Modele,
        1,
    ),
    ("minefield:light_blue_gold_lantern", Forme::Modele, 10),
    (
        "minefield:light_blue_terracotta_brick_stairs",
        Forme::Modele,
        3,
    ),
    ("minefield:light_blue_terracotta_stairs", Forme::Modele, 3),
    ("minefield:light_blue_wool_slab", Forme::Modele, 1),
    ("minefield:light_gray_coral", Forme::Modele, 2),
    ("minefield:light_gray_terracotta_slab", Forme::Modele, 1),
    ("minefield:lime_colored_glass", Forme::Modele, 1),
    ("minefield:lime_colored_glass_pane", Forme::Modele, 1),
    ("minefield:lime_terracotta_quarter_block", Forme::Modele, 1),
    ("minefield:lime_wood_lantern", Forme::Modele, 10),
    ("minefield:lumberjack_stone_sign", Forme::Modele, 1),
    (
        "minefield:magenta_cross_colored_glass_pane",
        Forme::Modele,
        1,
    ),
    (
        "minefield:magenta_terracotta_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:magenta_terracotta_brick_stairs",
        Forme::Modele,
        3,
    ),
    (
        "minefield:magenta_terracotta_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:magenta_terracotta_stairs", Forme::Modele, 3),
    ("minefield:magenta_wool_quarter_block", Forme::Modele, 1),
    ("minefield:mason_furnace", Forme::Modele, 8),
    ("minefield:metro_e", Forme::Cube, 1),
    ("minefield:metro_f", Forme::Cube, 1),
    ("minefield:minetoy_antenio", Forme::Modele, 1),
    ("minefield:minetoy_dokmixer", Forme::Modele, 1),
    ("minefield:oak_barrel", Forme::Cube, 1),
    ("minefield:oak_bench_end", Forme::Modele, 3),
    ("minefield:oak_carved_log_3", Forme::Cube, 1),
    ("minefield:oak_carved_log_5", Forme::Cube, 1),
    ("minefield:oak_carved_log_7", Forme::Cube, 1),
    ("minefield:oak_central_leg_table", Forme::Modele, 2),
    ("minefield:oak_crate_diagonal", Forme::Cube, 1),
    ("minefield:oak_structure_slab", Forme::Modele, 1),
    ("minefield:oak_strut", Forme::Modele, 3),
    ("minefield:oak_wood_stairs", Forme::Modele, 3),
    ("minefield:orange_brass_lantern", Forme::Modele, 10),
    (
        "minefield:orange_cross_colored_glass_slab",
        Forme::Modele,
        1,
    ),
    ("minefield:orange_leaves_slab", Forme::Modele, 1),
    (
        "minefield:oxidized_brass_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:oxidized_brass_quarter_block", Forme::Modele, 1),
    ("minefield:oxidized_brass_slab", Forme::Modele, 1),
    ("minefield:oxidized_brass_wall", Forme::Modele, 1),
    (
        "minefield:oxidized_copper_large_brick_stairs",
        Forme::Modele,
        3,
    ),
    ("minefield:oxidized_copper_stairs", Forme::Modele, 3),
    ("minefield:paper_block_3", Forme::Cube, 1),
    ("minefield:paper_block_4", Forme::Cube, 1),
    (
        "minefield:patterned_marble_brick_2_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:patterned_marble_brick_7_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:patterned_marble_brick_slab_8", Forme::Modele, 1),
    ("minefield:patterned_marble_bricks_1", Forme::Cube, 1),
    ("minefield:patterned_marble_bricks_3", Forme::Cube, 1),
    ("minefield:patterned_marble_bricks_7", Forme::Cube, 1),
    ("minefield:patterned_marble_bricks_8", Forme::Cube, 1),
    ("minefield:pink_colored_glass_pane", Forme::Modele, 1),
    ("minefield:pink_coral", Forme::Modele, 2),
    (
        "minefield:pink_cross_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:pink_smoke", Forme::Vide, 0),
    ("minefield:placeable_black_open_book", Forme::Modele, 4),
    ("minefield:placeable_diamond_axe", Forme::Modele, 8),
    ("minefield:placeable_golden_pickaxe", Forme::Modele, 12),
    ("minefield:placeable_green_open_book", Forme::Modele, 4),
    ("minefield:placeable_harpoon", Forme::Modele, 6),
    ("minefield:placeable_iron_chisel", Forme::Modele, 4),
    (
        "minefield:placeable_iron_hammer_and_chisel",
        Forme::Modele,
        10,
    ),
    ("minefield:placeable_magenta_open_book", Forme::Modele, 4),
    ("minefield:placeable_obsidian_axe", Forme::Modele, 11),
    ("minefield:placeable_orange_open_book", Forme::Modele, 4),
    ("minefield:placeable_purple_closed_book", Forme::Modele, 4),
    ("minefield:placeable_red_closed_book", Forme::Modele, 4),
    ("minefield:placeable_stone_hoe", Forme::Modele, 5),
    ("minefield:placeable_wooden_hoe", Forme::Modele, 5),
    ("minefield:placeable_yellow_closed_book", Forme::Modele, 4),
    ("minefield:purple_copper_lantern", Forme::Modele, 10),
    (
        "minefield:purple_terracotta_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:purple_wool_slab", Forme::Modele, 1),
    ("minefield:purple_wool_stairs", Forme::Modele, 3),
    ("minefield:raw_jade_block", Forme::Cube, 1),
    ("minefield:red_bird_skull", Forme::Modele, 2),
    ("minefield:red_brass_lantern", Forme::Modele, 10),
    ("minefield:red_colored_glass", Forme::Modele, 1),
    (
        "minefield:red_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:red_cross_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:red_easter_egg", Forme::Modele, 18),
    ("minefield:red_leaves", Forme::Modele, 1),
    ("minefield:red_steel_lantern", Forme::Modele, 10),
    ("minefield:red_terracotta_slab", Forme::Modele, 1),
    ("minefield:rope", Forme::Modele, 1),
    (
        "minefield:smooth_black_marble_flat_stairs",
        Forme::Modele,
        15,
    ),
    ("minefield:smooth_black_marble_wall", Forme::Modele, 1),
    ("minefield:smooth_stone_large_vase", Forme::Modele, 2),
    ("minefield:spruce_carved_log_2", Forme::Cube, 1),
    ("minefield:spruce_carved_log_7", Forme::Cube, 1),
    ("minefield:spruce_chair", Forme::Modele, 7),
    ("minefield:spruce_structure_stairs", Forme::Modele, 3),
    ("minefield:spruce_wood_slab", Forme::Modele, 1),
    ("minefield:steel_brick_wall", Forme::Modele, 1),
    (
        "minefield:steel_large_brick_quarter_block",
        Forme::Modele,
        1,
    ),
    ("minefield:steel_large_brick_slab", Forme::Modele, 1),
    ("minefield:steel_structure_quarter_block", Forme::Modele, 1),
    ("minefield:steel_strut", Forme::Modele, 3),
    ("minefield:steel_vase", Forme::Modele, 2),
    ("minefield:stone_flat_stairs", Forme::Modele, 15),
    ("minefield:stone_ladder", Forme::Modele, 23),
    ("minefield:stone_quarter_block", Forme::Modele, 1),
    ("minefield:storage_diamond", Forme::Cube, 1),
    ("minefield:storage_iron_ore", Forme::Cube, 1),
    ("minefield:storage_lapis_ore", Forme::Cube, 1),
    ("minefield:storage_redsand", Forme::Cube, 1),
    ("minefield:storage_redstone_ore", Forme::Cube, 1),
    ("minefield:storage_ruby_block", Forme::Cube, 1),
    ("minefield:straw", Forme::Cube, 1),
    ("minefield:straw_stairs", Forme::Modele, 3),
    ("minefield:thorns_2", Forme::Modele, 1),
    ("minefield:very_oxidized_brass_bricks", Forme::Cube, 1),
    ("minefield:very_oxidized_brass_door", Forme::Modele, 1),
    ("minefield:very_oxidized_brass_slab", Forme::Modele, 1),
    ("minefield:very_oxidized_brass_strut", Forme::Modele, 3),
    ("minefield:very_oxidized_brass_wall", Forme::Modele, 1),
    ("minefield:very_oxidized_copper_slab", Forme::Modele, 1),
    ("minefield:very_oxidized_copper_wall", Forme::Modele, 1),
    ("minefield:white_colored_glass", Forme::Modele, 1),
    ("minefield:white_colored_glass_pane", Forme::Modele, 1),
    ("minefield:white_marble_slab", Forme::Modele, 1),
    ("minefield:white_steel_lantern", Forme::Modele, 10),
    ("minefield:white_terracotta_brick_slab", Forme::Modele, 1),
    ("minefield:white_terracotta_brick_stairs", Forme::Modele, 3),
    ("minefield:yellow_bird_skull", Forme::Modele, 2),
    ("minefield:yellow_colored_glass_pane", Forme::Modele, 1),
    (
        "minefield:yellow_cross_colored_glass_quarter_block",
        Forme::Modele,
        1,
    ),
    (
        "minefield:yellow_cross_colored_glass_slab",
        Forme::Modele,
        1,
    ),
    ("minefield:yellow_leaves_quarter_block", Forme::Modele, 1),
    ("minefield:yellow_steel_lantern", Forme::Modele, 10),
    ("minefield:yellow_terracotta_brick_slab", Forme::Modele, 1),
];

/// Le bloc le plus lourd du serveur : **82 cuboïdes** dans une seule case.
///
/// Il n'est PAS dans `BLOCS`, et c'est délibéré. Un sur 1236, l'y glisser
/// tirerait la moyenne et ferait porter le pire cas par tous les chiffres.
/// Les benchs moyens utilisent `BLOCS` ; le bench du pire cas utilise
/// celui-ci.
pub const PIRE_CAS: (&str, Forme, u8) = ("minefield:red_pumpkin_treat_bag", Forme::Modele, 82);

/// Nombre moyen de cuboïdes d'un bloc-modèle DANS LE PACK, × 100.
///
/// Figé ici pour qu'un test puisse refuser un échantillon qui dérive. Une
/// fixture dont la forme s'éloigne de la cible mesure autre chose que la
/// cible, et ne le dit pas.
pub const CUBOIDES_MOYENS_CODEX: u32 = 352;
