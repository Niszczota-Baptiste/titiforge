//! Comparer des GÉOMÉTRIES, quand les angles ne suffisent plus.
//!
//! Une rotation se lit dans le pack : deux états sont des rotations l'un de
//! l'autre s'ils rendent le même modèle à des `y` différents. **Un miroir, non.**
//! Un escalier réfléchi n'est pas un escalier tourné — son `shape` passe de
//! `inner_left` à `inner_right`, et le pack déclare ça comme deux modèles
//! DIFFÉRENTS. Mesuré : la dérivation par les angles plafonne à 85 % sur les
//! miroirs, et les 15 % manquants sont les escaliers, les cloches, tout ce qui
//! a une main gauche et une main droite.
//!
//! Ici on descend d'un cran : on compare les cuboïdes eux-mêmes. Deux modèles
//! sont miroirs si l'un, réfléchi, a exactement les cuboïdes de l'autre. C'est
//! toujours dérivé du pack — juste lu plus profond.
//!
//! Ça sert aussi aux ROTATIONS, pour un cas que les angles ne voient pas : une
//! bûche verticale n'a qu'un seul état, donc aucun angle à comparer. Sa forme,
//! elle, est INVARIANTE par rotation — et ça se mesure.

use tf_assets::modele::cuboides;
use tf_assets::rotation;
use tf_assets::{Catalogue, Id};

use crate::transfo::Transfo;

/// Un cuboïde en **256e de bloc**, pour comparer sans flottants.
///
/// Les modèles sont en seizièmes avec des décimales — mesuré, 17 % des
/// coordonnées du pack ne sont pas entières. Comparer des `f32` avec `==`
/// ferait rater des égalités qui n'en sont pas à un bit près ; l'entier tranche.
pub type Boite = [i32; 6];

/// Le centre d'un bloc, dans la même unité.
const CENTRE: i32 = 8 * 16;

fn en_entier(v: f32) -> i32 {
    (v * 16.0).round() as i32
}

/// Applique une rotation de bloc puis, éventuellement, un miroir.
///
/// Un cuboïde tourné d'un multiple de 90° reste un cuboïde : il suffit de
/// transformer ses deux coins et de les remettre dans l'ordre.
///
/// La rotation de la VARIANTE vient de `tf_assets::rotation` et n'est pas
/// réécrite ici. Elle l'a été, et les deux copies avaient divergé : celle-ci
/// tournait autour de X dans le sens inverse de celle du rendu. Le format
/// applique `x` puis `y`, dans le sens négatif — ancré sur `mushroom_stem`,
/// dont les six parts doivent atterrir sur ses six faces, et vérifié par un
/// test de `tf-assets`.
fn transformer(b: Boite, x: u16, y: u16, apres: Option<Transfo>) -> Boite {
    let a = rotation::axes(x, y);
    let mut coins = [[b[0], b[1], b[2]], [b[3], b[4], b[5]]];
    for c in coins.iter_mut() {
        let depart = *c;
        for (i, &(j, s)) in a.iter().enumerate() {
            c[j] = CENTRE + (depart[i] - CENTRE) * s as i32;
        }
        // La transformation du BUILD, appliquée après celle du bloc.
        if let Some(t) = apres {
            match t {
                Transfo::MiroirX => c[0] = 2 * CENTRE - c[0],
                Transfo::MiroirZ => c[2] = 2 * CENTRE - c[2],
                rot => {
                    for _ in 0..(rot.degres() / 90) {
                        let (dx, dz) = (c[0] - CENTRE, c[2] - CENTRE);
                        c[0] = CENTRE - dz;
                        c[2] = CENTRE + dx;
                    }
                }
            }
        }
    }
    [
        coins[0][0].min(coins[1][0]),
        coins[0][1].min(coins[1][1]),
        coins[0][2].min(coins[1][2]),
        coins[0][0].max(coins[1][0]),
        coins[0][1].max(coins[1][1]),
        coins[0][2].max(coins[1][2]),
    ]
}

/// Au-delà de ce nombre de cellules, on renonce à canoniser.
///
/// Le pack du serveur monte à 82 cuboïdes dans une case : la grille induite y
/// ferait 164³ cellules. Ce bloc-là n'a pas d'état, donc rien à tourner — mais
/// le plafond est ce qui garantit que le coût reste borné quoi qu'on nous
/// donne.
const CELLULES_MAX: usize = 4096;

