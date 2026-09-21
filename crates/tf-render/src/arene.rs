//! L'arène GPU : un seul tampon pour tous les quads.
//!
//! Un tampon par section donnerait un appel de dessin par section — 24 576 sur
//! une région. La référence à battre est 1 281 appels, mesurée sur
//! `ExeWorldEdit` ; la cible est **moins de cinq**.
//!
//! Tous les quads vivent donc dans UN tampon, et chaque section occupe une
//! TRANCHE nommée. C'est ce qui permettra plus tard de remailler une section
//! sans toucher aux autres, et de ne dessiner que les tranches visibles avec un
//! seul `multi_draw_indirect`.

use bytemuck::{Pod, Zeroable};
use tf_mesh::{Adresse, Chantier};

/// Ce qu'une instance porte au GPU. **16 octets**, et pas un de plus.
///
/// Elle en faisait 32, et c'est la mesure qui a désigné ce champ : sur une
/// région bâtie, l'arène gloutonne pèse 132 Mo — **trois fois** la passe de
/// modèles, qu'on venait pourtant d'optimiser. Le raisonnement est celui de la
/// pose : un quad tient dans sa SECTION, il n'a aucun besoin d'une position en
/// flottants monde.
///
/// La mémoire n'est pas un confort ici : la fenêtre de résidence est plafonnée
/// en OCTETS (invariant n° 7), donc diviser l'arène par deux double ce qu'on
/// peut tenir résident.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct InstanceQuad {
    /// `x | y<<5 | z<<10 | (l−1)<<15 | (h−1)<<19 | face<<23`.
    ///
    /// Positions et tailles en **BLOCS**, locales à la section : 0..16 tient
    /// sur cinq bits, une taille de 1..16 sur quatre, la face sur trois — 26
    /// bits en tout. Un quad glouton tombe toujours sur des bords de bloc,
    /// c'est ce qui rend l'entier possible ; la passe de modèles, elle, garde
    /// ses flottants parce que 17 % des coordonnées du pack ne sont pas
    /// entières.
    pub geo: u32,
    pub couche: u32,
    /// Ce par quoi multiplier le texel, en RGBA8. **Pas une couleur** : un
    /// FACTEUR, qui vaut `0xFFFFFFFF` sur une face non teintée.
    ///
    /// Les textures teintées du jeu sont GRISES — `grass_block_top.png` vaut
    /// (147, 147, 147) — et c'est le jeu qui les multiplie par une couleur de
    /// biome. Sans ce champ, le sol de tout terrain sort blanchâtre : la
    /// texture s'affiche, simplement pas de la bonne couleur.
    pub teinte: u32,
    /// Quelle section — donc quelle origine. Le même index que la passe de
    /// modèles : les deux arènes parcourent les lots du chantier dans le même
    /// ordre, et un test le fige.
    pub section: u32,
}

/// Empaquette la géométrie d'un quad glouton.
///
/// Les seizièmes du mailleur redeviennent des blocs. Un quad qui ne tomberait
/// pas sur un bord de bloc serait TRONQUÉ ici, silencieusement — d'où
/// l'assertion : c'est une propriété de la passe gloutonne, pas une chance.
pub fn empaqueter(min: [f32; 3], taille: [f32; 2], face: u32) -> u32 {
    let bloc = |v: f32| {
        debug_assert!(
            (0.0..=256.0).contains(&v) && (v / 16.0).fract() == 0.0,
            "un quad glouton tombe sur un bord de bloc, pas sur {v} seizièmes"
        );
        (v / 16.0) as u32
    };
    let (x, y, z) = (bloc(min[0]), bloc(min[1]), bloc(min[2]));
    let (l, h) = (bloc(taille[0]), bloc(taille[1]));
    debug_assert!(l >= 1 && h >= 1 && l <= 16 && h <= 16, "taille {l} × {h}");
    x | (y << 5) | (z << 10) | ((l - 1) << 15) | ((h - 1) << 19) | (face << 23)
}

