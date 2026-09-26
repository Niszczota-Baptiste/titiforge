//! La passe **fluides** : l'eau et la lave.
//!
//! Un fluide n'a pas de modèle de bloc — `block/water` ne déclare aucun
//! élément, et c'est pourquoi l'eau était de l'AIR pour les deux autres
//! passes : 6,2 millions de cases sur une seule région d'un vrai monde, et
//! pas une n'était dessinée. Le jeu la dessine avec son propre code
//! (`LiquidBlockRenderer`), et c'est ce code qui est suivi ici, règle par
//! règle :
//!
//! - **une face n'existe que contre ce qui n'est pas le même fluide** ni un
//!   bloc qui bouche sa case — l'intérieur d'un lac n'a aucune face ;
//! - **le dessus n'est pas plat** : chacun de ses quatre coins prend la
//!   moyenne des colonnes qui l'entourent, pondérée par dix au-dessus de
//!   8/10 de bloc, et monte à la case entière dès qu'une de ces colonnes a du
//!   même fluide au-dessus (`getWaterHeight`, 1.18). C'est ce qui fait d'un
//!   courant une PENTE et non un escalier ;
//! - **une surface qui court porte la texture de courant**, tournée dans le
//!   sens du courant (`FlowingFluid::getFlow`) ; une surface immobile et tout
//!   dessous portent la texture fixe ; un côté contre du verre ou des
//!   feuilles porte le voile.
//!
//! Ce que la passe ne fait PAS, et qui est nommé : l'occultation d'un fluide
//! par son PROPRE bloc — une dalle inondée cache la face du dessous de son
//! eau dans le jeu, pas ici — et la matière des blocs (`Formes::solide`), que
//! le pack ne dit pas.
//!
//! **Les faces plates se fusionnent**, comme la passe gloutonne : un océan
//! immobile sort en un quad par section au lieu de 256. Une face dont les
//! coins diffèrent, ou dont le courant tourne la texture, sort seule — c'est
//! la surface d'un courant, étroite par nature.
//!
//! La visibilité se décide par RANGÉES, comme dans `Opacite` : une rangée de
//! dix-huit cases tient dans un `u32`, et « du fluide dont le voisin n'est ni
//! le même fluide ni opaque » est une opération de bits pour seize cases.
//! Seules les faces visibles coûtent ensuite une lecture de voisins — et une
//! section d'eau profonde, qui n'en a aucune, ne coûte presque rien.

use tf_anvil::StateId;

use crate::forme::{Face, Formes, GenreFluide, FACES};
use crate::glouton::axes_du_plan;
use crate::maillage::{FaceFluide, TextureFluide};
use crate::opacite::{Opacite, DEDANS};
use crate::voisinage::{index_pad, Voisinage, COTE, COTE_PAD, VOL_PAD};

/// La hauteur d'une case pleine, en 255e : ce qu'un coin vaut dès qu'une
/// colonne voisine porte le même fluide au-dessus.
pub const PLEIN: u8 = 255;

/// La hauteur d'une SOURCE, `8/9` de bloc, telle que le jeu l'écrit en
/// flottant — `0.8888889F` — et pas `8.0 / 9.0`, qui ne tombe pas forcément
/// sur le même flottant. C'est cette constante que le calcul du courant
/// retranche.
const HAUTEUR_SOURCE: f32 = 0.888_888_9;

