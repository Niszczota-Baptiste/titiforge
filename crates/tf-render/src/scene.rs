//! La passe qui dessine les quads, et la cible hors écran.
//!
//! **Un seul appel de dessin** pour toute l'arène : les quads sont des
//! instances d'un même quad unitaire, et la géométrie se déduit de la face.
//! La référence à battre est 1 281 appels, mesurée sur `ExeWorldEdit`.
//!
//! **La scène se TIENT, elle ne se refait pas.** Ses tampons vivent aussi
//! longtemps qu'elle, grandissent au GPU quand l'arène grandit, et ne
//! reçoivent que ce que les arènes disent avoir réécrit (`synchroniser`). La
//! reconstruire à chaque arrivée renvoyait toute la scène, recompilait les
//! pipelines et remontait l'atlas avec ses mips — pour trois sections.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::arene::Arene;
use crate::camera::{Camera, CameraGpu};
use crate::modeles::{AreneModeles, FaceModele, Origine, Pose};
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
    /// Groupe 0 : caméra, atlas, échantillonneur. Ne se refait que si
    /// l'ATLAS change (`changer_atlas`).
    disposition_commune: wgpu::BindGroupLayout,
    commune: wgpu::BindGroup,
    camera: wgpu::Buffer,
    /// Groupe 1 : la table des origines, PARTAGÉE par les deux passes. Deux
    /// tables se décaleraient le jour où l'une saute une section vide, et
    /// tout un pan du build se dessinerait ailleurs — sans la moindre erreur.
    disposition_origines: wgpu::BindGroupLayout,
    liaison_origines: wgpu::BindGroup,
    origines: Tampon,
    /// Origines que le GPU tient à jour.
    nb_origines: usize,
    instances: Tampon,
    nombre: u32,
    modeles: PasseModeles,
    /// Le QUADRILLAGE — chunks, `.mca`, sélection. Absent par défaut : une
    /// scène qui n'en demande pas n'en paie pas.
    lignes: Option<PasseLignes>,
    /// Le format de la cible. Gardé parce que le quadrillage se pose APRÈS la
    /// scène et doit construire son pipeline pour la même.
    format: wgpu::TextureFormat,
    /// Octets envoyés au GPU depuis la création. **Le compteur qui prouve
    /// qu'une synchronisation paie ce qui a changé**, et pas la scène.
    envoyes: u64,
    /// Combien de fois un tampon a dû grandir.
    agrandissements: u32,
    /// Écritures envoyées depuis la création — une par plage contiguë.
    ecritures: u64,
}

/// Ce que le quadrillage tient au GPU.
struct PasseLignes {
    pipeline: wgpu::RenderPipeline,
    liaison: wgpu::BindGroup,
    /// Les lignes telles qu'on les a posées, dans le monde.
    brutes: crate::Lignes,
    /// Ce qui en reste dans le champ de la DERNIÈRE caméra — la place de
    /// toutes les brutes, puisque découper n'en ajoute jamais.
    sommets: wgpu::Buffer,
    /// Combien de sommets de `sommets` sont à dessiner. Une `Cell` : la
    /// découpe se fait au dessin, qui prend `&self`.
    nombre: std::cell::Cell<u32>,
}

/// Ce que la passe de modèles tient au GPU.
///
/// Toujours là, même sans le moindre bloc-modèle : c'est son APPEL qui se
/// saute quand il n'y a rien à dessiner — une passe vide reste un changement
/// de pipeline. La construire à la demande obligerait à la refaire le jour où
/// le premier escalier arrive par le streaming.
struct PasseModeles {
    pipeline: wgpu::RenderPipeline,
    disposition: wgpu::BindGroupLayout,
    liaison: wgpu::BindGroup,
    faces: Tampon,
    nb_faces: usize,
    poses: Tampon,
    nb_poses: usize,
    /// Nombre de FACES à dessiner — une instance chacune, trous compris.
    a_dessiner: u32,
}