/// L'inverse, pour les bornes et les tests. Rend
/// `(position en blocs, taille en blocs, face)`.
pub fn depaqueter(geo: u32) -> ([u32; 3], [u32; 2], u32) {
    (
        [geo & 31, (geo >> 5) & 31, (geo >> 10) & 31],
        [((geo >> 15) & 15) + 1, ((geo >> 19) & 15) + 1],
        (geo >> 23) & 7,
    )
}

/// Un facteur `0..1` par canal, empaqueté en RGBA8.
///
/// Huit bits suffisent : c'est la précision de la texture qu'il multiplie.
pub(crate) fn en_rgba8(t: [f32; 3]) -> u32 {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    c(t[0]) | (c(t[1]) << 8) | (c(t[2]) << 16) | (0xFF << 24)
}

/// Une tranche de l'arène : ce qu'une section occupe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tranche {
    pub adresse: Adresse,
    pub debut: u32,
    pub nombre: u32,
}

impl Tranche {
    pub fn est_vide(&self) -> bool {
        self.nombre == 0
    }
}

/// Les instances de tout un chantier, à plat, avec la carte des tranches.
#[derive(Debug, Default)]
pub struct Arene {
    pub instances: Vec<InstanceQuad>,
    pub tranches: Vec<Tranche>,
    /// L'origine de chaque section, indexée comme les lots du chantier.
    ///
    /// Partagée avec la passe de modèles : deux tables se décaleraient le jour
    /// où l'une saute une section vide, et tout un pan du build se dessinerait
    /// ailleurs.
    pub origines: Vec<crate::modeles::Origine>,
}

impl Arene {
    /// Empile un chantier. `apparence` dit, pour un état et une FACE, quelle
    /// tuile d'atlas l'habille et par quoi multiplier son texel.
    ///
    /// Par face, et pas seulement par état : un modèle déclare une texture par
    /// face, et prendre celle du dessus habille les côtés d'un bloc d'herbe
    /// avec de l'herbe.
    ///
    /// Le troisième argument est le BIOME de la case. Il vaut zéro partout où
    /// le bloc n'en prend pas la couleur — la fusion gloutonne ne coupe un
    /// quad sur une frontière de biome que pour les états teintés. Un
    /// appelant qui n'a pas de biomes passe donc la même fonction qu'avant et
    /// l'ignore.
    pub fn depuis(
        chantier: &Chantier,
        apparence: &dyn Fn(
            tf_anvil::StateId,
            tf_mesh::forme::Face,
            tf_anvil::StateId,
        ) -> (u32, [f32; 3]),
    ) -> Arene {
        let mut a = Arene {
            origines: crate::modeles::origines(chantier),
            ..Arene::default()
        };
        for (section, lot) in chantier.lots.iter().enumerate() {
            let debut = a.instances.len() as u32;
            for q in &lot.quads.quads {
                let (couche, teinte) = apparence(q.id, q.face, q.biome);
                a.instances.push(InstanceQuad {
                    geo: empaqueter(q.min, q.taille, q.face as u32),
                    couche,
                    teinte: en_rgba8(teinte),
                    // Le quad est LOCAL à sa section : sans l'origine, tout le
                    // monde se dessinerait empilé sur la section zéro.
                    section: section as u32,
                });
            }
            a.tranches.push(Tranche {
                adresse: lot.adresse,
                debut,
                nombre: a.instances.len() as u32 - debut,
            });
        }
        a
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn octets(&self) -> usize {
        std::mem::size_of_val(&self.instances[..])
    }

    /// Bornes du contenu, en blocs monde. `None` si l'arène est vide.
    pub fn bornes(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for i in &self.instances {
            let (p, _, _) = depaqueter(i.geo);
            let o = match self.origines.get(i.section as usize) {
                Some(o) => o.position,
                None => continue,
            };
            for k in 0..3 {
                let v = o[k] / 16.0 + p[k] as f32;
                min[k] = min[k].min(v);
                max[k] = max[k].max(v);
            }
        }
        if min[0] > max[0] {
            return None;
        }
        // Un quad s'étend au-delà de son coin : sans ça, la boîte serait trop
        // petite d'un bloc sur chaque axe et la caméra couperait le bord.
        for m in max.iter_mut() {
            *m += 1.0;
        }
        Some((min, max))
    }
}
