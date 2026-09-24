//! La passe qui dessine les quads, et la cible hors écran.
//!
//! **Un seul appel de dessin** pour toute l'arène : les quads sont des
//! instances d'un même quad unitaire, et la géométrie se déduit de la face.
//! La référence à battre est 1 281 appels, mesurée sur `ExeWorldEdit`.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::arene::Arene;
use crate::camera::{Camera, CameraGpu};
use crate::Appareil;

/// Le format de la cible HORS ÉCRAN.
///
/// **La surface d'une fenêtre n'offre pas forcément celui-là**, et c'est une
/// leçon payée : sur X11 avec le pilote logiciel, la surface ne propose que
/// `Bgra8UnormSrgb` — la forcer fait paniquer wgpu à la configuration.
/// `Scene::pour` prend donc le format en paramètre, et la fenêtre lui donne
/// celui que sa surface accepte. Ce qui ne se négocie PAS est le sRGB : un
/// format linéaire ferait sortir toutes les couleurs autrement, sans erreur.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const PROFONDEUR: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Ce qu'un rendu a coûté. Ce sont ces chiffres qu'on compare, pas les images
/// par seconde d'un rastériseur logiciel — celles-là ne valent rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Compte {
    pub appels_de_dessin: u32,
    pub instances: u32,
}

pub struct Scene {
    appareil: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    liaison: wgpu::BindGroup,
    camera: wgpu::Buffer,
    instances: wgpu::Buffer,
    nombre: u32,
    /// La passe de MODÈLES. Absente quand la scène n'a aucun bloc-modèle —
    /// une passe qui ne dessine rien reste un changement de pipeline.
    modeles: Option<PasseModeles>,
    /// Le QUADRILLAGE — chunks, `.mca`, sélection. Absent par défaut : une
    /// scène qui n'en demande pas n'en paie pas.
    lignes: Option<PasseLignes>,
    /// Le format de la cible. Gardé parce que le quadrillage se pose APRÈS la
    /// scène et doit construire son pipeline pour la même.
    format: wgpu::TextureFormat,
}

/// Ce que le quadrillage tient au GPU.
struct PasseLignes {
    pipeline: wgpu::RenderPipeline,
    liaison: wgpu::BindGroup,
    sommets: wgpu::Buffer,
    nombre: u32,
}

/// Ce que la passe de modèles tient au GPU.
struct PasseModeles {
    pipeline: wgpu::RenderPipeline,
    liaison: wgpu::BindGroup,
    /// Nombre de FACES à dessiner — une instance chacune.
    faces: u32,
}

impl Scene {
    /// Prépare la passe : l'atlas monte au GPU, les instances aussi.
    pub fn nouvelle(app: &Appareil, arene: &Arene, atlas: &AtlasGpu) -> Scene {
        Scene::avec_modeles(app, arene, &crate::AreneModeles::default(), atlas)
    }

    /// La scène complète : les cubes gloutons ET les blocs-modèles.
    pub fn avec_modeles(
        app: &Appareil,
        arene: &Arene,
        modeles: &crate::AreneModeles,
        atlas: &AtlasGpu,
    ) -> Scene {
        Scene::pour(app, arene, modeles, atlas, FORMAT)
    }

