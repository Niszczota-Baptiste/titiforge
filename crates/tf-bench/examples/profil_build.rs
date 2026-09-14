//! Ce que la fixture « build Minefield » produit VRAIMENT — relevé sur le
//! `.mca`, par le décodeur, pas par le générateur.

use std::collections::{BTreeMap, BTreeSet};

use tf_anvil::{decode_section, inflate, read, scan, Interner};
use tf_bench::catalogue::{Forme, BLOCS};
use tf_bench::{build, Build};

fn forme_de(nom: &str) -> Option<Forme> {
    BLOCS.iter().find(|(n, ..)| *n == nom).map(|(_, f, _)| *f)
}

fn main() {
    // La densité de décor est un RÉGLAGE, pas une mesure : on la balaie.
    let cas = [
        ("petit · décor 20 %", Build::petit().avec_decor(20)),
        ("petit · décor 55 %", Build::petit()),
        ("petit · décor 90 %", Build::petit().avec_decor(90)),
        ("région pleine", Build::region_pleine()),
    ];
    for (etiquette, b) in cas {
        let octets = build::region(&b);
        let r = read(&octets, 0, 0).unwrap();
        let mut sections = 0usize;
        let mut homogenes = 0usize;
        let mut palettes: Vec<usize> = Vec::new();
        let mut air = 0usize;
        let mut total = 0usize;
        let mut par_forme: BTreeMap<Forme, usize> = BTreeMap::new();
        let mut distincts: BTreeSet<String> = BTreeSet::new();
        let mut cuboides = 0usize;

        for cz in 0..b.side as i32 {
            for cx in 0..b.side as i32 {
                let brut = r.get(cx, cz).unwrap();
                let inflated = inflate(&brut.payload, brut.compression).unwrap();
                let sc = scan(&inflated).unwrap();
                let mut interner = Interner::new();
                for s in &sc.sections {
                    let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap()
                    else {
                        continue;
                    };
                    sections += 1;
                    palettes.push(sec.palette.len());
                    if sec.palette.len() == 1 {
                        homogenes += 1;
                    }
                    let idx = sec.unpack();
                    for i in 0..4096 {
                        let nom = interner.resolve(sec.palette[idx[i] as usize]).unwrap();
                        let nu = nom.split('|').next().unwrap();
                        total += 1;
                        if nu == "minecraft:air" {
                            air += 1;
                            continue;
                        }
                        distincts.insert(nom.to_string());
                        let f = forme_de(nu).unwrap();
                        *par_forme.entry(f).or_insert(0) += 1;
                        if f == Forme::Modele {
                            cuboides += BLOCS
                                .iter()
                                .find(|(n, ..)| *n == nu)
                                .map(|(_, _, c)| *c as usize)
                                .unwrap_or(1);
                        }
                    }
                }
            }
        }

        palettes.sort_unstable();
        let bâties: Vec<usize> = palettes.iter().copied().filter(|n| *n > 1).collect();
        let pose = total - air;
        println!(
            "\n╭─ {etiquette} · {} blocs · {} Ko de .mca",
            b.blocs(),
            octets.len() / 1024
        );
        println!(
            "│ sections           {sections} dont {homogenes} homogènes ({:.0} %)",
            100.0 * homogenes as f64 / sections as f64
        );
        println!(
            "│ palette (bâties)   médiane {} · max {}",
            bâties[bâties.len() / 2],
            bâties.last().unwrap()
        );
        println!(
            "│ blocs posés        {pose} ({:.1} % du volume)",
            100.0 * pose as f64 / total as f64
        );
        println!("│ états distincts    {}", distincts.len());
        for (f, n) in &par_forme {
            println!(
                "│   {f:?}  {n}  ({:.1} % des posés)",
                100.0 * *n as f64 / pose as f64
            );
        }
        println!("│ cuboïdes à mailler {cuboides} pour la passe de modèles");
        println!(
            "╰─ quads gloutons possibles au mieux : {} faces de cubes",
            par_forme.get(&Forme::Cube).unwrap_or(&0) * 6
        );
    }
}