/// Un tampon GPU qui GRANDIT sans repartir de zéro.
///
/// Sa capacité croît par moitiés, et l'ancien contenu passe dans le nouveau
/// par une copie GPU → GPU : ni le fil principal ni le bus ne revoient ce
/// qu'ils avaient déjà envoyé.
struct Tampon {
    buf: wgpu::Buffer,
    capacite: u64,
    usage: wgpu::BufferUsages,
    nom: &'static str,
}

impl Tampon {
    /// Seize octets : wgpu refuse un tampon de taille nulle, et une scène
    /// vide est un cas de test légitime.
    fn vide(device: &wgpu::Device, nom: &'static str, usage: wgpu::BufferUsages) -> Tampon {
        let usage = usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        Tampon {
            buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(nom),
                size: 16,
                usage,
                mapped_at_creation: false,
            }),
            capacite: 16,
            usage,
            nom,
        }
    }

    /// Assez de place pour `octets`, en gardant les `garder` premiers.
    ///
    /// Rend vrai si le TAMPON a changé : ce qui le lie doit alors être refait.
    /// La copie est enregistrée dans `enc`, qui doit partir AVANT toute
    /// écriture dans le nouveau tampon — voir `Scene::envoyer`.
    fn assurer(
        &mut self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        octets: u64,
        garder: u64,
    ) -> bool {
        if octets <= self.capacite {
            return false;
        }
        let capacite = octets
            .max(self.capacite + self.capacite / 2)
            .next_multiple_of(16);
        let neuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.nom),
            size: capacite,
            usage: self.usage,
            mapped_at_creation: false,
        });
        let garder = garder.min(self.capacite) & !(wgpu::COPY_BUFFER_ALIGNMENT - 1);
        if garder > 0 {
            enc.copy_buffer_to_buffer(&self.buf, 0, &neuf, 0, garder);
        }
        self.buf = neuf;
        self.capacite = capacite;
        true
    }

    /// Écrit `donnees` à partir de l'élément `debut`. Rend les octets écrits.
    fn ecrire<T: bytemuck::Pod>(&self, queue: &wgpu::Queue, debut: usize, donnees: &[T]) -> u64 {
        if donnees.is_empty() {
            return 0;
        }
        let b: &[u8] = bytemuck::cast_slice(donnees);
        queue.write_buffer(&self.buf, (debut * std::mem::size_of::<T>()) as u64, b);
        b.len() as u64
    }
}

/// Ce qu'une synchronisation doit envoyer, par plages `(début, nombre)`.
struct Sales {
    instances: Vec<(u32, u32)>,
    origines: Vec<(u32, u32)>,
    poses: Vec<(u32, u32)>,
    /// Début de la queue de `faces` à envoyer.
    faces_depuis: usize,
}

/// Des indices triés en plages contiguës.
fn en_plages(indices: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &i in indices {
        match out.last_mut() {
            Some((d, n)) if *d + *n == i => *n += 1,
            _ => out.push((i, 1)),
        }
    }
    out
}

/// Ajoute la plage `debut..fin`, puis trie et recolle : une case couverte
/// deux fois partirait deux fois, et le compteur d'octets mentirait.
fn ajouter_queue(plages: &mut Vec<(u32, u32)>, debut: usize, fin: usize) {
    if fin > debut {
        plages.push((debut as u32, (fin - debut) as u32));
    }
    plages.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(plages.len());
    for &(d, n) in plages.iter() {
        match out.last_mut() {
            Some((pd, pn)) if *pd + *pn >= d => *pn = (*pn).max(d + n - *pd),
            _ => out.push((d, n)),
        }
    }
    *plages = out;
}

