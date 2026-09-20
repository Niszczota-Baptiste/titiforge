//! Le presse-papiers : un extrait de monde, détaché de sa save.
//!
//! C'est ce sur quoi reposent `//copy`, `//paste`, `//rotate` et `//flip` — et
//! c'est le premier endroit où les règles de transformation de `tf-blocks`
//! servent à autre chose qu'à être mesurées. Elles couvrent 99,3 % des
//! rotations Minefield depuis un moment ; rien ne les appelait.
//!
//! ## Une transformation se fait sur la PALETTE
//!
//! Tourner un build d'un quart de tour, c'est deux choses : déplacer les cases,
//! et transformer les ÉTATS — un escalier qui regardait l'est regarde le sud.
//!
//! La seconde ne se fait pas par bloc. Un extrait d'un million de cases porte
//! deux cents états distincts : on transforme les deux cents, on en fait une
//! table de correspondance, et le parcours des cases n'est plus qu'une
//! indirection. C'est le même raisonnement que l'étage palette des opérations,
//! appliqué au presse-papiers — et un test le fige en comptant les appels.
//!
//! ## Ce qu'on ne sait pas transformer, on n'y touche pas
//!
//! Un bloc dont la table ne connaît pas la rotation reste tel quel, et il est
//! SIGNALÉ. Le supposer symétrique produirait un build subtilement faux : une
//! moitié tournée, l'autre non, et rien à l'écran pour le dire.

use tf_anvil::{Interner, StateId};
use tf_blocks::Transfo;

/// Un extrait de monde, en coordonnées LOCALES.
///
/// Les cases sont rangées en **YZX**, comme partout ailleurs dans le dépôt :
/// `i = (y × sz + z) × sx + x`. Une seconde convention d'ordre ici ferait
/// sortir les builds en miroir un jour sur deux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presse {
    /// Dimensions en blocs, dans l'ordre X, Y, Z.
    pub taille: [u32; 3],
    /// Un état par case. Longueur = produit des dimensions.
    pub blocs: Vec<StateId>,
    /// Le point que `//paste` remettra là où l'on est, relatif au coin de plus
    /// petites coordonnées.
    ///
    /// Il suit les transformations comme le reste : sans lui, un build tourné
    /// se collerait décalé de sa propre largeur, ce qui se lit « le collage est
    /// cassé » et ne désigne pas la cause. Il peut sortir de la boîte — on
    /// copie souvent depuis l'extérieur de sa sélection.
    pub ancre: [i32; 3],
}

/// Ce qu'une transformation a produit, et ce qu'elle n'a pas su faire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transforme {
    pub presse: Presse,
    /// Les états que la règle n'a pas su transformer, laissés TELS QUELS.
    ///
    /// Rendus plutôt que tus : à moitié tourné, un build est faux d'une façon
    /// qu'aucune capture d'écran ne montre.
    pub intacts: Vec<StateId>,
}

impl Presse {
    /// Un extrait vide de cette taille, tout à l'état donné.
    pub fn uniforme(taille: [u32; 3], id: StateId) -> Presse {
        Presse {
            blocs: vec![id; Presse::volume(taille)],
            taille,
            ancre: [0, 0, 0],
        }
    }

    pub fn volume(taille: [u32; 3]) -> usize {
        taille[0] as usize * taille[1] as usize * taille[2] as usize
    }

