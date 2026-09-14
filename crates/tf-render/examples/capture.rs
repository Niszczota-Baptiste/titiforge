//! Rend un build Minefield dans un PNG, **sans écran**.
//!
//! ```text
//! cargo run --release -p tf-render --example capture -- ../titisite/public/codex sortie.png
//! ```

use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_assets::catalogue::{blocs_translucides, table_formes, textures_citees, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};
use tf_bench::{build, Build};
use tf_mesh::Grille;
use tf_render::{Appareil, Arene, AtlasGpu, Camera, Cible, Scene};

fn main() {
    let mut args = std::env::args().skip(1);
    let racine = args
        .next()
        .expect("usage : capture <codex> <sortie.png> [côté]");
    let sortie = args.next().unwrap_or_else(|| "capture.png".into());
    let cote: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);

    // ── le build : un morceau, pas une région entière
    let b = Build {
        side: 4,
        sections: 5,
        ..Build::default()
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

    // ── l'atlas : SEULEMENT les textures des blocs présents
    //
    // Le pack en cite 2 207, et une texture-tableau est plafonnée à 2 048
    // couches — sur la carte comme sur le pilote logiciel. Charger tout le
    // catalogue dépasserait la limite ET paierait des textures qu'aucun bloc
    // de la scène n'emploie.
    let cles: Vec<String> = (0..interner.len() as StateId)
        .map(|i| interner.resolve(i).unwrap().to_string())
        .collect();
    let mut voulues: Vec<String> = Vec::new();
    for cle in &cles {
        let (nom, etat) = tf_assets::catalogue::decouper(cle);
        let Some(bs) = cat.blockstate(nom) else {
            continue;
        };
        for v in bs.pour(&etat) {
            if let Some(m) = cat.modele(&v.modele) {
                for e in &m.elements {
                    for fd in e.faces.values() {
                        if !fd.texture.starts_with('#') {
                            voulues.push(fd.texture.clone());
                        }
                    }
                }
            }
        }
    }
    voulues.sort();
    voulues.dedup();
    let atlas = Atlas::batir(&src, voulues.clone(), &|n| {
        Disposition::Codex.chemins_texture(n)
    });

    // Pour la translucidité, il faut l'atlas COMPLET du catalogue : un bloc
    // absent de la scène peut quand même être cité par un voisin.
    let atlas_complet = Atlas::batir(&src, textures_citees(&cat), &|n| {
        Disposition::Codex.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &atlas_complet);
    let table = table_formes(&cat, cles.iter().cloned(), &|n| translucides.contains(n));

    // ── mailler
    let t = Instant::now();
    let chantier = grille.mailler_parallele(&table);
    let t_maille = t.elapsed();

    // ── quelle couche d'atlas pour quel état ?
    let couche_de: Vec<u32> = cles
        .iter()
        .map(|cle| {
            let (nom, etat) = tf_assets::catalogue::decouper(cle);
            cat.blockstate(nom)
                .and_then(|bs| {
                    bs.pour(&etat).first().and_then(|v| {
                        cat.modele(&v.modele).and_then(|m| {
                            m.elements.first().and_then(|e| {
                                // La face du DESSUS d'abord : c'est celle qu'on
                                // voit d'une vue de trois quarts.
                                e.faces
                                    .get(&tf_mesh::Face::PlusY)
                                    .or_else(|| e.faces.values().next())
                                    .and_then(|fd| atlas.couche(&fd.texture))
                            })
                        })
                    })
                })
                .unwrap_or(0)
        })
        .collect();

    let arene = Arene::depuis(&chantier, &|id| {
        couche_de.get(id as usize).copied().unwrap_or(0)
    });

    // ── dessiner
    let app = Appareil::ouvrir().expect("un adaptateur graphique");
    println!("adaptateur : {}", app.decrire());
    let atlas_gpu = AtlasGpu::avec_mips(&app, atlas.cote, atlas.len() as u32, &atlas.pyramide());
    let cible = Cible::nouvelle(&app, cote, (cote * 5) / 8);
    let scene = Scene::nouvelle(&app, &arene, &atlas_gpu);

    let (min, max) = arene.bornes().expect("l'arène doit avoir du contenu");
    let aspect = cible.largeur as f32 / cible.hauteur as f32;
    // Deux cadrages : le build entier, et un coin de PRÈS. Le premier dit si
    // la géométrie tient, le second si les textures sont les bonnes — une vue
    // de loin les réduit à quelques pixels et cache tout.
    let large = Camera::cadrer(min, max, aspect);
    let coin = [
        (min[0] + max[0]) * 0.5,
        max[1] - 6.0,
        (min[2] + max[2]) * 0.5,
    ];
    let pres = Camera::cadrer(
        [coin[0] - 7.0, coin[1] - 7.0, coin[2] - 7.0],
        [coin[0] + 7.0, coin[1] + 7.0, coin[2] + 7.0],
        aspect,
    );
    let camera = if std::env::var("TF_PRES").is_ok() {
        pres
    } else {
        large
    };

    let t = Instant::now();
    let (pixels, compte) = scene.rendre(&cible, &camera);
    let t_rendu = t.elapsed();

    println!("build            : {} × {} blocs", b.side * 16, b.side * 16);
    println!(
        "atlas            : {} couches de {}",
        atlas.len(),
        atlas.cote
    );
    println!(
        "mailler          : {:.0} ms",
        t_maille.as_secs_f64() * 1000.0
    );
    println!("quads            : {}", chantier.quads());
    println!(
        "poses de modèles : {} (pas encore dessinées)",
        chantier.poses()
    );
    println!(
        "arène            : {} instances, {:.2} Mo",
        arene.len(),
        arene.octets() as f64 / 1e6
    );
    println!("APPELS DE DESSIN : {}", compte.appels_de_dessin);
    println!(
        "rendu            : {:.0} ms (rastériseur LOGICIEL — ce temps ne vaut rien)",
        t_rendu.as_secs_f64() * 1000.0
    );

    let f = std::fs::File::create(&sortie).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), cible.largeur, cible.hauteur);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&pixels).unwrap();
    println!("→ {sortie}");
}
