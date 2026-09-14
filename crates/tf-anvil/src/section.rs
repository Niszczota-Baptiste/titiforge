//! La section 16³ **packée** : palette d'états internés + indices bit-packés.
//!
//! C'est la réponse au mur qui a tué `we-engine` et MCEdit2 avant lui : un
//! `{Name, Properties}` par bloc coûte 154 octets mesurés, donc 15 Go pour
//! pivoter 100 millions de blocs. Ici une section, c'est une palette de
//! quelques dizaines d'entiers et 4096 indices de 4 à 12 bits.
//!
//! Repère Minecraft : `i = y*256 + z*16 + x` (ordre **YZX**), x,y,z ∈ [0,15].
//! +X = Est, +Z = Sud, +Y = Haut.
//!
//! Format 1.16+ : un indice ne chevauche **jamais** deux longs — les bits de
//! poids fort inutilisés de chaque long sont à zéro. (Litematica, elle, packe
//! AVEC chevauchement : ne pas confondre les deux.)

use crate::format::{self, Packing};
use crate::state::StateId;

/// Nombre de blocs dans une section.
pub const VOL: usize = 4096;

/// Palette maximale du format : 4096 entrées, soit 12 bits par indice.
pub const MAX_PALETTE: usize = 4096;

/// Bits par indice pour une palette de `len` entrées — règle Minecraft.
///
/// Plancher à 4, même pour une palette de 2. **Plafond à 12**, parce qu'une
/// section ne contient que 4096 blocs : elle ne peut pas porter plus de 4096
/// états distincts, et le jeu ne lit pas au-delà.
///
/// Sans le plafond, une palette de 5000 entrées produisait 13 bits et un
/// fichier qu'aucun Minecraft ne relit — sans la moindre erreur. Au-delà de
/// 16 bits, les indices ne tenaient même plus dans le `u16` du dépack et
/// sortaient tronqués à 65535.
#[inline]
pub fn bits_for(len: usize) -> u8 {
    let n = len.max(2);
    let mut b = 0u32;
    while (1usize << b) < n {
        b += 1;
    }
    (b as u8).clamp(4, MAX_BITS)
}

/// Bits par indice au maximum du format : 12, soit 4096 entrées de palette.
pub const MAX_BITS: u8 = 12;

/// Index local d'un bloc dans une section, en ordre YZX.
///
/// Les trois coordonnées doivent être dans `[0, 15]`. Au-delà, les bits
/// débordent sur l'axe voisin et l'index désigne une AUTRE case sans rien
/// signaler : `local_index(16, 0, 0)` vaut 16, c'est-à-dire `(0, 0, 1)`. Le
/// `debug_assert` attrape l'erreur dans les tests ; `Section::get` la borne
/// aussi en production, pour ne jamais rendre le bloc du voisin.
#[inline]
pub fn local_index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(
        x < 16 && y < 16 && z < 16,
        "coordonnées locales hors [0,15] : ({x},{y},{z})"
    );
    (y << 8) | (z << 4) | x
}

/// Vraie si les trois coordonnées tiennent dans une section.
#[inline]
pub fn in_section(x: usize, y: usize, z: usize) -> bool {
    x < 16 && y < 16 && z < 16
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Hauteur de la section, en pas de 16 blocs. Peut être négative.
    pub y: i8,
    pub palette: Vec<StateId>,
    pub bits: u8,
    /// Vide pour une section homogène (palette d'une entrée), comme Minecraft.
    pub data: Box<[u64]>,
    /// Comment `data` est rangé. Portée par la section et non déduite d'une
    /// version : on réécrit une section dans le packing du fichier d'où elle
    /// vient, sinon un monde 1.15 ressortirait illisible pour son propre jeu.
    pub packing: Packing,
}

impl Section {
    /// Section homogène d'un seul état, au packing moderne.
    pub fn uniform(y: i8, id: StateId) -> Self {
        Section {
            y,
            palette: vec![id],
            bits: bits_for(1),
            data: Box::new([]),
            packing: Packing::NoStraddle,
        }
    }

    /// Octets occupés par la forme packée.
    pub fn packed_bytes(&self) -> usize {
        self.data.len() * 8 + self.palette.len() * 4 + std::mem::size_of::<Section>()
    }

    /// Vraie si la section ne porte qu'un seul état.
    pub fn is_uniform(&self) -> bool {
        self.palette.len() <= 1 || self.data.is_empty()
    }

    /// Dépacke les 4096 indices de palette.
    pub fn unpack(&self) -> Box<[u16]> {
        let mut out = vec![0u16; VOL].into_boxed_slice();
        self.unpack_into(&mut out);
        out
    }