    /// La même, pour un format de cible donné.
    ///
    /// La fenêtre s'en sert avec le format de SA surface : sur X11 et pilote
    /// logiciel, elle ne propose que `Bgra8UnormSrgb`, et forcer le format du
    /// hors-écran fait paniquer wgpu. Le sRGB, lui, ne se négocie pas.
    pub fn pour(
        app: &Appareil,
        arene: &Arene,
        modeles: &crate::AreneModeles,
        atlas: &AtlasGpu,
        format: wgpu::TextureFormat,
    ) -> Scene {
        debug_assert!(
            format.is_srgb(),
            "une cible non sRGB ferait sortir toutes les couleurs autrement, \
             sans la moindre erreur"
        );
        let device = app.device.clone();
        let queue = app.queue.clone();

        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instances = tampon_pages(
            &device,
            "arène",
            wgpu::BufferUsages::VERTEX,
            &arene.instances,
        );

        // UNE table d'origines pour les deux passes. Deux se décaleraient le
        // jour où l'une saute une section vide, et tout un pan du build se
        // dessinerait ailleurs — sans la moindre erreur.
        let origines = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("origines de section"),
            // Un tampon de stockage VIDE est refusé par wgpu, et une scène
            // vide est un cas de test parfaitement légitime.
            contents: if arene.origines().is_empty() {
                &[0u8; 16]
            } else {
                bytemuck::cast_slice(arene.origines())
            },
            usage: wgpu::BufferUsages::STORAGE,
        });

        let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scène"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        // Un TABLEAU, pas une planche : la répétition d'un quad
                        // glouton ne peut alors pas mordre sur la tuile
                        // voisine.
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let liaison = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scène"),
            layout: &disposition,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.vue),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.echantillonneur),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: origines.as_entire_binding(),
                },
            ],
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scène"),
            bind_group_layouts: &[&disposition],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quads"),
            layout: Some(&agencement),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::arene::InstanceQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Uint32, 1 => Uint32, 2 => Unorm8x4, 3 => Uint32
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // **Pas de culling par orientation.** Le mailleur n'émet que
                // des faces visibles et ne garantit pas leur sens de rotation :
                // trier par orientation en ferait disparaître la moitié, et
                // c'est le genre de défaut qu'on met des heures à voir.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: PROFONDEUR,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let passe_modeles = (!modeles.is_empty())
            .then(|| PasseModeles::nouvelle(&device, &disposition, modeles, format));

        Scene {
            appareil: device,
            queue,
            pipeline,
            liaison,
            camera,
            instances,
            nombre: arene.len() as u32,
            modeles: passe_modeles,
            lignes: None,
            format,
        }
    }

    /// Pose (ou remplace) le quadrillage.
    ///
    /// Séparé du constructeur exprès : le quadrillage CHANGE à chaque
    /// déplacement de la caméra — il suit le joueur — alors que l'arène et
    /// l'atlas ne bougent pas. Les faire naître ensemble obligerait à
    /// reconstruire la scène entière pour déplacer une grille.
    ///
    /// Vide le retire.
    pub fn poser_lignes(&mut self, lignes: &crate::Lignes) {
        self.lignes = (!lignes.is_empty())
            .then(|| PasseLignes::nouvelle(&self.appareil, &self.camera, lignes, self.format));
    }

    /// Dessine dans une cible hors écran et rend l'image en RGBA8.
    /// **LA passe, écrite une fois.** La fenêtre et la capture hors écran
    /// n'en ont pas deux : deux copies finiraient par montrer deux images
    /// différentes, et on comparerait ce qui ne se compare pas.
    fn passe(
        &self,
        enc: &mut wgpu::CommandEncoder,
        couleur: &wgpu::TextureView,
        profondeur: &wgpu::TextureView,
    ) {
        let mut passe = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("quads"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: couleur,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.055,
                        g: 0.075,
                        b: 0.086,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: profondeur,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        passe.set_pipeline(&self.pipeline);
        passe.set_bind_group(0, &self.liaison, &[]);
        passe.set_vertex_buffer(0, self.instances.slice(..));
        // UN appel pour toute l'arène.
        passe.draw(0..6, 0..self.nombre);

        // Et UN pour tous les blocs-modèles, quel que soit leur nombre de
        // faces : la pose porte le rang de sa première face, le sommet
        // retrouve la sienne par dichotomie.
        if let Some(m) = &self.modeles {
            passe.set_pipeline(&m.pipeline);
            passe.set_bind_group(0, &self.liaison, &[]);
            passe.set_bind_group(1, &m.liaison, &[]);
            passe.draw(0..6, 0..m.faces);
        }

        // **Le quadrillage passe en DERNIER, et sans test de
        // profondeur.** Un repère qui disparaît derrière le mur qu'on est
        // en train d'aligner n'est pas un repère : c'est un calque, il se
        // dessine par-dessus. La contrepartie — une ligne lointaine peut
        // recouvrir ce qui est devant — est tenue par le rayon borné du
        // découpage, qui ne montre que le voisinage.
        if let Some(l) = &self.lignes {
            passe.set_pipeline(&l.pipeline);
            passe.set_bind_group(0, &l.liaison, &[]);
            passe.set_vertex_buffer(0, l.sommets.slice(..));
            passe.draw(0..l.nombre, 0..1);
        }
    }

    /// Dessine sur des vues QUELCONQUES — la surface d'une fenêtre, par
    /// exemple.
    ///
    /// La cible par défaut reste une texture (`rendre`) : un moteur qui ne
    /// sait dessiner que dans une fenêtre ne se teste pas. Celle-ci est le
    /// même code avec d'autres attachements, et c'est ce qui garantit que la
    /// fenêtre et la capture montrent la MÊME image — deux passes séparées
    /// divergeraient, et on comparerait deux choses qui ne veulent pas dire la
    /// même chose.
    pub fn dessiner_sur(
        &self,
        couleur: &wgpu::TextureView,
        profondeur: &wgpu::TextureView,
        largeur: u32,
        hauteur: u32,
        camera: &Camera,
    ) -> Compte {
        let aspect = largeur as f32 / hauteur.max(1) as f32;
        self.queue
            .write_buffer(&self.camera, 0, bytemuck::bytes_of(&camera.gpu(aspect)));
        let mut enc = self
            .appareil
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scène"),
            });
        self.passe(&mut enc, couleur, profondeur);
        self.queue.submit([enc.finish()]);
        self.compte()
    }

    fn compte(&self) -> Compte {
        Compte {
            appels_de_dessin: 1
                + u32::from(self.modeles.is_some())
                + u32::from(self.lignes.is_some()),
            instances: self.nombre + self.modeles.as_ref().map_or(0, |m| m.faces),
        }
    }

    pub fn rendre(&self, cible: &Cible, camera: &Camera) -> (Vec<u8>, Compte) {
        let aspect = cible.largeur as f32 / cible.hauteur as f32;
        self.queue
            .write_buffer(&self.camera, 0, bytemuck::bytes_of(&camera.gpu(aspect)));

        let mut enc = self
            .appareil
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scène"),
            });
        self.passe(&mut enc, &cible.couleur, &cible.profondeur);
        cible.copier(&mut enc);
        self.queue.submit([enc.finish()]);

        (
            cible.relire(&self.appareil),
            Compte {
                appels_de_dessin: 1
                    + u32::from(self.modeles.is_some())
                    + u32::from(self.lignes.is_some()),
                instances: self.nombre + self.modeles.as_ref().map_or(0, |m| m.faces),
            },
        )
    }
}

