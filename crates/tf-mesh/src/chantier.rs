//! Mailler un **paquet de sections** — et une peau qui ne ment pas.
//!
//! Une section seule ne peut pas être maillée juste : ses faces de bord
//! dépendent des six sections voisines, et trois d'entre elles sont dans
//! d'autres chunks. Un mailleur qui remplit sa peau d'air produit un mur de
//! faces fantômes le long de **chaque** frontière de chunk — invisible en
//! test, et le premier défaut qu'on voit à l'écran.
//!
//! `Grille` tient les sections décodées d'une zone et sait donc composer une
//! peau vraie. Elle ne connaît ni fichier, ni région, ni disque : on lui donne
//! des sections avec leurs coordonnées, elle rend des maillages.
//!
//! Elle sait aussi **ne pas travailler** : une section dont la palette est une
//! seule entrée d'air n'a rien à mailler, et ça se voit sur la palette, pas
//! après l'avoir parcourue. Mesuré, une section d'air coûte 17 µs au mailleur.

use std::collections::HashMap;

use tf_anvil::{Section, StateId};

use crate::forme::Formes;
use crate::maillage::{Instances, Maillage};
use crate::opacite::Opacite;
use crate::voisinage::{Voisinage, COTE};

/// Où vit une section : coordonnées de chunk MONDE, et hauteur de section.
pub type Adresse = (i32, i32, i8);

/// Les sections décodées d'une zone, adressables par leurs coordonnées.
#[derive(Default)]
pub struct Grille {
    sections: HashMap<Adresse, Section>,
    /// Les 64 cellules de biome d'une section, quand on les a.
    ///
    /// Une table à part plutôt qu'un champ de `Section` : les biomes sont une
    /// SECONDE palette, ils n'existent que depuis 1.18, et la moitié des
    /// appelants du mailleur — bancs, tests — n'en a rien à faire. Les rendre
    /// obligatoires ferait payer un décodage à tout le monde.
    biomes: HashMap<Adresse, Vec<StateId>>,
}

impl Grille {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn poser(&mut self, chunk_x: i32, chunk_z: i32, s: Section) {
        self.sections.insert((chunk_x, chunk_z, s.y), s);
    }

    /// Pose les 64 cellules de biome d'une section. Une longueur inattendue
    /// est REFUSÉE plutôt que tronquée : un biome décalé donne un sol de la
    /// mauvaise couleur, et rien ne le dirait.
    pub fn poser_biomes(&mut self, chunk_x: i32, chunk_z: i32, y: i8, cells: Vec<StateId>) -> bool {
        if cells.len() != crate::voisinage::VOL_BIOME {
            return false;
        }
        self.biomes.insert((chunk_x, chunk_z, y), cells);
        true
    }

