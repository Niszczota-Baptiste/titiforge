//! `//hollow` — vider ce qui ne touche pas le dehors.
//!
//! Creuser n'est pas « enlever l'intérieur d'une boîte ». C'est enlever ce
//! qu'aucun chemin de VIDE ne relie au dehors : une salle déjà ouverte par
//! une porte ne se remplit pas, une sphère pleine se vide, et une paroi d'un
//! bloc reste une paroi. Le critère est topologique, pas géométrique.
//!
//! ## La seule opération qui demande tout le volume
//!
//! Un remplissage par diffusion ne se découpe ni par section ni par colonne :
//! un couloir peut traverser la sélection de part en part. Il faut donc
//! MATÉRIALISER l'extrait — ce que `copier` sait déjà faire — puis calculer, puis
//! reposer. C'est assumé et c'est borné : le coût est celui de la sélection,
//! une fois, et l'appelant le connaît avant de commencer.
//!
//! ## La diffusion est ITÉRATIVE, jamais récursive
//!
//! Une sélection de cent blocs de côté fait un million de cases. Une
//! récursion y déborde la pile, et **un débordement de pile n'est pas
//! rattrapable en Rust** : le processus meurt sans message, sur la sauvegarde
//! de quelqu'un. La pile est explicite.

use tf_anvil::StateId;

use crate::masque::Masque;
use crate::presse::Presse;

/// Ce qu'un creusage a trouvé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creux {
    /// Une case par volume de l'extrait : vraie si elle est INTÉRIEURE, donc
    /// à vider.
    pub interieur: Vec<bool>,
    /// Combien de cases sont intérieures.
    pub cases: u64,
}

