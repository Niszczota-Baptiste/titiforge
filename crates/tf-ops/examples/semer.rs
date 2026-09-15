//! Écrit un monde d'ESSAI sur disque, pour éprouver `editer` sans risque.
//!
//! Ce n'est pas une save Minecraft complète — il n'y a pas de `level.dat` et le
//! jeu n'y verrait rien d'intéressant. C'est un dossier `region/` avec de
//! vraies régions Anvil, écrites par le même code que les fixtures de bench :
//! de quoi exercer la chaîne complète sur des fichiers, pas sur des tampons.
//!
//! ```text
//! cargo run --release -p tf-ops --example semer -- /tmp/monde-essai
//! cargo run --release -p tf-ops --example editer -- /tmp/monde-essai
//! ```

use tf_bench::{region, Terrain};

fn main() {
    let mut a = std::env::args().skip(1);
    let Some(dossier) = a.next() else {
        eprintln!("usage : semer <dossier> [côté en régions, défaut 2]");
        std::process::exit(2);
    };
    let cote: i32 = a.next().and_then(|s| s.parse().ok()).unwrap_or(2);

    let racine = std::path::Path::new(&dossier);
    let region_dir = racine.join("region");
    std::fs::create_dir_all(&region_dir).expect("dossier créable");

    let t = Terrain::petite();
    let brut = region(&t);
    let mut n = 0;
    for x in 0..cote {
        for z in 0..cote {
            let nom = tf_anvil::region::region_file_name(x, z);
            std::fs::write(region_dir.join(&nom), &brut).expect("écriture");
            n += 1;
        }
    }
    println!(
        "{n} régions écrites dans {} · {:.1} Mo",
        region_dir.display(),
        n as f64 * brut.len() as f64 / 1e6
    );
    println!(
        "chaque région porte {} chunks peuplés, soit les blocs 0..{} en x et z de sa région",
        t.side * t.side,
        t.side * 16 - 1
    );
}
