//! **Ce que décider coûte** — le croisement demande × résidence, à l'échelle.
//!
//! Décider quoi charger doit être négligeable devant le chargement lui-même :
//! une région bâtie met 867 ms à venir (`tf-app --example residence`), donc
//! une décision qui prendrait des millisecondes serait un comble.
//!
//! Écrite en `contains` sur deux tranches, `planifier` était QUADRATIQUE, et
//! le chiffre le disait : 11,4 ms à rayon 40, c'est-à-dire tout le budget
//! d'image dépassé pour décider quatre-vingts chargements. Un rayon de 40
//! chunks est une distance d'affichage ordinaire. Par ensembles : 0,60 ms.
//!
//! Le cas mesuré est le cas NORMAL en vol — on a avancé d'une cellule, donc
//! presque tout se recoupe — et pas un cas limite choisi pour flatter.
//!
//! ```text
//! cargo run --release -p tf-world --example demande
//! ```

use std::time::Instant;

use tf_world::coords::BlockPos;
use tf_world::demande::{planifier, voulues};
use tf_world::Niveau;

fn main() {
    let oeil = BlockPos::new(8, 64, 8);
    let regard = [1.0, 0.0, 0.0];
    let y = (-64, 319);
    println!(
        "  {:>6} {:>9} {:>9} {:>9} {:>9}",
        "rayon", "cellules", "charger", "jeter", "décider"
    );
    for r in [8u32, 16, 24, 40, 64] {
        let v = voulues(oeil, regard, r, Niveau::Chunk, y);
        // On a avancé d'UNE cellule : le recoupement est quasi total, ce qui
        // est exactement ce qu'une image de vol demande.
        let residentes: Vec<_> = voulues(BlockPos::new(8 + 16, 64, 8), regard, r, Niveau::Chunk, y)
            .into_iter()
            .map(|w| w.cellule)
            .collect();

        // Médiane de 5 : une mesure unique n'est pas une mesure, et ce dépôt
        // a déjà vu 18 % d'écart à code identique.
        let mut temps: Vec<f64> = (0..5)
            .map(|_| {
                let t = Instant::now();
                let p = planifier(v.clone(), &residentes);
                std::hint::black_box(&p);
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        temps.sort_by(f64::total_cmp);
        let p = planifier(v.clone(), &residentes);
        println!(
            "  {:>6} {:>9} {:>9} {:>9} {:>6.2} ms",
            r,
            v.len(),
            p.charger.len(),
            p.jetables.len(),
            temps[temps.len() / 2]
        );
    }
}
