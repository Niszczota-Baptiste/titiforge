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
        let device = app.device.clone();
        let queue = app.queue.clone();

        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("arène"),
            contents: bytemuck::cast_slice(&arene.instances),
            usage: wgpu::BufferUsages::VERTEX,
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
                        0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Uint32,
                        4 => Unorm8x4
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
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
            .then(|| PasseModeles::nouvelle(&device, &disposition, modeles, FORMAT));

        Scene {
            appareil: device,
            queue,
            pipeline,
            liaison,
            camera,
            instances,
            nombre: arene.len() as u32,
            modeles: passe_modeles,
        }
    }

    /// Dessine dans une cible hors écran et rend l'image en RGBA8.
    pub fn rendre(&self, cible: &Cible, camera: &Camera) -> (Vec<u8>, Compte) {
        let aspect = cible.largeur as f32 / cible.hauteur as f32;
        self.queue
            .write_buffer(&self.camera, 0, bytemuck::bytes_of(&camera.gpu(aspect)));

        let mut enc = self
            .appareil
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scène"),
            });
        {
            let mut passe = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("quads"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cible.couleur,
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
                    view: &cible.profondeur,
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
        }
        cible.copier(&mut enc);
        self.queue.submit([enc.finish()]);

        (
            cible.relire(&self.appareil),
            Compte {
                appels_de_dessin: 1 + u32::from(self.modeles.is_some()),
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
        let poses = tampon("poses", bytemuck::cast_slice(&a.poses));
        let origines = tampon("origines de section", bytemuck::cast_slice(&a.origines));

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
            entries: &[lecture(0), lecture(1), lecture(2)],
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: origines.as_entire_binding(),
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

    fn copier(&self, enc: &mut wgpu::CommandEncoder) {
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

    fn relire(&self, device: &wgpu::Device) -> Vec<u8> {
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
