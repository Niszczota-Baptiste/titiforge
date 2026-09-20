//! La vue par COLONNE d'un chunk — pour ce qui lit hors de sa section.
//!
//! Le trait `Operation` travaille section par section, et c'est ce qui rend
//! les trois étages possibles : une section porte une palette et 4 096
//! indices, et presque toute opération WorldEdit s'exprime là-dessus.
//!
//! Mais pas toutes. Naturaliser une surface demande de savoir **où est la
//! surface**, donc de regarder la colonne au-dessus — qui traverse jusqu'à
//! vingt-quatre sections. Lisser demande le voisin d'à côté. Creuser demande
//! tout le volume. Ces trois-là ne peuvent pas décider avec une section sous
//! les yeux, et les forcer produirait le défaut classique : une opération qui
//! lit de l'air au bord de sa section et écrit comme si le monde s'arrêtait
//! là.
//!
//! D'où cette vue. Elle décode les sections du chunk que la sélection touche,
//! les DÉPACKE une fois, et donne un accès par coordonnée MONDE en Y. Le
//! repack se fait à la fin, et seulement pour les sections qu'on a changées.
//!
//! ## Ce qu'elle ne fait pas, et pourquoi
//!
//! **Elle ne franchit pas les bords du chunk.** Une opération qui lit le
//! voisin en X ou en Z verrait de l'air là où il y a de la pierre — le piège
//! `cold_read` sous une autre forme. C'est pourquoi la portée s'appelle
//! `Colonne` et pas `Voisinage` : elle promet exactement ce qu'elle tient, et
//! le lissage devra attendre une vue qui déborde d'un chunk.

use tf_anvil::section::{in_section, Section, VOL};
use tf_anvil::StateId;

/// Ce qu'une opération a besoin de LIRE pour décider.
///
/// Déclarée par l'opération, honorée par `edition.rs`. Une portée trop petite
/// fait lire de l'air ; trop grande, elle fait décoder tout un chunk pour
/// changer trois blocs. C'est la leçon de `PORTEE` dans `ExeWorldEdit`, où un
/// `warmup(extent)` systématique faisait payer 5,2 s pour soixante-deux blocs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portee {
    /// Sa section, et rien d'autre. Le défaut, et le seul qui donne accès aux
    /// étages `Section` et `Palette`.
    Section,
    /// Toute la colonne `(x, z)` de son chunk, sur la hauteur de la sélection.
    Colonne,
}

/// Une section décodée et dépackée, prête à être lue par colonne.
struct Tranche {
    /// Son rang dans le balayage du chunk — c'est par là qu'on la réécrit.
    rang: usize,
    section: Section,
    idx: Box<[u16]>,
    /// A-t-elle changé ? Un drapeau ici est sûr : il ne décide pas de ce qu'on
    /// écrit, seulement de ce qu'on repacke. `section_edits` compare ensuite
    /// les OCTETS, et c'est lui qui a le dernier mot.
    touchee: bool,
}

/// Les sections d'un chunk, dépackées, adressées en Y MONDE.
pub struct Colonnes {
    tranches: Vec<Tranche>,
    /// `y_section - base` → rang dans `tranches`. Une section absente du
    /// chunk (il y en a) laisse un trou.
    ou: Vec<Option<u16>>,
    base: i32,
}

impl Colonnes {
    /// Construit la vue. Les sections arrivent avec leur rang de balayage.
    pub fn depuis(sections: Vec<(usize, Section)>) -> Colonnes {
        if sections.is_empty() {
            return Colonnes {
                tranches: Vec::new(),
                ou: Vec::new(),
                base: 0,
            };
        }
        let base = sections.iter().map(|(_, s)| s.y as i32).min().unwrap();
        let haut = sections.iter().map(|(_, s)| s.y as i32).max().unwrap();
        let mut ou = vec![None; (haut - base + 1) as usize];
        let mut tranches = Vec::with_capacity(sections.len());
        for (rang, section) in sections {
            let idx = section.unpack();
            ou[(section.y as i32 - base) as usize] = Some(tranches.len() as u16);
            tranches.push(Tranche {
                rang,
                section,
                idx,
                touchee: false,
            });
        }
        Colonnes { tranches, ou, base }
    }

