//! Ce que le mailleur produit.

use tf_anvil::StateId;

use crate::forme::Face;

/// Un quad, en **seizièmes de bloc** dans le repère de la section.
///
/// Des entiers et non des flottants : les modèles Minecraft sont déjà en
/// seizièmes, la passe gloutonne travaille sur des bords de bloc, et un entier
/// se compare exactement. Un quad de 3 × 5 blocs sort à 48 × 80.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quad {
    /// Coin de plus petites coordonnées, dans le repère de la section.
    pub min: [i16; 3],
    /// Étendue dans les deux axes du plan de la face, dans l'ordre croissant
    /// des axes restants. Pour `±Y` : `[x, z]`.
    pub taille: [i16; 2],
    pub face: Face,
    /// L'état qui a produit ce quad — c'est lui qui désigne la tuile d'atlas.
    pub id: StateId,
}

impl Quad {
    /// Surface en seizièmes carrés. Sert aux tests de conservation : la somme
    /// des surfaces d'un maillage glouton doit égaler celle du maillage naïf.
    pub fn aire(&self) -> i64 {
        self.taille[0] as i64 * self.taille[1] as i64
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

    pub fn aire(&self) -> i64 {
        self.quads.iter().map(Quad::aire).sum()
    }
}