/// Les cases qu'aucun chemin de vide ne relie au dehors.
///
/// `solide` dit ce qui BLOQUE la diffusion. Tout le reste la laisse passer —
/// et c'est ce qui fait qu'une salle ouverte par une porte ne se remplit pas.
///
/// `epaisseur` est le nombre de couches de paroi à GARDER. Une épaisseur de 1
/// garde la peau ; de 3, garde trois blocs. Zéro n'a pas de sens et vaut un :
/// creuser jusqu'à ne rien garder viderait la sélection entière, ce qu'un
/// `//set air` fait déjà et dit mieux.
pub fn creuser(p: &Presse, solide: &Masque, epaisseur: u32) -> Creux {
    let [sx, sy, sz] = p.taille;
    let n = Presse::volume(p.taille);
    if n == 0 {
        return Creux {
            interieur: Vec::new(),
            cases: 0,
        };
    }
    let (sx, sy, sz) = (sx as i64, sy as i64, sz as i64);
    let idx = |x: i64, y: i64, z: i64| -> usize {
        (y as usize * sz as usize + z as usize) * sx as usize + x as usize
    };
    let dedans = |x: i64, y: i64, z: i64| x >= 0 && y >= 0 && z >= 0 && x < sx && y < sy && z < sz;
    let bordure = |x: i64, y: i64, z: i64| {
        x == 0 || y == 0 || z == 0 || x == sx - 1 || y == sy - 1 || z == sz - 1
    };
    const PAS: [(i64, i64, i64); 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    // ── 1. la diffusion depuis le DEHORS, à travers ce qui n'est pas solide
    //
    // **Ce qui est hors de la sélection est ouvert, par définition** : la
    // sélection s'arrête là, et ce qu'il y a au-delà ne nous regarde pas. Les
    // cases non solides du BORD sont donc la graine.
    //
    // Une case solide du bord n'est PAS semée — on ne diffuse pas à travers
    // la roche — mais elle sera gardée plus bas, parce qu'elle touche ce
    // dehors-là. C'est cette distinction qui manquait à la première écriture :
    // sans elle, un cube entièrement plein n'avait aucune graine, donc aucun
    // dehors, donc il partait EN ENTIER.
    let mut dehors = vec![false; n];
    let mut pile: Vec<(i64, i64, i64)> = Vec::new();
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                if !bordure(x, y, z) {
                    continue;
                }
                let i = idx(x, y, z);
                if solide.accepte(p.blocs[i]) {
                    continue;
                }
                dehors[i] = true;
                pile.push((x, y, z));
            }
        }
    }
    // La pile est EXPLICITE : une récursion déborderait sur une sélection de
    // cent blocs de côté, et un débordement de pile tue le processus sans
    // message.
    while let Some((x, y, z)) = pile.pop() {
        for (dx, dy, dz) in PAS {
            let (vx, vy, vz) = (x + dx, y + dy, z + dz);
            if !dedans(vx, vy, vz) {
                continue;
            }
            let i = idx(vx, vy, vz);
            if dehors[i] || solide.accepte(p.blocs[i]) {
                continue;
            }
            dehors[i] = true;
            pile.push((vx, vy, vz));
        }
    }

    // ── 2. la première couche de paroi
    //
    // Est gardé ce qui EST ouvert, ce qui TOUCHE de l'ouvert, et ce qui est au
    // bord de la sélection — puisque le bord touche le dehors.
    let mut garde = vec![false; n];
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                let i = idx(x, y, z);
                garde[i] = dehors[i]
                    || bordure(x, y, z)
                    || PAS.iter().any(|(dx, dy, dz)| {
                        let (vx, vy, vz) = (x + dx, y + dy, z + dz);
                        dedans(vx, vy, vz) && dehors[idx(vx, vy, vz)]
                    });
            }
        }
    }

    // ── 3. l'épaissir
    //
    // Chaque passe ajoute la couche voisine de ce qui est déjà gardé : une
    // épaisseur de trois garde trois blocs de paroi.
    for _ in 1..epaisseur.max(1) {
        let source = garde.clone();
        for y in 0..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let i = idx(x, y, z);
                    if source[i] {
                        continue;
                    }
                    garde[i] = PAS.iter().any(|(dx, dy, dz)| {
                        let (vx, vy, vz) = (x + dx, y + dy, z + dz);
                        dedans(vx, vy, vz) && source[idx(vx, vy, vz)]
                    });
                }
            }
        }
    }

    // ── 4. ce qui reste, et qui est solide, est l'intérieur
    let mut interieur = vec![false; n];
    let mut cases = 0u64;
    for i in 0..n {
        // Le vide déjà présent n'est pas « intérieur à vider » : il est déjà
        // vide, et le compter gonflerait le rapport d'un travail qui n'a pas
        // lieu.
        if !garde[i] && solide.accepte(p.blocs[i]) {
            interieur[i] = true;
            cases += 1;
        }
    }
    Creux { interieur, cases }
}

/// L'extrait creusé : les cases intérieures remplacées par `vide`.
///
/// **Un coffre dont le bloc part s'en va avec lui.** La jonction
/// (`edition.rs`) retire bien les block entities devenues orphelines — mais
/// une entité POSÉE par un collage gagne sur ce verdict, et c'est voulu :
/// c'est ce qui fait qu'un extrait reposé à sa place garde ses coffres. Un
/// coffre laissé dans l'extrait sur une case qu'on vient de vider serait donc
/// reposé DANS LE VIDE, et on l'apprendrait en ouvrant un coffre qui n'existe
/// plus. C'est le piège du contenu de coffre, une case plus loin.
pub fn extrait_creuse(p: &Presse, c: &Creux, vide: StateId) -> Presse {
    let mut out = p.clone();
    for (i, dedans) in c.interieur.iter().enumerate() {
        if *dedans {
            out.blocs[i] = vide;
        }
    }
    let videe = |case: [i32; 3]| -> bool {
        let [x, y, z] = case;
        if x < 0 || y < 0 || z < 0 {
            return false;
        }
        match p.index(x as u32, y as u32, z as u32) {
            Some(i) => c.interieur.get(i).copied().unwrap_or(false),
            None => false,
        }
    };
    out.entites.retain(|e| !videe(e.case));
    out
}