    pub fn est_vide(&self) -> bool {
        self.tranches.is_empty()
    }

    /// Le Y monde le plus bas couvert, et le plus haut (INCLUS).
    pub fn bornes_y(&self) -> Option<(i32, i32)> {
        if self.tranches.is_empty() {
            return None;
        }
        let haut = self.base + self.ou.len() as i32 - 1;
        Some((self.base * 16, haut * 16 + 15))
    }

    /// Le rang de tranche qui porte ce Y monde.
    ///
    /// Division PLANCHER : le bloc −1 est dans la section −1. Une division
    /// entière naïve désignerait la mauvaise tranche au-dessus de zéro comme
    /// en dessous, et sans rien signaler.
    #[inline]
    fn tranche_de(&self, wy: i32) -> Option<usize> {
        let s = wy.div_euclid(16) - self.base;
        if s < 0 {
            return None;
        }
        self.ou
            .get(s as usize)
            .copied()
            .flatten()
            .map(|v| v as usize)
    }

    #[inline]
    fn case(wy: i32, lx: usize, lz: usize) -> usize {
        let ly = wy.rem_euclid(16) as usize;
        debug_assert!(in_section(lx, ly, lz));
        ((ly << 8) | (lz << 4) | lx).min(VOL - 1)
    }

    /// L'état d'une case, en `x`/`z` LOCAUX au chunk et `y` MONDE.
    ///
    /// `None` quand la colonne ne porte pas de section à cette hauteur —
    /// c'est-à-dire « on ne sait pas », jamais « c'est de l'air ». Confondre
    /// les deux est le piège `cold_read`, et il écrit de la pierre là où il
    /// n'y a rien à écrire.
    pub fn get(&self, lx: usize, wy: i32, lz: usize) -> Option<StateId> {
        let t = &self.tranches[self.tranche_de(wy)?];
        t.section
            .palette
            .get(t.idx[Self::case(wy, lx, lz)] as usize)
            .copied()
    }

    /// Pose un état. Rend `false` si la case n'existe pas, ou si elle portait
    /// déjà cet état.
    ///
    /// **La comparaison porte sur l'ÉTAT, pas sur l'indice.** L'invariant n° 4
    /// du dépôt : la palette ne dédoublonne pas, donc le même état y figure
    /// parfois deux fois, et deux indices différents désignent alors la même
    /// chose. Réécrire l'un par l'autre change les octets sans changer le
    /// monde — un correctif de journal pour rien.
    pub fn set(&mut self, lx: usize, wy: i32, lz: usize, id: StateId) -> bool {
        let Some(r) = self.tranche_de(wy) else {
            return false;
        };
        let t = &mut self.tranches[r];
        let i = Self::case(wy, lx, lz);
        if t.section.palette.get(t.idx[i] as usize) == Some(&id) {
            return false;
        }
        let k = match t.section.palette.iter().position(|&e| e == id) {
            Some(k) => k,
            None => {
                t.section.palette.push(id);
                t.section.palette.len() - 1
            }
        };
        t.idx[i] = k as u16;
        t.touchee = true;
        true
    }

    /// Repacke ce qui a changé, et rend les sections à réécrire avec leur rang
    /// de balayage. Celles qu'on n'a pas touchées ne sortent pas : les
    /// réencoder pour rien ferait grossir le journal d'entrées vides.
    pub fn finir(self) -> Vec<(usize, Section)> {
        self.tranches
            .into_iter()
            .filter(|t| t.touchee)
            .map(|mut t| {
                t.section.repack(&t.idx);
                (t.rang, t.section)
            })
            .collect()
    }
}
