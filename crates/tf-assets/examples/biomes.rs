//! Ce que le jeu dit de la COULEUR de ses biomes.
//!
//! Rien n'est écrit à la main ici : la couleur d'herbe d'un biome se lit dans
//! deux fichiers du jeu, `colormap/grass.png` (une table de 256 × 256) et
//! `worldgen/biome/<nom>.json` (la température et l'humidité qui l'indexent).
//!
//! **Il faut une INSTALLATION.** Un resource pack seul ne porte pas les
//! données de biome — elles vivent du côté `data/`, pas `assets/`. C'est la
//! seconde raison de lire l'installation de l'utilisateur plutôt qu'un pack
//! embarqué ; la première était les textures.
//!
//! ```text
//! cargo run --release -p tf-assets --example biomes -- %APPDATA%\.minefield_1_18
//! ```

use tf_assets::climat::Climat;

fn main() {
    let Some(racine) = std::env::args().nth(1) else {
        eprintln!("usage : biomes <installation, pack ou codex>");
        std::process::exit(2);
    };
    let (pile, genre) = match tf_assets::jeu::ouvrir(&racine) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("illisible : {e:?}");
            std::process::exit(1);
        }
    };
    println!("{racine} ({genre:?})");

    let debut = std::time::Instant::now();
    let c = Climat::charger(&pile);
    println!(
        "{} biome(s) lus en {:.0} ms",
        c.nb_biomes(),
        debut.elapsed().as_secs_f64() * 1000.0
    );

    if !c.manques.is_empty() {
        println!("\nCE QUI MANQUE — et qui explique ce qu'on ne peut pas colorer :");
        for m in &c.manques {
            println!("  · {m}");
        }
    }
    if c.nb_biomes() == 0 {
        println!(
            "\nAucune donnée de biome. Un resource pack n'en porte pas : il faut une\n\
             INSTALLATION de launcher (un dossier avec `versions/`), ou un datapack."
        );
        return;
    }

    // Les biomes les plus parlants d'abord — ceux dont la couleur se remarque.
    let vedettes = [
        "minecraft:plains",
        "minecraft:forest",
        "minecraft:dark_forest",
        "minecraft:swamp",
        "minecraft:jungle",
        "minecraft:desert",
        "minecraft:badlands",
        "minecraft:snowy_plains",
        "minecraft:taiga",
        "minecraft:savanna",
    ];
    println!(
        "\n{:26} {:>10} {:>11} {:>8}",
        "biome", "herbe", "feuillage", "eau"
    );
    let mut montres = 0usize;
    for nom in vedettes {
        let (Some(h), Some(f), Some(e)) = (c.herbe(nom), c.feuillage(nom), c.eau(nom)) else {
            continue;
        };
        println!(
            "{:26} {:>10} {:>11} {:>8}{}",
            nom.trim_start_matches("minecraft:"),
            hex(h),
            hex(f),
            hex(e),
            if c.approche(nom) { "  (approché)" } else { "" }
        );
        montres += 1;
    }
    if montres == 0 {
        println!("(aucun biome vanilla connu — ce jeu a ses propres noms)");
    }

    println!(
        "\nLes couleurs sont DÉRIVÉES du jeu : la table est lue, les températures aussi.\n\
         Seul le marais est approché — le jeu y tire entre deux verts d'après un bruit\n\
         de position, qu'on ne peut pas rejouer sans le générateur de monde."
    );
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
