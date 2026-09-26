//! Quels états portent un FLUIDE, et de quelles textures il s'habille.
//!
//! Le pack ne dit rien des fluides : `block/water` est un modèle sans
//! élément, et le jeu dessine l'eau avec son propre code. Ce qu'on sait d'un
//! fluide vient donc de l'ÉTAT — `level` pour l'eau et la lave,
//! `waterlogged=true` pour tout ce qui s'inonde — et de quelques blocs que le
//! jeu déclare pleins d'eau en dur. Ce n'est pas une liste de blocs à tenir :
//! un bloc `minefield:*` inondable porte `waterlogged` comme les autres.

use tf_mesh::{Fluide, GenreFluide, TextureFluide};

use crate::atlas::Atlas;

/// Les blocs TOUJOURS pleins d'eau, sans propriété qui le dise : le jeu leur
/// rend une source en dur (`getFluidState`). Varech, herbes marines, colonne
/// de bulles — le fond de chaque océan.
pub const TOUJOURS_INONDES: [&str; 5] = [
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:seagrass",
    "minecraft:tall_seagrass",
    "minecraft:bubble_column",
];

pub const EAU_IMMOBILE: &str = "minecraft:block/water_still";
pub const EAU_COURANTE: &str = "minecraft:block/water_flow";
/// Le voile que le jeu pose contre le verre et les feuilles. Absent du codex
/// du site — aucun modèle ne le cite — et présent dans tout `.jar` : le repli
/// est la texture de courant (`couche`).
pub const EAU_VOILE: &str = "minecraft:block/water_overlay";
pub const LAVE_IMMOBILE: &str = "minecraft:block/lava_still";
pub const LAVE_COURANTE: &str = "minecraft:block/lava_flow";

/// Le fluide que porte un état, d'après son nom et ses propriétés.
pub fn fluide_de(nom: &str, etat: &[(String, String)]) -> Option<Fluide> {
    let niveau = || {
        etat.iter()
            .find(|(k, _)| k == "level")
            .and_then(|(_, v)| v.parse::<u8>().ok())
            .unwrap_or(0)
            .min(15)
    };
    match nom {
        "minecraft:water" => Some(Fluide {
            genre: GenreFluide::Eau,
            niveau: niveau(),
        }),
        "minecraft:lava" => Some(Fluide {
            genre: GenreFluide::Lave,
            niveau: niveau(),
        }),
        n if TOUJOURS_INONDES.contains(&n) => Some(Fluide::source(GenreFluide::Eau)),
        _ if etat.iter().any(|(k, v)| k == "waterlogged" && v == "true") => {
            Some(Fluide::source(GenreFluide::Eau))
        }
        _ => None,
    }
}

/// Les textures qu'un fluide peut porter — ce que l'atlas doit monter quand
/// un état de ce fluide est dans la scène.
pub fn textures(genre: GenreFluide) -> &'static [&'static str] {
    match genre {
        GenreFluide::Eau => &[EAU_IMMOBILE, EAU_COURANTE, EAU_VOILE],
        GenreFluide::Lave => &[LAVE_IMMOBILE, LAVE_COURANTE],
    }
}

/// **La couche d'atlas d'une face de fluide**, avec ses replis : le voile se
/// replie sur le courant, le courant sur l'immobile et l'inverse. Zéro si le
/// pack n'a aucune texture de ce fluide — la même réponse qu'un état sans
/// habillage.
pub fn couche(atlas: &Atlas, genre: GenreFluide, texture: TextureFluide) -> u32 {
    let essais: &[&str] = match (genre, texture) {
        (GenreFluide::Eau, TextureFluide::Immobile) => &[EAU_IMMOBILE, EAU_COURANTE],
        (GenreFluide::Eau, TextureFluide::Courant) => &[EAU_COURANTE, EAU_IMMOBILE],
        (GenreFluide::Eau, TextureFluide::Voile) => &[EAU_VOILE, EAU_COURANTE, EAU_IMMOBILE],
        (GenreFluide::Lave, TextureFluide::Immobile) => &[LAVE_IMMOBILE, LAVE_COURANTE],
        (GenreFluide::Lave, _) => &[LAVE_COURANTE, LAVE_IMMOBILE],
    };
    essais.iter().find_map(|n| atlas.couche(n)).unwrap_or(0)
}
