//! Matérialiser les sections d'une **portion** de save.
//!
//! **Il n'existe aucun état « le monde est chargé »** (invariant n° 7), et ce
//! module ne l'introduit pas : il prend une emprise et ne lit que ce qui
//! tombe dedans. Une région pleine fait déjà 100 millions de blocs et un monde
//! Minefield 80 milliards ; une fonction qui accepterait « toute la save »
//! serait une promesse qu'on ne peut pas tenir.
//!
//! Ce qu'il apporte par rapport à un `read` + `scan` + `decode_section` écrit
//! sur place, c'est trois décisions qu'on ne veut pas voir se dédoubler :
//!
//! 1. **Les coordonnées viennent du CONTENU**, jamais du nom de fichier. Un
//!    nom peut mentir — Windows renomme un téléchargement en double
//!    `r.0.0 (16).mca`.
//! 2. **Un seul interner pour toute la lecture.** Un `StateId` n'a de sens que
//!    relativement au sien : deux lectures indépendantes du même monde
//!    numérotent dans l'ordre où elles rencontrent les états, et comparer
//!    leurs identifiants revient à comparer deux systèmes de coordonnées.
//! 3. **Une charge illisible est SAUTÉE, jamais devinée.** Un `.mca` abîmé
//!    existe ; il ne doit ni tuer le processus ni faire croire à de l'air.

use tf_anvil::{decode_section, inflate, read, scan, Interner, Section};

use crate::coords::{BBox, ChunkPos, RegionPos};
use crate::source::{Dimension, Folder, RegionSource};

/// Une section matérialisée, avec le chunk d'où elle vient.
#[derive(Debug, Clone)]
pub struct SectionLue {
    pub chunk: ChunkPos,
    pub section: Section,
}

/// Ce qu'une lecture a rencontré. Rendu d'office : un relevé qui ne dit pas ce
/// qu'il a sauté laisse croire qu'il n'a rien sauté.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bilan {
    pub regions: usize,
    pub chunks: usize,
    pub sections: usize,
    /// Sections sans champ de blocs — elles existent (éclairage aux bords du
    /// monde) et il ne faut ni les décoder ni les réécrire.
    pub sans_blocs: usize,
    /// Charges qu'on n'a pas su lire. Zéro sur une save saine.
    pub illisibles: usize,
}

/// Les sections d'une emprise, matérialisées.
///
/// `sel` borne le travail en X et Z **au chunk près** ; la hauteur n'est pas
/// filtrée, parce qu'une section est l'unité que le format livre et que la
/// découper coûterait plus que de la garder.
///
/// `poser` reçoit chaque section au fil de l'eau plutôt qu'un `Vec` : sur une
/// emprise large, tout accumuler avant de rendre la main doublerait la pointe
/// de mémoire pour rien.
pub fn sections_de<S: RegionSource>(
    src: &S,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    interner: &mut Interner,
    mut poser: impl FnMut(SectionLue),
) -> Bilan {
    let mut bilan = Bilan::default();
    let (a, b) = (sel.min.chunk(), sel.max.chunk());
    for pos in sel.regions() {
        let Ok(octets) = src.read_region(dim, folder, pos) else {
            continue; // région absente : ce n'est pas une anomalie
        };
        let Ok(region) = read(&octets, pos.x, pos.z) else {
            bilan.illisibles += 1;
            continue;
        };
        bilan.regions += 1;
        for brut in region.iter() {
            let Ok(inflated) = inflate(&brut.payload, brut.compression) else {
                bilan.illisibles += 1;
                continue;
            };
            let Ok(sc) = scan(&inflated) else {
                bilan.illisibles += 1;
                continue;
            };
            let chunk = coordonnees(&sc, pos, brut.index);
            if chunk.x < a.x || chunk.x > b.x || chunk.z < a.z || chunk.z > b.z {
                continue;
            }
            bilan.chunks += 1;
            for s in &sc.sections {
                if s.spans.is_none() {
                    bilan.sans_blocs += 1;
                    continue;
                }
                match decode_section(&inflated, &sc, s, interner) {
                    Ok(Some(section)) => {
                        bilan.sections += 1;
                        poser(SectionLue { chunk, section });
                    }
                    Ok(None) => bilan.sans_blocs += 1,
                    Err(_) => bilan.illisibles += 1,
                }
            }
        }
    }
    bilan
}

/// Les coordonnées MONDE d'un chunk : celles de son contenu, et son index dans
/// l'en-tête en dernier recours.
///
/// Un `.mca` porte ses propres coordonnées ; le nom du fichier est une
/// métadonnée qui peut mentir. Mais un chunk d'avant 1.18 peut ne pas les
/// écrire — d'où le repli sur la position du chunk dans sa région, qui est
/// exacte tant que le fichier est à sa place.
fn coordonnees(sc: &tf_anvil::ChunkScan, region: RegionPos, index: u16) -> ChunkPos {
    ChunkPos {
        x: sc
            .x_pos
            .unwrap_or_else(|| region.x * 32 + (index % 32) as i32),
        z: sc
            .z_pos
            .unwrap_or_else(|| region.z * 32 + (index / 32) as i32),
    }
}
