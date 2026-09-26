//! Ce que coûte chaque format, sur un extrait de la taille d'un vrai build.
//!
//! L'extrait est copié d'une fixture de TERRAIN (16 × 16 chunks, 12
//! sections : 12,6 millions de cases, un décor de blocs à propriétés) —
//! `tf_bench`, pas une grille uniforme qu'aucun format ne trouverait
//! difficile. Médiane de trois, en release.
//!
//! ```text
//! cargo run --release -p tf-formats --example mesurer
//! ```

use std::time::Instant;

use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_formats::{ecrire, lire, Format, Meta};
use tf_ops::edition::copier;
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

fn mediane(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let t = Terrain::peuplee(3);
    let m = MemorySource::new();
    m.put_region(
        Dimension::Overworld,
        Folder::Region,
        RegionPos { x: 0, z: 0 },
        region(&t),
    );
    let st = Staging::new(m, MemorySource::new());
    let cote = (t.side * 16) as i32;
    let haut = (t.sections * 16) as i32;
    let sel = BBox::new(
        BlockPos::new(0, -64, 0),
        BlockPos::new(cote - 1, -64 + haut - 1, cote - 1),
    );
    let mut interner = Interner::new();
    let presse = copier(
        &st,
        &Dimension::Overworld,
        Folder::Region,
        &sel,
        &mut interner,
    )
    .unwrap();
    let cases = presse.blocs.len();
    let etats = presse.palette().len();
    println!(
        "extrait : {} × {} × {} = {cases} cases, {etats} états, {} block entities",
        presse.taille[0],
        presse.taille[1],
        presse.taille[2],
        presse.entites.len()
    );
    let meta = Meta {
        data_version: 2975,
        nom: "mesure".into(),
        auteur: String::new(),
        description: String::new(),
        date_ms: 0,
    };
    println!();
    println!("| format | écrire | lire | taille | octets / case |");
    println!("|---|---:|---:|---:|---:|");
    for format in Format::TOUS {
        let mut ecrit = None;
        let mut te = Vec::new();
        let mut tl = Vec::new();
        for _ in 0..3 {
            let t0 = Instant::now();
            let e = match ecrire(&presse, format, &interner, &meta) {
                Ok(e) => e,
                Err(e) => {
                    println!("| {} | refusé : {e} | | | |", format.nom());
                    break;
                }
            };
            te.push(t0.elapsed().as_secs_f64() * 1000.0);
            let t0 = Instant::now();
            let lu = lire(&e.octets, &mut Interner::new()).unwrap();
            tl.push(t0.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(lu.presse.blocs.len(), cases);
            ecrit = Some(e);
        }
        if let Some(e) = ecrit {
            println!(
                "| {} | {:.0} ms | {:.0} ms | {:.2} Mo | {:.3} |",
                format.nom(),
                mediane(te),
                mediane(tl),
                e.octets.len() as f64 / 1e6,
                e.octets.len() as f64 / cases as f64
            );
        }
    }
}
