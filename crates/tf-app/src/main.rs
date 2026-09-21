//! **titiforge — la coque.**
//!
//! ```text
//! titiforge <assets> [--monde <dossier>] [--zone "cx0,cz0,cx1,cz1"]
//! titiforge <assets> --capture sortie.png [--taille 1400x900]
//! ```
//!
//! **`--capture` n'est pas un mode dégradé.** La coque sait se dessiner dans
//! une TEXTURE, exactement comme le rendu : c'est ce qui permet de la vérifier
//! au pixel, et de montrer à quoi elle ressemble depuis une machine qui n'a
//! pas d'écran. Un morceau d'interface qui n'existe que derrière un serveur
//! graphique ne se teste pas.

use tf_app::etat::Etat;
use tf_app::{interface, scene};
use tf_render::{Appareil, AtlasGpu, Cible, Scene};

fn main() {
    let mut args = std::env::args().skip(1);
    let racine = match args.next() {
        Some(r) => r,
        None => {
            eprintln!(
                "usage : titiforge <assets> [--monde <dossier>] [--zone \"cx0,cz0,cx1,cz1\"]\n\
                 \x20       titiforge <assets> --capture sortie.png [--taille 1400x900]\n\n\
                 <assets> : un codex extrait, un pack, ou une INSTALLATION de launcher.\n\
                 Le genre se reconnaît au CONTENU — demander de le choisir serait\n\
                 demander de connaître nos formats."
            );
            std::process::exit(2);
        }
    };
    let mut monde: Option<String> = None;
    let mut zone = [0i32, 0, 1, 1];
    let mut capture: Option<String> = None;
    let mut mode = tf_render::controles::Mode::Edition;
    let (mut larg, mut haut) = (1400u32, 900u32);
    while let Some(o) = args.next() {
        match o.as_str() {
            "--monde" => monde = args.next(),
            "--mode" => {
                if let Some(m) = args.next() {
                    mode = match m.as_str() {
                        "conception" | "c" => tf_render::controles::Mode::Conception,
                        _ => tf_render::controles::Mode::Edition,
                    };
                }
            }
            "--capture" => capture = args.next(),
            "--zone" => {
                let v: Vec<i32> = args
                    .next()
                    .unwrap_or_default()
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if v.len() == 4 {
                    zone = [
                        v[0].min(v[2]),
                        v[1].min(v[3]),
                        v[0].max(v[2]),
                        v[1].max(v[3]),
                    ];
                }
            }
            "--taille" => {
                if let Some(t) = args.next() {
                    let v: Vec<u32> = t.split('x').filter_map(|s| s.parse().ok()).collect();
                    if v.len() == 2 {
                        larg = v[0].max(64);
                        haut = v[1].max(64);
                    }
                }
            }
            autre => eprintln!("option inconnue ignorée : {autre}"),
        }
    }

    let m = match scene::charger(&racine, monde.as_deref(), zone) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("scène : {} · {} quads, {} poses", m.quoi, m.quads, m.poses);

    match capture {
        Some(png) => capturer(&m, &png, larg, haut, mode),
        None => fenetre(m, larg, haut),
    }
}

/// Une image de l'interface, sans écran.
fn capturer(
    m: &scene::Monde,
    sortie: &str,
    larg: u32,
    haut: u32,
    mode: tf_render::controles::Mode,
) {
    let app = match Appareil::ouvrir() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pas d'adaptateur graphique : {e}");
            std::process::exit(1);
        }
    };
    println!("adaptateur : {}", app.decrire());
    let aspect = larg as f32 / haut as f32;
    let mut etat = Etat::cadre(m.min, m.max, aspect);
    etat.mode = mode;
    // Une sélection de démonstration : l'interface n'a rien à montrer sans
    // elle, et un panneau vide ne dit pas ce qu'il saurait dire. Posée en
    // SAILLIE au-dessus du build, parce qu'un contour noyé dans trois cents
    // segments de quadrillage ne se distingue pas — ce qui est un défaut de
    // la démonstration, pas du calque.
    //
    // Les coordonnées se DÉRIVENT des bornes réelles, jamais devinées : la
    // première écriture posait la sélection à y = 64 alors que la fixture vit
    // autour de y = −10. Les douze segments étaient bien émis, et hors champ —
    // une absence qui ne se voit pas, une fois de plus.
    let demo = tf_world::coords::BlockPos::new;
    let c = |k: usize| ((m.min[k] + m.max[k]) * 0.5) as i32;
    let quart = |k: usize| (((m.max[k] - m.min[k]) * 0.25) as i32).max(1);
    etat.selection.poser_coin1(demo(
        c(0) - quart(0),
        m.max[1] as i32 - 2 * quart(1),
        c(2) - quart(2),
    ));
    etat.selection.poser_coin2(demo(
        c(0) + quart(0),
        m.max[1] as i32 + quart(1),
        c(2) + quart(2),
    ));
    etat.quadrillage.chunks = Some(1);
    etat.quadrillage.mca = Some(0);

    let cible = Cible::nouvelle(&app, larg, haut);
    let atlas = AtlasGpu::avec_mips(
        &app,
        m.atlas.cote,
        m.atlas.len() as u32,
        &m.atlas.pyramide(),
    );
    let mut sc = Scene::avec_modeles(&app, &m.arene, &m.modeles, &atlas);

    let camera = etat.vue.camera(&modele_camera(m));
    etat.relever_reticule(&camera, aspect, 256.0, &m.solide());

    // Le quadrillage suit le JOUEUR, pas le build : c'est autour de lui qu'on
    // veut voir le découpage.
    let oeil = tf_world::coords::BlockPos::new(
        camera.oeil[0] as i32,
        camera.oeil[1] as i32,
        camera.oeil[2] as i32,
    );
    let y = (m.min[1] as i32, m.max[1] as i32);
    let mut lignes = scene::quadrillage(&etat.quadrillage, oeil, y);
    let quadrillage = lignes.len();
    lignes
        .sommets
        .extend(scene::contour_selection(&etat.selection).sommets);
    println!(
        "calque : {quadrillage} segment(s) de découpage, {} de sélection",
        lignes.len() - quadrillage
    );
    if lignes.len() > quadrillage {
        let v = &lignes.sommets[quadrillage * 2];
        println!("  premier sommet de sélection : {:?}", v.position);
        println!("  oeil : {:?} · cible : {:?}", camera.oeil, camera.cible);
    }
    sc.poser_lignes(&lignes);

    let (pixels, compte) = sc.rendre(&cible, &camera);
    println!(
        "viewport : {} appels de dessin, {} instances",
        compte.appels_de_dessin, compte.instances
    );

    // ── l'interface par-dessus, dans la MÊME texture
    let pixels = dessiner_interface(&app, &mut etat, larg, haut, pixels);

    ecrire_png(sortie, larg, haut, &pixels);
    println!("→ {sortie}");
}

