//! Ce que la passe de modèles COÛTE, avec la vraie géométrie du serveur.
//!
//! `mailler_reel` mesure le maillage ; ici on descend d'un cran, jusqu'à ce
//! qui part au GPU. C'est le chiffre qui dimensionne le rendu, et il ne se
//! déduit pas du nombre de blocs : un bloc-modèle vaut plusieurs cuboïdes, un
//! cuboïde plusieurs faces, et une face gloutonne en couvre plusieurs.
//!
//! La comparaison qui justifie toute la conception est la dernière colonne :
//! ce que les mêmes blocs-modèles pèseraient si on les émettait en quads.
//!
//! ```text
//! cargo run --release -p tf-render --example mesurer -- ../titisite/public/codex
//! ```

use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_assets::catalogue::{blocs_translucides, textures_citees, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};
use tf_bench::{build, Build};
use tf_mesh::forme::Formes;
use tf_mesh::Grille;
use tf_render::{faces_de, Arene, AreneModeles, HabillageFaces, InstanceQuad};

fn main() {
    let Some(racine) = std::env::args().nth(1) else {
        eprintln!("usage : mesurer <dossier de codex>");
        std::process::exit(2);
    };
    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);
    let atlas = Atlas::batir(&src, textures_citees(&cat), &|n| {
        Disposition::Codex.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &atlas);
    let teintes = tf_assets::Teintes::default();
    println!(
        "pack : {} blocs, {} modèles, atlas de {} couches",
        cat.nb_blocs(),
        cat.nb_modeles(),
        atlas.len()
    );

    let mo = |o: usize| o as f64 / 1e6;
    println!(
        "\n{:<12} {:>10} {:>9} {:>10} {:>9} {:>10} {:>9} {:>8}",
        "décor", "quads", "arène Mo", "poses", "faces", "modèles Mo", "en quads", "× gain"
    );

    for densite in [20u32, 55, 90] {
        let b = Build::petit().avec_decor(densite);
        let octets = build::region(&b);
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

        let cles: Vec<String> = (0..interner.len() as StateId)
            .map(|i| interner.resolve(i).unwrap().to_string())
            .collect();
        let (table, habillage) =
            tf_assets::table_rendu(&cat, &atlas, &teintes, cles.iter().cloned(), &|n| {
                translucides.contains(n)
            });

        let t = Instant::now();
        let chantier = grille.mailler_parallele(&table);
        let ms_maille = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let arene = Arene::depuis(&chantier, &|id, face| match habillage.get(id as usize) {
            Some(h) => {
                let a = h.cube[face.indice()];
                (a.couche, a.teinte)
            }
            None => (0, [1.0; 3]),
        });
        let modeles = AreneModeles::depuis(&chantier, &|id| {
            let Some(h) = habillage.get(id as usize) else {
                return Vec::new();
            };
            let hab: Vec<HabillageFaces> = h
                .cuboides
                .iter()
                .map(|f| std::array::from_fn(|k| (f[k].couche, f[k].teinte, f[k].uv)))
                .collect();
            faces_de(table.cuboides(id), &hab)
        });
        let ms_arenes = t.elapsed().as_secs_f64() * 1000.0;

        // Ce que les mêmes faces pèseraient en quads : c'est la conception
        // qu'on compare, pas une optimisation qu'on espère.
        let en_quads = modeles.faces_a_dessiner as usize * std::mem::size_of::<InstanceQuad>();
        println!(
            "{:<12} {:>10} {:>9.2} {:>10} {:>9} {:>10.2} {:>9.2} {:>7.1}×",
            format!("{densite} %"),
            chantier.quads(),
            mo(arene.octets()),
            chantier.poses(),
            modeles.faces_a_dessiner,
            mo(modeles.octets()),
            mo(en_quads),
            en_quads as f64 / modeles.octets().max(1) as f64
        );
        println!(
            "             mailler {ms_maille:.0} ms · arènes {ms_arenes:.0} ms · \
             {} états dont {} modèles · table de faces {:.0} ko",
            table.len(),
            habillage.iter().filter(|h| !h.cuboides.is_empty()).count(),
            modeles.faces.len() * std::mem::size_of::<tf_render::FaceModele>() / 1000
        );
    }
}
