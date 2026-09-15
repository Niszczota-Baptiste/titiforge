//! Ce que coûte chaque niveau de compression, sur une région RÉELLE.
//!
//! La recompression pèse **68 %** d'une opération complète — mesuré, 541 ms sur
//! 800. C'est donc là, et nulle part ailleurs, que se décide la réactivité de
//! l'éditeur. Et le choix n'est pas « le plus petit possible » : ce fichier est
//! réécrit à CHAQUE opération, pendant que l'utilisateur attend.
//!
//! ```text
//! cargo run --release -p tf-ops --example compression
//! ```

use tf_anvil::codec::{deflate_level, inflate};
use tf_anvil::region::read;
use tf_anvil::Compression;
use tf_bench::{region, Terrain};

/// Médiane de trois : une mesure unique à froid ne vaut rien, et ce dépôt s'est
/// déjà fait avoir par un premier passage.
fn mesurer(inflates: &[Vec<u8>], niveau: u32) -> (f64, usize) {
    let mut temps = Vec::new();
    let mut taille = 0usize;
    for _ in 0..3 {
        let t0 = std::time::Instant::now();
        taille = inflates
            .iter()
            .map(|v| deflate_level(v, Compression::Zlib, niveau).unwrap().len())
            .sum();
        temps.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    temps.sort_by(f64::total_cmp);
    (temps[1], taille)
}

fn main() {
    let t = Terrain::region_pleine();
    let brut = region(&t);
    let r = read(&brut, 0, 0).unwrap();
    let inflates: Vec<Vec<u8>> = r
        .iter()
        .map(|c| inflate(&c.payload, c.compression).unwrap())
        .collect();
    println!(
        "{} chunks · {:.1} Mo décompressés",
        inflates.len(),
        inflates.iter().map(|v| v.len()).sum::<usize>() as f64 / 1e6
    );

    let niveaux = [0u32, 1, 2, 3, 4, 6, 9];
    let mesures: Vec<(u32, f64, usize)> = niveaux
        .iter()
        .map(|&n| {
            let (ms, taille) = mesurer(&inflates, n);
            (n, ms, taille)
        })
        .collect();
    let (_, ref_ms, ref_taille) = *mesures.iter().find(|(n, _, _)| *n == 6).unwrap();

    println!("\n niveau      temps      taille   × plus rapide   + gros que le 6");
    for (n, ms, taille) in &mesures {
        println!(
            "   {n}     {ms:8.1} ms   {:6.1} Mo      × {:5.2}        {:+6.1} %",
            *taille as f64 / 1e6,
            ref_ms / ms,
            100.0 * (*taille as f64 - ref_taille as f64) / ref_taille as f64
        );
    }
}
