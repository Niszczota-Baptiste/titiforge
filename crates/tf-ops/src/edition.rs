//! De l'opération au FICHIER : staging, splice, journal.
//!
//! C'est la jonction, et c'est l'endroit le plus dangereux du dépôt. Chaque
//! pièce est testée de son côté — le plan, le splice, le journal, la copie de
//! travail — et `ExeWorldEdit` a payé cher la leçon que ça ne suffit pas :
//! deux moitiés justes dont la JONCTION ne l'est pas produisent un résultat
//! parfaitement plausible et faux (là-bas, des hauteurs en blocs passées à une
//! fonction qui attendait un rapport 0..1 ; toute cellule non nulle devenait 1,
//! et le relief sortait plat).
//!
//! Les invariants que cette fonction tient :
//!
//! 1. **On ne touche jamais au fichier source.** Tout passe par le staging.
//! 2. **Un chunk non modifié est réémis octet pour octet** — on ne le
//!    ré-encode pas, on ne le décompresse même pas si la sélection ne le
//!    touche pas.
//! 3. **Un chunk modifié n'est pas ré-encodé non plus** : on remplace les
//!    seules PLAGES d'octets des sections qu'on a touchées. Heightmaps,
//!    structures, données de mods : le lecteur n'y touche pas, donc il ne peut
//!    pas les abîmer.
//! 4. **Les deux sens de l'annulation sont enregistrés**, parce que le sens
//!    « refaire » ne se déduit pas du sens « annuler » une fois l'annulation
//!    faite.
//! 5. **Une charge déportée (`.mcc`) est résolue avant lecture.** Un chunk
//!    déporté dont on oublierait la charge serait vu comme un chunk VIDE, et
//!    l'opération l'écraserait.

use std::borrow::Cow;

use tf_anvil::chunk::{decode_section, scan, section_edits, splice, EncodeError};
use tf_anvil::codec::{deflate, inflate, CodecError};
use tf_anvil::region::{external_file_name, read, write, ReadError, WriteError};
use tf_anvil::Interner;
use tf_world::coords::{BBox, ChunkPos, RegionPos, SectionPos};
use tf_world::journal::{ChunkPatch, Cible};
use tf_world::source::{Dimension, Folder, RegionSource, SourceError};
use tf_world::staging::{RegionStore, Staging};

use crate::plan::{Etage, Plan};

/// Ce qu'une opération a fait à une région.
#[derive(Debug, Default)]
pub struct RapportRegion {
    /// Les correctifs à pousser dans le journal, un par chunk modifié.
    pub patches: Vec<ChunkPatch>,
    /// Combien de sections sont passées par chaque étage, dans l'ordre
    /// `rien`, `section`, `palette`, `bloc`.
    ///
    /// Public et rendu d'office : le prototype a annoncé une fois un chemin
    /// rapide que la mesure a démenti. Un rapport qui ne dit pas par où c'est
    /// passé ne permet pas de le vérifier.
    pub etages: [usize; 4],
    /// Blocs modifiés, si le plan comptait.
    pub blocs: Option<u64>,
    /// Ce que l'opération a écrit, en coordonnées monde.
    pub bornes: Option<BBox>,
}

impl RapportRegion {
    pub fn est_vide(&self) -> bool {
        self.patches.is_empty()
    }
}

#[derive(Debug)]
pub enum Erreur {
    Source(SourceError),
    Lecture(ReadError),
    Ecriture(WriteError),
    Codec(CodecError),
    /// Le NBT du chunk est tronqué ou mal formé.
    Nbt(tf_nbt::Trunc),
    Encode(EncodeError),
    Splice(tf_anvil::chunk::SpliceError),
}

macro_rules! de {
    ($($src:ty => $var:ident),* $(,)?) => {$(
        impl From<$src> for Erreur {
            fn from(e: $src) -> Erreur {
                Erreur::$var(e)
            }
        }
    )*};
}
de! {
    SourceError => Source,
    ReadError => Lecture,
    WriteError => Ecriture,
    CodecError => Codec,
    tf_nbt::Trunc => Nbt,
    EncodeError => Encode,
    tf_anvil::chunk::SpliceError => Splice,
}

impl std::fmt::Display for Erreur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Erreur::Source(e) => write!(f, "source : {e:?}"),
            Erreur::Lecture(e) => write!(f, "lecture de région : {e:?}"),
            Erreur::Ecriture(e) => write!(f, "écriture de région : {e:?}"),
            Erreur::Codec(e) => write!(f, "compression : {e:?}"),
            Erreur::Nbt(e) => write!(f, "balayage de chunk : {e:?}"),
            Erreur::Encode(e) => write!(f, "encodage de section : {e:?}"),
            Erreur::Splice(e) => write!(f, "recollement : {e:?}"),
        }
    }
}

