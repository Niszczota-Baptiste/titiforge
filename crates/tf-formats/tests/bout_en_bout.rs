//! **Copier, exporter, réimporter, coller — et obtenir le monde qu'un collage
//! direct aurait donné.**
//!
//! Le test de JONCTION des formats. L'aller-retour prouve que le
//! presse-papiers traverse un fichier ; celui-ci prouve que ce qui en revient
//! se COLLE comme l'original : les coordonnées ajoutées en fin de block
//! entity sont bien celles que le collage réécrit, les entités reprennent
//! leur place, leurs passagers et de nouveaux `UUID`, et les cases retenues
//! suivent — quand le format sait dans quel repère elles sont.
//!
//! Le monde collé est relu par le décodeur GELÉ, pas par le moteur.

mod common;
#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use std::collections::BTreeMap;

use common::*;
use frozen::{chunk_states, decode_region, Tag};
use tf_anvil::Interner;
use tf_bench::mobiles::{region_entites, Occupant, Trait, DV_1_18_2};
use tf_bench::{region, Terrain};
use tf_formats::{ecrire, lire, Format, Meta};
use tf_ops::edition::{coller, copier, Pas};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn monde() -> Staging<MemorySource, MemorySource> {
    let m = MemorySource::new();
    m.put_region(SURFACE, Folder::Region, ZERO, region(&Terrain::peuplee(3)));
    let occupants = vec![
        Occupant::nouveau(
            "minecraft:armor_stand",
            [5.5, 64.0, 5.5],
            30.0,
            [1, 2, 3, 4],
        )
        .avec(Trait::Pose),
        Occupant::cadre([8, 65, 3], 3, "minecraft:filled_map", 3, [5, 6, 7, 8]),
        Occupant::cadre([9, 64, 9], 1, "minecraft:diamond", 1, [9, 10, 11, 12]),
        Occupant::tableau([12, 66, 4], 0, "minecraft:pool", [13, 14, 15, 16]),
        Occupant::nouveau(
            "minecraft:villager",
            [3.5, 64.0, 10.5],
            -90.0,
            [17, 18, 19, 20],
        )
        .avec(Trait::Dort([3, 64, 11]))
        .avec(Trait::Souvenir(
            "minecraft:home",
            [3, 64, 11],
            "minecraft:overworld",
        ))
        .avec(Trait::Souvenir(
            "minecraft:job_site",
            [200, 64, 200],
            "minecraft:overworld",
        )),
        Occupant::nouveau("minecraft:pig", [6.5, 64.0, 12.5], 0.0, [21, 22, 23, 24]).portant(
            Occupant::nouveau("minecraft:zombie", [6.5, 64.9, 12.5], 0.0, [25, 26, 27, 28]),
        ),
    ];
    m.put_region(
        SURFACE,
        Folder::Entities,
        ZERO,
        region_entites(0, 0, &[(0, 0, DV_1_18_2, occupants)]),
    );
    Staging::new(m, MemorySource::new())
}

/// Le chunk (0, 0), des coffres enfouis aux entités de surface.
fn sel() -> BBox {
    BBox::new(BlockPos::new(0, -40, 0), BlockPos::new(15, 80, 15))
}

/// Où l'on colle : le chunk (4, 5).
const ICI: BlockPos = BlockPos {
    x: 64,
    y: -40,
    z: 80,
};
const CHUNK: (u32, u32) = (4, 5);

/// Colle un extrait dans un monde neuf, et rend ce que le chunk d'arrivée
/// porte : ses sections, ses block entities et ses entités — relus par le
/// décodeur gelé, sous forme comparable.
fn coller_et_relire(
    p: &tf_ops::Presse,
    interner: &Interner,
) -> (BTreeMap<i8, Vec<String>>, Vec<Tag>, Vec<Tag>) {
    let st = monde();
    let pas = Pas {
        d: [0; 3],
        avec_air: false,
        air: interner.get("minecraft:air").unwrap(),
        compter: false,
    };
    coller(&st, &SURFACE, Folder::Region, p, ICI, pas, interner).unwrap();
    let blocs = st.read_region(&SURFACE, Folder::Region, ZERO).unwrap();
    let chunk = decode_region(&blocs).remove(&CHUNK).unwrap();
    let mut bes: Vec<Tag> = chunk
        .root
        .get("block_entities")
        .and_then(|t| t.as_list())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(canon)
        .collect();
    bes.sort_by_key(|t| format!("{t:?}"));
    let ents_region = st.read_region(&SURFACE, Folder::Entities, ZERO).unwrap();
    let mut ents: Vec<Tag> = decode_region(&ents_region)
        .remove(&CHUNK)
        .map(|c| c.root.get("Entities").unwrap().as_list().unwrap().clone())
        .unwrap_or_default()
        .iter()
        .map(canon)
        .collect();
    ents.sort_by_key(|t| format!("{:?}", t.get("UUID")));
    (chunk_states(&chunk), bes, ents)
}

/// Retire d'une entité ce qu'un format sans repère ne sait pas faire suivre :
/// la case du lit et les souvenirs.
fn sans_souvenirs(t: &Tag) -> Tag {
    match t {
        Tag::Compound(v) => Tag::Compound(
            v.iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "SleepingX" | "SleepingY" | "SleepingZ" | "Brain"
                    )
                })
                .cloned()
                .collect(),
        ),
        autre => autre.clone(),
    }
}

#[test]
fn un_build_exporte_puis_reimporte_se_colle_comme_l_original() {
    let st = monde();
    let mut interner = Interner::new();
    let original = copier(&st, &SURFACE, Folder::Region, &sel(), &mut interner).unwrap();
    assert_eq!(original.entites.len(), 3, "les trois coffres du chunk");
    assert_eq!(original.mobiles.len(), 6);
    let attendu = coller_et_relire(&original, &interner);
    assert_eq!(attendu.1.len(), 3);
    assert_eq!(attendu.2.len(), 6);

    let meta = Meta {
        data_version: DV_1_18_2,
        nom: "chunk".into(),
        auteur: String::new(),
        description: String::new(),
        date_ms: 0,
    };
    for format in Format::TOUS {
        let ecrit = ecrire(&original, format, &interner, &meta).unwrap();
        let mut neuf = Interner::new();
        neuf.intern("minecraft:air");
        let lu = lire(&ecrit.octets, &mut neuf).unwrap();
        let obtenu = coller_et_relire(&lu.presse, &neuf);
        assert_eq!(obtenu.0, attendu.0, "{format:?} : les blocs");
        assert_eq!(obtenu.1, attendu.1, "{format:?} : les coffres");
        let repere_connu = matches!(format, Format::SpongeV2 | Format::SpongeV3);
        if repere_connu {
            assert_eq!(obtenu.2, attendu.2, "{format:?} : les entités");
        } else {
            let a: Vec<Tag> = attendu.2.iter().map(sans_souvenirs).collect();
            let b: Vec<Tag> = obtenu.2.iter().map(sans_souvenirs).collect();
            assert_eq!(b, a, "{format:?} : les entités, hors souvenirs");
        }
    }
}