    /// Dépacke dans un tampon fourni — permet de réutiliser une zone de
    /// travail au lieu d'allouer par section.
    pub fn unpack_into(&self, out: &mut [u16]) {
        debug_assert!(out.len() >= VOL);
        out[..VOL].fill(0);
        if self.is_uniform() {
            return;
        }
        format::unpack_into(&self.data, VOL, self.bits as usize, self.packing, out);
    }

    /// Repacke depuis 4096 indices, en recalculant `bits` d'après la palette.
    ///
    /// **Compacte d'abord si la palette dépasse 4096 entrées.** Ce n'est pas
    /// une précaution : c'est ce qui garantit qu'un `repack` produit toujours
    /// une section que le jeu sait relire. Et le compactage ne peut pas
    /// échouer — une section n'a que 4096 blocs, donc au plus 4096 états
    /// distincts y sont référencés ; tout le reste est mort et se retire.
    pub fn repack(&mut self, idx: &[u16]) {
        assert_eq!(idx.len(), VOL, "repack attend exactement {VOL} indices");
        if self.palette.len() > MAX_PALETTE {
            let idx = self.compacter(idx);
            return self.repack_brut(&idx);
        }
        self.repack_brut(idx);
    }

    fn repack_brut(&mut self, idx: &[u16]) {
        if self.palette.len() <= 1 {
            self.bits = bits_for(1);
            self.data = Box::new([]);
            return;
        }
        debug_assert!(
            self.palette.len() <= MAX_PALETTE,
            "palette de {} entrées : au-delà de ce que le format peut porter",
            self.palette.len()
        );
        let bits = bits_for(self.palette.len()) as usize;
        self.bits = bits as u8;
        self.data = format::pack(idx, bits, self.packing).into_boxed_slice();
    }

    /// Réduit la palette à ses entrées RÉELLEMENT référencées, dédoublonnées,
    /// et rend les indices remappés. Le cœur commun de `compact_palette` et du
    /// garde-fou de `repack`.
    fn compacter(&mut self, idx: &[u16]) -> Vec<u16> {
        let mut newpal: Vec<StateId> = Vec::with_capacity(self.palette.len().min(MAX_PALETTE));
        let mut lut: Vec<u16> = vec![0; self.palette.len()];
        let mut vus: std::collections::HashMap<StateId, u16> = std::collections::HashMap::new();

        let mut used = vec![false; self.palette.len()];
        for &v in idx {
            if let Some(u) = used.get_mut(v as usize) {
                *u = true;
            }
        }
        for (i, &e) in self.palette.iter().enumerate() {
            if !used[i] {
                continue; // laissé à 0 : aucun indice ne le désigne
            }
            let pos = *vus.entry(e).or_insert_with(|| {
                newpal.push(e);
                (newpal.len() - 1) as u16
            });
            lut[i] = pos;
        }
        if newpal.is_empty() {
            newpal.push(self.palette.first().copied().unwrap_or(0));
        }
        self.palette = newpal;
        idx.iter().map(|&o| lut[o as usize]).collect()
    }

    /// État à une position locale, ou `None` hors de la section.
    ///
    /// La borne est explicite : sans elle, `get(16, 0, 0)` rendait le bloc de
    /// `(0, 0, 1)` — un résultat plausible, et faux.
    pub fn get(&self, x: usize, y: usize, z: usize) -> Option<StateId> {
        if !in_section(x, y, z) {
            return None;
        }
        let n = local_index(x, y, z);
        if self.is_uniform() {
            return self.palette.first().copied();
        }
        let bits = self.bits as usize;
        let mask = (1u64 << bits) - 1;
        let i = match self.packing {
            Packing::NoStraddle => {
                let per_long = 64 / bits;
                let w = *self.data.get(n / per_long)?;
                ((w >> ((n % per_long) * bits)) & mask) as usize
            }
            Packing::Straddle => {
                let off = n * bits;
                let low = *self.data.get(off / 64)?;
                let b = off % 64;
                let v = if b + bits <= 64 {
                    (low >> b) & mask
                } else {
                    let high = self.data.get(off / 64 + 1).copied().unwrap_or(0);
                    ((low >> b) | (high << (64 - b))) & mask
                };
                v as usize
            }
        };
        self.palette.get(i).copied()
    }