    /// L'index d'une case, en YZX. `None` hors de la boîte.
    pub fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        let [sx, sy, sz] = self.taille;
        if x >= sx || y >= sy || z >= sz {
            return None;
        }
        Some((y as usize * sz as usize + z as usize) * sx as usize + x as usize)
    }

    pub fn get(&self, x: u32, y: u32, z: u32) -> Option<StateId> {
        self.index(x, y, z).map(|i| self.blocs[i])
    }

    /// Les états distincts présents, triés. C'est la « palette » de l'extrait.
    pub fn palette(&self) -> Vec<StateId> {
        let mut v = self.blocs.clone();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// L'extrait transformé : les cases déplacées, les états réécrits.
    ///
    /// `regle` rend la clé d'état transformée, ou `None` si elle ne sait pas —
    /// injectée plutôt qu'importée pour que `tf-ops` se teste sans pack, et
    /// pour que le jour où une autre source de règles arrive (un mod, une table
    /// écrite à la main pour un cas tordu) elle se branche ici sans toucher à
    /// l'opération.
    pub fn transformer(
        &self,
        t: Transfo,
        interner: &mut Interner,
        regle: &dyn Fn(&str, Transfo) -> Option<String>,
    ) -> Transforme {
        // ── 1. la palette, et elle SEULE
        let palette = self.palette();
        let mut vers: std::collections::HashMap<StateId, StateId> =
            std::collections::HashMap::with_capacity(palette.len());
        let mut intacts = Vec::new();
        for id in palette {
            let Some(cle) = interner.resolve(id).map(str::to_string) else {
                // Un identifiant qu'aucun interner ne résout ne se devine pas.
                intacts.push(id);
                vers.insert(id, id);
                continue;
            };
            match regle(&cle, t) {
                Some(neuve) => {
                    let n = interner.intern(&neuve);
                    vers.insert(id, n);
                }
                None => {
                    intacts.push(id);
                    vers.insert(id, id);
                }
            }
        }

        // ── 2. la géométrie
        let [sx, sy, sz] = self.taille;
        let taille = t.taille_apres(self.taille);
        let mut blocs = vec![StateId::default(); Presse::volume(taille)];
        let [nx, _, nz] = taille;
        for y in 0..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let (ax, az) = t.case_apres((x, z), (sx, sz));
                    let src = (y as usize * sz as usize + z as usize) * sx as usize + x as usize;
                    let dst = (y as usize * nz as usize + az as usize) * nx as usize + ax as usize;
                    blocs[dst] = vers[&self.blocs[src]];
                }
            }
        }

        Transforme {
            presse: Presse {
                taille,
                blocs,
                ancre: t.point_apres(self.ancre, self.taille),
            },
            intacts,
        }
    }
}

/// Ce qu'une transformation fait à une BOÎTE — sa taille, ses cases, un point.
///
/// Écrit une fois ici et pas dans l'opération : les trois doivent s'accorder,
/// et deux d'entre elles écrites à deux endroits finiraient par diverger. La
/// convention est celle de tout le dépôt, celle que `tf-blocks` applique à la
/// géométrie d'un modèle : **un quart de tour envoie `+X` sur `+Z`**.
pub trait TransfoBoite {
    fn taille_apres(self, taille: [u32; 3]) -> [u32; 3];
    fn case_apres(self, xz: (u32, u32), taille: (u32, u32)) -> (u32, u32);
    fn point_apres(self, p: [i32; 3], taille: [u32; 3]) -> [i32; 3];
}

impl TransfoBoite for Transfo {
    fn taille_apres(self, [sx, sy, sz]: [u32; 3]) -> [u32; 3] {
        match self {
            // Un quart de tour échange la largeur et la profondeur. L'oublier
            // rendrait un extrait non carré tronqué d'un côté et vide de
            // l'autre.
            Transfo::Rot90 | Transfo::Rot270 => [sz, sy, sx],
            _ => [sx, sy, sz],
        }
    }

    fn case_apres(self, (x, z): (u32, u32), (sx, sz): (u32, u32)) -> (u32, u32) {
        match self {
            Transfo::Rot90 => (sz - 1 - z, x),
            Transfo::Rot180 => (sx - 1 - x, sz - 1 - z),
            Transfo::Rot270 => (z, sx - 1 - x),
            Transfo::MiroirX => (sx - 1 - x, z),
            Transfo::MiroirZ => (x, sz - 1 - z),
        }
    }

    /// Le même calcul, pour un point qui peut SORTIR de la boîte.
    ///
    /// L'ancre est souvent dehors — on copie depuis là où l'on se tient. La
    /// formule est celle des cases, sans la borne : `sz - 1 - z` s'écrit
    /// `sz as i32 - 1 - z` et reste juste pour un `z` négatif.
    fn point_apres(self, [x, y, z]: [i32; 3], [sx, _, sz]: [u32; 3]) -> [i32; 3] {
        let (sx, sz) = (sx as i32, sz as i32);
        let (ax, az) = match self {
            Transfo::Rot90 => (sz - 1 - z, x),
            Transfo::Rot180 => (sx - 1 - x, sz - 1 - z),
            Transfo::Rot270 => (z, sx - 1 - x),
            Transfo::MiroirX => (sx - 1 - x, z),
            Transfo::MiroirZ => (x, sz - 1 - z),
        };
        [ax, y, az]
    }
}
