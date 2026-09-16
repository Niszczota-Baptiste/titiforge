//! Rend un build Minefield dans un PNG, **sans écran**.
//!
//! ```text
//! cargo run --release -p tf-render --example capture -- ../titisite/public/codex sortie.png
//! cargo run --release -p tf-render --example capture -- ../titisite/public/codex sortie.png --monde D:\\monde --zone "4,7,10,12"
//! ```
//!
//! Sans `--monde`, c'est la fixture `Build` qui est rendue. Avec, c'est une
//! VRAIE save — et `--zone` la borne au chunk près, parce qu'il n'existe aucun
//! état « le monde est chargé » : une région pleine fait déjà 100 millions de
//! blocs.

use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_assets::catalogue::{blocs_translucides, table_formes, textures_citees, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};
use tf_bench::{build, Build};
use tf_mesh::Grille;
use tf_render::{Appareil, Arene, AtlasGpu, Camera, Cible, Scene};
use tf_world::{sections_de, BBox, BlockPos, Dimension, Folder, FsSource};

fn main() {
    let mut args = std::env::args().skip(1);
    let racine = args.next().expect(
        "usage : capture <codex> <sortie.png> [côté] [--monde <dir>] [--zone \"cx0,cz0,cx1,cz1\"]",
    );
    let sortie = args.next().unwrap_or_else(|| "capture.png".into());
    let mut cote: u32 = 1000;
    let mut monde: Option<String> = None;
    let mut zone: Option<[i32; 4]> = None;
    while let Some(o) = args.next() {
        match o.as_str() {
            "--monde" => monde = args.next(),
            "--zone" => {
                let v: Vec<i32> = args
                    .next()
                    .unwrap_or_default()
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                assert_eq!(v.len(), 4, "--zone attend cx0,cz0,cx1,cz1");
                zone = Some([
                    v[0].min(v[2]),
                    v[1].min(v[3]),
                    v[0].max(v[2]),
                    v[1].max(v[3]),
                ]);
            }
            autre => cote = autre.parse().unwrap_or(cote),
        }
    }

    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);

    // ── la scène : une vraie save, ou la fixture
    let mut grille = Grille::new();
    let mut interner = Interner::new();
    let quoi = match &monde {
        Some(dir) => {
            let src = FsSource::open(dir).expect("monde lisible");
            // Une emprise BORNÉE, toujours : il n'existe aucun état « le monde
            // est chargé ». Sans --zone, on prend le premier 2 × 2 chunks, ce
            // qui est un aperçu et pas un défaut à étendre.
            let [x0, z0, x1, z1] = zone.unwrap_or([0, 0, 1, 1]);
            let sel = BBox::new(
                BlockPos {
                    x: x0 * 16,
                    y: -64,
                    z: z0 * 16,
                },
                BlockPos {
                    x: x1 * 16 + 15,
                    y: 319,
                    z: z1 * 16 + 15,
                },
            );
            let bilan = sections_de(
                &src,
                &Dimension::Overworld,
                Folder::Region,
                &sel,
                &mut interner,
                |s| grille.poser(s.chunk.x, s.chunk.z, s.section),
            );
            println!(
                "monde : {dir} · chunks {x0}..{x1} × {z0}..{z1} · {} chunks, {} sections lues",
                bilan.chunks, bilan.sections
            );
            format!("{} × {} blocs", (x1 - x0 + 1) * 16, (z1 - z0 + 1) * 16)
        }
        None => {
            let b = Build {
                side: 4,
                sections: 5,
                ..Build::default()
            };
            let octets = build::region(&b);
            let r = read(&octets, 0, 0).unwrap();
            for cz in 0..b.side as i32 {
                for cx in 0..b.side as i32 {
                    let brut = r.get(cx, cz).unwrap();
                    let inflated = inflate(&brut.payload, brut.compression).unwrap();
                    let sc = scan(&inflated).unwrap();
                    for s in &sc.sections {
                        if let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap()
                        {
                            grille.poser(cx, cz, sec);
                        }
                    }
                }
            }
            format!("fixture · {} × {} blocs", b.side * 16, b.side * 16)
        }
    };

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

    println!("scène            : {quoi}");
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