fn lecture_seule(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl Scene {
    /// Prépare la passe : l'atlas monte au GPU, les instances aussi.
    pub fn nouvelle(app: &Appareil, arene: &Arene, atlas: &AtlasGpu) -> Scene {
        Scene::avec_modeles(app, arene, &AreneModeles::default(), atlas)
    }

    /// La scène complète : les cubes gloutons ET les blocs-modèles.
    pub fn avec_modeles(
        app: &Appareil,
        arene: &Arene,
        modeles: &AreneModeles,
        atlas: &AtlasGpu,
    ) -> Scene {
        Scene::pour(app, arene, modeles, atlas, FORMAT)
    }

    /// La même, pour un format de cible donné, remplie d'un coup.
    ///
    /// Elle envoie TOUT et ne touche pas aux listes de ce qui a changé : une
    /// scène qui doit ensuite suivre les arènes naît avec [`Scene::vide`] et
    /// les rattrape par [`Scene::synchroniser`], sans rien envoyer deux fois.
    pub fn pour(
        app: &Appareil,
        arene: &Arene,
        modeles: &AreneModeles,
        atlas: &AtlasGpu,
        format: wgpu::TextureFormat,
    ) -> Scene {
        let mut s = Scene::vide(app, atlas, format);
        let tout = Sales {
            instances: vec![(0, arene.len() as u32)],
            origines: vec![(0, arene.origines().len() as u32)],
            poses: vec![(0, modeles.poses.len() as u32)],
            faces_depuis: 0,
        };
        s.envoyer(arene, modeles, tout);
        s
    }

    /// Une scène sans rien à dessiner : les pipelines, l'atlas et des tampons
    /// minuscules. Construite UNE fois ; ce qu'elle dessine arrive ensuite
    /// par [`Scene::synchroniser`].
    ///
    /// La fenêtre s'en sert avec le format de SA surface : sur X11 et pilote
    /// logiciel, elle ne propose que `Bgra8UnormSrgb`, et forcer le format du
    /// hors-écran fait paniquer wgpu. Le sRGB, lui, ne se négocie pas.
    pub fn vide(app: &Appareil, atlas: &AtlasGpu, format: wgpu::TextureFormat) -> Scene {
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

        let disposition_commune =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                            // Un TABLEAU, pas une planche : la répétition d'un
                            // quad glouton ne peut alors pas mordre sur la
                            // tuile voisine.
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
        let commune = liaison_commune(&device, &disposition_commune, &camera, atlas);

        let disposition_origines =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("origines de section"),
                entries: &[lecture_seule(0)],
            });
        let origines = Tampon::vide(&device, "origines de section", wgpu::BufferUsages::STORAGE);
        let liaison_origines = liaison_origines(&device, &disposition_origines, &origines);
        let instances = Tampon::vide(&device, "arène", wgpu::BufferUsages::VERTEX);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scène"),
            bind_group_layouts: &[&disposition_commune, &disposition_origines],
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

        let modeles =
            PasseModeles::nouvelle(&device, &disposition_commune, &disposition_origines, format);

        Scene {
            appareil: device,
            queue,
            pipeline,
            disposition_commune,
            commune,
            camera,
            disposition_origines,
            liaison_origines,
            origines,
            nb_origines: 0,
            instances,
            nombre: 0,
            modeles,
            lignes: None,
            format,
            envoyes: 0,
            agrandissements: 0,
            ecritures: 0,
        }
    }

    /// **Rattrape les arènes : n'envoie que ce qu'elles ont réécrit.**
    ///
    /// Les arènes tiennent la liste de leurs plages sales ; celle-ci les
    /// PREND, donc une scène synchronise UNE paire d'arènes. Une arène neuve
    /// (`depuis`, un rechargement) est sale de bout en bout, et la
    /// synchronisation l'envoie en entier sans rien avoir à deviner.
    ///
    /// Rend les octets envoyés par cet appel.
    pub fn synchroniser(&mut self, arene: &mut Arene, modeles: &mut AreneModeles) -> u64 {
        let (mut instances, emplacements) = arene.prendre_sales();
        let (mut poses, faces_depuis) = modeles.prendre_sales();
        let mut origines = en_plages(&emplacements);
        // **Ce que le GPU n'a jamais tenu est sale, quoi que disent les
        // arènes.** Au-delà de l'ancienne longueur, le tampon porte au mieux
        // une scène périmée : une case que l'arène aurait laissée à sa valeur
        // vide sans la marquer y dessinerait l'ancienne.
        ajouter_queue(&mut instances, self.nombre as usize, arene.len());
        ajouter_queue(&mut origines, self.nb_origines, arene.origines().len());
        ajouter_queue(&mut poses, self.modeles.nb_poses, modeles.poses.len());
        let faces_depuis = faces_depuis.min(self.modeles.nb_faces);
        self.envoyer(
            arene,
            modeles,
            Sales {
                instances,
                origines,
                poses,
                faces_depuis,
            },
        )
    }

    /// Envoie les plages dites, en agrandissant d'abord ce qui doit l'être.
    fn envoyer(&mut self, arene: &Arene, modeles: &AreneModeles, s: Sales) -> u64 {
        const QUAD: usize = std::mem::size_of::<crate::arene::InstanceQuad>();
        const ORIGINE: usize = std::mem::size_of::<Origine>();
        const FACE: usize = std::mem::size_of::<FaceModele>();
        const POSE: usize = std::mem::size_of::<Pose>();
        let device = self.appareil.clone();
        let queue = self.queue.clone();

        let (n, o) = (arene.len(), arene.origines().len());
        let (f, p) = (modeles.faces.len(), modeles.poses.len());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("agrandir la scène"),
        });
        let m = &mut self.modeles;
        let grandi = [
            self.instances.assurer(
                &device,
                &mut enc,
                (n * QUAD) as u64,
                self.nombre as u64 * QUAD as u64,
            ),
            self.origines.assurer(
                &device,
                &mut enc,
                (o * ORIGINE) as u64,
                (self.nb_origines * ORIGINE) as u64,
            ),
            m.faces.assurer(
                &device,
                &mut enc,
                (f * FACE) as u64,
                (m.nb_faces * FACE) as u64,
            ),
            m.poses.assurer(
                &device,
                &mut enc,
                (p * POSE) as u64,
                (m.nb_poses * POSE) as u64,
            ),
        ];
        if grandi.iter().any(|&g| g) {
            // **La copie part AVANT les écritures.** `write_buffer` s'exécute
            // au début de la PROCHAINE soumission, avant ses commandes : si la
            // copie de l'ancien tampon partait avec le dessin, elle écraserait
            // ce qu'on vient d'écrire par l'ancien contenu.
            queue.submit([enc.finish()]);
            self.agrandissements += grandi.iter().filter(|&&g| g).count() as u32;
        }
        if grandi[1] {
            self.liaison_origines =
                liaison_origines(&device, &self.disposition_origines, &self.origines);
        }

        self.ecritures += (s.instances.len()
            + s.origines.len()
            + s.poses.len()
            + usize::from(s.faces_depuis < f)) as u64;
        let mut envoyes = 0u64;
        for &(d, k) in &s.instances {
            let fin = (d as usize + k as usize).min(n);
            for (i, t) in arene.instances.plage(d as usize, fin) {
                envoyes += self.instances.ecrire(&queue, i, t);
            }
        }
        for &(d, k) in &s.origines {
            let fin = (d as usize + k as usize).min(o);
            if (d as usize) < fin {
                envoyes +=
                    self.origines
                        .ecrire(&queue, d as usize, &arene.origines()[d as usize..fin]);
            }
        }
        if s.faces_depuis < f {
            envoyes += m
                .faces
                .ecrire(&queue, s.faces_depuis, &modeles.faces[s.faces_depuis..]);
        }
        for &(d, k) in &s.poses {
            let fin = (d as usize + k as usize).min(p);
            for (i, t) in modeles.poses.plage(d as usize, fin) {
                envoyes += m.poses.ecrire(&queue, i, t);
            }
        }
        // Les poses sont liées à leur taille EXACTE : la liaison se refait
        // dès que leur nombre change, pas seulement quand le tampon grandit.
        if grandi[2] || grandi[3] || p != m.nb_poses {
            m.liaison = liaison_modeles(&device, &m.disposition, &m.faces, &m.poses, p);
        }

        self.nombre = n as u32;
        self.nb_origines = o;
        m.nb_faces = f;
        m.nb_poses = p;
        m.a_dessiner = modeles.faces_a_dessiner;
        self.envoyes += envoyes;
        envoyes
    }

    /// Relie un autre atlas. Les tampons ne bougent pas : c'est l'atlas qui
    /// grandit quand un état jamais vu arrive, pas la scène.
    pub fn changer_atlas(&mut self, atlas: &AtlasGpu) {
        self.commune = liaison_commune(
            &self.appareil,
            &self.disposition_commune,
            &self.camera,
            atlas,
        );
    }

    /// Octets envoyés au GPU depuis la création.
    pub fn octets_envoyes(&self) -> u64 {
        self.envoyes
    }

    /// Combien de fois un tampon a grandi.
    pub fn agrandissements(&self) -> u32 {
        self.agrandissements
    }

    /// Plages envoyées depuis la création : autant d'écritures dans la file.
    pub fn ecritures(&self) -> u64 {
        self.ecritures
    }

    /// Capacités en octets : instances, origines, faces, poses. Pour les
    /// tests, qui doivent pouvoir PROUVER qu'un tampon a de la marge — c'est
    /// là que se cachent les poses périmées.
    #[doc(hidden)]
    pub fn capacites(&self) -> [u64; 4] {
        [
            self.instances.capacite,
            self.origines.capacite,
            self.modeles.faces.capacite,
            self.modeles.poses.capacite,
        ]
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
        passe.set_bind_group(0, &self.commune, &[]);
        passe.set_bind_group(1, &self.liaison_origines, &[]);
        passe.set_vertex_buffer(0, self.instances.buf.slice(..));
        // UN appel pour toute l'arène.
        passe.draw(0..6, 0..self.nombre);

        // Et UN pour tous les blocs-modèles, quel que soit leur nombre de
        // faces : la pose porte le rang de sa première face, le sommet
        // retrouve la sienne par dichotomie.
        let m = &self.modeles;
        if m.a_dessiner > 0 {
            passe.set_pipeline(&m.pipeline);
            passe.set_bind_group(0, &self.commune, &[]);
            passe.set_bind_group(1, &self.liaison_origines, &[]);
            passe.set_bind_group(2, &m.liaison, &[]);
            passe.draw(0..6, 0..m.a_dessiner);
        }

        // **Le quadrillage passe en DERNIER, et sans test de
        // profondeur.** Un repère qui disparaît derrière le mur qu'on est
        // en train d'aligner n'est pas un repère : c'est un calque, il se
        // dessine par-dessus. La contrepartie — une ligne lointaine peut
        // recouvrir ce qui est devant — est tenue par le rayon borné du
        // découpage, qui ne montre que le voisinage.
        if let Some(l) = self.lignes.as_ref().filter(|l| l.nombre.get() > 0) {
            passe.set_pipeline(&l.pipeline);
            passe.set_bind_group(0, &l.liaison, &[]);
            passe.set_vertex_buffer(0, l.sommets.slice(..));
            passe.draw(0..l.nombre.get(), 0..1);
        }
    }

    /// **Découpe les lignes au champ de la caméra qui va dessiner**, et envoie
    /// ce qui en reste. Au dessin et pas à la pose : la caméra est celle-là,
    /// et aucun appelant ne peut oublier de découper — ni découper avec une
    /// autre. Voir `Lignes::dans_le_champ`.
    fn decouper_lignes(&self, camera: &CameraGpu) {
        let Some(l) = &self.lignes else {
            return;
        };
        let dans = l.brutes.dans_le_champ(&camera.vue_projection);
        if !dans.is_empty() {
            self.queue
                .write_buffer(&l.sommets, 0, bytemuck::cast_slice(&dans.sommets));
        }
        l.nombre.set(dans.sommets.len() as u32);
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
        let mut enc = self
            .appareil
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scène"),
            });
        self.dessiner(&mut enc, couleur, profondeur, largeur, hauteur, camera);
        self.queue.submit([enc.finish()]);
        self.compte()
    }

    /// **Le dessin, pour les DEUX entrées** — la fenêtre et la capture.
    /// Tout ce qui dépend de la caméra se prépare ici, une fois : l'uniforme,
    /// et la découpe des lignes. Le recopier dans chaque entrée laisserait la
    /// fenêtre sans découpe le jour où l'une des deux copies changerait, et
    /// seule la capture est vérifiée au pixel.
    fn dessiner(
        &self,
        enc: &mut wgpu::CommandEncoder,
        couleur: &wgpu::TextureView,
        profondeur: &wgpu::TextureView,
        largeur: u32,
        hauteur: u32,
        camera: &Camera,
    ) {
        let gpu = camera.gpu(largeur as f32 / hauteur.max(1) as f32);
        self.queue
            .write_buffer(&self.camera, 0, bytemuck::bytes_of(&gpu));
        self.decouper_lignes(&gpu);
        self.passe(enc, couleur, profondeur);
    }

    fn compte(&self) -> Compte {
        Compte {
            appels_de_dessin: 1
                + u32::from(self.modeles.a_dessiner > 0)
                + u32::from(self.lignes.as_ref().is_some_and(|l| l.nombre.get() > 0)),
            instances: self.nombre + self.modeles.a_dessiner,
        }
    }

    pub fn rendre(&self, cible: &Cible, camera: &Camera) -> (Vec<u8>, Compte) {
        let mut enc = self
            .appareil
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scène"),
            });
        self.dessiner(
            &mut enc,
            &cible.couleur,
            &cible.profondeur,
            cible.largeur,
            cible.hauteur,
            camera,
        );
        cible.copier(&mut enc);
        self.queue.submit([enc.finish()]);

        (cible.relire(&self.appareil), self.compte())
    }
}

