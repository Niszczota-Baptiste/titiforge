//! Ce que la dérivation couvre sur un pack RÉEL.
//!
//! ```text
//! cargo run --release -p tf-blocks --example deriver -- ../titisite/public/codex
//! ```

use std::collections::BTreeMap;

use tf_assets::catalogue::Disposition;
use tf_assets::{Catalogue, Dossier};
use tf_blocks::{Table, Transfo, TOUTES};

fn main() {
    let racine = std::env::args().nth(1).expect("usage : deriver <pack>");
    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);

    let t0 = std::time::Instant::now();
    let table = Table::deriver(&cat);
    println!(
        "dérivation : {} blocs en {:.0} ms",
        table.len(),
        t0.elapsed().as_secs_f64() * 1000.0
    );

    // Un bloc SANS état n'a rien à tourner : le compter comme un échec ferait
    // passer une réussite pour un problème.
    let a_etat: Vec<&String> = cat
        .blocs()
        .filter(|(_, bs)| {
            matches!(bs, tf_assets::Blockstate::Variants(v)
                     if v.len() > 1 && v.iter().any(|(c, _)| !c.is_empty()))
        })
        .map(|(n, _)| n)
        .collect();

    for ns in ["minefield", "minecraft"] {
        let cible: Vec<&&String> = a_etat.iter().filter(|n| n.starts_with(ns)).collect();
        println!("\n── {ns} · {} blocs à état", cible.len());
        for t in TOUTES {
            let ok = cible.iter().filter(|n| table.connait(n, t)).count();
            println!(
                "   {:16} {ok} / {} ({:.1} %)",
                t.nom(),
                cible.len(),
                100.0 * ok as f64 / cible.len().max(1) as f64
            );
        }
    }

    println!("\nmanques : {}", table.manques.len());
    let mut familles: BTreeMap<(&str, &str), (usize, String)> = BTreeMap::new();
    for m in &table.manques {
        let ns = if m.bloc().starts_with("minefield") {
            "minefield"
        } else {
            "minecraft"
        };
        let e = familles
            .entry((ns, m.genre()))
            .or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() {
            e.1 = m.to_string();
        }
    }
    for ((ns, genre), (n, exemple)) in &familles {
        println!("   {ns:10} {genre:20} {n:4}   ex. {exemple}");
    }

    // Les blocs Minefield qu'on ne sait PAS tourner : c'est la cible première.
    let perdus: Vec<&&String> = a_etat
        .iter()
        .filter(|n| n.starts_with("minefield") && !table.connait(n, Transfo::Rot90))
        .collect();
    println!("\nminefield sans rotation 90° : {}", perdus.len());
    for n in perdus.iter().take(12) {
        println!("   {n}");
    }

    // ── les lois du groupe, vérifiées sur TOUS les états du pack
    let mut verifies = 0usize;
    let mut fautes = 0usize;
    for (nom, bs) in cat.blocs() {
        let tf_assets::Blockstate::Variants(v) = bs else {
            continue;
        };
        for (cle, _) in v {
            if cle.is_empty() {
                continue;
            }
            let etat = format!("{nom}|{cle}");
            // Quatre rotations de 90° = l'identité.
            if table.connait(nom, Transfo::Rot90) {
                let mut c = etat.clone();
                let mut complet = true;
                for _ in 0..4 {
                    match table.transformer(&c, Transfo::Rot90) {
                        Some(n) => c = n,
                        None => {
                            complet = false;
                            break;
                        }
                    }
                }
                if complet {
                    verifies += 1;
                    if normaliser(&c) != normaliser(&etat) {
                        fautes += 1;
                        if fautes <= 3 {
                            println!("   faute : {etat} → ×4 → {c}");
                        }
                    }
                }
            }
            // Un miroir est sa propre inverse.
            for t in [Transfo::MiroirX, Transfo::MiroirZ] {
                if let Some(a) = table.transformer(&etat, t) {
                    if let Some(b) = table.transformer(&a, t) {
                        if normaliser(&b) != normaliser(&etat) {
                            fautes += 1;
                            if fautes <= 6 {
                                println!("   faute miroir {} : {etat} → {a} → {b}", t.nom());
                            }
                        }
                    }
                }
            }
            // rot90 ∘ rot90 = rot180.
            if let (Some(a), Some(b)) = (
                table
                    .transformer(&etat, Transfo::Rot90)
                    .and_then(|x| table.transformer(&x, Transfo::Rot90)),
                table.transformer(&etat, Transfo::Rot180),
            ) {
                if normaliser(&a) != normaliser(&b) {
                    fautes += 1;
                    if fautes <= 6 {
                        println!("   faute 90+90≠180 : {etat} → {a} / {b}");
                    }
                }
            }
        }
    }
    println!("\nlois du groupe vérifiées sur {verifies} états : {fautes} fautes");

    // ── un exemple lisible
    for exemple in [
        "minefield:dark_oak_ladder|facing=north",
        "minefield:steel_slab|type=bottom,vertical=false,facing=north",
        "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,vertical=false",
        "minecraft:oak_stairs|facing=east,half=bottom,shape=inner_right,vertical=false",
    ] {
        println!("\n{exemple}");
        for t in TOUTES {
            match table.transformer(exemple, t) {
                Some(r) => println!("   {:16} → {}", t.nom(), r),
                None => println!("   {:16} → non dérivable", t.nom()),
            }
        }
    }
}

/// Deux clés d'état sont la même si leurs propriétés le sont, quel que soit
/// l'ordre d'écriture.
fn normaliser(cle: &str) -> String {
    match cle.split_once('|') {
        None => cle.to_string(),
        Some((n, p)) => {
            let mut v: Vec<&str> = p.split(',').collect();
            v.sort_unstable();
            format!("{n}|{}", v.join(","))
        }
    }
}