impl PasseModeles {
    fn nouvelle(
        device: &wgpu::Device,
        commun: &wgpu::BindGroupLayout,
        a: &crate::AreneModeles,
        format: wgpu::TextureFormat,
    ) -> PasseModeles {
        let tampon = |nom: &str, octets: &[u8]| {
            // Un tampon de stockage VIDE est refusé par wgpu, et une arène peut
            // n'avoir aucune origine si toutes ses sections sont sans modèle.
            // Seize octets de rien coûtent moins qu'une branche par usage.
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(nom),
                contents: if octets.is_empty() {
                    &[0u8; 16]
                } else {
                    octets
                },
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let faces = tampon("faces de modèle", bytemuck::cast_slice(&a.faces));
        let poses = tampon_pages(device, "poses", wgpu::BufferUsages::STORAGE, &a.poses);
        let lecture = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("modèles"),
            entries: &[lecture(0), lecture(1)],
        });
        let liaison = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("modèles"),
            layout: &disposition,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: faces.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: poses.as_entire_binding(),
                },
            ],
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modèles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("modeles.wgsl").into()),
        });
        let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("modèles"),
            bind_group_layouts: &[commun, &disposition],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("modèles"),
            layout: Some(&agencement),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                compilation_options: Default::default(),
                // AUCUN tampon de sommets : la géométrie se lit dans les
                // tampons de stockage. C'est ce qui permet qu'une pose ne pèse
                // que seize octets.
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Pas de tri par orientation, même raison que la passe
                // gloutonne : un modèle ne garantit pas le sens de ses faces.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: PROFONDEUR,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        PasseModeles {
            pipeline,
            liaison,
            faces: a.faces_a_dessiner,
        }
    }
}

