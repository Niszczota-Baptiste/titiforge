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
use tf_world::demande::{par_region, planifier, voulues};
use tf_world::Niveau;

/// **Ce que l'emprise d'un lot fait décoder en TROP.**
///
/// Le chargeur lit un `.mca` une fois par lot, sur le rectangle qui couvre
/// toutes ses cellules. Mais la demande est un DISQUE : un lot au bord de
/// l'horizon n'occupe qu'un coin de son rectangle, et les chunks du reste
/// sont décodés pour rien. Le chiffre dit s'il faut s'en occuper, ou si c'est
/// du travail qu'on aurait fait de toute façon.
fn gaspillage(oeil: BlockPos, r: u32) -> (usize, usize) {
    let v = voulues(oeil, [1.0, 0.0, 0.0], r, Niveau::Chunk, (-64, 319));
    let voulus = v.len();
    let couverts: usize = par_region(&v)
        .iter()
        .map(|l| {
            let (mut x0, mut x1) = (i32::MAX, i32::MIN);
            let (mut z0, mut z1) = (i32::MAX, i32::MIN);
            for c in &l.cellules {
                x0 = x0.min(c.cellule.x);
                x1 = x1.max(c.cellule.x);
                z0 = z0.min(c.cellule.z);
                z1 = z1.max(c.cellule.z);
            }
            ((x1 - x0 + 1) as usize) * ((z1 - z0 + 1) as usize)
        })
        .sum();
    (voulus, couverts)
}

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
    println!();
    println!("Chunks décodés en trop — l'emprise d'un lot est un RECTANGLE,");
    println!("la demande un DISQUE :");
    println!(
        "  {:>6} {:>9} {:>9} {:>8}",
        "rayon", "voulus", "couverts", "en trop"
    );
    for r in [8u32, 16, 24, 40, 64] {
        let (voulus, couverts) = gaspillage(oeil, r);
        println!(
            "  {:>6} {:>9} {:>9} {:>7.0} %",
            r,
            voulus,
            couverts,
            (couverts as f64 / voulus as f64 - 1.0) * 100.0
        );
    }
}
