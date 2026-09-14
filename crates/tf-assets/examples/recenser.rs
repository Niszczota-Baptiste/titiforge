//! Recense un vrai pack. Aucune donnée n'est copiée dans le dépôt — on lit ce
//! que l'utilisateur a sur son disque, et on rend des CHIFFRES.
//!
//! ```text
//! cargo run --release -p tf-assets --example recenser -- ../titisite/public/codex
//! ```

use std::collections::BTreeMap;

use tf_assets::catalogue::{Classement, Disposition};
use tf_assets::{Catalogue, Dossier};

fn main() {
    let Some(racine) = std::env::args().nth(1) else {
        eprintln!("usage : recenser <dossier de pack ou de codex>");
        std::process::exit(2);
    };
    let src = match Dossier::ouvrir(&racine) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let debut = std::time::Instant::now();
    let mut cat = Catalogue::new(Disposition::Codex);
    match cat.charger_codex(&src) {
        Ok(n) => println!("blockstates lus : {n}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    cat.resoudre_modeles(&src);
    let duree = debut.elapsed();

    println!("modèles résolus : {}", cat.nb_modeles());
    println!("introuvables    : {}", cat.introuvables.len());
    for id in cat.introuvables.iter().take(5) {
        println!("    {id}");
    }
    println!("durée           : {:.0} ms", duree.as_secs_f64() * 1000.0);

    // Le recensement, par namespace — c'est la cible qui compte.
    let mut par_ns: BTreeMap<&str, BTreeMap<Classement, usize>> = BTreeMap::new();
    let mut cuboides_par_modele: Vec<usize> = Vec::new();
    let mut sans_uv = 0usize;
    let mut avec_uv = 0usize;
    let mut cullface = 0usize;
    let mut tint = 0usize;
    let mut deborde = 0usize;

    for (nom, _) in cat.blocs() {
        let ns = nom.split(':').next().unwrap_or("?");
        let Some(m) = cat.modele_de(nom) else {
            continue;
        };
        let c = tf_assets::catalogue::classer(m);
        *par_ns.entry(ns).or_default().entry(c).or_insert(0) += 1;
        if c == Classement::Modele {
            cuboides_par_modele.push(m.elements.len());
        }
        for e in &m.elements {
            if e.from
                .iter()
                .chain(e.to.iter())
                .any(|v| *v < 0.0 || *v > 16.0)
            {
                deborde += 1;
            }
            for fd in e.faces.values() {
                if fd.uv.is_some() {
                    avec_uv += 1;
                } else {
                    sans_uv += 1;
                }
                if fd.cullface.is_some() {
                    cullface += 1;
                }
                if fd.tintindex.is_some() {
                    tint += 1;
                }
            }
        }
    }

    for (ns, formes) in &par_ns {
        let total: usize = formes.values().sum();
        println!("\n── {ns} · {total} blocs");
        for (c, n) in formes {
            println!("   {c:?}  {n}  ({:.1} %)", 100.0 * *n as f64 / total as f64);
        }
    }

    if !cuboides_par_modele.is_empty() {
        cuboides_par_modele.sort_unstable();
        let moy =
            cuboides_par_modele.iter().sum::<usize>() as f64 / cuboides_par_modele.len() as f64;
        println!(
            "\ncuboïdes par bloc-modèle : moyenne {moy:.2}, médiane {}, pire {}",
            cuboides_par_modele[cuboides_par_modele.len() / 2],
            cuboides_par_modele.last().unwrap()
        );
    }
    let faces = sans_uv + avec_uv;
    println!(
        "faces : {faces} · uv ABSENTE {sans_uv} ({:.1} %) · cullface {cullface} ({:.1} %) · tintindex {tint}",
        100.0 * sans_uv as f64 / faces as f64,
        100.0 * cullface as f64 / faces as f64
    );
    println!("éléments qui débordent du bloc : {deborde}");
}
