//! La fenêtre : `winit` + `egui`, et le même rendu que la capture.
//!
//! **La répartition des boutons est fixe** (`tf_render::controles`) : molette
//! enfoncée pour tourner, + Maj pour le panoramique, molette roulée pour
//! avancer. Gauche et droit restent aux outils — donner la caméra au clic
//! droit coûterait l'un des deux coins de WorldEdit.
//!
//! Le piège d'`ExeWorldEdit` — *un outil qui coupe la caméra entière enferme
//! l'utilisateur* — ne peut pas se produire : la caméra a un bouton à elle,
//! qu'aucun outil ne prend.

use std::sync::Arc;

use tf_app::etat::Etat;
use tf_app::etat::Outil;
use tf_app::moteur::Moteur;
use tf_app::{interface, scene};
use tf_render::controles::Mode;
use tf_render::{Appareil, AtlasGpu, Scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Radians par pixel de souris. Réglé comme Minecraft à sensibilité moyenne :
/// une habitude de jeu ne se rééduque pas, elle se sert.
const SENSIBILITE: f32 = 0.0035;
/// Blocs par cran de molette, en FACTEUR de la distance au regard — un pas
/// fixe est trop lent pour traverser un build et traverse l'objet d'un cran
/// quand on est contre.
const PAS_AVANT: f32 = 2.0;

pub fn lancer(ouvert: scene::Ouvert, larg: u32, haut: u32) {
    let boucle = match EventLoop::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("pas de serveur graphique ({e}) — utiliser --capture pour une image");
            std::process::exit(1);
        }
    };
    boucle.set_control_flow(ControlFlow::Poll);
    // **Le fil moteur démarre AVANT la fenêtre.** S'il ne démarre pas, autant
    // le savoir tout de suite : une coque qui ouvre une fenêtre et découvre
    // ensuite qu'elle ne peut rien éditer aurait menti par omission.
    let chemin = std::path::PathBuf::from(&ouvert.nom);
    let moteur = ouvert.staging.clone().map(|st| {
        Moteur::lancer(
            st,
            tf_world::Dimension::Overworld,
            tf_world::journal::Journal::new(),
            Some(chemin),
        )
    });
    let mut app = Coque {
        ouvert,
        moteur,
        remailler: false,
        taille: (larg, haut),
        fenetre: None,
        gpu: None,
    };
    if let Err(e) = boucle.run_app(&mut app) {
        eprintln!("boucle interrompue : {e}");
    }
}

struct Gpu {
    appareil: Appareil,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scene: Scene,
    egui: egui::Context,
    etat_egui: egui_winit::State,
    peintre: egui_wgpu::Renderer,
    profondeur: wgpu::TextureView,
    etat: Etat,
    /// Touches maintenues, pour le vol.
    avance: [bool; 6],
    /// La molette est-elle ENFONCÉE ? C'est elle qui tourne la caméra.
    tourne: bool,
    maj: bool,
    /// Ctrl est-il tenu ? Regardé, jamais supposé : sans lui, « Z » répondrait
    /// aussi à Ctrl+Z.
    ctrl: bool,
    souris: Option<(f64, f64)>,
}

struct Coque {
    ouvert: scene::Ouvert,
    /// `None` pour la fixture : elle n'a pas de save derrière elle.
    moteur: Option<Moteur>,
    /// Le monde a changé : il faut relire la zone et remailler.
    ///
    /// Un drapeau et non un appel immédiat : plusieurs réponses peuvent
    /// arriver dans la même image, et remailler trois fois de suite coûterait
    /// trois fois pour le même résultat.
    remailler: bool,
    taille: (u32, u32),
    fenetre: Option<Arc<Window>>,
    gpu: Option<Gpu>,
}