/// **Un tampon GPU rempli page par page** depuis un tableau par pages.
///
/// Sans copie intermédiaire : recoller les pages dans un `Vec` contigu pour
/// le seul plaisir de `create_buffer_init` referait exactement la recopie de
/// toute la scène que les pages existent pour éviter. Le tampon est projeté
/// en mémoire à la création, chaque page y est écrite à son décalage.
///
/// Seize octets au moins : wgpu refuse un tampon de taille nulle, et une
/// scène vide est un cas de test légitime.
fn tampon_pages<T: bytemuck::Pod>(
    device: &wgpu::Device,
    nom: &str,
    usage: wgpu::BufferUsages,
    p: &crate::pages::Pages<T>,
) -> wgpu::Buffer {
    let octets = p.len() * std::mem::size_of::<T>();
    let taille = (octets.max(16) as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
    let tampon = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(nom),
        size: taille,
        usage,
        mapped_at_creation: true,
    });
    {
        let mut vue = tampon.slice(..).get_mapped_range_mut();
        let mut o = 0;
        for page in p.pages() {
            let b: &[u8] = bytemuck::cast_slice(page);
            vue[o..o + b.len()].copy_from_slice(b);
            o += b.len();
        }
    }
    tampon.unmap();
    tampon
}

/// L'atlas, monté au GPU en texture-TABLEAU.
pub struct AtlasGpu {
    pub vue: wgpu::TextureView,
    pub echantillonneur: wgpu::Sampler,
    pub couches: u32,
}

impl AtlasGpu {
    /// `pixels` porte `couches` images RGBA8 de `cote × cote`.
    pub fn nouveau(app: &Appareil, cote: u32, couches: u32, pixels: &[u8]) -> AtlasGpu {
        Self::avec_mips(app, cote, couches, &[(cote, pixels.to_vec())])
    }

    /// Avec sa pyramide de mips, un niveau par entrée.
    ///
    /// Sans eux, tout build vu de loin est du BRUIT : une texture de 16 × 16
    /// écrasée dans dix pixels d'écran échantillonne un pixel sur deux, et le
    /// résultat scintille à chaque mouvement de caméra.
    pub fn avec_mips(
        app: &Appareil,
        cote: u32,
        couches: u32,
        pyramide: &[(u32, Vec<u8>)],
    ) -> AtlasGpu {
        let couches = couches.max(1);
        // **Un tableau de textures est plafonné**, et pas très haut : 2 048
        // couches sur la carte comme sur le pilote logiciel. Le pack du
        // serveur en cite 2 207 — monter tout le catalogue dépasse la limite
        // ET paie des tuiles qu'aucun bloc de la scène n'emploie.
        //
        // Sans cette garde, wgpu lève une erreur de VALIDATION qui parle de
        // `depth_or_array_layers` : exact, et illisible pour qui vient de
        // charger une save. On dit ce qui dépasse et quoi faire.
        let plafond = app.device.limits().max_texture_array_layers;
        assert!(
            couches <= plafond,
            "atlas de {couches} couches pour un plafond de {plafond} : ne montez \
             que les textures des blocs PRÉSENTS (`textures_des_etats`), pas \
             tout le catalogue"
        );
        let niveaux = pyramide.len().max(1) as u32;
        let texture = app.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: cote,
                height: cote,
                depth_or_array_layers: couches,
            },
            mip_level_count: niveaux,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (niveau, (c, donnees)) in pyramide.iter().enumerate() {
            let attendu = (c * c * 4 * couches) as usize;
            let mut d = donnees.clone();
            d.resize(attendu, 0);
            app.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: niveau as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &d,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(c * 4),
                    rows_per_image: Some(*c),
                },
                wgpu::Extent3d {
                    width: *c,
                    height: *c,
                    depth_or_array_layers: couches,
                },
            );
        }
        AtlasGpu {
            vue: texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            // Au PLUS PROCHE VOISIN. Minecraft est en pixels nets ; un
            // filtrage linéaire ferait baver chaque bord de bloc, et le
            // résultat ne ressemblerait plus au jeu.
            echantillonneur: app.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("atlas"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                // AGRANDIR au plus proche voisin : Minecraft est en pixels
                // nets, et un filtrage linéaire ferait baver chaque bord de
                // bloc. RÉDUIRE par les mips, en revanche, est ce qui empêche
                // un mur de pierre vu de loin de ressembler à de la neige.
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                lod_min_clamp: 0.0,
                lod_max_clamp: 32.0,
                ..Default::default()
            }),
            couches,
        }
    }
}

