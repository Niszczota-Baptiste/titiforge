//! **L'accrochage : viser ce qui est BÂTI, pas seulement la grille.**
//!
//! C'est ce qui fait SketchUp, et sur une grille de blocs le problème n'est
//! pas celui de la CAO. En CAO, accrocher à la grille est la moitié du
//! travail ; ici **tout est déjà sur la grille**, donc ça ne vaut rien. Ce
//! qui manque est d'accrocher à ce qui EXISTE : le nu d'un mur, le coin d'une
//! tour, la hauteur de la fenêtre d'à côté.
//!
//! ## Un seul mécanisme pour les trois accroches de SketchUp
//!
//! On accroche **axe par axe**, indépendamment. Une référence est un point ;
//! de là découlent les trois accroches, sans code séparé :
//!
//! | Axes accrochés | Ce que ça donne |
//! |---|---|
//! | un | un PLAN — le nu d'un mur, une altitude |
//! | deux | une DROITE — l'arête d'un bâtiment, un alignement |
//! | trois | le POINT lui-même |
//!
//! Poser `pos1` puis tirer en n'accrochant que `y` et `z` donne exactement la
//! ligne droite le long de X que tout le monde attend. C'est la réduction
//! juste pour une grille, et elle évite trois implémentations qui
//! divergeraient.
//!
//! ## Une inférence qui accroche en SILENCE est une inférence qu'on combat
//!
//! Dans SketchUp, ce qui rend l'accrochage utilisable n'est pas sa précision,
//! c'est qu'il DIT ce qu'il a attrapé. Sans ça, la position saute et
//! l'utilisateur ne sait pas s'il a mal visé ou si l'outil a décidé pour lui.
//! `Accroche` rend donc une raison PAR AXE, avec sa référence et son écart —
//! de quoi écrire « aligné sur le coin du mur » et, surtout, de quoi
//! comprendre pourquoi ça n'a pas bougé où l'on voulait.

use crate::coords::{BBox, BlockPos};

/// Ce qu'une référence REPRÉSENTE. Sert à départager, et à le DIRE.
///
/// Nommé `Ancre` et pas `Genre` : le journal a déjà un `Genre`, qui désigne
/// une sorte d'entrée. Deux types du même nom à la racine d'un crate
/// s'important l'un l'autre par accident, et le compilateur ne les distingue
/// qu'au point d'usage — là où on lit le moins.
///
/// L'ordre est la PRIORITÉ, du plus fort au plus faible — c'est celui de
/// SketchUp : un coin l'emporte sur un milieu, qui l'emporte sur un centre.
/// Deux références à égale distance ne doivent pas alterner d'une image à
/// l'autre : l'accrochage clignoterait, et on ne saurait pas quoi viser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ancre {
    /// Un coin de quelque chose — le plus fort.
    Coin,
    /// Le milieu d'une arête.
    Milieu,
    /// Le centre d'un volume ou d'une face.
    Centre,
    /// Le nu d'une paroi qu'on vient de survoler.
    Nu,
    /// Le dernier point posé.
    Dernier,
}

impl Ancre {
    pub const fn nom(self) -> &'static str {
        match self {
            Ancre::Coin => "coin",
            Ancre::Milieu => "milieu",
            Ancre::Centre => "centre",
            Ancre::Nu => "nu",
            Ancre::Dernier => "dernier point",
        }
    }
}

/// Un point auquel on peut s'accrocher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    pub point: BlockPos,
    pub genre: Ancre,
}

impl Reference {
    pub const fn new(point: BlockPos, genre: Ancre) -> Reference {
        Reference { point, genre }
    }
}

/// Pourquoi un axe a bougé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raison {
    pub genre: Ancre,
    /// La référence qui a gagné, en entier — pour la DESSINER : c'est le trait
    /// pointillé de SketchUp, et sans lui on ne voit pas à quoi on s'accroche.
    pub reference: BlockPos,
    /// De combien de blocs l'axe a été déplacé. Zéro veut dire qu'on était
    /// déjà dessus.
    pub ecart: i32,
}

/// Le résultat : une position, et ce qui l'explique axe par axe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accroche {
    pub position: BlockPos,
    /// Indexé par axe : 0 = X, 1 = Y, 2 = Z.
    pub raisons: [Option<Raison>; 3],
}

impl Accroche {
    /// Combien d'axes ont accroché. Un, c'est un plan ; deux, une droite ;
    /// trois, un point.
    pub fn axes_accroches(&self) -> usize {
        self.raisons.iter().filter(|r| r.is_some()).count()
    }

    pub fn a_bouge(&self) -> bool {
        self.raisons.iter().flatten().any(|r| r.ecart != 0)
    }
}

/// La tolérance par défaut, en BLOCS.
///
/// **Elle est en blocs et pas en pixels, et l'appelant doit le savoir.**
/// SketchUp exprime la sienne à l'écran, ce qui la rend indépendante du zoom.
/// Ici la géométrie est entière et l'accrochage vit côté monde : deux blocs
/// est un choix qui marche à la distance de travail habituelle. Vu de très
/// loin, une coque avisée l'augmentera — c'est son affaire, pas celle de la
/// géométrie, et lui faire deviner le zoom la lierait à la caméra.
pub const TOLERANCE: i32 = 2;

