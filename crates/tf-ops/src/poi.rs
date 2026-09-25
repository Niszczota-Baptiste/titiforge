//! Faire relire les POINTS D'INTÉRÊT par le jeu — lits, postes de travail,
//! cloches, ruches — sur les chunks dont les blocs ont changé.
//!
//! Voir `tf_anvil::poi` pour le pourquoi : à `Valid` = 1, le jeu fait
//! confiance à `poi/` et ne relit pas les blocs.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use tf_anvil::chunk::splice;
use tf_anvil::codec::{deflate_level, inflate};
use tf_anvil::poi::faire_invalider;
use tf_anvil::region::{external_file_name, read};
use tf_world::coords::{ChunkPos, RegionPos};
use tf_world::journal::{ChunkPatch, Cible};
use tf_world::source::{Dimension, Folder, RegionSource, SourceError};
use tf_world::staging::{RegionStore, Staging};

use crate::edition::{ecrire_region, Erreur, RapportRegion, NIVEAU_STAGING};

/// Met `Valid` à 0 dans les chunks de `poi/` qui correspondent à `chunks`.
///
/// Une région ou un chunk absents de `poi/` n'ont rien à invalider : le jeu y
/// créera les enregistrements depuis les blocs, la première fois qu'il en
/// verra un qui peut en porter.
pub(crate) fn invalider<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    chunks: &[ChunkPos],
) -> Result<RapportRegion, Erreur> {
    let mut rap = RapportRegion::default();
    let mut par_region: BTreeMap<RegionPos, BTreeSet<ChunkPos>> = BTreeMap::new();
    for c in chunks {
        par_region.entry(c.region()).or_default().insert(*c);
    }
    for (pos, cs) in par_region {
        let octets = match staging.read_region(dim, Folder::Poi, pos) {
            Ok(b) => b,
            Err(SourceError::NotFound) => continue,
            Err(e) => return Err(e.into()),
        };
        let mut region = read(&octets, pos.x, pos.z)?;
        let mut modifie = false;
        for c in cs {
            let (lx, lz) = (c.x.rem_euclid(32), c.z.rem_euclid(32));
            let Some(brut) = region.get_mut(lx, lz) else {
                continue;
            };
            if brut.needs_external() {
                let charge =
                    staging.read_external(dim, Folder::Poi, &external_file_name(c.x, c.z))?;
                brut.resolve_external(charge);
            }
            if brut.payload.is_empty() {
                continue;
            }
            let avant = inflate(&brut.payload, brut.compression)?;
            let mut edits = faire_invalider(&avant)?;
            if edits.is_empty() {
                continue;
            }
            let apres = splice(&avant, &mut edits)?;
            let cible = Cible {
                dim: dim.clone(),
                folder: Folder::Poi,
                region: pos,
                chunk: c.index_in_region() as u16,
            };
            rap.patches
                .push(ChunkPatch::record(cible, &avant, &apres, &edits)?);
            brut.payload = Cow::Owned(deflate_level(&apres, brut.compression, NIVEAU_STAGING)?);
            rap.poi += 1;
            modifie = true;
        }
        if modifie {
            ecrire_region(staging, dim, Folder::Poi, pos, &region, Vec::new())?;
        }
    }
    Ok(rap)
}
