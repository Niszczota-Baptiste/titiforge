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
    /// Les 64 cellules de biome, quand la section en porte.
    ///
    /// `None` veut dire « on ne sait pas » — 1.13–1.17, ou une palette d'un
    /// type inattendu — jamais « pas de biome ». L'appelant doit pouvoir
    /// retomber sur son réglage plutôt que de colorer au jugé.
    pub biomes: Option<Vec<tf_anvil::StateId>>,
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
    /// Chunks DÉCOMPRESSÉS — le coût d'une lecture, que le reste du bilan ne
    /// voit pas : un chunk décompressé pour rien ne pose aucune section.
    pub decompresses: usize,
    /// Sections dont on a su lire les biomes.
    pub avec_biomes: usize,
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
pub fn sections_de<S: RegionSource + ?Sized>(
    src: &S,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    interner: &mut Interner,
    poser: impl FnMut(SectionLue),
) -> Bilan {
    sections_de_si(src, dim, folder, sel, |_| true, interner, poser)
}

/// La même lecture, restreinte aux chunks que `garder` retient — **avant**
/// d'en décompresser un seul.
///
/// Pour relire plusieurs endroits d'une même région sans payer ce qu'il y a
/// entre eux : un remaillage après un `//move` lointain a besoin de la source
/// et de la destination, et leur boîte commune contient tout le reste. La
/// boîte dit quelles régions ouvrir ; le filtre, quels chunks inflater.
pub fn sections_de_si<S: RegionSource + ?Sized>(
    src: &S,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    garder: impl Fn(ChunkPos) -> bool,
    interner: &mut Interner,
    mut poser: impl FnMut(SectionLue),
) -> Bilan {
    let mut bilan = Bilan::default();
    let (a, b) = (sel.min.chunk(), sel.max.chunk());
    // Les sections que la hauteur demandée touche, en division PLANCHER : le
    // bloc −1 est dans la section −1, pas la 0.
    let (ymin, ymax) = (sel.min.y.div_euclid(16), sel.max.y.div_euclid(16));
    for pos in sel.regions() {
        let Ok(octets) = src.read_region(dim, folder, pos) else {
            continue; // région absente : ce n'est pas une anomalie
        };
        let Ok(region) = read(&octets, pos.x, pos.z) else {
            bilan.illisibles += 1;
            continue;
        };
        bilan.regions += 1;
        // Les emplacements que l'en-tête annonçait et qu'on n'a pas su
        // repérer : la région les laisse vides pour sauver les autres, le
        // bilan les dit.
        bilan.illisibles += region.illisibles;
        for brut in region.iter() {
            // **On filtre AVANT d'inflater.** Le filtre était posé après le
            // scan : une région bâtie porte 256 chunks, donc lire UNE section
            // en décompressait 256. Mesuré sur la fixture de terrain, une
            // section décodée coûtait 17,5 ms là où les octets de la région
            // s'obtenaient en 0,2 — tout le reste était du travail jeté.
            //
            // L'index donne les coordonnées sans rien décompresser. Le
            // CONTENU reste juge — un `.mca` porte ses propres coordonnées, et
            // c'est ce qui rend lisible un fichier mal nommé — mais il ne peut
            // trancher que pour les chunks qu'on a ouverts. Un chunk rangé à
            // un index qui ment sur sa place serait donc sauté ; il serait
            // aussi illisible par le jeu.
            let approx = ChunkPos {
                x: pos.x * 32 + (brut.index % 32) as i32,
                z: pos.z * 32 + (brut.index / 32) as i32,
            };
            if approx.x < a.x || approx.x > b.x || approx.z < a.z || approx.z > b.z {
                continue;
            }
            if !garder(approx) {
                continue;
            }
            bilan.decompresses += 1;
            let Ok(inflated) = inflate(&brut.payload, brut.compression) else {
                bilan.illisibles += 1;
                continue;
            };
            let Ok(sc) = scan(&inflated) else {
                bilan.illisibles += 1;
                continue;
            };
            let chunk = coordonnees(&sc, pos, brut.index);
            if chunk.x < a.x || chunk.x > b.x || chunk.z < a.z || chunk.z > b.z || !garder(chunk) {
                continue;
            }
            bilan.chunks += 1;
            for s in &sc.sections {
                if s.spans.is_none() {
                    bilan.sans_blocs += 1;
                    continue;
                }
                // **La HAUTEUR de la sélection compte aussi.** Elle était
                // ignorée : lire une section en décodait toutes celles de son
                // chunk, biomes compris. Sur un monde 1.18 c'est vingt-quatre
                // sections pour une.
                if (s.y as i32) < ymin || (s.y as i32) > ymax {
                    continue;
                }
                match decode_section(&inflated, &sc, s, interner) {
                    Ok(Some(section)) => {
                        bilan.sections += 1;
                        // Les biomes sont une SECONDE palette. On les lit ici
                        // parce que le chunk est déjà inflaté et balayé : les
                        // relire plus tard coûterait un second passage
                        // complet sur la save.
                        let biomes = match tf_anvil::decode_biomes(&inflated, s, interner) {
                            Ok(Some(b)) => {
                                bilan.avec_biomes += 1;
                                let idx = b.unpack();
                                Some(
                                    idx.iter()
                                        .map(|&k| b.palette.get(k as usize).copied().unwrap_or(0))
                                        .collect(),
                                )
                            }
                            _ => None,
                        };
                        poser(SectionLue {
                            chunk,
                            section,
                            biomes,
                        });
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