/// Accroche une position brute aux références, axe par axe.
///
/// `verrous` force un axe à une valeur — c'est le verrouillage d'axe de
/// SketchUp (maintenir une direction). **Un verrou l'emporte sur tout**, y
/// compris sur une référence plus proche : il a été demandé explicitement, et
/// un verrou qu'une accroche peut défaire n'est pas un verrou.
///
/// Départage, dans cet ordre : le plus petit ÉCART, puis le GENRE le plus
/// fort, puis la plus petite coordonnée. Trois critères pour que deux
/// références à égale distance ne se relaient pas d'une image à l'autre —
/// l'accrochage clignoterait, et un clignotement se lit « l'outil est
/// instable » sans qu'on sache quoi regarder.
pub fn accrocher(
    brut: BlockPos,
    references: &[Reference],
    tolerance: i32,
    verrous: [Option<i32>; 3],
) -> Accroche {
    let mut axes = [brut.x, brut.y, brut.z];
    let mut raisons: [Option<Raison>; 3] = [None; 3];
    let tolerance = tolerance.max(0);

    for k in 0..3 {
        if let Some(v) = verrous[k] {
            let ancien = axes[k];
            axes[k] = v;
            raisons[k] = Some(Raison {
                genre: Ancre::Dernier,
                reference: BlockPos::new(
                    if k == 0 { v } else { brut.x },
                    if k == 1 { v } else { brut.y },
                    if k == 2 { v } else { brut.z },
                ),
                ecart: v - ancien,
            });
            continue;
        }
        let mut meilleure: Option<(i32, Ancre, BlockPos)> = None;
        for r in references {
            let cible = [r.point.x, r.point.y, r.point.z][k];
            // En `i64` : `brut` et la référence peuvent être aux deux bouts du
            // domaine, et la différence déborderait un `i32`.
            let ecart = (cible as i64 - axes[k] as i64).unsigned_abs();
            if ecart > tolerance as u64 {
                continue;
            }
            let candidat = (ecart as i32, r.genre, r.point);
            meilleure = Some(match meilleure {
                None => candidat,
                Some(m) => {
                    // (écart, genre, point) : trois critères, et le dernier
                    // est total. Sans lui, deux coins symétriques d'une même
                    // boîte se relaieraient.
                    let cle = |c: &(i32, Ancre, BlockPos)| (c.0, c.1, c.2.x, c.2.y, c.2.z);
                    if cle(&candidat) < cle(&m) {
                        candidat
                    } else {
                        m
                    }
                }
            });
        }
        if let Some((_, genre, point)) = meilleure {
            let cible = [point.x, point.y, point.z][k];
            raisons[k] = Some(Raison {
                genre,
                reference: point,
                ecart: cible - axes[k],
            });
            axes[k] = cible;
        }
    }

    Accroche {
        position: BlockPos::new(axes[0], axes[1], axes[2]),
        raisons,
    }
}

impl BBox {
    /// Les points remarquables d'une boîte : huit coins, douze milieux
    /// d'arête, un centre.
    ///
    /// **Le milieu d'une arête PAIRE n'existe pas**, et le choix doit être
    /// stable : une arête de quatre blocs n'a pas de bloc central. On prend
    /// systématiquement le plus BAS par division plancher. Alterner selon la
    /// parité ferait sauter l'accroche d'un bloc quand on redimensionne, ce
    /// qui se lit « le milieu bouge tout seul ».
    ///
    /// Les milieux portent sur le SOLIDE, donc de `min` à `max + 1` : le
    /// milieu d'une section va de 0 à 16, donc 8 — pas 7, qui serait le milieu
    /// des indices de blocs et tomberait à côté du centre visuel.
    pub fn references(&self) -> Vec<Reference> {
        let bas = [self.min.x, self.min.y, self.min.z];
        let haut = [self.max.x, self.max.y, self.max.z];
        // Le milieu du SOLIDE : (min + max + 1) / 2, en division plancher.
        let mid = |k: usize| {
            ((bas[k] as i64 + haut[k] as i64 + 1).div_euclid(2))
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };
        let milieu = [mid(0), mid(1), mid(2)];
        let mut out = Vec::with_capacity(21);

        // Les huit coins.
        for i in 0..8 {
            let c = |k: usize| if (i >> k) & 1 == 0 { bas[k] } else { haut[k] };
            out.push(Reference::new(BlockPos::new(c(0), c(1), c(2)), Ancre::Coin));
        }
        // Les douze milieux d'arête : pour chaque axe, les quatre arêtes
        // parallèles portent leur milieu sur cet axe.
        for k in 0..3 {
            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
            for i in 0..4 {
                let mut p = [0i32; 3];
                p[k] = milieu[k];
                p[a] = if i & 1 == 0 { bas[a] } else { haut[a] };
                p[b] = if i & 2 == 0 { bas[b] } else { haut[b] };
                out.push(Reference::new(
                    BlockPos::new(p[0], p[1], p[2]),
                    Ancre::Milieu,
                ));
            }
        }
        out.push(Reference::new(
            BlockPos::new(milieu[0], milieu[1], milieu[2]),
            Ancre::Centre,
        ));
        out
    }
}
