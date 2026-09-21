//! La passe **gloutonne** : les cubes pleins opaques.
//!
//! Elle ne sait traiter que les blocs qui BOUCHENT leur case, parce qu'elle
//! travaille sur une grille d'identifiants et qu'un identifiant n'a pas de
//! forme. Sur la cible Minefield c'est 32 % du catalogue — d'où la passe de
//! modèles à côté, qui n'est pas un repli.
//!
//! Le principe : pour chacune des six faces, balayer les 16 tranches
//! perpendiculaires. Dans chaque tranche, marquer les cases dont la face est
//! visible, puis fusionner les rectangles de même identifiant. Une muraille de
//! 64 × 40 blocs sort en un quad au lieu de 2 560.
//!
//! La marque ne se fait PAS case par case. Une rangée de seize cases tient dans
//! un `u32`, et « visible de ce côté » est une opération de bits sur la rangée
//! entière (`Opacite`). Il ne reste qu'à visiter les bits posés — exactement
//! les faces à dessiner. Mesuré : une section pleine, qui rend six quads, est
//! passée de 94 µs à quelques microsecondes, parce que le coût suit enfin la
//! SORTIE et non le volume.

use tf_anvil::StateId;

use crate::forme::{Formes, FACES};
use crate::maillage::{Maillage, Quad};
use crate::opacite::Opacite;
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

/// La clé de fusion d'une case : l'identifiant, et le BIOME quand il compte.
///
/// `id + 1` parce que zéro veut dire « rien à dessiner ici » dans le masque.
/// Le biome ne rejoint la clé que pour un état teinté : l'ajouter partout
/// couperait les quads d'une muraille de pierre à chaque frontière de biome,
/// pour une couleur que la pierre ne prend pas.
///
/// **C'est le BIOME qu'on met dans la clé, pas la cellule qui le porte.** La
/// cellule y paraît équivalente — elle change à la même frontière — et elle
/// est fausse : deux cellules VOISINES du même biome ont deux indices
/// différents, donc deux clés, donc plus aucune fusion au-delà de quatre
/// blocs. Mesuré en écrivant la faute : une face de section pleine sortait en
/// 64 quads de 4 × 4 au lieu d'un seul, sur un terrain d'un seul biome. Le
/// rendu en était juste ; le maillage, seize fois trop cher.
///
/// `biome + 1` pour la même raison que l'identifiant : un état non teinté
/// laisse les bits hauts à zéro, et c'est ce qui permet de relire « aucun
/// biome » à l'émission au lieu de lire la cellule zéro.
#[inline]
fn cle<F: Formes + ?Sized>(v: &Voisinage, f: &F, x: i32, y: i32, z: i32) -> u64 {
    let id = v.get(x, y, z);
    let base = id as u64 + 1;
    if f.teinte_biome(id) {
        base | (v.biome(x, y, z) as u64 + 1) << 32
    } else {
        base
    }
}

/// Ajoute au maillage les quads gloutons du voisinage.
pub fn mailler<F: Formes + ?Sized>(v: &Voisinage, f: &F, out: &mut Maillage) {
    mailler_avec(v, f, &Opacite::relever(v, f), out)
}