impl ApplicationHandler for Coque {
    fn resumed(&mut self, evb: &ActiveEventLoop) {
        if self.fenetre.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("titiforge")
            .with_inner_size(winit::dpi::LogicalSize::new(self.taille.0, self.taille.1));
        let f = match evb.create_window(attrs) {
            Ok(f) => Arc::new(f),
            Err(e) => {
                eprintln!("fenêtre impossible : {e}");
                evb.exit();
                return;
            }
        };
        match preparer(&f, &self.ouvert.monde) {
            Ok(g) => {
                self.gpu = Some(g);
                self.fenetre = Some(f);
            }
            Err(e) => {
                eprintln!("{e}");
                evb.exit();
            }
        }
    }

    fn window_event(&mut self, evb: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        let (Some(f), Some(g)) = (self.fenetre.as_ref(), self.gpu.as_mut()) else {
            return;
        };
        // egui voit l'événement EN PREMIER : s'il l'a consommé (un curseur sur
        // un panneau), la caméra ne doit pas bouger derrière.
        let pris = g.etat_egui.on_window_event(f, &ev).consumed;

        match ev {
            WindowEvent::CloseRequested => evb.exit(),
            WindowEvent::Resized(t) => {
                g.config.width = t.width.max(1);
                g.config.height = t.height.max(1);
                g.surface.configure(&g.appareil.device, &g.config);
                // La profondeur se recrée : restée à l'ancienne taille, elle
                // fait silencieusement échouer la passe.
                g.profondeur =
                    tf_render::scene::profondeur(&g.appareil, g.config.width, g.config.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let bas = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(c) = event.physical_key {
                    match c {
                        // Échap abandonne d'abord le geste en cours. Quitter
                        // l'application au milieu d'un tirage serait une
                        // surprise coûteuse.
                        KeyCode::Escape if bas => {
                            if !g.etat.abandonner() {
                                evb.exit();
                            }
                        }
                        // **Ctrl est REGARDÉ, pas supposé.** « Z » qui répond
                        // aussi à Ctrl+Z, c'est une annulation qui change
                        // d'outil au passage — piège payé dans
                        // `ExeWorldEdit`, et la parade est de comparer les
                        // modificateurs ABSENTS autant que les présents.
                        KeyCode::KeyZ if bas && g.ctrl => {
                            g.etat.demande = Some(tf_app::moteur::Commande::Annuler);
                        }
                        KeyCode::KeyY if bas && g.ctrl => {
                            g.etat.demande = Some(tf_app::moteur::Commande::Refaire);
                        }
                        KeyCode::ControlLeft | KeyCode::ControlRight => g.ctrl = bas,
                        KeyCode::KeyW => g.avance[0] = bas,
                        KeyCode::KeyZ => g.avance[0] = bas,
                        KeyCode::KeyS => g.avance[1] = bas,
                        KeyCode::KeyA | KeyCode::KeyQ => g.avance[2] = bas,
                        KeyCode::KeyD => g.avance[3] = bas,
                        KeyCode::Space => g.avance[4] = bas,
                        KeyCode::ShiftLeft => {
                            g.avance[5] = bas;
                            g.maj = bas;
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // **La molette ENFONCÉE, et elle seule, pour la caméra.**
                // Gauche et droit restent aux outils — c'est le partage fixe,
                // et c'est ce qui empêche le piège d'`ExeWorldEdit` : un outil
                // qui coupe la caméra entière enferme l'utilisateur.
                if button == MouseButton::Middle {
                    g.tourne = state == ElementState::Pressed;
                } else if state == ElementState::Pressed && !pris {
                    match (g.etat.mode, button) {
                        // **Édition** : gauche = coin 1, droit = coin 2. La
                        // convention de WorldEdit, que la main de tout
                        // constructeur connaît.
                        (Mode::Edition, MouseButton::Left) => {
                            g.etat.poser_coin(true);
                        }
                        (Mode::Edition, MouseButton::Right) => {
                            g.etat.poser_coin(false);
                        }
                        // **Conception** : l'OUTIL décide. Le mode dit ce
                        // qu'on manipule, l'outil dit avec quoi.
                        (Mode::Conception, b) => {
                            let (cam, aspect) = vue_courante(g);
                            match (g.etat.outil, b) {
                                (Outil::Tirer, MouseButton::Left) => {
                                    g.etat.attraper(&cam, aspect);
                                }
                                (Outil::Tirer, MouseButton::Right) => {
                                    g.etat.abandonner();
                                }
                                (Outil::Poser, MouseButton::Left)
                                | (Outil::Casser, MouseButton::Right) => {
                                    g.etat.demande = g.etat.poser_un_bloc();
                                }
                                (Outil::Poser, MouseButton::Right)
                                | (Outil::Casser, MouseButton::Left) => {
                                    g.etat.demande = g.etat.casser_un_bloc();
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                } else if state == ElementState::Released
                    && button == MouseButton::Left
                    && g.etat.tirage.is_some()
                {
                    // Le trait part au moteur au RELÂCHEMENT, pas à chaque
                    // image : sinon une poignée tirée de vingt blocs
                    // produirait vingt entrées de journal, et vingt Ctrl+Z
                    // pour les défaire.
                    g.etat.demande = g.etat.lacher();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x, position.y);
                // Un tirage en cours suit la souris — c'est le seul geste qui
                // lit sa position à l'écran plutôt que le réticule, parce que
                // c'est une POIGNÉE qu'on déplace, pas une visée.
                if g.etat.tirage.is_some() && !pris {
                    let (cam, aspect) = vue_courante(g);
                    let ndc = [
                        (p.0 as f32 / g.config.width as f32) * 2.0 - 1.0,
                        1.0 - (p.1 as f32 / g.config.height as f32) * 2.0,
                    ];
                    g.etat.tirer(&cam, aspect, ndc);
                }
                if let (Some(a), true) = (g.souris, g.tourne && !pris) {
                    let (dx, dy) = ((p.0 - a.0) as f32, (p.1 - a.1) as f32);
                    if g.maj {
                        // Panoramique : le PLAN de l'écran.
                        g.etat.vue.glisser(-dx * 0.05, dy * 0.05);
                    } else {
                        // Tourner : le regard pivote, l'œil ne bouge pas.
                        g.etat.vue.tourner(dx * SENSIBILITE, -dy * SENSIBILITE);
                    }
                }
                g.souris = Some(p);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !pris {
                    let n = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                    g.etat.vue.deplacer(n * PAS_AVANT, 0.0, 0.0);
                }
            }
            WindowEvent::RedrawRequested => {
                voler(g);
                // **Ramasser AVANT de dessiner.** Une réponse arrivée pendant
                // l'image précédente doit être à l'écran maintenant, pas dans
                // une image de plus : « le bouton met du temps à répondre » et
                // « le bouton ne répond pas » se ressemblent trop.
                self.remailler |= ramasser(&mut self.moteur, &mut g.etat);
                g.etat.occupe = self.moteur.as_ref().is_some_and(|m| m.occupe());
                g.etat.editable = self.ouvert.editable();
                if let Err(e) = dessiner(f, g, &self.ouvert.monde) {
                    eprintln!("image perdue : {e}");
                }
                // Ce que l'interface a décidé pendant le dessin part
                // maintenant : le fil travaillera pendant l'image suivante.
                envoyer(&mut self.moteur, &mut g.etat);
                if self.remailler {
                    self.remailler = false;
                    if let Err(e) = self.ouvert.remailler() {
                        g.etat.message = format!("remaillage : {e}");
                    } else if let Err(e) = regarnir(g, &self.ouvert.monde) {
                        g.etat.message = format!("remaillage : {e}");
                    }
                }
            }
            _ => {}
        }
        f.request_redraw();
    }
}

/// La caméra et le rapport d'image du moment. Deux endroits en avaient besoin,
/// et les recopier aurait fini par donner deux champs de vision différents
/// selon le geste.
fn vue_courante(g: &Gpu) -> (tf_render::Camera, f32) {
    let aspect = g.config.width as f32 / g.config.height as f32;
    let cam = g.etat.vue.camera(&tf_render::Camera {
        oeil: [0.0; 3],
        cible: [0.0, 0.0, 1.0],
        fov: 50f32.to_radians(),
        proche: 0.1,
        loin: 4096.0,
    });
    (cam, aspect)
}

/// Ramasse ce que le fil a rendu, et le met à l'écran.
///
/// **Ne bloque jamais** : c'est toute la raison d'être du fil. Une réponse qui
/// n'est pas encore là ne coûte rien, et l'image suivante la trouvera.
fn ramasser(m: &mut Option<Moteur>, e: &mut Etat) -> bool {
    let Some(moteur) = m else { return false };
    let mut bouge = false;
    for r in moteur.recevoir() {
        e.message = r.texte();
        // **Des bornes, donc des blocs ont changé.** C'est le seul critère :
        // une opération qui n'a rien écrit ne rend pas de bornes, et
        // remailler pour rien coûterait la zone entière à chaque clic.
        bouge |= r.bornes().is_some();
    }
    bouge
}

/// Envoie ce que l'interface a demandé pendant l'image.
fn envoyer(m: &mut Option<Moteur>, e: &mut Etat) {
    let Some(demande) = e.demande.take() else {
        return;
    };
    let Some(moteur) = m else {
        e.message = "la fixture n'a pas de save derrière elle — ouvrir un monde \
                     avec --monde"
            .into();
        return;
    };
    if !moteur.envoyer(demande) {
        e.message = "le moteur s'est arrêté".into();
    }
}

/// Reconstruit ce que le GPU dessine après un remaillage.
///
/// Le matériau et l'atlas changent avec la scène : un bloc qui apparaît pour
/// la première fois amène sa texture, et l'atlas ne monte que les textures des
/// blocs PRÉSENTS. Garder l'ancien afficherait la mauvaise tuile.
fn regarnir(g: &mut Gpu, m: &scene::Monde) -> Result<(), String> {
    let atlas = AtlasGpu::avec_mips(
        &g.appareil,
        m.atlas.cote,
        m.atlas.len() as u32,
        &m.atlas.pyramide(),
    );
    g.scene = Scene::pour(&g.appareil, &m.arene, &m.modeles, &atlas, g.config.format);
    Ok(())
}
fn voler(g: &mut Gpu) {
    let v = 0.6;
    let a = (g.avance[0] as i32 - g.avance[1] as i32) as f32;
    let c = (g.avance[3] as i32 - g.avance[2] as i32) as f32;
    let h = (g.avance[4] as i32 - g.avance[5] as i32) as f32;
    if a != 0.0 || c != 0.0 || h != 0.0 {
        g.etat.vue.deplacer(a * v, c * v, h * v);
    }
}

fn preparer(f: &Arc<Window>, m: &scene::Monde) -> Result<Gpu, String> {
    let appareil = Appareil::ouvrir().map_err(|e| format!("pas d'adaptateur : {e}"))?;
    let surface = appareil
        .instance
        .create_surface(f.clone())
        .map_err(|e| format!("surface : {e}"))?;
    let t = f.inner_size();
    let mut config = surface
        .get_default_config(&appareil.adaptateur, t.width.max(1), t.height.max(1))
        .ok_or("format de surface non supporté")?;
    // **Le format se NÉGOCIE, il ne se force pas.** Sur X11 avec le pilote
    // logiciel, la surface ne propose que `Bgra8UnormSrgb` : lui imposer le
    // format du hors-écran fait paniquer wgpu à la configuration. Trouvé par
    // un simple essai sous Xvfb — aucun test de rendu ne pouvait le voir,
    // puisqu'ils dessinent tous dans une texture qu'on choisit.
    //
    // Ce qui ne se négocie PAS est le sRGB : un format linéaire ferait sortir
    // toutes les couleurs autrement, sans la moindre erreur.
    let formats = surface.get_capabilities(&appareil.adaptateur).formats;
    config.format = *formats
        .iter()
        .find(|f| f.is_srgb())
        .ok_or("aucun format sRGB proposé par la surface")?;
    config.view_formats = vec![config.format];
    surface.configure(&appareil.device, &config);

    let atlas = AtlasGpu::avec_mips(
        &appareil,
        m.atlas.cote,
        m.atlas.len() as u32,
        &m.atlas.pyramide(),
    );
    let scene = Scene::pour(&appareil, &m.arene, &m.modeles, &atlas, config.format);
    let egui = egui::Context::default();
    let etat_egui = egui_winit::State::new(
        egui.clone(),
        egui::ViewportId::ROOT,
        f.as_ref(),
        None,
        None,
        None,
    );
    let peintre = egui_wgpu::Renderer::new(&appareil.device, config.format, None, 1, false);
    let aspect = config.width as f32 / config.height as f32;
    let profondeur = tf_render::scene::profondeur(&appareil, config.width, config.height);
    Ok(Gpu {
        profondeur,
        etat: Etat::cadre(m.min, m.max, aspect),
        appareil,
        surface,
        config,
        scene,
        egui,
        etat_egui,
        peintre,
        avance: [false; 6],
        tourne: false,
        maj: false,
        ctrl: false,
        souris: None,
    })
}

fn dessiner(f: &Arc<Window>, g: &mut Gpu, m: &scene::Monde) -> Result<(), String> {
    let image = g
        .surface
        .get_current_texture()
        .map_err(|e| format!("surface : {e}"))?;
    let vue = image
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let (camera, aspect) = vue_courante(g);
    g.etat.relever_reticule(&camera, aspect, 256.0, &m.solide());

    let oeil = tf_world::coords::BlockPos::new(
        camera.oeil[0] as i32,
        camera.oeil[1] as i32,
        camera.oeil[2] as i32,
    );
    let y = (m.min[1] as i32, m.max[1] as i32);
    let mut lignes = scene::quadrillage(&g.etat.quadrillage, oeil, y);
    lignes
        .sommets
        .extend(scene::contour_selection(&g.etat.selection).sommets);
    g.scene.poser_lignes(&lignes);

    g.scene.dessiner_sur(
        &vue,
        &g.profondeur,
        g.config.width,
        g.config.height,
        &camera,
    );

    let entree = g.etat_egui.take_egui_input(f.as_ref());
    let sortie = g
        .egui
        .run(entree, |ctx| interface::dessiner(ctx, &mut g.etat));
    g.etat_egui
        .handle_platform_output(f.as_ref(), sortie.platform_output.clone());
    let primitives = g.egui.tessellate(sortie.shapes, sortie.pixels_per_point);
    let desc = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [g.config.width, g.config.height],
        pixels_per_point: sortie.pixels_per_point,
    };
    let mut enc = g
        .appareil
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui"),
        });
    // Les deltas de CETTE image : l'atlas de polices arrive dans le premier,
    // et un delta sauté laisse toutes les images suivantes référencer une
    // texture jamais montée — sans erreur, et sans rien dessiner.
    for (id, delta) in &sortie.textures_delta.set {
        g.peintre
            .update_texture(&g.appareil.device, &g.appareil.queue, *id, delta);
    }
    g.peintre.update_buffers(
        &g.appareil.device,
        &g.appareil.queue,
        &mut enc,
        &primitives,
        &desc,
    );
    {
        let mut passe = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &vue,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        g.peintre.render(&mut passe, &primitives, &desc);
    }
    g.appareil.queue.submit([enc.finish()]);
    image.present();
    for id in &sortie.textures_delta.free {
        g.peintre.free_texture(id);
    }
    Ok(())
}
