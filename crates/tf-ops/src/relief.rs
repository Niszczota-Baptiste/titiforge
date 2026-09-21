//! Le LISSAGE — `//smooth`, et la carte de hauteurs qui le rend possible.
//!
//! ## Pourquoi ça ne tient dans aucune portée existante
//!
//! Lisser, c'est moyenner la hauteur d'une colonne avec celle de ses VOISINES.
//! Or une voisine peut être dans un autre chunk, et une opération n'en voit
//! qu'un. `Portee::Colonne` ne suffit donc pas : il faudrait une vue qui
//! déborde, et une vue qui déborde coûte cher à tenir juste — c'est le piège
//! `cold_read`, qui fait lire de l'air là où il y a de la pierre.
//!
//! La sortie est de faire DEUX passes plutôt qu'une vue plus large :
//!
//! 1. une passe de LECTURE qui relève la hauteur de chaque colonne de la
//!    sélection, marge comprise. Elle n'écrit rien, donc elle n'a aucune
//!    portée à respecter ;
//! 2. un calcul PUR sur la carte de hauteurs, hors de tout chunk ;
//! 3. une passe d'ÉCRITURE à portée `Colonne`, qui applique la carte déjà
//!    calculée. Chaque chunk y lit la carte entière, qui tient en mémoire :
//!    une sélection de mille blocs de côté fait quatre mégaoctets.
//!
//! ## L'unité, et l'erreur qu'elle a déjà coûtée
//!
//! **Les hauteurs sont en BLOCS, en coordonnées MONDE**, jamais en rapport
//! 0..1 ni en index local. `ExeWorldEdit` a payé exactement ça : `toHeights`
//! rendait des blocs, `applyHeightmap` attendait un rapport, chaque moitié
//! passait ses tests, et ensemble toute cellule non nulle devenait 1 — un
//! plateau plat à la place du relief, 1 022 cellules fausses sur 1 024. Ici
//! le type porte l'unité dans son nom et un test traverse la frontière.

use tf_anvil::{Interner, StateId};
use tf_world::coords::{BBox, BlockPos, ChunkPos};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::staging::{RegionStore, Staging};

use crate::colonnes::{Colonnes, Portee};
use crate::edition::Erreur;
use crate::masque::Masque;
use crate::plan::{Etage, Operation, Rapport};

/// Une carte de hauteurs, en BLOCS et en coordonnées MONDE.
///
/// `SANS_SOL` marque une colonne où l'on n'a trouvé aucun bloc solide. Ce
/// n'est pas une hauteur basse : une colonne vide ne doit ni tirer ses
/// voisines vers le bas ni se voir remplir. La confondre avec `y_min`
/// creuserait une fosse au bord de chaque sélection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carte {
    /// Coin de plus petites coordonnées, en monde.
    pub x0: i32,
    pub z0: i32,
    pub largeur: u32,
    pub profondeur: u32,
    /// Une hauteur par colonne, en `z * largeur + x`.
    pub h: Vec<i32>,
}

/// Aucune colonne solide ici.
pub const SANS_SOL: i32 = i32::MIN;

impl Carte {
    pub fn vide(x0: i32, z0: i32, largeur: u32, profondeur: u32) -> Carte {
        Carte {
            x0,
            z0,
            largeur,
            profondeur,
            h: vec![SANS_SOL; largeur as usize * profondeur as usize],
        }
    }

    #[inline]
    pub fn index(&self, x: i32, z: i32) -> Option<usize> {
        let (dx, dz) = (x - self.x0, z - self.z0);
        if dx < 0 || dz < 0 || dx >= self.largeur as i32 || dz >= self.profondeur as i32 {
            return None;
        }
        Some(dz as usize * self.largeur as usize + dx as usize)
    }

    pub fn get(&self, x: i32, z: i32) -> Option<i32> {
        self.index(x, z)
            .map(|i| self.h[i])
            .filter(|v| *v != SANS_SOL)
    }

