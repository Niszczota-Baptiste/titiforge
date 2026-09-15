//! L'empreinte du fichier produit par une opération.
//!
//! Sert à croiser le chemin PARALLÈLE et le chemin SÉQUENTIEL : ils doivent
//! rendre le même octet. Une propriété qu'aucun test ne peut vérifier seul,
//! parce que le choix se fait à la compilation.
//!
//! ```text
//! cargo run --release -p tf-ops --example empreinte
//! cargo run --release -p tf-ops --example empreinte --no-default-features
//! ```

use tf_anvil::chunk::{decode_section, scan};
use tf_anvil::codec::inflate;
use tf_anvil::region::read;
use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::appliquer;
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::empreinte;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn main() {
    let t = Terrain::petite();
    let brut = region(&t);
    let mut interner = Interner::new();
    {
        let r = read(&brut, 0, 0).unwrap();
        for c in r.iter() {
            let inf = inflate(&c.payload, c.compression).unwrap();
            let s = scan(&inf).unwrap();
            for sc in &s.sections {
                decode_section(&inf, &s, sc, &mut interner).unwrap();
            }
        }
    }
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();

    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 255,
            y: 128,
            z: 255,
        },
    );
    for (nom, plan) in [
        (
            "remplacer",
            Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre)).en_comptant(),
        ),
        (
            "mélanger ",
            Plan::nouveau(Masque::Tout, Motif::melange(vec![(3, pierre), (1, terre)]))
                .avec_seed(4242)
                .en_comptant(),
        ),
    ] {
        let src = MemorySource::new();
        src.put_region(SURFACE, DOSSIER, ZERO, brut.clone());
        let st = Staging::new(src, MemorySource::new());
        let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();
        let bytes = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
        let cibles: Vec<u16> = rap.patches.iter().map(|p| p.cible.chunk).collect();
        println!(
            "{nom} · fichier {:016x} · correctifs {:016x} · blocs {:?} · étages {:?}",
            empreinte(&bytes),
            empreinte(
                &cibles
                    .iter()
                    .flat_map(|c| c.to_le_bytes())
                    .collect::<Vec<u8>>()
            ),
            rap.blocs,
            rap.etages
        );
    }
    println!(
        "chemin : {}",
        if cfg!(feature = "parallele") {
            "PARALLÈLE"
        } else {
            "séquentiel"
        }
    );
}