    pub fn len(&self) -> usize {
        self.sections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn adresses(&self) -> Vec<Adresse> {
        let mut v: Vec<Adresse> = self.sections.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn section(&self, a: Adresse) -> Option<&Section> {
        self.sections.get(&a)
    }

    /// Un bloc, en coordonnées MONDE. `0` — l'air — pour ce qui n'est pas là.
    ///
    /// Ce qui manque vaut de l'air et non « opaque » : au bord d'une zone
    /// chargée, supposer opaque effacerait des faces réelles. Entre deux
    /// erreurs on prend celle qui se voit.
    #[inline]
    pub fn bloc(&self, x: i32, y: i32, z: i32) -> StateId {
        let a = (x.div_euclid(16), z.div_euclid(16), y.div_euclid(16) as i8);
        match self.sections.get(&a) {
            Some(s) => s
                .get(
                    x.rem_euclid(16) as usize,
                    y.rem_euclid(16) as usize,
                    z.rem_euclid(16) as usize,
                )
                .unwrap_or(0),
            None => 0,
        }
    }

    /// Vrai si cette section ne peut rien produire : palette d'une entrée, et
    /// c'est de l'air.
    ///
    /// C'est l'appelant — pas le mailleur — qui peut le savoir gratuitement.
    /// Une section d'air et une section de plantes sont indiscernables du point
    /// de vue de l'opacité ; leurs PALETTES, elles, ne le sont pas.
    pub fn sans_contenu<F: Formes + ?Sized>(&self, a: Adresse, f: &F) -> bool {
        match self.sections.get(&a) {
            None => true,
            Some(s) => s.palette.iter().all(|id| f.est_air(*id)),
        }
    }

    /// Remplit un voisinage pour une section, peau comprise et VRAIE.
    ///
    /// Les VINGT-SEPT sections autour (la sienne comprise) sont relevées une
    /// fois, puis indexées par un tableau. Chercher la section de chaque case
    /// de peau coûtait 1 944 hachages par section — mesuré, un tiers du temps
    /// de maillage d'une région partait là.
    pub fn voisinage(&self, a: Adresse, v: &mut Voisinage) {
        let (cx, cz, sy) = a;
        let mut autour: [Option<&Section>; 27] = [None; 27];
        for dy in -1..=1i32 {
            let Some(ny) = i8::try_from(sy as i32 + dy).ok() else {
                continue;
            };
            for dz in -1..=1i32 {
                for dx in -1..=1i32 {
                    autour[((dy + 1) * 9 + (dz + 1) * 3 + (dx + 1)) as usize] =
                        self.sections.get(&(cx + dx, cz + dz, ny));
                }
            }
        }

        // Les biomes de LA section, sans peau : la teinte d'une face se prend
        // dans la case qui la porte.
        match self.biomes.get(&a) {
            Some(b) => v.poser_biomes(b),
            None => v.poser_biomes(&[]),
        }

        // Le cœur vient de la section elle-même, dépackée d'un coup : 4 096
        // extractions de bits valent mieux que 4 096 appels qui redécident du
        // packing à chaque case.
        let n = COTE as i32;
        match autour[13] {
            Some(s) => {
                let idx = s.unpack();
                for i in 0..4096usize {
                    let y = (i / 256) as i32;
                    let z = ((i / 16) % 16) as i32;
                    let x = (i % 16) as i32;
                    // Un indice que la palette ne contient pas vient d'un
                    // `.mca` corrompu ou forgé : `bits` se DÉDUIT de la
                    // longueur de palette, donc deux entrées se lisent sur
                    // quatre bits et seize valeurs sont représentables. Le
                    // mailleur ne doit pas mourir dessus — on ne DESSINE pas ce
                    // qu'on ne comprend pas, on rend de l'air.
                    v.set(
                        x,
                        y,
                        z,
                        s.palette.get(idx[i] as usize).copied().unwrap_or(0),
                    );
                }
            }
            None => {
                for y in 0..n {
                    for z in 0..n {
                        for x in 0..n {
                            v.set(x, y, z, 0);
                        }
                    }
                }
            }
        }

        // La peau. Ce qui manque vaut de l'air et non « opaque » : au bord
        // d'une zone chargée, supposer opaque effacerait des faces RÉELLES.
        // Entre deux erreurs, on prend celle qui se voit.
        let cote = |k: i32| {
            if k < 0 {
                -1
            } else if k >= n {
                1
            } else {
                0
            }
        };
        for y in -1..=n {
            for z in -1..=n {
                for x in -1..=n {
                    if Voisinage::dedans(x, y, z) {
                        continue;
                    }
                    let k = ((cote(y) + 1) * 9 + (cote(z) + 1) * 3 + (cote(x) + 1)) as usize;
                    let id = match autour[k] {
                        Some(s) => s
                            .get(
                                x.rem_euclid(16) as usize,
                                y.rem_euclid(16) as usize,
                                z.rem_euclid(16) as usize,
                            )
                            .unwrap_or(0),
                        None => 0,
                    };
                    v.set(x, y, z, id);
                }
            }
        }
    }

    /// Maille tout ce que la grille porte, en séquence.
    pub fn mailler<F: Formes + ?Sized>(&self, f: &F) -> Chantier {
        let mut out = Chantier::default();
        let mut v = Voisinage::new();
        for a in self.adresses() {
            if self.sans_contenu(a, f) {
                out.sautees += 1;
                continue;
            }
            self.voisinage(a, &mut v);
            let op = Opacite::relever(&v, f);
            let mut lot = Lot::vide(a);
            crate::glouton::mailler_avec(&v, f, &op, &mut lot.quads);
            crate::modeles::instancier_avec(&v, f, &op, &mut lot.poses);
            out.lots.push(lot);
        }
        out
    }
}

/// Le maillage d'UNE section, avec son adresse.
///
/// Les quads sont en coordonnées locales à la section. Les fondre tous dans une
/// même liste perdrait la seule information dont le rendu a besoin : quelle
/// section les porte. C'est elle qui se cull, qui se remaille quand on pose un
/// bloc, et qui occupe une tranche de l'arène GPU.
#[derive(Debug)]
pub struct Lot {
    pub adresse: Adresse,
    pub quads: Maillage,
    pub poses: Instances,
}

impl Lot {
    pub fn vide(adresse: Adresse) -> Self {
        Lot {
            adresse,
            quads: Maillage::new(),
            poses: Instances::new(),
        }
    }

    /// Coin de plus petites coordonnées de la section, en blocs MONDE.
    pub fn origine(&self) -> [i32; 3] {
        let (cx, cz, sy) = self.adresse;
        [cx * 16, sy as i32 * 16, cz * 16]
    }

    pub fn est_vide(&self) -> bool {
        self.quads.is_empty() && self.poses.is_empty()
    }
}

/// Ce qu'un chantier a produit.
#[derive(Debug, Default)]
pub struct Chantier {
    pub lots: Vec<Lot>,
    /// Sections sautées faute de contenu.
    pub sautees: usize,
}

impl Chantier {
    pub fn maillees(&self) -> usize {
        self.lots.len()
    }

    pub fn quads(&self) -> usize {
        self.lots.iter().map(|l| l.quads.len()).sum()
    }

    pub fn poses(&self) -> usize {
        self.lots.iter().map(|l| l.poses.len()).sum()
    }

    /// Octets que ça pèse : 16 par quad, 12 par pose.
    pub fn octets(&self) -> usize {
        self.quads() * 16 + self.poses() * std::mem::size_of::<crate::maillage::Instance>()
    }

    pub fn fusionner(&mut self, autre: Chantier) {
        self.lots.extend(autre.lots);
        self.sautees += autre.sautees;
    }

    /// Les lots dans un ordre stable, quel que soit celui de production.
    ///
    /// Un chantier parallèle rend ses lots dans l'ordre où les fils finissent.
    /// Comparer deux chantiers sans trier comparerait l'ordonnancement.
    pub fn trier(&mut self) {
        self.lots.sort_by_key(|l| l.adresse);
    }
}

#[cfg(feature = "parallele")]
mod parallele {
    use super::*;
    use rayon::prelude::*;

    impl Grille {
        /// Le même chantier, sur tous les cœurs.
        ///
        /// Une section est une unité de travail parfaite : elle ne lit que la
        /// grille, qui ne change pas, et elle n'écrit que son propre lot. Il
        /// n'y a rien à fusionner — contrairement au chargement, où les
        /// palettes des fils doivent être réunifiées.
        ///
        /// Le `Voisinage` fait 23 Ko et se réutilise par FIL, pas par section :
        /// l'allouer à chaque section rendrait le parallélisme à l'allocateur.
        pub fn mailler_parallele<F>(&self, f: &F) -> Chantier
        where
            F: Formes + Sync + ?Sized,
        {
            let adresses = self.adresses();
            let (lots, sautees): (Vec<Vec<Lot>>, Vec<usize>) = adresses
                .par_chunks(16)
                .map(|paquet| {
                    let mut v = Voisinage::new();
                    let mut lots = Vec::with_capacity(paquet.len());
                    let mut sautees = 0usize;
                    for &a in paquet {
                        if self.sans_contenu(a, f) {
                            sautees += 1;
                            continue;
                        }
                        self.voisinage(a, &mut v);
                        let op = Opacite::relever(&v, f);
                        let mut lot = Lot::vide(a);
                        crate::glouton::mailler_avec(&v, f, &op, &mut lot.quads);
                        crate::modeles::instancier_avec(&v, f, &op, &mut lot.poses);
                        lots.push(lot);
                    }
                    (lots, sautees)
                })
                .unzip();
            Chantier {
                lots: lots.into_iter().flatten().collect(),
                sautees: sautees.into_iter().sum(),
            }
        }
    }
}