    pub fn set(&mut self, x: i32, z: i32, y: i32) {
        if let Some(i) = self.index(x, z) {
            self.h[i] = y;
        }
    }

    /// La carte lissée par une moyenne de rayon `rayon`, répétée `passes`
    /// fois.
    ///
    /// **Pure, et c'est tout l'intérêt** : le relief se teste sans monter un
    /// monde, et la même fonction servira au jour où un greffon voudra sa
    /// propre forme de lissage.
    ///
    /// Les colonnes SANS SOL ne participent ni ne reçoivent : une moyenne qui
    /// les compterait comme zéro creuserait une fosse au bord de la carte,
    /// exactement là où l'utilisateur regarde.
    pub fn lissee(&self, rayon: u32, passes: u32) -> Carte {
        let mut out = self.clone();
        if rayon == 0 {
            return out;
        }
        let r = rayon as i32;
        for _ in 0..passes {
            let source = out.clone();
            for dz in 0..self.profondeur as i32 {
                for dx in 0..self.largeur as i32 {
                    let i = dz as usize * self.largeur as usize + dx as usize;
                    if source.h[i] == SANS_SOL {
                        continue;
                    }
                    let (mut somme, mut n) = (0i64, 0i64);
                    for oz in -r..=r {
                        for ox in -r..=r {
                            let (vx, vz) = (dx + ox, dz + oz);
                            if vx < 0
                                || vz < 0
                                || vx >= self.largeur as i32
                                || vz >= self.profondeur as i32
                            {
                                continue;
                            }
                            let v = source.h[vz as usize * self.largeur as usize + vx as usize];
                            if v == SANS_SOL {
                                continue;
                            }
                            somme += v as i64;
                            n += 1;
                        }
                    }
                    if n > 0 {
                        // Division EUCLIDIENNE : une hauteur négative est la
                        // norme depuis 1.18 (le monde descend à −64), et une
                        // division qui tronque vers zéro remonterait le relief
                        // d'un bloc sous y = 0 et pas au-dessus. Une marche
                        // d'un bloc à l'altitude zéro, exactement là où
                        // personne ne la cherche.
                        out.h[i] = (somme.div_euclid(n)) as i32;
                    }
                }
            }
        }
        out
    }

    /// La carte resserrée sur une boîte — la marge de lecture retirée.
    pub fn resserree(&self, x0: i32, z0: i32, largeur: u32, profondeur: u32) -> Carte {
        let mut out = Carte::vide(x0, z0, largeur, profondeur);
        for dz in 0..profondeur as i32 {
            for dx in 0..largeur as i32 {
                if let Some(i) = self.index(x0 + dx, z0 + dz) {
                    out.h[dz as usize * largeur as usize + dx as usize] = self.h[i];
                }
            }
        }
        out
    }
}

/// Relève la hauteur de chaque colonne d'une boîte.
///
/// Passe de LECTURE : elle n'écrit rien, donc aucune portée à respecter. La
/// boîte passée est la sélection ÉLARGIE de la marge du noyau — sans elle, le
/// bord de la sélection se lisserait contre le vide et s'effondrerait.
pub fn relever<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    boite: &BBox,
    solide: &Masque,
    interner: &mut Interner,
) -> Result<Carte, Erreur> {
    let (sx, _, sz) = boite.size();
    let mut carte = Carte::vide(boite.min.x, boite.min.z, sx, sz);
    // On réutilise `copier` : c'est la seule opération qui lit sans écrire, et
    // elle sait déjà traverser régions, chunks et sections.
    let extrait = crate::edition::copier(staging, dim, folder, boite, interner)?;
    let [ex, ey, ez] = extrait.taille;
    for z in 0..ez {
        for x in 0..ex {
            // De haut en bas : la première case solide EST la surface.
            for y in (0..ey).rev() {
                let Some(id) = extrait.get(x, y, z) else {
                    continue;
                };
                if solide.accepte(id) {
                    carte.set(
                        boite.min.x + x as i32,
                        boite.min.z + z as i32,
                        boite.min.y + y as i32,
                    );
                    break;
                }
            }
        }
    }
    Ok(carte)
}

