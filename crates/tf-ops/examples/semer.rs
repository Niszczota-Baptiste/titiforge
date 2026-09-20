//! Écrit un monde d'ESSAI sur disque, pour éprouver `editer` sans risque.
//!
//! Ce n'est pas une save Minecraft complète — il n'y a pas de `level.dat` et le
//! jeu n'y verrait rien d'intéressant. C'est un dossier `region/` avec de
//! vraies régions Anvil, écrites par le même code que les fixtures de bench :
//! de quoi exercer la chaîne complète sur des fichiers, pas sur des tampons.
//!
//! ```text
//! .\semer.exe  D:\monde-essai
//! .\editer.exe D:\monde-essai
//! ```

use tf_bench::Terrain;

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

    // Des coffres pleins : sans eux, rien n'exercerait le chemin qui fait
    // suivre les block entities, et une démonstration de `--copier-vers`
    // passerait à côté de ce qu'elle doit montrer.
    let t = Terrain::peuplee(2);
    let mut n = 0;
    let mut octets = 0usize;
    for x in 0..cote {
        for z in 0..cote {
            // La région est POSÉE là où on l'écrit. Écrire partout les octets
            // de `r.0.0` donnerait des chunks qui annoncent tous (0, 0) — et
            // les coordonnées MONDE d'un coffre viennent de son contenu, pas
            // du nom du fichier : elles désigneraient alors la mauvaise
            // région, et tout ce qui les lit chercherait au mauvais endroit.
            let brut = tf_bench::region_en(&t, x, z);
            let nom = tf_anvil::region::region_file_name(x, z);
            std::fs::write(region_dir.join(&nom), &brut).expect("écriture");
            octets += brut.len();
            n += 1;
        }
    }
    println!(
        "{n} régions écrites dans {} · {:.1} Mo · {} coffres par chunk",
        region_dir.display(),
        octets as f64 / 1e6,
        t.coffres
    );
    println!(
        "chaque région porte {} chunks peuplés, soit les blocs 0..{} en x et z de sa région",
        t.side * t.side,
        t.side * 16 - 1
    );
}