/// La décomposition CANONIQUE d'un ensemble de cuboïdes.
///
/// Deux modèles qui décrivent le même solide ne le découpent pas forcément
/// pareil, et ça n'est pas un cas d'école : `oak_stairs_inner` réfléchi est
/// exactement `oak_stairs_inner` tourné — même solide, boîtes différentes.
/// Comparer les listes de boîtes répondait « formes différentes », la route
/// géométrique ne trouvait RIEN pour les escaliers en coin, et le miroir
/// retombait sur la route des angles, qui rend l'escalier TOURNÉ. Mesuré :
/// 371 incohérences sur le pack, toutes de la famille des escaliers.
///
/// On découpe donc sur la grille induite par les coordonnées présentes, puis on
/// refusionne dans un ordre FIXE — X, puis Y, puis Z. Le résultat ne dépend que
/// du solide, jamais de la manière dont on l'a écrit. Les boîtes PLATES (une
/// fleur en croix a des faces d'épaisseur nulle) n'occupent aucune cellule :
/// elles sont mises de côté et recollées telles quelles, sinon elles
/// disparaîtraient.
fn canoniser(v: Vec<Boite>) -> Vec<Boite> {
    let (pleins, mut plats): (Vec<Boite>, Vec<Boite>) = v
        .into_iter()
        .partition(|b| b[0] < b[3] && b[1] < b[4] && b[2] < b[5]);
    plats.sort_unstable();
    if pleins.is_empty() {
        return plats;
    }

    let plans = |bas: usize, haut: usize| -> Vec<i32> {
        let mut p: Vec<i32> = pleins.iter().flat_map(|b| [b[bas], b[haut]]).collect();
        p.sort_unstable();
        p.dedup();
        p
    };
    let (xs, ys, zs) = (plans(0, 3), plans(1, 4), plans(2, 5));
    let (nx, ny, nz) = (xs.len() - 1, ys.len() - 1, zs.len() - 1);
    if nx * ny * nz > CELLULES_MAX {
        let mut brut = pleins;
        brut.extend(plats);
        brut.sort_unstable();
        return brut;
    }

    // L'occupation de la grille induite. Deux boîtes qui se recouvrent y
    // deviennent le même solide — ce qu'elles sont.
    let mut occ = vec![false; nx * ny * nz];
    let bande = |axe: &[i32], min: i32, max: i32| -> (usize, usize) {
        (
            axe.partition_point(|&c| c < min),
            axe.partition_point(|&c| c < max),
        )
    };
    for b in &pleins {
        let (x0, x1) = bande(&xs, b[0], b[3]);
        let (y0, y1) = bande(&ys, b[1], b[4]);
        let (z0, z1) = bande(&zs, b[2], b[5]);
        for iz in z0..z1 {
            for iy in y0..y1 {
                for ix in x0..x1 {
                    occ[(iz * ny + iy) * nx + ix] = true;
                }
            }
        }
    }

    // Fusion X : les suites de cellules pleines d'une même rangée.
    let mut boites: Vec<Boite> = Vec::new();
    for iz in 0..nz {
        for iy in 0..ny {
            let mut ix = 0;
            while ix < nx {
                if !occ[(iz * ny + iy) * nx + ix] {
                    ix += 1;
                    continue;
                }
                let debut = ix;
                while ix < nx && occ[(iz * ny + iy) * nx + ix] {
                    ix += 1;
                }
                boites.push([xs[debut], ys[iy], zs[iz], xs[ix], ys[iy + 1], zs[iz + 1]]);
            }
        }
    }
    // Fusion Y, puis Z : deux boîtes de mêmes autres bornes et contiguës.
    for (colle, autres) in [(1usize, [0usize, 2, 3, 5]), (2, [0, 1, 3, 4])] {
        let haut = colle + 3;
        boites.sort_unstable_by_key(|b| (autres.map(|a| b[a]), b[colle]));
        let mut fusion: Vec<Boite> = Vec::with_capacity(boites.len());
        for b in boites {
            match fusion.last_mut() {
                Some(d) if autres.iter().all(|&a| d[a] == b[a]) && d[haut] == b[colle] => {
                    d[haut] = b[haut];
                }
                _ => fusion.push(b),
            }
        }
        boites = fusion;
    }

    boites.extend(plats);
    boites.sort_unstable();
    boites
}

/// L'empreinte géométrique d'un état : ses cuboïdes, transformés et TRIÉS.
///
/// CANONISÉS, parce que ni l'ordre ni le découpage d'un modèle n'ont de sens
/// géométrique : deux modèles qui décrivent le même solide sont le même
/// solide, quelles que soient les boîtes qu'ils emploient pour l'écrire.
pub fn empreinte(
    cat: &Catalogue,
    modele: &Id,
    x: u16,
    y: u16,
    apres: Option<Transfo>,
) -> Option<Vec<Boite>> {
    let m = cat.modele(modele)?;
    let v: Vec<Boite> = cuboides(m)
        .iter()
        .map(|c| {
            transformer(
                [
                    en_entier(c.min[0]),
                    en_entier(c.min[1]),
                    en_entier(c.min[2]),
                    en_entier(c.max[0]),
                    en_entier(c.max[1]),
                    en_entier(c.max[2]),
                ],
                x,
                y,
                apres,
            )
        })
        .collect();
    if v.is_empty() {
        return None;
    }
    Some(canoniser(v))
}
