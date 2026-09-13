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

use crate::state::StateId;

/// Nombre de blocs dans une section.
pub const VOL: usize = 4096;

/// Palette maximale du format : 4096 entrées, soit 12 bits par indice.
pub const MAX_PALETTE: usize = 4096;

/// Bits par indice pour une palette de `len` entrées — règle Minecraft.
/// Plancher à 4, même pour une palette de 2.
#[inline]
pub fn bits_for(len: usize) -> u8 {
    let n = len.max(2);
    let mut b = 0u32;
    while (1usize << b) < n {
        b += 1;
    }
    (b as u8).max(4)
}

/// Index local d'un bloc dans une section, en ordre YZX.
#[inline]
pub fn local_index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Hauteur de la section, en pas de 16 blocs. Peut être négative.
    pub y: i8,
    pub palette: Vec<StateId>,
    pub bits: u8,
    /// Vide pour une section homogène (palette d'une entrée), comme Minecraft.
    pub data: Box<[u64]>,
}

impl Section {
    /// Section homogène d'un seul état.
    pub fn uniform(y: i8, id: StateId) -> Self {
        Section {
            y,
            palette: vec![id],
            bits: bits_for(1),
            data: Box::new([]),
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
        let bits = self.bits as usize;
        let per_long = 64 / bits;
        let mask = (1u64 << bits) - 1;
        let mut n = 0usize;
        for &w in self.data.iter() {
            if n >= VOL {
                break;
            }
            let up_to = per_long.min(VOL - n);
            for k in 0..up_to {
                out[n] = ((w >> (k * bits)) & mask) as u16;
                n += 1;
            }
        }
    }

    /// Repacke depuis 4096 indices, en recalculant `bits` d'après la palette.
    pub fn repack(&mut self, idx: &[u16]) {
        assert_eq!(idx.len(), VOL, "repack attend exactement {VOL} indices");
        if self.palette.len() <= 1 {
            self.bits = bits_for(1);
            self.data = Box::new([]);
            return;
        }
        let bits = bits_for(self.palette.len()) as usize;
        let per_long = 64 / bits;
        let long_count = VOL.div_ceil(per_long);
        let mut out = vec![0u64; long_count];
        for (li, w) in out.iter_mut().enumerate() {
            let mut acc = 0u64;
            for k in 0..per_long {
                let n = li * per_long + k;
                if n >= VOL {
                    break;
                }
                acc |= (idx[n] as u64) << (k * bits);
            }
            *w = acc;
        }
        self.bits = bits as u8;
        self.data = out.into_boxed_slice();
    }

    /// État à une position locale.
    pub fn get(&self, x: usize, y: usize, z: usize) -> Option<StateId> {
        let n = local_index(x, y, z);
        if self.is_uniform() {
            return self.palette.first().copied();
        }
        let bits = self.bits as usize;
        let per_long = 64 / bits;
        let w = *self.data.get(n / per_long)?;
        let k = n % per_long;
        let i = ((w >> (k * bits)) & ((1u64 << bits) - 1)) as usize;
        self.palette.get(i).copied()
    }

    /// Compte les blocs valant `id`.
    ///
    /// Cherche **toutes** les occurrences dans la palette, jamais la première :
    /// l'étage palette ne dédoublonne pas, donc un état peut y figurer
    /// plusieurs fois. Voir `replace_state`.
    pub fn count_of(&self, id: StateId) -> usize {
        let hits: Vec<u16> = self
            .palette
            .iter()
            .enumerate()
            .filter(|(_, &e)| e == id)
            .map(|(i, _)| i as u16)
            .collect();
        if hits.is_empty() {
            return 0;
        }
        if self.is_uniform() {
            return VOL;
        }
        let idx = self.unpack();
        idx.iter().filter(|v| hits.contains(v)).count()
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

        // Quelles entrées sont réellement utilisées ?
        let mut used = vec![false; before];
        if self.data.is_empty() {
            used[0] = true;
        } else {
            for &v in idx.iter() {
                if let Some(u) = used.get_mut(v as usize) {
                    *u = true;
                }
            }
        }

        let mut newpal: Vec<StateId> = Vec::with_capacity(before);
        let mut lut: Vec<u16> = vec![0; before];
        for (i, &e) in self.palette.iter().enumerate() {
            if !used[i] {
                continue; // laissé à 0 : aucun indice ne le désigne
            }
            let pos = match newpal.iter().position(|&x| x == e) {
                Some(p) => p,
                None => {
                    newpal.push(e);
                    newpal.len() - 1
                }
            };
            lut[i] = pos as u16;
        }
        if newpal.is_empty() {
            newpal.push(self.palette[0]);
        }
        if newpal.len() == before && lut.iter().enumerate().all(|(i, &v)| i as u16 == v) {
            return 0; // déjà compacte : ne rien réécrire
        }

        self.palette = newpal;
        if self.palette.len() <= 1 {
            self.bits = bits_for(1);
            self.data = Box::new([]);
        } else {
            let next: Vec<u16> = idx.iter().map(|&o| lut[o as usize]).collect();
            self.repack(&next);
        }
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