/// Une hauteur de bloc `0..=1` en 255e — ce que le GPU reçoit.
#[inline]
pub fn en_255(h: f32) -> u8 {
    (h.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Ce que la passe relit du voisinage, une fois : le genre et la quantité de
/// fluide de chaque case, et les rangées de chaque genre.
struct Carte {
    /// Bits 0-1 : le genre (0 = aucun). Bits 2-5 : la quantité (1 à 8).
    code: [u8; VOL_PAD],
    /// Une rangée de dix-huit cases par `(y, z)` paddé, par genre : le bit
    /// `k` vaut la case `x = k − 1`, comme `Opacite::rangee`.
    rangees: [[u32; COTE_PAD * COTE_PAD]; 2],
    /// Aucune case INTÉRIEURE ne porte de fluide.
    vide: bool,
}

impl Carte {
    fn relever<F: Formes + ?Sized>(v: &Voisinage, f: &F) -> Carte {
        let mut code = [0u8; VOL_PAD];
        let mut rangees = [[0u32; COTE_PAD * COTE_PAD]; 2];
        for (r, rangee) in v.ids().chunks_exact(COTE_PAD).enumerate() {
            for (k, &id) in rangee.iter().enumerate() {
                let Some(fl) = f.fluide(id) else {
                    continue;
                };
                let g = fl.genre as u8;
                code[r * COTE_PAD + k] = g | (fl.quantite() << 2);
                rangees[(g - 1) as usize][r] |= 1 << k;
            }
        }
        let n = COTE as i32;
        let vide = rangees
            .iter()
            .all(|rg| (0..n).all(|y| (0..n).all(|z| rg[Self::rang(y, z)] & DEDANS == 0)));
        Carte {
            code,
            rangees,
            vide,
        }
    }

    #[inline]
    fn rang(y: i32, z: i32) -> usize {
        ((y + 1) as usize) * COTE_PAD + (z + 1) as usize
    }

    /// La rangée d'un genre en `(y, z)` paddés.
    #[inline]
    fn rangee(&self, g: GenreFluide, y: i32, z: i32) -> u32 {
        self.rangees[(g as u8 - 1) as usize][Self::rang(y, z)]
    }

    /// Le genre d'une case, 0 sans fluide.
    #[inline]
    fn genre(&self, x: i32, y: i32, z: i32) -> u8 {
        self.code[index_pad(x, y, z)] & 3
    }

    /// La hauteur propre d'une case de fluide, en blocs.
    #[inline]
    fn hauteur(&self, x: i32, y: i32, z: i32) -> f32 {
        (self.code[index_pad(x, y, z)] >> 2) as f32 / 9.0
    }
}

/// **La hauteur d'un coin de surface**, en 255e — `getWaterHeight` de 1.18,
/// règle pour règle.
///
/// Le coin `(cx, cz)` est le sommet commun des quatre colonnes `cx − 1..cx`
/// × `cz − 1..cz`, au niveau `y`. Une seule de ces colonnes qui porte le même
/// fluide AU-DESSUS, et le coin monte à la case entière. Sinon chaque colonne
/// de ce fluide compte sa hauteur — dix fois quand elle dépasse 8/10, ce qui
/// garde une étendue de sources à 8/9 jusqu'à son bord — et une colonne qui
/// ne l'arrête pas (de l'air, une fleur, l'autre fluide) compte pour zéro :
/// c'est elle qui fait plonger le bord d'une cascade. Une paroi ne compte
/// pas du tout.
fn coin<F: Formes + ?Sized>(
    v: &Voisinage,
    f: &F,
    c: &Carte,
    g: u8,
    cx: i32,
    y: i32,
    cz: i32,
) -> u8 {
    let mut total = 0.0f32;
    let mut n = 0u32;
    // L'ordre du jeu : (0, 0), (−1, 0), (0, −1), (−1, −1).
    for j in 0..4 {
        let x = cx - (j & 1);
        let z = cz - ((j >> 1) & 1);
        if c.genre(x, y + 1, z) == g {
            return PLEIN;
        }
        if c.genre(x, y, z) == g {
            let h = c.hauteur(x, y, z);
            if h >= 0.8 {
                total += h * 10.0;
                n += 10;
            } else {
                total += h;
                n += 1;
            }
        } else if !f.solide(v.get(x, y, z)) {
            n += 1;
        }
    }
    // La case elle-même est l'une des quatre colonnes, et elle porte ce
    // fluide : `n` vaut au moins un.
    debug_assert!(n > 0, "un coin se calcule pour une case de fluide");
    en_255(total / n.max(1) as f32)
}

/// Les quatre coins d'une case de fluide : `[NO, SO, SE, NE]`, l'ordre où le
/// jeu émet les sommets de sa surface.
fn coins<F: Formes + ?Sized>(
    v: &Voisinage,
    f: &F,
    c: &Carte,
    g: u8,
    x: i32,
    y: i32,
    z: i32,
) -> [u8; 4] {
    [
        coin(v, f, c, g, x, y, z),
        coin(v, f, c, g, x, y, z + 1),
        coin(v, f, c, g, x + 1, y, z + 1),
        coin(v, f, c, g, x + 1, y, z),
    ]
}

/// **Le sens du courant** d'une case — la partie horizontale de
/// `FlowingFluid::getFlow`, la seule qui décide de la texture du dessus.
///
/// Rend `(dx, dz)` avant normalisation : zéro sur les deux axes, et la
/// surface est immobile. La composante verticale d'une chute ne change pas
/// l'angle, elle n'est donc pas calculée.
fn courant<F: Formes + ?Sized>(
    v: &Voisinage,
    f: &F,
    c: &Carte,
    g: u8,
    x: i32,
    y: i32,
    z: i32,
) -> (f64, f64) {
    let propre = c.hauteur(x, y, z);
    let (mut dx, mut dz) = (0.0f64, 0.0f64);
    // `Direction.Plane.HORIZONTAL` : nord, est, sud, ouest.
    for (px, pz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        let (nx, nz) = (x + px, z + pz);
        let gn = c.genre(nx, y, nz);
        // `affectsFlow` : pas de fluide, ou le même.
        if gn != 0 && gn != g {
            continue;
        }
        let voisin = if gn == g { c.hauteur(nx, y, nz) } else { 0.0 };
        let mut ecart = 0.0f32;
        if voisin == 0.0 {
            if !f.solide(v.get(nx, y, nz)) {
                let gb = c.genre(nx, y - 1, nz);
                if gb == 0 || gb == g {
                    let dessous = if gb == g {
                        c.hauteur(nx, y - 1, nz)
                    } else {
                        0.0
                    };
                    if dessous > 0.0 {
                        ecart = propre - (dessous - HAUTEUR_SOURCE);
                    }
                }
            }
        } else {
            ecart = propre - voisin;
        }
        if ecart != 0.0 {
            dx += (px as f32 * ecart) as f64;
            dz += (pz as f32 * ecart) as f64;
        }
    }
    (dx, dz)
}

/// L'angle d'une texture de courant, en 65 536e de tour : `atan2(dz, dx) −
/// π/2`, ramené dans `[0, 2π)`.
pub fn angle_du_courant(dx: f64, dz: f64) -> u16 {
    let a = (dz.atan2(dx) as f32 - std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
    ((a / std::f32::consts::TAU * 65_536.0).round() as u32 % 65_536) as u16
}

/// Les hauteurs du haut d'un CÔTÉ, `[bout bas, bout haut]` le long de l'axe
/// horizontal du plan, tirées des quatre coins `[NO, SO, SE, NE]`.
///
/// Le jeu les prend coin par coin (`tesselate`) : au nord NO puis NE, au sud
/// SO puis SE, à l'ouest NO puis SO, à l'est NE puis SE.
fn haut_du_cote(face: Face, c: [u8; 4]) -> [u8; 2] {
    let [no, so, se, ne] = c;
    match face {
        Face::MoinsZ => [no, ne],
        Face::PlusZ => [so, se],
        Face::MoinsX => [no, so],
        Face::PlusX => [ne, se],
        Face::MoinsY | Face::PlusY => [0, 0],
    }
}

/// Une face qu'on peut fusionner, en attente de sa tranche.
struct AFusionner {
    face: Face,
    /// Coordonnée de la case le long de l'axe de la face.
    profondeur: i32,
    /// Coordonnées dans le plan, dans l'ordre de `axes_du_plan`.
    iu: i32,
    iv: i32,
    cle: u64,
}

/// La clé de fusion d'une face plate : tout ce qui doit être égal pour que
/// deux faces n'en fassent qu'une.
///
/// Le biome (plus un) dans les 33 bits du bas, pour l'EAU seulement — la
/// lave n'en prend pas la couleur, et le mettre couperait ses quads à chaque
/// frontière pour rien. Puis le genre, la texture et la hauteur.
///
/// **Deux côtés empilés ne se fondent qu'à pleine hauteur, et la règle du
/// jeu le garantit** : sous une case de fluide, les quatre coins de la case
/// du dessous montent à la case entière. Un côté plus bas que la case n'a
/// donc jamais sous lui un côté de même clé — le haut d'un quad fusionné,
/// `rangées − 1 + hauteur`, ne vaut jamais que pour sa dernière rangée. Le
/// croisement avec la référence le vérifie case par case.
fn cle(genre: GenreFluide, texture: TextureFluide, hauteur: u8, biome: StateId) -> u64 {
    let b = match genre {
        GenreFluide::Eau => biome as u64 + 1,
        GenreFluide::Lave => 0,
    };
    b | (genre as u64) << 33 | (texture as u64) << 35 | (hauteur as u64) << 37
}

/// Relit une clé : `(genre, texture, hauteur, biome)`.
fn relire(c: u64) -> (GenreFluide, TextureFluide, u8, StateId) {
    let genre = if (c >> 33) & 3 == 2 {
        GenreFluide::Lave
    } else {
        GenreFluide::Eau
    };
    let texture = match (c >> 35) & 3 {
        1 => TextureFluide::Courant,
        2 => TextureFluide::Voile,
        _ => TextureFluide::Immobile,
    };
    let biome = match c & ((1u64 << 33) - 1) {
        0 => 0,
        b => (b - 1) as StateId,
    };
    (genre, texture, ((c >> 37) & 0xFF) as u8, biome)
}

/// Compose une position de case depuis « profondeur + deux axes du plan ».
#[inline]
const fn compose(axe: usize, d: i32, u: i32, v: i32) -> [i32; 3] {
    match axe {
        0 => [d, u, v],
        1 => [u, d, v],
        _ => [u, v, d],
    }
}

/// Maille les fluides d'un voisinage.
pub fn mailler<F: Formes + ?Sized>(v: &Voisinage, f: &F, out: &mut Vec<FaceFluide>) {
    mailler_avec(v, f, &Opacite::relever(v, f), out)
}

/// La même passe, avec la carte d'opacité déjà relevée par les deux autres.
pub fn mailler_avec<F: Formes + ?Sized>(
    v: &Voisinage,
    f: &F,
    op: &Opacite,
    out: &mut Vec<FaceFluide>,
) {
    let carte = Carte::relever(v, f);
    if carte.vide {
        return;
    }
    let n = COTE as i32;
    let mut a_fusionner: Vec<AFusionner> = Vec::new();

    for g in [GenreFluide::Eau, GenreFluide::Lave] {
        let gc = g as u8;
        for y in 0..n {
            for z in 0..n {
                // La rangée ENTIÈRE pour les voisines, peau comprise ; ses
                // seize cases intérieures pour ce qu'on maille. Masquer
                // avant de décaler ferait des deux cases de peau de l'air,
                // et chaque bord de section montrerait un mur d'eau.
                let rangee = carte.rangee(g, y, z);
                let fl = rangee & DEDANS;
                if fl == 0 {
                    continue;
                }
                // Les six faces visibles de la rangée, par bits. Contre le même
                // fluide, rien ; contre un bloc qui bouche sa case, rien — sauf
                // le DESSUS, qui ne se cache sous un bloc que s'il monte à la
                // case entière : on le décide case par case plus bas.
                let pas_moi = |r: u32, o: u32| !r & !o;
                let visibles: [u32; 6] = [
                    // −X, +X : la voisine est dans la même rangée.
                    fl & pas_moi(rangee << 1, op.rangee(y, z) << 1),
                    fl & pas_moi(rangee >> 1, op.rangee(y, z) >> 1),
                    // −Y, +Y
                    fl & pas_moi(carte.rangee(g, y - 1, z), op.rangee(y - 1, z)),
                    fl & !carte.rangee(g, y + 1, z),
                    // −Z, +Z
                    fl & pas_moi(carte.rangee(g, y, z - 1), op.rangee(y, z - 1)),
                    fl & pas_moi(carte.rangee(g, y, z + 1), op.rangee(y, z + 1)),
                ];
                let mut cases = visibles.iter().fold(0u32, |m, v| m | v) & DEDANS;
                while cases != 0 {
                    let x = cases.trailing_zeros() as i32 - 1;
                    cases &= cases - 1;
                    let bit = 1u32 << (x + 1);
                    // Les coins ne servent qu'au dessus et aux côtés ; un
                    // dessous seul n'en a pas besoin.
                    let mut quatre: Option<[u8; 4]> = None;
                    let mut les_coins =
                        || *quatre.get_or_insert_with(|| coins(v, f, &carte, gc, x, y, z));
                    let biome = match g {
                        GenreFluide::Eau => v.biome(x, y, z),
                        GenreFluide::Lave => 0,
                    };
                    for face in FACES {
                        if visibles[face.indice()] & bit == 0 {
                            continue;
                        }
                        let axe = face.axe();
                        let (au, av) = axes_du_plan(axe);
                        let p = [x, y, z];
                        let emettre = |texture, hauteurs, angle| FaceFluide {
                            pos: [x as u8, y as u8, z as u8],
                            taille: [1, 1],
                            face,
                            genre: g,
                            texture,
                            hauteurs,
                            angle,
                            biome,
                        };
                        match face {
                            Face::PlusY => {
                                let c = les_coins();
                                // Sous un bloc qui bouche sa case, un dessus
                                // qui monte à la case entière se confond avec
                                // la face du bloc : le jeu le cache.
                                if c.iter().all(|&h| h == PLEIN) && op.est(x, y + 1, z) {
                                    continue;
                                }
                                let (dx, dz) = courant(v, f, &carte, gc, x, y, z);
                                if dx != 0.0 || dz != 0.0 {
                                    out.push(emettre(
                                        TextureFluide::Courant,
                                        c,
                                        angle_du_courant(dx, dz),
                                    ));
                                } else if c.iter().all(|&h| h == c[0]) {
                                    a_fusionner.push(AFusionner {
                                        face,
                                        profondeur: y,
                                        iu: p[au],
                                        iv: p[av],
                                        cle: cle(g, TextureFluide::Immobile, c[0], biome),
                                    });
                                } else {
                                    out.push(emettre(TextureFluide::Immobile, c, 0));
                                }
                            }
                            Face::MoinsY => a_fusionner.push(AFusionner {
                                face,
                                profondeur: y,
                                iu: p[au],
                                iv: p[av],
                                cle: cle(g, TextureFluide::Immobile, 0, biome),
                            }),
                            _ => {
                                let [h0, h1] = haut_du_cote(face, les_coins());
                                let d = face.pas();
                                // Le voile : de l'eau contre un bloc translucide
                                // qui remplit sa case — verre, feuilles.
                                let voisin = v.get(x + d[0], y, z + d[2]);
                                let texture = if g == GenreFluide::Eau
                                    && !f.opaque(voisin)
                                    && f.cuboides(voisin).iter().any(|c| c.remplit())
                                {
                                    TextureFluide::Voile
                                } else {
                                    TextureFluide::Courant
                                };
                                if h0 == h1 {
                                    a_fusionner.push(AFusionner {
                                        face,
                                        profondeur: p[axe],
                                        iu: p[au],
                                        iv: p[av],
                                        cle: cle(g, texture, h0, biome),
                                    });
                                } else {
                                    out.push(emettre(texture, [h0, h1, 0, 0], 0));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── La fusion, tranche par tranche.
    if a_fusionner.is_empty() {
        return;
    }
    a_fusionner.sort_unstable_by_key(|a| (a.face, a.profondeur));
    let mut masque = [0u64; COTE * COTE];
    let mut i = 0;
    while i < a_fusionner.len() {
        let (face, profondeur) = (a_fusionner[i].face, a_fusionner[i].profondeur);
        let mut j = i;
        while j < a_fusionner.len()
            && a_fusionner[j].face == face
            && a_fusionner[j].profondeur == profondeur
        {
            let a = &a_fusionner[j];
            masque[(a.iv * n + a.iu) as usize] = a.cle;
            j += 1;
        }
        fusionner(face, profondeur, &mut masque, out);
        i = j;
    }
}

/// Fusionne les rectangles de même clé d'une tranche, et laisse le masque
/// PROPRE — la tranche suivante ne le nettoie pas.
fn fusionner(
    face: Face,
    profondeur: i32,
    masque: &mut [u64; COTE * COTE],
    out: &mut Vec<FaceFluide>,
) {
    let n = COTE as i32;
    let axe = face.axe();
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
            let (genre, texture, hauteur, biome) = relire(marque);
            let pos = compose(axe, profondeur, iu, iv);
            let hauteurs = match face {
                Face::PlusY => [hauteur; 4],
                Face::MoinsY => [0; 4],
                _ => [hauteur, hauteur, 0, 0],
            };
            out.push(FaceFluide {
                pos: [pos[0] as u8, pos[1] as u8, pos[2] as u8],
                taille: [w as u8, h as u8],
                face,
                genre,
                texture,
                hauteurs,
                angle: 0,
                biome,
            });
            iu += w;
        }
    }
    debug_assert!(
        masque.iter().all(|c| *c == 0),
        "la fusion doit laisser le masque propre"
    );
}