    /// Compte les blocs valant `id`.
    ///
    /// Cherche **toutes** les occurrences dans la palette, jamais la première :
    /// l'étage palette ne dédoublonne pas, donc un état peut y figurer
    /// plusieurs fois. Voir `replace_state`.
    pub fn count_of(&self, id: StateId) -> usize {
        // Une TABLE indexée sur la palette, pas une recherche par bloc. La
        // version en `contains()` coûtait O(palette × 4096) : mesuré à 340 µs
        // pour une section à grosse palette, soit 8,4 s pour une région.
        let mut cible = vec![false; self.palette.len()];
        let mut aucune = true;
        for (i, &e) in self.palette.iter().enumerate() {
            if e == id {
                cible[i] = true;
                aucune = false;
            }
        }
        if aucune {
            return 0;
        }
        if self.is_uniform() {
            return VOL;
        }
        let idx = self.unpack();
        idx.iter()
            .filter(|&&v| cible.get(v as usize).copied().unwrap_or(false))
            .count()
    }

    // ── ÉTAGE SECTION ───────────────────────────────────────────────────────

    /// Toute la section devient `id` : palette d'une entrée, aucun indice.
    /// O(1), et le fichier rétrécit.
    pub fn set_uniform(&mut self, id: StateId) {
        self.palette.clear();
        self.palette.push(id);
        self.bits = bits_for(1);
        self.data = Box::new([]);
    }

    // ── ÉTAGE PALETTE ───────────────────────────────────────────────────────

    /// Remplace `from` par `to` **sans toucher un seul indice de bloc**, et
    /// **sans dédoublonner**.
    ///
    /// Le dédoublonnage est ce qui annulait tout le gain. Mesuré sur du vrai
    /// terrain : une section qui contient de la pierre contient presque
    /// toujours de la terre, donc fusionner les deux entrées obligeait à
    /// remapper les 4096 indices — 9 216 sections sur 9 216 prenaient le
    /// chemin lent, et l'étage palette retombait au niveau de l'étage bloc
    /// (23,3 ms contre 26,8). Sans fusion : **0,26 ms**.
    ///
    /// Le format l'autorise : Anvil n'interdit pas deux entrées identiques, le
    /// jeu lit `palette[indice]` et obtient un état valide dans les deux cas.
    /// La longueur de la palette ne bouge pas, donc `bits` non plus.
    ///
    /// **Corollaire, et c'est la partie dangereuse :** tout ce qui cherche un
    /// état dans une palette doit chercher TOUTES les occurrences. Un
    /// `position()` au lieu d'un filtre ferait rater la moitié d'un `//replace`
    /// suivant, en silence.
    ///
    /// Rend le nombre d'entrées de palette réécrites.
    pub fn replace_state(&mut self, from: StateId, to: StateId) -> usize {
        let mut n = 0;
        for e in self.palette.iter_mut() {
            if *e == from {
                *e = to;
                n += 1;
            }
        }
        n
    }

    /// Vraie si la palette contient `id` — le test rapide qui permet de sauter
    /// une section sans la dépacker.
    pub fn contains_state(&self, id: StateId) -> bool {
        self.palette.contains(&id)
    }

    /// Compacte la palette : fusionne les entrées identiques et retire celles
    /// qui ne sont plus référencées. Coûte un dépack + repack, donc ne
    /// s'appelle **qu'à l'écriture**, jamais dans une boucle chaude.
    ///
    /// Rend le nombre d'entrées supprimées.
    pub fn compact_palette(&mut self) -> usize {
        let before = self.palette.len();
        if before <= 1 {
            return 0;
        }
        let idx = self.unpack();
        let avant_pal = self.palette.clone();
        let next = self.compacter(&idx);

        // Déjà compacte : ne rien réécrire. Un repack inutile coûte le prix
        // d'un repack, et `compact_palette` tourne sur chaque section à
        // l'écriture.
        if self.palette.len() == before && self.palette == avant_pal {
            return 0;
        }
        self.repack_brut(&next);
        before - self.palette.len()
    }

    // ── ÉTAGE BLOC ──────────────────────────────────────────────────────────

    /// Applique `f` à chaque bloc. Rend le nombre de blocs changés.
    /// Réservé aux opérations qui lisent vraiment chaque case.
    pub fn map_blocks(&mut self, mut f: impl FnMut(StateId) -> StateId) -> usize {
        if self.is_uniform() {
            let old = match self.palette.first() {
                Some(&v) => v,
                None => return 0,
            };
            let new = f(old);
            if new == old {
                return 0;
            }
            self.palette[0] = new;
            return VOL;
        }
        // La palette est petite : on transforme les ENTRÉES, pas les blocs,
        // puis on ne compte que ce qui a bougé.
        let mut changed_ids = vec![false; self.palette.len()];
        let mut any = false;
        for (i, e) in self.palette.iter_mut().enumerate() {
            let new = f(*e);
            if new != *e {
                *e = new;
                changed_ids[i] = true;
                any = true;
            }
        }
        if !any {
            return 0;
        }
        let idx = self.unpack();
        idx.iter()
            .filter(|&&v| changed_ids.get(v as usize).copied().unwrap_or(false))
            .count()
    }
}