/// Une cible hors écran, relisible en mémoire.
/// Une texture de PROFONDEUR seule, pour dessiner sur une surface de fenêtre.
///
/// La surface fournit sa couleur ; la profondeur, non. Elle se recrée au
/// redimensionnement — une profondeur restée à l'ancienne taille fait
/// silencieusement échouer la passe.
pub fn profondeur(app: &Appareil, largeur: u32, hauteur: u32) -> wgpu::TextureView {
    app.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("profondeur de fenêtre"),
            size: wgpu::Extent3d {
                width: largeur.max(1),
                height: hauteur.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PROFONDEUR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

pub struct Cible {
    pub largeur: u32,
    pub hauteur: u32,
    pub couleur: wgpu::TextureView,
    pub profondeur: wgpu::TextureView,
    texture: wgpu::Texture,
    lecture: wgpu::Buffer,
    /// Les copies GPU exigent des lignes alignées sur 256 octets. Ignorer ça
    /// rend une image DÉCALÉE d'un peu plus à chaque ligne — une image en
    /// escalier, parfaitement plausible et fausse.
    pas: u32,
}

impl Cible {
    pub fn nouvelle(app: &Appareil, largeur: u32, hauteur: u32) -> Cible {
        let taille = wgpu::Extent3d {
            width: largeur,
            height: hauteur,
            depth_or_array_layers: 1,
        };
        let texture = app.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cible"),
            size: taille,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let profondeur = app.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("profondeur"),
            size: taille,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PROFONDEUR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let pas = (largeur * 4).div_ceil(256) * 256;
        let lecture = app.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lecture"),
            size: (pas * hauteur) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Cible {
            largeur,
            hauteur,
            couleur: texture.create_view(&Default::default()),
            profondeur: profondeur.create_view(&Default::default()),
            texture,
            lecture,
            pas,
        }
    }

    /// Copie la couleur vers le tampon relisible.
    ///
    /// Publique parce que la COQUE compose son interface dans une seconde
    /// cible avant de la mélanger : un rendu hors écran qui ne se relit que
    /// depuis son propre crate n'est plus un rendu hors écran, c'est un détail
    /// interne.
    pub fn copier(&self, enc: &mut wgpu::CommandEncoder) {
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.lecture,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.pas),
                    rows_per_image: Some(self.hauteur),
                },
            },
            wgpu::Extent3d {
                width: self.largeur,
                height: self.hauteur,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn relire(&self, device: &wgpu::Device) -> Vec<u8> {
        let tranche = self.lecture.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        tranche.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().ok();
        let vue = tranche.get_mapped_range();
        // On retire le rembourrage d'alignement : l'image rendue fait
        // `largeur * 4`, pas `pas`.
        let mut out = Vec::with_capacity((self.largeur * self.hauteur * 4) as usize);
        for y in 0..self.hauteur {
            let d = (y * self.pas) as usize;
            out.extend_from_slice(&vue[d..d + (self.largeur * 4) as usize]);
        }
        drop(vue);
        self.lecture.unmap();
        out
    }
}

impl PasseLignes {
    fn nouvelle(
        device: &wgpu::Device,
        camera: &wgpu::Buffer,
        lignes: &crate::Lignes,
        format: wgpu::TextureFormat,
    ) -> PasseLignes {
        // Une disposition à ELLE : le quadrillage n'a besoin que de la
        // caméra. Réutiliser celle de la scène l'obligerait à déclarer
        // l'atlas et les origines de section, c'est-à-dire à dépendre de
        // choses qu'il n'emploie pas — et à casser le jour où l'une d'elles
        // change.
        let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quadrillage"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let liaison = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quadrillage"),
            layout: &disposition,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });

        let sommets = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quadrillage"),
            contents: bytemuck::cast_slice(&lignes.sommets),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lignes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lignes.wgsl").into()),
        });
        let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quadrillage"),
            bind_group_layouts: &[&disposition],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quadrillage"),
            layout: Some(&agencement),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::lignes::Sommet>() as u64,
                    // Par SOMMET, pas par instance : chaque paire est un
                    // segment distinct, il n'y a rien à répéter.
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                cull_mode: None,
                ..Default::default()
            },
            // **Ni test ni écriture de profondeur.** L'attachement est quand
            // même déclaré : la passe de rendu en a un, et un pipeline qui ne
            // le déclarerait pas serait incompatible avec elle. `Always` et
            // `false` disent exactement « je suis un calque ».
            depth_stencil: Some(wgpu::DepthStencilState {
                format: PROFONDEUR,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        PasseLignes {
            pipeline,
            liaison,
            sommets,
            nombre: lignes.sommets.len() as u32,
        }
    }
}
