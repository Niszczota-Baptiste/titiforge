//! La passe **gloutonne** : les cubes pleins opaques.
//!
//! Elle ne sait traiter que les blocs qui BOUCHENT leur case, parce qu'elle
//! travaille sur une grille d'identifiants et qu'un identifiant n'a pas de
//! forme. Sur du terrain vanilla c'est l'écrasante majorité des blocs ; sur la
//! cible Minefield, c'est 32 % du catalogue — d'où la passe de modèles à côté,
//! qui n'est pas un repli.
//!
//! Le principe : pour chacune des six faces, balayer les 16 tranches
//! perpendiculaires. Dans chaque tranche, marquer les cases dont la face est
//! visible (le voisin de ce côté n'est pas opaque), puis fusionner les
//! rectangles de même identifiant. Une muraille de 64 × 40 blocs sort en un
//! quad au lieu de 2 560.

use tf_anvil::StateId;

use crate::forme::{Formes, FACES};
use crate::maillage::{Maillage, Quad};
use crate::voisinage::{Voisinage, COTE};

/// Les deux axes du plan d'une face, dans l'ordre croissant.
///
/// Cet ordre EST celui de `Quad::taille` : le lire autrement échangerait
/// largeur et hauteur sur deux faces sur six, ce qui donne un maillage qui a
/// l'air juste tant que les quads sont carrés.
#[inline]
pub const fn axes_du_plan(axe: usize) -> (usize, usize) {
    match axe {
        0 => (1, 2), // ±X : le plan est (Y, Z)
        1 => (0, 2), // ±Y : le plan est (X, Z)
        _ => (0, 1), // ±Z : le plan est (X, Y)
    }
}

/// Compose un triplet de coordonnées depuis « profondeur + deux axes du plan ».
#[inline]
const fn compose(axe: usize, d: i32, u: i32, v: i32) -> [i32; 3] {
    match axe {
        0 => [d, u, v],
        1 => [u, d, v],
        _ => [u, v, d],
    }
}

/// Ajoute au maillage les quads gloutons du voisinage.
pub fn mailler(v: &Voisinage, f: &dyn Formes, out: &mut Maillage) {
    let n = COTE as i32;
    // Un seul masque réutilisé pour les 96 tranches : 256 entrées, alloué une
    // fois. Le réallouer par tranche coûterait plus que le maillage lui-même.
    let mut masque = [0u32; COTE * COTE];

    for face in FACES {
        let axe = face.axe();
        let pas = face.pas()[axe];

        for d in 0..n {
            // ── 1. marquer les faces visibles de cette tranche
            let mut vide = true;
            for iv in 0..n {
                for iu in 0..n {
                    let p = compose(axe, d, iu, iv);
                    let id = v.get(p[0], p[1], p[2]);
                    // Seuls les cubes pleins opaques passent ici. Tout le reste
                    // est le travail de la passe de modèles — et l'oublier le
                    // ferait dessiner DEUX fois.
                    if !f.opaque(id) {
                        masque[(iv * n + iu) as usize] = 0;
                        continue;
                    }
                    let mut q = p;
                    q[axe] += pas;
                    // Le voisin peut être dans la peau : c'est exactement à ça
                    // qu'elle sert.
                    if f.opaque(v.get(q[0], q[1], q[2])) {
                        masque[(iv * n + iu) as usize] = 0;
                        continue;
                    }
                    // `id + 1` : zéro veut dire « pas de face ». Un identifiant
                    // de voxel vaut l'indice PLUS UN — confondre les deux
                    // décale toute la tranche d'un cran.
                    masque[(iv * n + iu) as usize] = id + 1;
                    vide = false;
                }
            }
            if vide {
                continue;
            }

            // ── 2. fusionner les rectangles
            for iv in 0..n {
                let mut iu = 0;
                while iu < n {
                    let marque = masque[(iv * n + iu) as usize];
                    if marque == 0 {
                        iu += 1;
                        continue;
                    }
                    // Largeur : on avance tant que c'est le même état.
                    let mut w = 1;
                    while iu + w < n && masque[(iv * n + iu + w) as usize] == marque {
                        w += 1;
                    }
                    // Hauteur : on descend tant que la rangée ENTIÈRE suit.
                    let mut h = 1;
                    'hauteur: while iv + h < n {
                        for k in 0..w {
                            if masque[((iv + h) * n + iu + k) as usize] != marque {
                                break 'hauteur;
                            }
                        }
                        h += 1;
                    }

                    for dv in 0..h {
                        for du in 0..w {
                            masque[((iv + dv) * n + iu + du) as usize] = 0;
                        }
                    }

                    // Le plan de la face est du côté POSITIF de la case quand
                    // la face l'est : sinon les deux faces d'un même bloc
                    // sortiraient au même endroit.
                    let profondeur = if face.positif() { d + 1 } else { d };
                    // `compose` place déjà `iu` et `iv` sur les axes que
                    // `axes_du_plan` nomme, dans le même ordre : les deux
                    // fonctions se répondent, et un test le vérifie.
                    let min = compose(axe, profondeur, iu, iv);
                    out.quads.push(Quad {
                        min: [min[0] as i16 * 16, min[1] as i16 * 16, min[2] as i16 * 16],
                        taille: [(w * 16) as i16, (h * 16) as i16],
                        face,
                        id: (marque - 1) as StateId,
                    });
                    out.quads_glouton += 1;

                    iu += w;
                }
            }
        }
    }
}
