//! Mailler un build avec la **VRAIE géométrie** du serveur.
//!
//! Les benchs de `tf-mesh` inventent leurs cuboïdes : ils ont la bonne
//! DISTRIBUTION (mesurée sur le codex) mais pas la bonne forme. Ici la
//! géométrie vient du pack, résolue parent par parent. C'est le seul chiffre
//! qui vaut pour dimensionner le rendu.
//!
//! ```text
//! cargo run --release -p tf-assets --example mailler_reel -- ../titisite/public/codex
//! ```

use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_assets::catalogue::{blocs_translucides, table_formes, textures_citees, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};
use tf_bench::{build, Build};
use tf_mesh::Grille;

fn main() {
    let Some(racine) = std::env::args().nth(1) else {
        eprintln!("usage : mailler_reel <dossier de codex>");
        std::process::exit(2);
    };
    let src = Dossier::ouvrir(&racine).expect("pack lisible");

    let t0 = Instant::now();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);
    println!(
        "pack : {} blocs, {} modèles, {} introuvables, {:.0} ms",
        cat.nb_blocs(),
        cat.nb_modeles(),
        cat.introuvables.len(),
        t0.elapsed().as_secs_f64() * 1000.0
    );

    // ── le tableau d'atlas, et ce qu'il apprend sur l'opacité
    let ta = Instant::now();
    let citees = textures_citees(&cat);
    let atlas = Atlas::batir(&src, citees.iter().cloned(), &|n| {
        Disposition::Codex.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &atlas);
    println!(
        "atlas : {} couches de {} × {} ({:.1} Mo), {} manquantes, {:.0} ms",
        atlas.len(),
        atlas.cote,
        atlas.cote,
        atlas.octets() as f64 / 1e6,
        atlas.manquantes.len(),
        ta.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "        {} couches animées · {} couches transparentes · {} BLOCS translucides",
        atlas.couches.iter().filter(|c| c.images > 1).count(),
        atlas.transparentes().len(),
        translucides.len()
    );
    for n in atlas.manquantes.iter().take(4) {
        println!("        manquante : {n}");
    }

    for (etiquette, b) in [
        ("décor 20 %", Build::petit().avec_decor(20)),
        ("décor 55 %", Build::petit()),
        ("décor 90 %", Build::petit().avec_decor(90)),
    ] {
        let octets = build::region(&b);

        // ── charger la région dans une grille, avec UN seul interner
        let t1 = Instant::now();
        let r = read(&octets, 0, 0).unwrap();
        let mut grille = Grille::new();
        let mut interner = Interner::new();
        for cz in 0..b.side as i32 {
            for cx in 0..b.side as i32 {
                let brut = r.get(cx, cz).unwrap();
                let inflated = inflate(&brut.payload, brut.compression).unwrap();
                let sc = scan(&inflated).unwrap();
                for s in &sc.sections {
                    if let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap() {
                        grille.poser(cx, cz, sec);
                    }
                }
            }
        }
        let t_charge = t1.elapsed();

        // ── la table de formes, depuis le PACK
        let t2 = Instant::now();
        let cles: Vec<String> = (0..interner.len() as StateId)
            .map(|i| interner.resolve(i).unwrap().to_string())
            .collect();
        let inconnus = cles
            .iter()
            .filter(|c| {
                let (nom, _) = tf_assets::catalogue::decouper(c);
                !nom.ends_with("air") && cat.blockstate(nom).is_none()
            })
            .count();
        let table = table_formes(&cat, cles.iter().cloned(), &|n| translucides.contains(n));
        let t_table = t2.elapsed();

        // ── mailler
        let t3 = Instant::now();
        let c = grille.mailler_parallele(&table);
        let t_maille = t3.elapsed();

        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        println!("\n╭─ {etiquette} · {} blocs", b.blocs());
        println!(
            "│ états distincts  {} dont {inconnus} hors du pack",
            table.len()
        );
        println!("│ charger          {:8.0} ms", ms(t_charge));
        println!("│ table de formes  {:8.1} ms", ms(t_table));
        println!("│ MAILLER (4 fils) {:8.0} ms", ms(t_maille));
        println!(
            "│ sections         {} maillées, {} sautées",
            c.maillees(),
            c.sautees
        );
        println!("│ quads gloutons   {:10}", c.quads());
        println!("│ poses de modèles {:10}", c.poses());
        println!("╰─ {:.2} Mo pour le GPU", c.octets() as f64 / 1e6);
    }
}
