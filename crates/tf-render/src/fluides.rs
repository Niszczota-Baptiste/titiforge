//! L'arène des FLUIDES : les faces d'eau et de lave, chaque section à sa place.
//!
//! Une troisième arène et pas des quads de plus : une face de fluide porte ce
//! qu'un quad glouton n'a pas — quatre hauteurs de coin, un sens de courant —
//! et elle se dessine AUTREMENT. L'eau est translucide : elle passe après
//! tout ce qui est opaque, se mélange à ce qu'elle recouvre et n'écrit pas la
//! profondeur. La lave, opaque, se dessine avec les autres.
//!
//! Même mécanique de places que les quads (`Places`), mêmes emplacements —
//! donc mêmes origines : une face de fluide et le mur qu'elle touche se
//! placent par la même table.

use bytemuck::{Pod, Zeroable};
use tf_anvil::StateId;
use tf_mesh::{Adresse, Chantier, FaceFluide, GenreFluide, Lot, TextureFluide};

use crate::arene::{en_rgba8, Case, Emplacements, Places, Tranche};

/// Une face de fluide au GPU : **vingt octets**.
///
/// `geo` reprend l'empaquetage des quads gloutons — position et taille en
/// blocs, locales à la section — et y ajoute ce qui décide de la passe et de
/// la texture. Les hauteurs tiennent dans un mot : quatre 255e de bloc.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct InstanceFluide {
    /// `x | y<<5 | z<<10 | (l−1)<<15 | (h−1)<<19 | face<<23 | opaque<<26 |
    /// texture<<27`. `opaque` vaut 1 pour la lave : elle se dessine avec la
    /// profondeur, l'eau sans.
    pub geo: u32,
    /// Les quatre hauteurs de `FaceFluide::hauteurs`, un octet chacune, dans
    /// l'ordre (petit-boutiste).
    pub hauteurs: u32,
    /// La couche d'atlas dans les seize bits du bas, l'angle du courant dans
    /// ceux du haut.
    pub couche_angle: u32,
    /// Le facteur de teinte, RGBA8 : la couleur d'eau du biome, ou blanc.
    pub teinte: u32,
    /// L'emplacement de la section — donc son origine.
    pub section: u32,
}

impl InstanceFluide {
    /// Le trou : face 7, comme pour les quads — aucune face n'a ce rang.
    pub const VIDE: InstanceFluide = InstanceFluide {
        geo: 7 << 23,
        hauteurs: 0,
        couche_angle: 0,
        teinte: 0,
        section: 0,
    };

    /// La face (0 à 5), ou 7 pour un trou.
    pub fn face(&self) -> u32 {
        (self.geo >> 23) & 7
    }

    /// Vrai pour de la lave : la passe opaque la dessine.
    pub fn opaque(&self) -> bool {
        (self.geo >> 26) & 1 == 1
    }
}

impl Case for InstanceFluide {
    const VIDE: Self = InstanceFluide::VIDE;
    fn est_vide(&self) -> bool {
        self.face() > 5
    }
}

/// Empaquette une face de fluide.
pub fn empaqueter_fluide(
    f: &FaceFluide,
    couche: u32,
    teinte: [f32; 3],
    section: u32,
) -> InstanceFluide {
    debug_assert!(
        f.taille[0] >= 1 && f.taille[1] >= 1 && f.taille[0] <= 16 && f.taille[1] <= 16,
        "taille {:?}",
        f.taille
    );
    debug_assert!(couche < 1 << 16, "une couche d'atlas tient sur seize bits");
    let geo = f.pos[0] as u32
        | (f.pos[1] as u32) << 5
        | (f.pos[2] as u32) << 10
        | ((f.taille[0] - 1) as u32) << 15
        | ((f.taille[1] - 1) as u32) << 19
        | (f.face as u32) << 23
        | ((f.genre == GenreFluide::Lave) as u32) << 26
        | (f.texture as u32) << 27;
    InstanceFluide {
        geo,
        hauteurs: u32::from_le_bytes(f.hauteurs),
        couche_angle: (couche & 0xFFFF) | (f.angle as u32) << 16,
        teinte: en_rgba8(teinte),
        section,
    }
}

/// Ce que le rendu doit savoir d'une face de fluide : sa couche d'atlas et
/// sa teinte, selon le fluide, la texture et le BIOME de sa case.
pub type ApparenceFluide<'a> = dyn Fn(GenreFluide, TextureFluide, StateId) -> (u32, [f32; 3]) + 'a;

/// Les faces de fluide de tout un chantier.
#[derive(Debug, Default)]
pub struct AreneFluides {
    places: Places<InstanceFluide>,
}

impl AreneFluides {
    /// Empile un chantier, avec les emplacements que l'arène des quads vient
    /// d'attribuer pour lui.
    pub fn depuis(
        chantier: &Chantier,
        emplacements: &Emplacements,
        apparence: &ApparenceFluide,
    ) -> AreneFluides {
        let mut a = AreneFluides::default();
        for lot in &chantier.lots {
            a.poser(emplacements, lot, apparence);
        }
        a
    }

    /// **Refait les sections VISÉES**, et rien d'autre — le contrat de
    /// `Arene::remplacer`, qui doit passer AVANT : c'est elle qui attribue
    /// les emplacements que celle-ci lit.
    pub fn remplacer(
        &mut self,
        emplacements: &Emplacements,
        visees: &[Adresse],
        neufs: &[Lot],
        apparence: &ApparenceFluide,
    ) {
        let reviennent: std::collections::HashSet<Adresse> =
            neufs.iter().map(|l| l.adresse).collect();
        for a in visees {
            if !reviennent.contains(a) {
                self.places.enlever(a);
            }
        }
        for lot in neufs {
            self.poser(emplacements, lot, apparence);
        }
        self.places.tasser_si_besoin();
    }

    fn poser(&mut self, emplacements: &Emplacements, lot: &Lot, apparence: &ApparenceFluide) {
        let Some(slot) = emplacements.de(&lot.adresse) else {
            // Toute section maillée tient un emplacement : l'arène des quads
            // en donne un à chaque lot, même sans quad. Sans lui, ces faces
            // se dessineraient à l'origine du monde.
            debug_assert!(
                false,
                "{:?} n'a pas d'emplacement : l'arène des quads doit passer avant",
                lot.adresse
            );
            return;
        };
        let faces = &lot.fluides;
        self.places.poser(lot.adresse, faces.len() as u32, |k| {
            let f = &faces[k];
            let (couche, teinte) = apparence(f.genre, f.texture, f.biome);
            empaqueter_fluide(f, couche, teinte, slot)
        });
    }

    /// Le tableau que le GPU dessine, trous compris.
    pub fn instances(&self) -> &crate::pages::Pages<InstanceFluide> {
        &self.places.instances
    }

    pub fn tranches(&self) -> Vec<Tranche> {
        self.places.tranches()
    }

    pub fn visibles(&self) -> impl Iterator<Item = &InstanceFluide> + '_ {
        self.places.visibles()
    }

    /// Les plages réécrites depuis le dernier appel.
    pub fn prendre_sales(&mut self) -> Vec<(u32, u32)> {
        self.places.prendre_sales()
    }

    pub fn ecrites(&self) -> u64 {
        self.places.ecrites()
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    pub fn trous(&self) -> u64 {
        self.places.trous()
    }

    pub fn octets(&self) -> usize {
        self.places.octets()
    }
}
