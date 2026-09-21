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
//! On DESSINE aussi, et pas seulement pour la forme : un tampon trop grand,
//! un compte d'instances qui déborde un `u32`, une limite de pilote ne se
//! voient qu'en montant au GPU. `--region` prend une région PLEINE — 1 024
//! chunks — parce qu'un quart de région ne prouve rien sur les limites.
//!
//! ```text
//! cargo run --release -p tf-render --example mesurer -- ../titisite/public/codex
//! cargo run --release -p tf-render --example mesurer -- ../titisite/public/codex --region
//! ```

use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_assets::catalogue::{blocs_translucides, textures_citees, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};
use tf_bench::{build, Build};
use tf_mesh::forme::Formes;
use tf_mesh::Grille;
use tf_render::{
    faces_de, Appareil, Arene, AreneModeles, AtlasGpu, Camera, Cible, HabillageFaces, InstanceQuad,
    Scene,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(racine) = args.next() else {
        eprintln!("usage : mesurer <dossier de codex> [--region]");
        std::process::exit(2);
    };
    let pleine = args.any(|a| a == "--region");
    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);
    // L'atlas COMPLET sert à juger la translucidité — un bloc absent de la
    // scène peut être cité par un voisin — mais il ne monte jamais au GPU : le
    // pack cite 2 207 textures pour un plafond de 2 048 couches.
    let complet = Atlas::batir(&src, textures_citees(&cat), &|n| {
        Disposition::Codex.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &complet);
    let teintes = tf_assets::Teintes::default();
    println!(
        "pack : {} blocs, {} modèles, {} textures citées",
        cat.nb_blocs(),
        cat.nb_modeles(),
        complet.len()
    );

    let app = match Appareil::ouvrir() {
        Ok(a) => {
            println!("adaptateur : {}", a.decrire());
            Some(a)
        }
        Err(e) => {
            eprintln!("pas d'adaptateur ({e}) : on compte sans dessiner");
            None
        }
    };

    let mo = |o: usize| o as f64 / 1e6;
    println!(
        "\n{:<12} {:>10} {:>9} {:>10} {:>9} {:>10} {:>9} {:>8}",
        "décor", "quads", "arène Mo", "poses", "faces", "modèles Mo", "en quads", "× gain"
    );

    for densite in [20u32, 55, 90] {
        let b = if pleine {
            Build::default().avec_decor(densite)
        } else {
            Build::petit().avec_decor(densite)
        };
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
        // SEULEMENT les textures des blocs présents.
        let atlas = Atlas::batir(
            &src,
            tf_assets::textures_des_etats(&cat, cles.iter().cloned()),
            &|n| Disposition::Codex.chemins_texture(n),
        );
        let (table, habillage) =
            tf_assets::table_rendu(&cat, &atlas, &teintes, cles.iter().cloned(), &|n| {
                translucides.contains(n)
            });

        let t = Instant::now();
        let chantier = grille.mailler_parallele(&table);
        let ms_maille = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let arene = Arene::depuis(
            &chantier,
            &|id, face, _biome| match habillage.get(id as usize) {
                Some(h) => {
                    let a = h.cube[face.indice()];
                    (a.couche, a.teinte)
                }
                None => (0, [1.0; 3]),
            },
        );
        let modeles = AreneModeles::sans_biome(&chantier, &|id| {
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

        // Et on MONTE au GPU. Compter des instances ne dit rien d'une limite
        // de pilote ni d'un tampon qui ne tient pas.
        let Some(app) = app.as_ref() else { continue };
        let t = Instant::now();
        let atlas_gpu = AtlasGpu::avec_mips(app, atlas.cote, atlas.len() as u32, &atlas.pyramide());
        let scene = Scene::avec_modeles(app, &arene, &modeles, &atlas_gpu);
        let ms_gpu = t.elapsed().as_secs_f64() * 1000.0;
        let cible = Cible::nouvelle(app, 320, 200);
        let (min, max) = arene.bornes().expect("l'arène a du contenu");
        let (_, compte) = scene.rendre(&cible, &Camera::cadrer(min, max, 1.6));
        println!(
            "             GPU {ms_gpu:.0} ms · {} appels de dessin · {} instances",
            compte.appels_de_dessin, compte.instances
        );
    }
}