/// Les chunks d'UNE région que la sélection touche.
///
/// **Coupé avant d'itérer, jamais filtré après.** Parcourir tous les chunks de
/// la sélection puis jeter ceux des autres régions rend le tout quadratique :
/// une sélection de dix régions sur dix en contient 102 400, et les filtrer
/// cent fois fait dix millions d'itérations pour cent mille chunks utiles. Sur
/// un monde Minefield la sélection peut faire des milliers de régions ; c'est
/// le genre de coût qui ne se voit pas sur une fixture et qui rend l'outil
/// inutilisable chez l'utilisateur.
fn chunks_de(sel: &BBox, pos: RegionPos) -> impl Iterator<Item = ChunkPos> {
    let (a, b) = (sel.min.chunk(), sel.max.chunk());
    let x0 = a.x.max(pos.x * 32);
    let x1 = b.x.min(pos.x * 32 + 31);
    let z0 = a.z.max(pos.z * 32);
    let z1 = b.z.min(pos.z * 32 + 31);
    (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| ChunkPos::new(x, z)))
}

/// Applique un plan à une sélection, sur UNE région, à travers le staging.
///
/// Rend les correctifs à pousser dans le journal. Ne les pousse pas lui-même :
/// une opération qui porte sur plusieurs régions doit faire UNE entrée de
/// journal, pas une par région — sinon `Ctrl+Z` défait un tiers du travail.
pub fn appliquer_region<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    pos: RegionPos,
    sel: &BBox,
    plan: &Plan,
    interner: &mut Interner,
) -> Result<RapportRegion, Erreur> {
    let bytes = match staging.read_region(dim, folder, pos) {
        Ok(b) => b,
        // Une région absente est le cas NORMAL au bord d'un monde. La
        // confondre avec un échec ferait refuser une save parfaitement saine.
        Err(SourceError::NotFound) => return Ok(RapportRegion::default()),
        Err(e) => return Err(e.into()),
    };
    let mut region = read(&bytes, pos.x, pos.z)?;
    let mut rap = RapportRegion::default();
    let mut compte = plan.compter.then_some(0u64);

    for cpos in chunks_de(sel, pos) {
        let (lx, lz) = (cpos.x.rem_euclid(32), cpos.z.rem_euclid(32));
        let Some(brut) = region.get_mut(lx, lz) else {
            continue;
        };
        // Une charge déportée arrive VIDE : le crate Anvil ne touche pas au
        // disque. L'oublier ferait lire un chunk vide et l'écraser.
        if brut.needs_external() {
            let nom = external_file_name(cpos.x, cpos.z);
            let charge = staging.read_external(dim, folder, &nom)?;
            brut.resolve_external(charge);
        }
        if brut.payload.is_empty() {
            continue;
        }

        let avant = inflate(&brut.payload, brut.compression)?;
        let balayage = scan(&avant)?;
        let mut edits = Vec::new();
        for sc in &balayage.sections {
            let spos = SectionPos {
                x: cpos.x,
                y: sc.y as i32,
                z: cpos.z,
            };
            if sel.clip_to_section(spos).is_none() {
                continue;
            }
            let Some(mut section) = decode_section(&avant, &balayage, sc, interner)? else {
                continue;
            };
            let r = plan.appliquer(&mut section, sel, spos);
            rap.etages[match r.etage {
                Etage::Rien => 0,
                Etage::Section => 1,
                Etage::Palette => 2,
                Etage::Bloc => 3,
            }] += 1;
            if let (Some(c), Some(n)) = (compte.as_mut(), r.blocs) {
                *c += n;
            }
            if let Some(b) = r.bornes {
                rap.bornes = Some(match rap.bornes {
                    None => b,
                    Some(mut d) => {
                        d.extend(b.min);
                        d.extend(b.max);
                        d
                    }
                });
            }
            if r.etage == Etage::Rien {
                continue;
            }
            // `section_edits` compare ce qu'il va écrire à ce qui est DÉJÀ là :
            // une section que l'opération n'a pas vraiment changée ne produit
            // aucune édition, donc aucune entrée de journal vide.
            edits.extend(section_edits(&avant, &section, sc, interner)?);
        }
        if edits.is_empty() {
            continue;
        }

        let apres = splice(&avant, &mut edits)?;
        rap.patches.push(ChunkPatch::record(
            Cible {
                dim: dim.clone(),
                folder,
                region: pos,
                chunk: brut.index,
            },
            &avant,
            &apres,
            &edits,
        )?);
        brut.payload = Cow::Owned(deflate(&apres, brut.compression)?);
    }

    if !rap.patches.is_empty() {
        let out = write(&region)?;
        staging.write_region(dim, folder, pos, &out.region)?;
        for f in out.external {
            staging.write_external(dim, folder, &f.name, &f.bytes)?;
        }
        // Un `.mcc` devenu inutile qu'on laisserait occuperait le disque pour
        // toujours — et une save qui grossit sans raison finit par être
        // signalée comme un bug.
        for n in out.removed_external {
            staging.remove_external(dim, folder, &n)?;
        }
    }
    rap.blocs = compte;
    Ok(rap)
}

