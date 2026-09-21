//! Ce que le mailleur produit.

use tf_anvil::StateId;

use crate::forme::Face;

/// Un quad, en **seizièmes de bloc** dans le repère de la section.
///
/// La même unité que les modèles, et en flottants pour la même raison : 17 %
/// des coordonnées du pack Minefield ne sont pas entières. Les quads de la
/// passe gloutonne, eux, tombent toujours sur des bords de bloc — donc sur des
/// multiples de 16, exacts en flottant, et comparables sans tolérance. Un quad
/// de 3 × 5 blocs sort à 48 × 80.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Coin de plus petites coordonnées, dans le repère de la section.
    pub min: [f32; 3],
    /// Étendue dans les deux axes du plan de la face, dans l'ordre croissant
    /// des axes restants. Pour `±Y` : `[x, z]`.
    pub taille: [f32; 2],
    pub face: Face,
    /// L'état qui a produit ce quad — c'est lui qui désigne la tuile d'atlas.
    pub id: StateId,
    /// Le biome de la case, pour les états qui en prennent la couleur.
    ///
    /// Zéro — « on ne sait pas » — pour tous les autres, et c'est voulu :
    /// mettre le biome partout casserait la fusion gloutonne à chaque
    /// frontière, sur des blocs dont la couleur n'en dépend pas.
    pub biome: StateId,
}

impl Quad {
    /// Surface en seizièmes carrés. Sert aux tests de conservation : la somme
    /// des surfaces d'un maillage glouton doit égaler celle du maillage naïf.
    pub fn aire(&self) -> f64 {
        self.taille[0] as f64 * self.taille[1] as f64
    }
}

#[derive(Debug, Default, Clone)]
pub struct Maillage {
    pub quads: Vec<Quad>,
    /// Combien de quads viennent de la passe gloutonne.
    pub quads_glouton: usize,
    /// Combien viennent de la passe de modèles.
    pub quads_modele: usize,
}

impl Maillage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn est_vide(&self) -> bool {
        self.quads.is_empty()
    }

    pub fn len(&self) -> usize {
        self.quads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    pub fn aire(&self) -> f64 {
        self.quads.iter().map(Quad::aire).sum()
    }
}

/// Un bloc-modèle **posé**, sans sa géométrie.
///
/// C'est la réponse à la mesure de la passe de modèles : sur un build
/// Minefield, 349 k blocs-modèles produisaient **5,8 M de quads**, neuf
/// dixièmes du maillage. Or ces quads sont la MÊME géométrie répétée — deux
/// dalles de chêne posées côte à côte n'ont pas deux modèles, elles ont deux
/// positions.
///
/// On n'émet donc plus la géométrie mais la POSE : huit octets par bloc au
/// lieu d'un quad par face de chaque cuboïde. Le modèle vit une fois, dans un
/// tampon indexé par l'état, et le GPU le répète.
///
/// Contrepartie assumée : le masquage des faces ne peut plus être fait ici,
/// puisqu'on n'émet plus de faces. D'où `voisins_opaques`, que le shader
/// consulte — ça déplace le travail, ça ne le supprime pas. Mais il devient
/// proportionnel au nombre de BLOCS et non au nombre de faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    /// Position locale dans la section, `0..16` par axe.
    pub pos: [u8; 3],
    /// Un bit par face (ordre de `Face`) : le voisin de ce côté est opaque.
    pub voisins_opaques: u8,
    pub id: StateId,
}

/// Ce que produit la passe de modèles en mode instances.
#[derive(Debug, Default, Clone)]
pub struct Instances {
    pub poses: Vec<Instance>,
}

impl Instances {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.poses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.poses.is_empty()
    }

    /// Octets que ça pèse sur le GPU.
    pub fn octets(&self) -> usize {
        self.poses.len() * std::mem::size_of::<Instance>()
    }
}