/// La même passe, avec une carte d'opacité déjà relevée.
///
/// Elle coûte un balayage du voisinage, et les deux passes en ont besoin :
/// la partager évite de la payer deux fois.
pub fn mailler_avec<F: Formes + ?Sized>(v: &Voisinage, f: &F, op: &Opacite, out: &mut Maillage) {
    // Deux sorties immédiates, et elles couvrent les deux moitiés d'un monde :
    // le ciel et la roche. Sans elles, une section sans aucune face visible se
    // paie quand même en entier.
    if op.vide || op.bouchee {
        return;
    }
    let n = COTE as i32;
    // Le masque est nettoyé UNE fois. La fusion remet à zéro chaque case
    // qu'elle consomme, et elle les consomme toutes : il ressort propre. Le
    // reblanchir à chaque tranche coûterait 24 576 écritures par section —
    // exactement le coût qu'on vient de retirer.
    // **La clé de fusion est un `u64`, pas un `u32`.** Les 32 bits du bas
    // portent l'identifiant plus un ; les bits du haut portent la CELLULE DE
    // BIOME, et seulement pour les états dont la couleur en dépend. Un quad ne
    // peut donc pas enjamber une frontière de biome là où ça se verrait — et
    // il fusionne exactement comme avant partout ailleurs, ce qui est le cas
    // de l'immense majorité des blocs.
    //
    // Sans ça, il n'y a que trois issues et les trois sont mauvaises :
    // échantillonner le biome à un coin du quad (une bande de la mauvaise
    // couleur), prendre celui de la section (des coutures tous les seize
    // blocs), ou renoncer à la fusion (× 400 de quads sur une muraille).
    let mut masque = [0u64; COTE * COTE];
    // Les rangées visibles d'une face sur X, relevées une fois : une rangée
    // court le long de X, donc elle TRAVERSE les seize tranches. La recalculer
    // par tranche la referait seize fois.
    let mut vis_x = [0u32; COTE * COTE];

    for face in FACES {
        let axe = face.axe();
        let positif = face.positif();

        if axe == 0 {
            for iv in 0..n {
                for iu in 0..n {
                    // u = Y, v = Z
                    vis_x[(iv * n + iu) as usize] = Opacite::visibles_x(op.rangee(iu, iv), positif);
                }
            }
        }

        for d in 0..n {
            // ── 1. marquer les faces visibles de cette tranche, par RANGÉES
            let mut vide = true;
            match axe {
                0 => {
                    let bit = 1u32 << (d + 1);
                    for iv in 0..n {
                        for iu in 0..n {
                            if vis_x[(iv * n + iu) as usize] & bit == 0 {
                                continue;
                            }
                            masque[(iv * n + iu) as usize] = cle(v, f, d, iu, iv);
                            vide = false;
                        }
                    }
                }
                // ±Y : la profondeur est Y. La rangée (d, z) et sa voisine
                // (d ± 1, z) donnent seize faces d'un coup.
                1 => {
                    let dv = if positif { d + 1 } else { d - 1 };
                    for iv in 0..n {
                        // u = X, v = Z
                        let mut vis = Opacite::visibles_entre(op.rangee(d, iv), op.rangee(dv, iv));
                        while vis != 0 {
                            let iu = vis.trailing_zeros() as i32 - 1;
                            vis &= vis - 1;
                            masque[(iv * n + iu) as usize] = cle(v, f, iu, d, iv);
                            vide = false;
                        }
                    }
                }
                // ±Z : la profondeur est Z. Rangée (y, d) contre (y, d ± 1).
                _ => {
                    let dv = if positif { d + 1 } else { d - 1 };
                    for iv in 0..n {
                        // u = X, v = Y
                        let mut vis = Opacite::visibles_entre(op.rangee(iv, d), op.rangee(iv, dv));
                        while vis != 0 {
                            let iu = vis.trailing_zeros() as i32 - 1;
                            vis &= vis - 1;
                            masque[(iv * n + iu) as usize] = cle(v, f, iu, iv, d);
                            vide = false;
                        }
                    }
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
                    let mut w = 1;
                    while iu + w < n && masque[(iv * n + iu + w) as usize] == marque {
                        w += 1;
                    }
                    let mut h = 1;
                    'hauteur: while iv + h < n {
                        for k in 0..w {
                            if masque[((iv + h) * n + iu + k) as usize] != marque {
                                break 'hauteur;
                            }
                        }
                        h += 1;
                    }
                    for ddv in 0..h {
                        for ddu in 0..w {
                            masque[((iv + ddv) * n + iu + ddu) as usize] = 0;
                        }
                    }

                    // Le plan de la face est du côté POSITIF de la case quand
                    // la face l'est : sinon les deux faces d'un même bloc
                    // sortiraient au même endroit.
                    let profondeur = if positif { d + 1 } else { d };
                    // `compose` place `iu` et `iv` sur les axes que
                    // `axes_du_plan` nomme, dans le même ordre : les deux
                    // fonctions se répondent, et un test le vérifie.
                    let min = compose(axe, profondeur, iu, iv);
                    out.quads.push(Quad {
                        min: [
                            min[0] as f32 * 16.0,
                            min[1] as f32 * 16.0,
                            min[2] as f32 * 16.0,
                        ],
                        taille: [(w * 16) as f32, (h * 16) as f32],
                        face,
                        id: (marque & 0xFFFF_FFFF) as StateId - 1,
                        // Le biome voyage dans les bits hauts de la clé. Zéro
                        // y veut dire « cet état n'en prend pas la couleur »,
                        // et c'est une réponse, pas une valeur par défaut.
                        biome: match marque >> 32 {
                            0 => 0,
                            b => (b - 1) as StateId,
                        },
                    });
                    out.quads_glouton += 1;

                    iu += w;
                }
            }
            debug_assert!(
                masque.iter().all(|c| *c == 0),
                "la fusion doit laisser le masque propre : la tranche suivante \
                 ne le nettoie pas, et des marques oubliées produiraient des \
                 quads fantômes à la mauvaise profondeur"
            );
        }
    }
}
