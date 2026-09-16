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

/// Ce qu'une instance porte au GPU. **32 octets**, et pas un de plus.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct InstanceQuad {
    /// Coin de plus petites coordonnées, en seizièmes de bloc, en MONDE.
    ///
    /// Une région fait 512 blocs, soit 8 192 seizièmes : exact en flottant, et
    /// très loin du premier entier que `f32` ne sait plus représenter.
    pub position: [f32; 3],
    pub taille: [f32; 2],
    pub face: u32,
    pub couche: u32,
    /// Ce par quoi multiplier le texel, en RGBA8. **Pas une couleur** : un
    /// FACTEUR, qui vaut `0xFFFFFFFF` sur une face non teintée.
    ///
    /// Les textures teintées du jeu sont GRISES — `grass_block_top.png` vaut
    /// (147, 147, 147) — et c'est le jeu qui les multiplie par une couleur de
    /// biome. Sans ce champ, le sol de tout terrain sort blanchâtre : la
    /// texture s'affiche, simplement pas de la bonne couleur.
    pub teinte: u32,
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
}

impl Arene {
    /// Empile un chantier. `apparence` dit, pour un état et une FACE, quelle
    /// tuile d'atlas l'habille et par quoi multiplier son texel.
    ///
    /// Par face, et pas seulement par état : un modèle déclare une texture par
    /// face, et prendre celle du dessus habille les côtés d'un bloc d'herbe
    /// avec de l'herbe.
    pub fn depuis(
        chantier: &Chantier,
        apparence: &dyn Fn(tf_anvil::StateId, tf_mesh::forme::Face) -> (u32, [f32; 3]),
    ) -> Arene {
        let mut a = Arene::default();
        for lot in &chantier.lots {
            let debut = a.instances.len() as u32;
            let [ox, oy, oz] = lot.origine();
            for q in &lot.quads.quads {
                let (couche, teinte) = apparence(q.id, q.face);
                a.instances.push(InstanceQuad {
                    // Le quad est LOCAL à sa section : sans l'origine, tout le
                    // monde se dessinerait empilé sur la section zéro.
                    position: [
                        ox as f32 * 16.0 + q.min[0],
                        oy as f32 * 16.0 + q.min[1],
                        oz as f32 * 16.0 + q.min[2],
                    ],
                    taille: q.taille,
                    face: q.face as u32,
                    couche,
                    teinte: en_rgba8(teinte),
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
            for k in 0..3 {
                min[k] = min[k].min(i.position[k] / 16.0);
                max[k] = max[k].max(i.position[k] / 16.0);
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