/// Le modèle de caméra : champ de vision et plans. Le pilotage ne les connaît
/// pas — il ne décide que d'où l'on est et où l'on regarde.
fn modele_camera(m: &scene::Monde) -> tf_render::Camera {
    let d = (0..3)
        .map(|k| (m.max[k] - m.min[k]).abs())
        .fold(1.0f32, f32::max);
    tf_render::Camera {
        oeil: [0.0; 3],
        cible: [0.0, 0.0, 1.0],
        fov: 50f32.to_radians(),
        proche: 0.1,
        loin: d * 16.0 + 512.0,
    }
}

/// Dessine egui dans une texture et le compose sur l'image du viewport.
///
/// egui rend dans SA propre cible puis on mélange : deux passes dans la même
/// texture demanderaient de partager un format et une profondeur, et le
/// viewport a déjà les siens. Composer en mémoire est plus simple et
/// exactement aussi vérifiable.
fn dessiner_interface(
    app: &Appareil,
    etat: &mut Etat,
    larg: u32,
    haut: u32,
    fond: Vec<u8>,
) -> Vec<u8> {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.0);
    let entree = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(larg as f32, haut as f32),
        )),
        ..Default::default()
    };
    // DEUX passes : egui est immédiat, et certains widgets (les panneaux qui
    // se mesurent) n'ont leur taille définitive qu'au second tour. Capturer la
    // première donnerait une image que l'utilisateur ne voit jamais.
    let premiere = ctx.run(entree.clone(), |ctx| interface::dessiner(ctx, etat));
    let sortie = ctx.run(entree, |ctx| interface::dessiner(ctx, etat));

    let mut peintre =
        egui_wgpu::Renderer::new(&app.device, tf_render::scene::FORMAT, None, 1, false);
    let primitives = ctx.tessellate(sortie.shapes, 1.0);
    let desc = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [larg, haut],
        pixels_per_point: 1.0,
    };
    let mut enc = app
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui"),
        });
    // **Les deltas des DEUX passes.** L'atlas de polices est créé à la
    // PREMIÈRE image et n'apparaît que dans son delta ; jeter celui-là laisse
    // toutes les images suivantes référencer une texture qui n'a jamais été
    // montée. egui-wgpu ne s'en plaint pas — il ne dessine simplement rien, et
    // l'image reste parfaitement plausible.
    for (id, delta) in premiere
        .textures_delta
        .set
        .iter()
        .chain(sortie.textures_delta.set.iter())
    {
        peintre.update_texture(&app.device, &app.queue, *id, delta);
    }
    peintre.update_buffers(&app.device, &app.queue, &mut enc, &primitives, &desc);

    let cible = Cible::nouvelle(app, larg, haut);
    {
        let mut passe = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cible.couleur,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // TRANSPARENT : ce qui n'est pas de l'interface laisse
                        // voir le viewport dessous.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        peintre.render(&mut passe, &primitives, &desc);
    }
    cible.copier(&mut enc);
    app.queue.submit([enc.finish()]);
    let calque = cible.relire(&app.device);

    // Composition « source par-dessus », en prémultiplié : egui rend déjà ses
    // couleurs prémultipliées par l'alpha, donc `dst = src + dst × (1 − a)`.
    // Le refaire en non prémultiplié borderait chaque texte d'un halo.
    let mut out = fond;
    for (o, c) in out.chunks_exact_mut(4).zip(calque.chunks_exact(4)) {
        let a = c[3] as u32;
        for k in 0..3 {
            o[k] = (c[k] as u32 + (o[k] as u32 * (255 - a)) / 255).min(255) as u8;
        }
        o[3] = 255;
    }
    out
}

fn ecrire_png(chemin: &str, larg: u32, haut: u32, pixels: &[u8]) {
    let f = std::fs::File::create(chemin).expect("le PNG doit pouvoir s'écrire");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), larg, haut);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(pixels).unwrap();
}

#[cfg(not(feature = "fenetre"))]
fn fenetre(_m: scene::Monde, _l: u32, _h: u32) {
    eprintln!("compilé sans la fenêtre — utiliser --capture");
    std::process::exit(2);
}

#[cfg(feature = "fenetre")]
fn fenetre(m: scene::Monde, larg: u32, haut: u32) {
    crate::coque::lancer(m, larg, haut);
}

#[cfg(feature = "fenetre")]
mod coque;