/// Applique un plan à une sélection, sur TOUTES les régions qu'elle touche.
///
/// Une seule entrée de journal pour l'ensemble : une opération qui déborde sur
/// quatre régions doit s'annuler d'un seul `Ctrl+Z`, pas de quatre.
pub fn appliquer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    plan: &Plan,
    interner: &mut Interner,
) -> Result<RapportRegion, Erreur> {
    let mut total = RapportRegion::default();
    let mut compte = plan.compter.then_some(0u64);
    for pos in sel.regions() {
        let r = appliquer_region(staging, dim, folder, pos, sel, plan, interner)?;
        total.patches.extend(r.patches);
        for (a, b) in total.etages.iter_mut().zip(r.etages) {
            *a += b;
        }
        if let (Some(c), Some(n)) = (compte.as_mut(), r.blocs) {
            *c += n;
        }
        if let Some(b) = r.bornes {
            total.bornes = Some(match total.bornes {
                None => b,
                Some(mut d) => {
                    d.extend(b.min);
                    d.extend(b.max);
                    d
                }
            });
        }
    }
    total.blocs = compte;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_world::coords::BlockPos;

    fn boite(a: (i32, i32), b: (i32, i32)) -> BBox {
        BBox::new(
            BlockPos {
                x: a.0,
                y: 0,
                z: a.1,
            },
            BlockPos {
                x: b.0,
                y: 0,
                z: b.1,
            },
        )
    }

    #[test]
    fn les_chunks_d_une_region_sont_coupes_pas_filtres() {
        // Une sélection de 3 × 3 régions. Chaque région ne doit voir QUE ses
        // chunks, et la somme doit faire exactement ceux de la sélection : ni
        // trou, ni doublon.
        let sel = boite((-600, -600), (1100, 1100));
        let mut total = 0usize;
        let mut vus = std::collections::BTreeSet::new();
        for pos in sel.regions() {
            let mut n = 0;
            for c in chunks_de(&sel, pos) {
                assert_eq!(c.region(), pos, "{c:?} n'est pas dans {pos:?}");
                assert!(vus.insert((c.x, c.z)), "{c:?} vu deux fois");
                n += 1;
            }
            assert!(n > 0, "aucune région de la sélection n'est vide");
            total += n;
        }
        assert_eq!(
            total,
            sel.chunks().count(),
            "la somme des régions doit couvrir la sélection, exactement"
        );
    }

    #[test]
    fn une_selection_hors_region_ne_rend_aucun_chunk() {
        let sel = boite((0, 0), (15, 15));
        assert_eq!(chunks_de(&sel, RegionPos { x: 5, z: 5 }).count(), 0);
        // Et le bloc −1 est dans la région −1, pas la région 0.
        let sel = boite((-1, -1), (-1, -1));
        assert_eq!(chunks_de(&sel, RegionPos { x: 0, z: 0 }).count(), 0);
        assert_eq!(chunks_de(&sel, RegionPos { x: -1, z: -1 }).count(), 1);
    }

    #[test]
    fn les_coordonnees_negatives_tombent_dans_la_bonne_region() {
        // Le piège le mieux documenté du dépôt : une division entière naïve
        // charge la mauvaise moitié du monde sans rien signaler.
        let sel = boite((-1, -1), (0, 0));
        let mut par_region: std::collections::BTreeMap<(i32, i32), Vec<(i32, i32)>> =
            Default::default();
        for pos in sel.regions() {
            for c in chunks_de(&sel, pos) {
                par_region
                    .entry((pos.x, pos.z))
                    .or_default()
                    .push((c.x, c.z));
            }
        }
        assert_eq!(par_region[&(-1, -1)], vec![(-1, -1)]);
        assert_eq!(par_region[&(0, -1)], vec![(0, -1)]);
        assert_eq!(par_region[&(-1, 0)], vec![(-1, 0)]);
        assert_eq!(par_region[&(0, 0)], vec![(0, 0)]);
    }
}
