//! La section 16³ PACKÉE : palette d'identifiants internés + indices bit-packés.
//!
//! C'est la réponse au mur n° 1. we-engine tient un `{Name, Properties}` par
//! bloc — mesuré à 154 o/bloc sur `mirror-rotate`. Ici une section, c'est une
//! palette de quelques dizaines d'entiers et 4096 indices de 4 à 6 bits.
//!
//! Repère Minecraft repris de we-engine : i = y*256 + z*16 + x (ordre YZX).
//! Format 1.16+ : un indice ne chevauche JAMAIS deux longs.

pub const VOL: usize = 4096;

#[derive(Clone)]
pub struct Section {
    pub y: i8,
    /// Identifiants internés. `u32`, pas une chaîne : la comparaison d'états
    /// devient un `==` d'entier au lieu d'une fabrication de clé texte — le
    /// piège que we-engine a payé deux fois (`propsKey` à 18 %, `findIndex`
    /// à 44 % de l'écriture).
    pub palette: Vec<u32>,
    pub bits: u8,
    pub data: Box<[u64]>,
    /// Cache dépacké, construit à la demande pour les opérations qui lisent
    /// vraiment chaque case. L'étage PALETTE ne le touche jamais.
    pub unpacked: Option<Box<[u16]>>,
}

#[inline]
pub fn bits_for(len: usize) -> u8 {
    let n = len.max(2);
    let mut b = 0u32;
    while (1usize << b) < n {
        b += 1;
    }
    (b as u8).max(4)
}

impl Section {
    /// Octets réellement occupés par la forme packée (hors cache dépacké).
    pub fn packed_bytes(&self) -> usize {
        self.data.len() * 8 + self.palette.len() * 4 + std::mem::size_of::<Section>()
    }

    pub fn unpack(&self) -> Box<[u16]> {
        let mut out = vec![0u16; VOL].into_boxed_slice();
        if self.palette.len() <= 1 || self.data.is_empty() {
            return out; // section homogène : tous les indices à 0
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
        out
    }

    pub fn repack(&mut self, idx: &[u16]) {
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

    pub fn ensure_unpacked(&mut self) {
        if self.unpacked.is_none() {
            self.unpacked = Some(self.unpack());
        }
    }

    /// ÉTAGE PALETTE — le cœur de l'architecture.
    ///
    /// Remplace `from` par `to` sans toucher un seul indice de bloc, SAUF si
    /// la substitution crée un doublon dans la palette (la section contenait
    /// déjà `to`) ou fait baisser le nombre de bits. Dans ce cas seulement on
    /// paie un remap en O(4096) — pour CETTE section, pas pour la sélection.
    ///
    /// C'est la mesure honnête : sur du vrai terrain, beaucoup de sections
    /// contiennent à la fois de la pierre et de la terre, donc le chemin cher
    /// se déclenche vraiment.
    /// Rend `None` si la section ne contient pas `from`, `Some(false)` si le
    /// chemin RAPIDE a suffi (aucun indice touché), `Some(true)` s'il a fallu
    /// remapper. C'est cette distinction qui dit si l'étage palette vaut
    /// quelque chose sur de vraies données.
    pub fn replace_by_palette(&mut self, from: u32, to: u32) -> Option<bool> {
        if !self.palette.iter().any(|&e| e == from) {
            return None;
        }
        let before_bits = bits_for(self.palette.len());
        let mut newpal: Vec<u32> = Vec::with_capacity(self.palette.len());
        let mut lut: Vec<u16> = Vec::with_capacity(self.palette.len());
        for &e in &self.palette {
            let v = if e == from { to } else { e };
            match newpal.iter().position(|&x| x == v) {
                Some(i) => lut.push(i as u16),
                None => {
                    lut.push(newpal.len() as u16);
                    newpal.push(v);
                }
            }
        }
        let identity = lut.iter().enumerate().all(|(i, &v)| i as u16 == v);
        self.palette = newpal;
        let after_bits = bits_for(self.palette.len());
        if !identity || after_bits != before_bits {
            let old = self.unpack();
            let mut next = vec![0u16; VOL];
            for (n, &o) in old.iter().enumerate() {
                next[n] = lut[o as usize];
            }
            self.repack(&next);
            self.unpacked = None;
            return Some(true);
        }
        self.unpacked = None;
        Some(false)
    }

    /// ÉTAGE PALETTE, variante SANS DÉDOUBLONNAGE.
    ///
    /// La mesure a montré que le dédoublonnage annule tout le gain : sur du
    /// vrai terrain, une section qui contient de la pierre contient presque
    /// toujours de la terre, donc la fusion des deux entrées force un remap de
    /// 4096 indices — 9216 sections sur 9216.
    ///
    /// Or le format Anvil n'INTERDIT pas deux entrées de palette identiques :
    /// le jeu lit `palette[indice]` et obtient un état valide dans les deux
    /// cas. En laissant le doublon, la longueur de la palette ne bouge pas,
    /// donc `bits` non plus, donc AUCUN indice n'est touché.
    ///
    /// Contrepartie, assumée : la palette porte une entrée redondante jusqu'au
    /// prochain compactage (à l'écriture, ou jamais — le jeu recompacte à sa
    /// propre sauvegarde). Et tout ce qui cherche un état dans une palette doit
    /// chercher TOUTES les occurrences, jamais la première.
    pub fn replace_by_palette_nodedupe(&mut self, from: u32, to: u32) -> bool {
        let mut hit = false;
        for e in self.palette.iter_mut() {
            if *e == from {
                *e = to;
                hit = true;
            }
        }
        if hit {
            self.unpacked = None;
        }
        hit
    }

    /// Compte les blocs valant `id`. Sert UNIQUEMENT à prouver que les quatre
    /// stratégies produisent le même monde — une optimisation non vérifiée est
    /// une corruption silencieuse.
    pub fn count_of(&self, id: u32) -> usize {
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
        if self.palette.len() <= 1 || self.data.is_empty() {
            return VOL; // section homogène de cet état
        }
        let idx = self.unpack();
        idx.iter().filter(|v| hits.contains(v)).count()
    }

    /// ÉTAGE SECTION — une section entièrement couverte par la sélection et
    /// remplie d'une seule valeur devient une palette d'UNE entrée, sans
    /// tableau d'indices. C'est exactement ce que Minecraft écrit pour une
    /// section homogène : O(1), et le fichier rétrécit.
    pub fn set_uniform(&mut self, id: u32) {
        self.palette.clear();
        self.palette.push(id);
        self.bits = bits_for(1);
        self.data = Box::new([]);
        self.unpacked = None;
    }
}
