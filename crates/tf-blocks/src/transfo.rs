//! Les transformations d'un build, et ce qu'elles font à un ÉTAT.
//!
//! Déplacer les blocs est la partie facile. La partie qui casse les builds,
//! c'est l'état : un escalier tourné doit changer de `facing`, une bûche
//! d'`axis`, une trappe de `half`. Sur la cible Minefield, **910 blocs** portent
//! un état et **22 627 variantes** déclarent une rotation.

/// Ce qu'on peut faire subir à une sélection.
///
/// Les rotations sont autour de l'axe **Y** (le seul que les bâtisseurs
/// utilisent en pratique), les miroirs dans les plans verticaux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Transfo {
    Rot90,
    Rot180,
    Rot270,
    /// `x → −x`. Le plan du miroir est perpendiculaire à l'axe EST-OUEST.
    MiroirX,
    /// `z → −z`. Le plan du miroir est perpendiculaire à l'axe NORD-SUD.
    MiroirZ,
}

pub const TOUTES: [Transfo; 5] = [
    Transfo::Rot90,
    Transfo::Rot180,
    Transfo::Rot270,
    Transfo::MiroirX,
    Transfo::MiroirZ,
];

impl Transfo {
    pub const fn indice(self) -> usize {
        self as usize
    }

    /// Vraie pour un miroir — une transformation qui INVERSE l'orientation.
    pub const fn est_miroir(self) -> bool {
        matches!(self, Transfo::MiroirX | Transfo::MiroirZ)
    }

    /// L'angle d'une rotation, en degrés. Zéro pour un miroir.
    pub const fn degres(self) -> i32 {
        match self {
            Transfo::Rot90 => 90,
            Transfo::Rot180 => 180,
            Transfo::Rot270 => 270,
            _ => 0,
        }
    }

    /// La transformation qui annule celle-ci.
    ///
    /// Un miroir est sa propre inverse ; une rotation a pour inverse son
    /// complément à 360°. Un test le vérifie sur toutes les règles dérivées,
    /// parce que c'est ce qui rend `//rotate 90` puis `//rotate -90` sûr.
    pub const fn inverse(self) -> Transfo {
        match self {
            Transfo::Rot90 => Transfo::Rot270,
            Transfo::Rot270 => Transfo::Rot90,
            autre => autre,
        }
    }

    pub const fn nom(self) -> &'static str {
        match self {
            Transfo::Rot90 => "rotation 90°",
            Transfo::Rot180 => "rotation 180°",
            Transfo::Rot270 => "rotation 270°",
            Transfo::MiroirX => "miroir est-ouest",
            Transfo::MiroirZ => "miroir nord-sud",
        }
    }

    /// Applique la transformation à une position, autour d'une origine.
    ///
    /// Repère Minecraft : **+X = Est, +Z = Sud, +Y = Haut**. Une rotation de
    /// 90° va donc de l'est vers le sud.
    pub fn position(self, p: [i32; 3], origine: [i32; 3]) -> [i32; 3] {
        let (dx, dz) = (p[0] - origine[0], p[2] - origine[2]);
        let (nx, nz) = match self {
            Transfo::Rot90 => (-dz, dx),
            Transfo::Rot180 => (-dx, -dz),
            Transfo::Rot270 => (dz, -dx),
            Transfo::MiroirX => (-dx, dz),
            Transfo::MiroirZ => (dx, -dz),
        };
        [origine[0] + nx, p[1], origine[2] + nz]
    }
}