fn liaison_commune(
    device: &wgpu::Device,
    disposition: &wgpu::BindGroupLayout,
    camera: &wgpu::Buffer,
    atlas: &AtlasGpu,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scène"),
        layout: disposition,
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
    })
}

fn liaison_origines(
    device: &wgpu::Device,
    disposition: &wgpu::BindGroupLayout,
    origines: &Tampon,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("origines de section"),
        layout: disposition,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: origines.buf.as_entire_binding(),
        }],
    })
}

/// Les faces entières, les poses à leur nombre EXACT (voir `modeles.wgsl`).
/// Au moins une pose : une liaison de taille nulle est refusée, et une scène
/// sans modèle ne lance de toute façon pas l'appel qui la lirait.
fn liaison_modeles(
    device: &wgpu::Device,
    disposition: &wgpu::BindGroupLayout,
    faces: &Tampon,
    poses: &Tampon,
    nb_poses: usize,
) -> wgpu::BindGroup {
    let taille = (nb_poses.max(1) * std::mem::size_of::<Pose>()) as u64;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("modèles"),
        layout: disposition,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: faces.buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &poses.buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(taille),
                }),
            },
        ],
    })
}

impl PasseModeles {
    fn nouvelle(
        device: &wgpu::Device,
        commune: &wgpu::BindGroupLayout,
        origines: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> PasseModeles {
        let faces = Tampon::vide(device, "faces de modèle", wgpu::BufferUsages::STORAGE);
        let poses = Tampon::vide(device, "poses", wgpu::BufferUsages::STORAGE);
        let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("modèles"),
            entries: &[lecture_seule(0), lecture_seule(1)],
        });
        let liaison = liaison_modeles(device, &disposition, &faces, &poses, 0);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("modèles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("modeles.wgsl").into()),
        });
        let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("modèles"),
            bind_group_layouts: &[commune, origines, &disposition],
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
            disposition,
            liaison,
            faces,
            nb_faces: 0,
            poses,
            nb_poses: 0,
            a_dessiner: 0,
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

        // La place de TOUTES les lignes : la découpe au dessin n'en ajoute
        // jamais, elle en retire ou en raccourcit.
        let sommets = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quadrillage"),
            contents: bytemuck::cast_slice(&lignes.sommets),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
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
            brutes: lignes.clone(),
            sommets,
            nombre: std::cell::Cell::new(lignes.sommets.len() as u32),
        }
    }
}