/// Applique une carte de hauteurs déjà calculée.
///
/// Monter une colonne la remplit du bloc de sa SURFACE ; la descendre efface
/// jusqu'au remplissage. Reprendre le bloc de surface plutôt qu'un bloc fixe
/// est ce qui fait qu'une colline d'herbe reste en herbe et une dune de sable
/// en sable — sans avoir à deviner quoi que ce soit.
pub struct Lissage<'a> {
    pub carte: &'a Carte,
    /// Ce qu'on pose en descendant. De l'air, d'ordinaire.
    pub vide: StateId,
    pub compter: bool,
}

impl Operation for Lissage<'_> {
    fn appliquer(
        &self,
        _s: &mut tf_anvil::Section,
        _sel: &BBox,
        _p: tf_world::coords::SectionPos,
    ) -> Rapport {
        unreachable!("le lissage travaille par colonne")
    }

    fn compte(&self) -> bool {
        self.compter
    }

    fn portee(&self) -> Portee {
        Portee::Colonne
    }

    fn appliquer_colonnes(&self, c: &mut Colonnes, sel: &BBox, chunk: ChunkPos) -> Rapport {
        let Some((cy0, cy1)) = c.bornes_y() else {
            return Rapport::RIEN;
        };
        let y0 = sel.min.y.max(cy0);
        let y1 = sel.max.y.min(cy1);
        if y0 > y1 {
            return Rapport::RIEN;
        }
        let coin = BlockPos {
            x: chunk.x * 16,
            y: 0,
            z: chunk.z * 16,
        };
        let x0 = sel.min.x.max(coin.x);
        let x1 = sel.max.x.min(coin.x + 15);
        let z0 = sel.min.z.max(coin.z);
        let z1 = sel.max.z.min(coin.z + 15);

        let mut ecrits = 0u64;
        let mut bornes: Option<BBox> = None;
        let marquer = |p: BlockPos, bornes: &mut Option<BBox>| match bornes.as_mut() {
            Some(b) => b.extend(p),
            None => *bornes = Some(BBox::single(p)),
        };

        for wz in z0..=z1 {
            for wx in x0..=x1 {
                let Some(voulue) = self.carte.get(wx, wz) else {
                    continue; // colonne sans sol : on n'invente pas de terrain
                };
                let (lx, lz) = ((wx - coin.x) as usize, (wz - coin.z) as usize);
                // La hauteur ACTUELLE, relue dans le monde : la carte lissée
                // dit où aller, pas d'où l'on vient.
                let mut actuelle = None;
                for wy in (y0..=y1).rev() {
                    let Some(id) = c.get(lx, wy, lz) else {
                        continue;
                    };
                    if id != self.vide {
                        actuelle = Some((wy, id));
                        break;
                    }
                }
                let Some((haut, surface)) = actuelle else {
                    continue;
                };
                if voulue > haut {
                    // Monter : on prolonge avec le bloc de SURFACE.
                    for wy in (haut + 1)..=voulue.min(y1) {
                        if c.set(lx, wy, lz, surface) {
                            ecrits += 1;
                            marquer(
                                BlockPos {
                                    x: wx,
                                    y: wy,
                                    z: wz,
                                },
                                &mut bornes,
                            );
                        }
                    }
                } else {
                    // Descendre : on efface au-dessus de la hauteur voulue.
                    for wy in (voulue + 1).max(y0)..=haut {
                        if c.set(lx, wy, lz, self.vide) {
                            ecrits += 1;
                            marquer(
                                BlockPos {
                                    x: wx,
                                    y: wy,
                                    z: wz,
                                },
                                &mut bornes,
                            );
                        }
                    }
                }
            }
        }
        Rapport {
            etage: if ecrits == 0 {
                Etage::Rien
            } else {
                Etage::Bloc
            },
            blocs: self.compter.then_some(ecrits),
            bornes,
        }
    }
}
