//! **Les fichiers tels que les outils d'ORIGINE les écrivent.**
//!
//! Pas de fixture binaire dans le dépôt : chaque fichier est construit ici par
//! un écrivain d'arbre indépendant (`common`), en suivant pas à pas le code
//! qui les produit dans la nature — `SpongeSchematicV2Writer` et `V3Writer`
//! de WorldEdit, `LitematicaSchematic.writeToNBT` et `LitematicaBitArray`,
//! `StructureTemplate.save` du jeu. Ce sont eux, pas nos propres fichiers, qui
//! mettent à l'épreuve ce que l'aller-retour ne voit pas : un coin loin de
//! l'origine, des positions absolues, des propriétés dans le désordre, des
//! régions de taille négative, des cases vides.

mod common;
#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use common::*;
use frozen::Tag;
use tf_anvil::Interner;
use tf_bench::mobiles::{ecrire as ecrire_entite, Occupant, Trait};
use tf_formats::{lire, Lecture, Lu, Remarque};

/// L'état de chaque case d'un extrait lu, par sa clé.
fn etat(lu: &Lu, interner: &Interner, p: [u32; 3]) -> String {
    let i = lu.presse.index(p[0], p[1], p[2]).unwrap();
    interner.resolve(lu.presse.blocs[i]).unwrap().to_string()
}

/// Le compound d'une entité du générateur, en arbre.
fn entite_arbre(o: &Occupant) -> Vec<(String, Tag)> {
    match arbre(&ecrire_entite(o)) {
        Tag::Compound(v) => v,
        _ => unreachable!(),
    }
}

fn sans(v: &[(String, Tag)], noms: &[&str]) -> Vec<(String, Tag)> {
    v.iter()
        .filter(|(k, _)| !noms.contains(&k.as_str()))
        .cloned()
        .collect()
}

fn avec(mut v: Vec<(String, Tag)>, k: &str, t: Tag) -> Tag {
    v.push((k.to_string(), t));
    Tag::Compound(v)
}

fn varints(valeurs: &[u32]) -> Vec<u8> {
    let mut o = Vec::new();
    for &v in valeurs {
        let mut v = v;
        while v & !0x7f != 0 {
            o.push(((v & 0x7f) | 0x80) as u8);
            v >>= 7;
        }
        o.push(v as u8);
    }
    o
}

// ── WorldEdit 7.2 : Sponge v2 ───────────────────────────────────────────────

/// **Un `.schem` v2 de WorldEdit 7.2 se lit**, avec tout ce que cet outil
/// fait de particulier : le coin À SA PLACE dans le monde (`Offset`),
/// l'origine du collage dans `WEOffset`, les entités à leur position
/// ABSOLUE, leurs cases d'accroche et leurs ruches absolues aussi — et des
/// propriétés d'état dans l'ordre du bloc, pas trié.
#[test]
fn un_schem_v2_de_worldedit_se_lit() {
    let min = [1000, 60, -2000];
    let origine = [1003, 61, -1990]; // là où se tenait le joueur
    let taille = [4u32, 3, 5];

    // Une palette de 131 états : les derniers indices s'écrivent en DEUX
    // octets de varint.
    let mut palette = vec![
        "minecraft:air".to_string(),
        "minecraft:stone".into(),
        "minecraft:oak_stairs[waterlogged=false,facing=east,shape=straight,half=bottom]".into(),
        "minefield:chaise_chene[facing=south]".into(),
        "minecraft:chest[facing=west,type=single,waterlogged=false]".into(),
        "stone_bricks".into(),
    ];
    for k in palette.len()..131 {
        palette.push(format!("minecraft:remplissage_{k}"));
    }
    let volume = (taille[0] * taille[1] * taille[2]) as usize;
    let indices: Vec<u32> = (0..volume as u32).map(|i| (i * 37) % 131).collect();
    let mut indices = indices;
    // Les états qu'on vérifie, posés à la main — l'index d'une case est
    // `(y × 5 + z) × 4 + x`.
    let case = |x: usize, y: usize, z: usize| (y * 5 + z) * 4 + x;
    indices[case(2, 0, 0)] = 2; // l'escalier aux propriétés dans le désordre
    indices[case(1, 0, 0)] = 3; // le minefield
    indices[case(1, 1, 0)] = 5; // sans espace de noms
    indices[case(1, 2, 3)] = 4; // le coffre, là où sa block entity l'attend

    let cadre = Occupant::cadre(
        [min[0] + 2, min[1] + 1, min[2] + 4],
        3,
        "minecraft:map",
        0,
        [7, 7, 7, 7],
    );
    let abeille = Occupant::nouveau(
        "minecraft:bee",
        [
            min[0] as f64 + 3.5,
            min[1] as f64 + 2.0,
            min[2] as f64 + 0.5,
        ],
        0.0,
        [8, 8, 8, 8],
    )
    .avec(Trait::Ruche([min[0] + 1, min[1] + 2, min[2] + 3]));

    let schem = c(vec![
        ("Version", i(2)),
        ("DataVersion", i(2975)),
        (
            "Metadata",
            c(vec![
                ("WEOffsetX", i(min[0] - origine[0])),
                ("WEOffsetY", i(min[1] - origine[1])),
                ("WEOffsetZ", i(min[2] - origine[2])),
                (
                    "WorldEdit",
                    c(vec![
                        ("Version", s("7.2.10")),
                        ("EditingPlatform", s("enginehub:bukkit")),
                        (
                            "Offset",
                            ia([
                                min[0] - origine[0],
                                min[1] - origine[1],
                                min[2] - origine[2],
                            ]),
                        ),
                        (
                            "Platforms",
                            c(vec![("enginehub:bukkit", c(vec![("Name", s("Paper"))]))]),
                        ),
                    ]),
                ),
            ]),
        ),
        ("Width", Tag::Short(4)),
        ("Height", Tag::Short(3)),
        ("Length", Tag::Short(5)),
        ("Offset", ia(min)),
        ("PaletteMax", i(131)),
        (
            "Palette",
            Tag::Compound(
                palette
                    .iter()
                    .enumerate()
                    .map(|(k, e)| (e.clone(), Tag::Int(k as i32)))
                    .collect(),
            ),
        ),
        ("BlockData", Tag::ByteArray(varints(&indices))),
        (
            "BlockEntities",
            Tag::List(vec![c(vec![
                (
                    "Items",
                    Tag::List(vec![c(vec![
                        ("Slot", Tag::Byte(3)),
                        ("id", s("minecraft:emerald")),
                        ("Count", Tag::Byte(12)),
                    ])]),
                ),
                ("Lock", s("")),
                ("Id", s("minecraft:chest")),
                ("Pos", ia([1, 2, 3])),
            ])]),
        ),
        (
            "Entities",
            Tag::List(
                [&cadre, &abeille]
                    .iter()
                    .map(|o| {
                        // `WriterUtil.encodeEntity(clipboard, false)` : les
                        // données à plat, sans `id`, puis `Id`.
                        let v = sans(&entite_arbre(o), &["id"]);
                        avec(v, "Id", s(o.id))
                    })
                    .collect(),
            ),
        ),
        ("BiomePaletteMax", i(1)),
        ("BiomePalette", c(vec![("minecraft:plains", i(0))])),
        ("BiomeData", Tag::ByteArray(vec![0; 20])),
    ]);
    let fichier = gz(&nbt("Schematic", &schem));
    let mut interner = Interner::new();
    let lu = lire(&fichier, &mut interner).unwrap();

    assert_eq!(lu.lecture, Lecture::Sponge { version: 2 });
    assert_eq!(lu.presse.taille, taille);
    assert_eq!(
        lu.presse.ancre,
        [3, 1, 10],
        "l'origine du collage, relative au coin"
    );
    assert_eq!(
        etat(&lu, &interner, [2, 0, 0]),
        "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=false",
        "des propriétés TRIÉES : la clé d'une save"
    );
    assert_eq!(
        etat(&lu, &interner, [1, 0, 0]),
        "minefield:chaise_chene|facing=south",
        "un minefield reste un minefield"
    );
    assert_eq!(
        etat(&lu, &interner, [1, 1, 0]),
        "minecraft:stone_bricks",
        "sans espace de noms, c'est minecraft"
    );
    for (k, &v) in indices.iter().enumerate() {
        let k = k as u32;
        let p = [k % 4, k / 20, (k / 4) % 5];
        if v >= 6 {
            assert_eq!(
                etat(&lu, &interner, p),
                format!("minecraft:remplissage_{v}"),
                "case {p:?}"
            );
        }
    }

    assert_eq!(lu.presse.entites.len(), 1);
    let coffre = &lu.presse.entites[0];
    assert_eq!(coffre.case, [1, 2, 3]);
    assert_eq!(coffre.id(), Some("minecraft:chest"));
    let t = arbre(&coffre.octets());
    assert_eq!(
        t.get("x"),
        Some(&Tag::Int(1)),
        "les coordonnées sont LOCALES"
    );
    assert!(t.get("Items").is_some() && t.get("Id").is_none() && t.get("Pos").is_none());

    let cadre_lu = &lu.presse.mobiles[0];
    assert_eq!(
        cadre_lu.pos(),
        Some([2.5, 1.5, 4.03125]),
        "absolue → locale"
    );
    assert_eq!(cadre_lu.corps[0].tuile.as_ref().unwrap().v, [2, 1, 4]);
    let abeille_lue = &lu.presse.mobiles[1];
    assert_eq!(
        abeille_lue.corps[0]
            .retenues
            .iter()
            .map(|r| r.v)
            .collect::<Vec<_>>(),
        vec![[1, 2, 3]],
        "la ruche est DANS l'extrait : elle suivra le collage"
    );
    assert_eq!(lu.remarques, vec![Remarque::BiomesIgnores]);
}

// ── WorldEdit 7.3 : Sponge v3 ───────────────────────────────────────────────

/// **Un `.schem` v3 de WorldEdit 7.3 se lit** : la racine anonyme, le coin à
/// `Offset + Origin`, les block entities qui gardent dans `Data` leurs
/// coordonnées ABSOLUES — le lecteur les écrase —, les entités à position
/// relative dont les données portent encore la position et la case
/// d'accroche absolues.
#[test]
fn un_schem_v3_de_worldedit_se_lit() {
    let origine = [-512, 70, 4096];
    let min = [-515, 64, 4090];
    let offset = [0, 1, 2].map(|a| min[a] - origine[a]);
    let taille = [5u32, 2, 3];
    let palette = [
        "minecraft:air",
        "minecraft:barrel[facing=up,open=false]",
        "minecraft:glass",
    ];
    let volume = 5 * 2 * 3;
    let mut indices = vec![2u32; volume];
    indices[0] = 0;
    let tonneau = [4, 0, 2];
    indices[((tonneau[1] * 3 + tonneau[2]) * 5 + tonneau[0]) as usize] = 1;

    let tableau = Occupant::tableau(
        [min[0] + 1, min[1] + 1, min[2]],
        0,
        "minecraft:fighters",
        [9, 9, 9, 9],
    );
    let pos_rel = [0, 1, 2].map(|a| tableau.pos[a] - min[a] as f64);

    let schem = c(vec![
        ("Version", i(3)),
        ("DataVersion", i(3465)),
        (
            "Metadata",
            c(vec![
                ("Date", Tag::Long(1_700_000_000_000)),
                (
                    "WorldEdit",
                    c(vec![
                        ("Version", s("7.3.0")),
                        ("EditingPlatform", s("enginehub:fabric")),
                        ("Origin", ia(origine)),
                    ]),
                ),
            ]),
        ),
        ("Width", Tag::Short(5)),
        ("Height", Tag::Short(2)),
        ("Length", Tag::Short(3)),
        ("Offset", ia(offset)),
        (
            "Blocks",
            c(vec![
                (
                    "Palette",
                    Tag::Compound(
                        palette
                            .iter()
                            .enumerate()
                            .map(|(k, e)| (e.to_string(), Tag::Int(k as i32)))
                            .collect(),
                    ),
                ),
                ("Data", Tag::ByteArray(varints(&indices))),
                (
                    "BlockEntities",
                    Tag::List(vec![c(vec![
                        ("Id", s("minecraft:barrel")),
                        ("Pos", ia(tonneau)),
                        (
                            "Data",
                            c(vec![
                                ("id", s("minecraft:barrel")),
                                ("x", i(min[0] + tonneau[0])),
                                ("y", i(min[1] + tonneau[1])),
                                ("z", i(min[2] + tonneau[2])),
                                ("CustomName", s("{\"text\":\"Réserve\"}")),
                                ("Items", Tag::List(vec![])),
                            ]),
                        ),
                    ])]),
                ),
            ]),
        ),
        (
            "Biomes",
            c(vec![
                ("Palette", c(vec![("minecraft:plains", i(0))])),
                ("Data", Tag::ByteArray(vec![0; 30])),
            ]),
        ),
        (
            "Entities",
            Tag::List(vec![c(vec![
                ("Id", s("minecraft:painting")),
                ("Pos", ld(pos_rel)),
                (
                    "Data",
                    Tag::Compound(sans(&entite_arbre(&tableau), &["id"])),
                ),
            ])]),
        ),
    ]);
    let fichier = gz(&nbt("", &c(vec![("Schematic", schem)])));
    let mut interner = Interner::new();
    let lu = lire(&fichier, &mut interner).unwrap();

    assert_eq!(lu.lecture, Lecture::Sponge { version: 3 });
    assert_eq!(lu.presse.taille, taille);
    assert_eq!(lu.presse.ancre, offset.map(|v| -v));
    assert_eq!(
        etat(&lu, &interner, [4, 0, 2]),
        "minecraft:barrel|facing=up,open=false"
    );
    assert_eq!(etat(&lu, &interner, [0, 0, 0]), "minecraft:air");
    assert_eq!(etat(&lu, &interner, [1, 1, 1]), "minecraft:glass");

    let t = arbre(&lu.presse.entites[0].octets());
    assert_eq!(lu.presse.entites[0].case, tonneau);
    assert_eq!(
        [t.get("x"), t.get("y"), t.get("z")],
        [Some(&Tag::Int(4)), Some(&Tag::Int(0)), Some(&Tag::Int(2))],
        "les coordonnées absolues de Data sont remplacées"
    );
    assert_eq!(t.get("id").unwrap().as_str(), Some("minecraft:barrel"));

    let m = &lu.presse.mobiles[0];
    assert_eq!(m.pos(), Some(pos_rel));
    assert_eq!(
        m.corps[0].tuile.as_ref().unwrap().v,
        [1, 1, 0],
        "la case d'un tableau PAIR, retrouvée depuis sa position"
    );
    assert_eq!(lu.remarques, vec![Remarque::BiomesIgnores]);
}

// ── Litematica ──────────────────────────────────────────────────────────────

/// `LitematicaBitArray.setAt`, porté ligne à ligne depuis le Java.
fn set_at(longs: &mut [i64], bits: u32, index: u64, value: u32) {
    let start_offset = index * bits as u64;
    let start_arr = (start_offset >> 6) as usize;
    let end_arr = (((index + 1) * bits as u64 - 1) >> 6) as usize;
    let start_bit = (start_offset & 0x3f) as u32;
    let max = (1i64 << bits) - 1;
    longs[start_arr] = longs[start_arr] & !(max << start_bit) | (value as i64 & max) << start_bit;
    if start_arr != end_arr {
        let end_offset = 64 - start_bit;
        let j1 = bits - end_offset;
        longs[end_arr] =
            ((longs[end_arr] as u64 >> j1) << j1) as i64 | (value as i64 & max) >> end_offset;
    }
}

fn empaqueter(valeurs: &[u32], taille_palette: usize) -> Vec<i64> {
    let bits = (32 - ((taille_palette - 1) as u32).leading_zeros()).max(2);
    let n = (valeurs.len() as u64 * bits as u64).div_ceil(64).max(1) as usize;
    let mut longs = vec![0i64; n];
    for (k, &v) in valeurs.iter().enumerate() {
        set_at(&mut longs, bits, k as u64, v);
    }
    longs
}

fn etat_compound(nom: &str, props: &[(&str, &str)]) -> Tag {
    let mut v = vec![("Name", s(nom))];
    if !props.is_empty() {
        v.push((
            "Properties",
            Tag::Compound(props.iter().map(|(k, x)| (k.to_string(), s(x))).collect()),
        ));
    }
    c(v)
}

/// **Un `.litematic` de Litematica se lit** — trois régions dont une de
/// taille NÉGATIVE et une qui en recouvre une autre, des palettes de largeurs
/// différentes (2 et 5 bits), des block entities relatives au coin MINIMAL de
/// leur région, des entités relatives à sa POSITION, et des cases d'accroche
/// et de lit restées dans le repère du monde d'origine.
#[test]
fn un_litematic_de_litematica_se_lit() {
    // Tour : Position (−1, 0, 2), taille (3, 4, 2) → de (−1, 0, 2) à (1, 3, 3).
    let tour_pal = vec![
        etat_compound("minecraft:air", &[]),
        etat_compound("minecraft:stone", &[]),
        etat_compound("minecraft:oak_planks", &[]),
        etat_compound(
            "minecraft:chest",
            &[
                ("facing", "north"),
                ("type", "single"),
                ("waterlogged", "false"),
            ],
        ),
    ];
    let tour_taille = [3u32, 4, 2];
    let n_tour = 3 * 4 * 2;
    let mut tour: Vec<u32> = (0..n_tour).map(|k| 1 + (k % 2)).collect();
    // Le coffre en (1, 2, 0) de la région.
    tour[((2 * 2) * 3 + 1) as usize] = 3;

    // Rempart : Position (6, 1, 5), taille (−3, −1, −2) → coin minimal
    // (4, 1, 4), de (4, 1, 4) à (6, 1, 5).
    let mut rempart_pal = vec![etat_compound("minecraft:air", &[])];
    for k in 1..20 {
        rempart_pal.push(etat_compound(&format!("minefield:pierre_{k}"), &[]));
    }
    let n_rempart = 3 * 2;
    let rempart: Vec<u32> = (0..n_rempart).map(|k| 1 + (k * 7) % 19).collect();

    // Balcon : (1, 3, 2), taille (1, 1, 2) — DANS la tour : deux cases
    // recouvertes, et le balcon, écrit après, l'emporte.
    let balcon_pal = vec![etat_compound("minecraft:glass", &[])];

    // Un cadre du rempart, sa case dans le repère du fichier : (5, 1, 5).
    // `Pos` est relative à la Position de la région ; `TileX/Y/Z` restent
    // dans le repère du monde où le fichier a été sauvé.
    let monde = [100_000, 70, -30_000];
    let tuile_fichier = [5, 1, 5];
    let cadre = Occupant::cadre(
        [0, 1, 2].map(|a| tuile_fichier[a] + monde[a]),
        3,
        "minecraft:compass",
        1,
        [1, 2, 3, 4],
    );
    let pos_fichier = [0, 1, 2].map(|a| cadre.pos[a] - monde[a] as f64);
    let pos_rel = [0, 1, 2].map(|a| pos_fichier[a] - [6.0, 1.0, 5.0][a]);
    let mut cadre_nbt = sans(&entite_arbre(&cadre), &["Pos"]);
    cadre_nbt.push(("Pos".into(), ld(pos_rel)));
    let dormeur = Occupant::nouveau(
        "minecraft:villager",
        [100_004.5, 71.5, -29_995.5],
        0.0,
        [5, 5, 5, 5],
    )
    .avec(Trait::Dort([100_004, 71, -29_996]));
    let mut dormeur_nbt = sans(&entite_arbre(&dormeur), &["Pos"]);
    dormeur_nbt.push(("Pos".into(), ld([-1.5, 0.5, -0.5])));
    // Un cochon monté : Litematica réécrit la position de la MONTURE, et
    // laisse celle du passager dans le repère du monde.
    let cochon = Occupant::nouveau(
        "minecraft:pig",
        [100_005.5, 71.0, -29_995.5],
        0.0,
        [6, 6, 6, 6],
    )
    .portant(Occupant::nouveau(
        "minecraft:zombie",
        [100_005.5, 71.9, -29_995.5],
        0.0,
        [7, 7, 7, 7],
    ));
    let mut cochon_nbt = sans(&entite_arbre(&cochon), &["Pos"]);
    cochon_nbt.push(("Pos".into(), ld([-0.5, 0.0, -0.5])));
    // Une entité sans `id` : le jeu la jetterait.
    let sans_id = c(vec![
        ("Pos", ld([0.5, 0.0, 0.5])),
        ("Health", Tag::Float(20.0)),
    ]);

    let region = |pal: Vec<Tag>,
                  valeurs: &[u32],
                  pos: [i32; 3],
                  taille: [i32; 3],
                  be: Vec<Tag>,
                  ents: Vec<Tag>,
                  ticks: usize| {
        let n = pal.len();
        c(vec![
            ("BlockStatePalette", Tag::List(pal)),
            ("BlockStates", Tag::LongArray(empaqueter(valeurs, n))),
            ("TileEntities", Tag::List(be)),
            (
                "PendingBlockTicks",
                Tag::List(
                    (0..ticks)
                        .map(|_| {
                            c(vec![
                                ("Block", s("minecraft:water")),
                                ("Priority", i(0)),
                                ("SubTick", Tag::Long(0)),
                                ("Time", i(5)),
                                ("x", i(0)),
                                ("y", i(0)),
                                ("z", i(0)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("Entities", Tag::List(ents)),
            ("Position", xyz(pos)),
            ("Size", xyz(taille)),
        ])
    };
    let fichier = c(vec![
        ("MinecraftDataVersion", i(2975)),
        ("Version", i(6)),
        ("SubVersion", i(1)),
        (
            "Metadata",
            c(vec![
                ("Name", s("Château")),
                ("Author", s("Minefield")),
                ("Description", s("")),
                ("RegionCount", i(3)),
                ("TotalVolume", i(32)),
                ("TotalBlocks", i(30)),
                ("TimeCreated", Tag::Long(1)),
                ("TimeModified", Tag::Long(2)),
                ("EnclosingSize", xyz([8, 4, 4])),
            ]),
        ),
        (
            "Regions",
            c(vec![
                (
                    "Tour",
                    region(
                        tour_pal,
                        &tour,
                        [-1, 0, 2],
                        [3, 4, 2],
                        vec![c(vec![
                            ("id", s("minecraft:chest")),
                            ("Items", Tag::List(vec![])),
                            ("x", i(1)),
                            ("y", i(2)),
                            ("z", i(0)),
                        ])],
                        vec![],
                        2,
                    ),
                ),
                (
                    "Rempart",
                    region(
                        rempart_pal,
                        &rempart,
                        [6, 1, 5],
                        [-3, -1, -2],
                        // Un tonneau relatif au coin MINIMAL (4, 1, 4) : dans
                        // le repère du fichier, (6, 1, 5).
                        vec![c(vec![
                            ("id", s("minecraft:barrel")),
                            ("x", i(2)),
                            ("y", i(0)),
                            ("z", i(1)),
                        ])],
                        vec![
                            Tag::Compound(cadre_nbt),
                            Tag::Compound(dormeur_nbt),
                            Tag::Compound(cochon_nbt),
                            sans_id,
                        ],
                        0,
                    ),
                ),
                (
                    "Balcon",
                    region(balcon_pal, &[0, 0], [1, 3, 2], [1, 1, 2], vec![], vec![], 0),
                ),
            ]),
        ),
    ]);
    let mut interner = Interner::new();
    let lu = lire(&gz(&nbt("", &fichier)), &mut interner).unwrap();

    assert_eq!(
        lu.lecture,
        Lecture::Litematic {
            version: 6,
            regions: 3
        }
    );
    // La boîte englobante : de (−1, 0, 2) à (6, 3, 5).
    assert_eq!(lu.presse.taille, [8, 4, 4]);
    assert_eq!(lu.presse.ancre, [1, 0, -2], "l'origine du fichier");
    let local = |p: [i32; 3]| [0, 1, 2].map(|a| (p[a] - [-1, 0, 2][a]) as u32);
    // La tour, case par case — sauf ce que le balcon recouvre.
    for y in 0..4 {
        for z in 0..2 {
            for x in 0..3 {
                let v = tour[((y * 2 + z) * 3 + x) as usize];
                let attendu = ["minecraft:air", "minecraft:stone", "minecraft:oak_planks"]
                    .get(v as usize)
                    .map(|s| s.to_string())
                    .unwrap_or("minecraft:chest|facing=north,type=single,waterlogged=false".into());
                let p = [x - 1, y, z + 2];
                if p[0] == 1 && p[1] == 3 {
                    continue; // le balcon
                }
                assert_eq!(etat(&lu, &interner, local(p)), attendu, "tour {p:?}");
            }
        }
    }
    for z in 2..4 {
        assert_eq!(
            etat(&lu, &interner, local([1, 3, z])),
            "minecraft:glass",
            "le balcon l'emporte"
        );
    }
    // Le rempart, à partir de son coin MINIMAL (4, 1, 4).
    for z in 0..2 {
        for x in 0..3 {
            let v = rempart[(z * 3 + x) as usize];
            assert_eq!(
                etat(&lu, &interner, local([4 + x, 1, 4 + z])),
                format!("minefield:pierre_{v}")
            );
        }
    }
    // Hors des régions : de l'air.
    assert_eq!(etat(&lu, &interner, local([3, 0, 2])), "minecraft:air");

    // Le coffre : relatif au coin minimal de la tour ; le tonneau, à celui
    // du rempart. Rangés en YZX, comme `copier` les range : le tonneau, plus
    // bas, d'abord.
    assert_eq!(lu.presse.entites.len(), 2);
    assert_eq!(
        lu.presse.entites[0].case,
        local([6, 1, 5]).map(|v| v as i32)
    );
    assert_eq!(lu.presse.entites[0].id(), Some("minecraft:barrel"));
    assert_eq!(
        lu.presse.entites[1].case,
        local([0, 2, 2]).map(|v| v as i32)
    );

    // Le cadre : sa position depuis la Position (6, 1, 5), sa case RETROUVÉE.
    let m = &lu.presse.mobiles[0];
    let attendu = [0, 1, 2].map(|a| pos_fichier[a] - [-1.0, 0.0, 2.0][a]);
    assert_eq!(m.pos(), Some(attendu));
    assert_eq!(
        m.corps[0].tuile.as_ref().unwrap().v,
        local(tuile_fichier).map(|v| v as i32),
        "la case d'accroche vient de la position, pas du monde d'origine"
    );
    assert_eq!(m.corps[0].retenues.len(), 0);
    let d = &lu.presse.mobiles[1];
    assert!(
        d.corps[0].retenues.is_empty(),
        "le lit est dans un monde inconnu"
    );
    // Le passager, dans le repère du monde, est posé SUR sa monture.
    let monte = &lu.presse.mobiles[2];
    let sous = [0, 1, 2].map(|a| [5.5, 1.0, 4.5][a] - [-1.0, 0.0, 2.0][a]);
    assert_eq!(monte.corps[0].pos.unwrap().v, sous);
    assert_eq!(monte.corps[1].id.as_deref(), Some("minecraft:zombie"));
    assert_eq!(monte.corps[1].pos.unwrap().v, sous);
    assert_eq!(lu.presse.mobiles.len(), 3, "l'entité sans id est ignorée");

    assert_eq!(
        lu.remarques,
        vec![
            Remarque::RegionsFusionnees {
                regions: 3,
                recouvertes: 2
            },
            Remarque::TicksIgnores(2),
            Remarque::EntitesIgnorees(1),
            Remarque::SouvenirsLaisses(1),
        ]
    );
    assert_eq!(lu.nom.as_deref(), Some("Château"));
    let _ = tour_taille;
}

// ── le jeu : bloc de structure ──────────────────────────────────────────────

/// **Un `.nbt` de bloc de structure se lit** : l'air n'est PAS à l'indice 0,
/// deux cases manquent (des `structure_void`), le coffre porte son `id` mais
/// pas ses coordonnées, et les entités ont `pos` et `blockPos` relatifs mais
/// des données dans le repère du monde où la structure a été sauvée.
#[test]
fn un_nbt_du_jeu_se_lit() {
    let monde = [-7_000, 30, 12_000];
    let tableau = Occupant::tableau(
        [monde[0] + 2, monde[1] + 1, monde[2] + 1],
        2,
        "minecraft:pool",
        [3, 1, 4, 1],
    );
    let pos_rel = [0, 1, 2].map(|a| tableau.pos[a] - monde[a] as f64);
    let mut blocs = Vec::new();
    for y in 0..2 {
        for z in 0..2 {
            for x in 0..3 {
                if (x, y, z) == (0, 1, 0) || (x, y, z) == (1, 1, 0) {
                    continue; // des vides
                }
                let mut b = vec![
                    ("pos", li([x, y, z])),
                    ("state", i(if y == 0 { 0 } else { 1 })),
                ];
                if (x, y, z) == (2, 1, 1) {
                    b[1] = ("state", i(2));
                    b.push((
                        "nbt",
                        c(vec![
                            ("Items", Tag::List(vec![])),
                            ("id", s("minecraft:chest")),
                        ]),
                    ));
                }
                blocs.push(c(b));
            }
        }
    }
    let palette = Tag::List(vec![
        etat_compound("minecraft:stone", &[]),
        etat_compound("minecraft:air", &[]),
        etat_compound(
            "minecraft:chest",
            &[
                ("facing", "north"),
                ("type", "single"),
                ("waterlogged", "false"),
            ],
        ),
    ]);
    let structure = |pal: (&str, Tag)| {
        c(vec![
            ("size", li([3, 2, 2])),
            (
                "entities",
                Tag::List(vec![c(vec![
                    ("pos", ld(pos_rel)),
                    ("blockPos", li([2, 1, 1])),
                    ("nbt", Tag::Compound(entite_arbre(&tableau))),
                ])]),
            ),
            ("blocks", Tag::List(blocs.clone())),
            pal,
            ("DataVersion", i(2975)),
        ])
    };

    let mut interner = Interner::new();
    let lu = lire(
        &gz(&nbt("", &structure(("palette", palette.clone())))),
        &mut interner,
    )
    .unwrap();
    assert_eq!(lu.lecture, Lecture::Structure);
    assert_eq!(lu.presse.taille, [3, 2, 2]);
    assert_eq!(lu.presse.ancre, [0, 0, 0]);
    assert_eq!(etat(&lu, &interner, [0, 0, 0]), "minecraft:stone");
    assert_eq!(etat(&lu, &interner, [0, 1, 1]), "minecraft:air");
    assert_eq!(etat(&lu, &interner, [0, 1, 0]), "minecraft:air", "un vide");
    assert_eq!(
        etat(&lu, &interner, [2, 1, 1]),
        "minecraft:chest|facing=north,type=single,waterlogged=false"
    );
    assert_eq!(lu.presse.entites.len(), 1);
    assert_eq!(lu.presse.entites[0].case, [2, 1, 1]);
    assert_eq!(lu.presse.entites[0].id(), Some("minecraft:chest"));
    let m = &lu.presse.mobiles[0];
    assert_eq!(m.pos(), Some(pos_rel));
    assert_eq!(
        m.corps[0].tuile.as_ref().unwrap().v,
        [2, 1, 1],
        "la case d'accroche retrouvée, qui est aussi le blockPos du jeu"
    );
    assert_eq!(lu.remarques, vec![Remarque::CasesVides(2)]);

    // Une structure à VARIANTES : seule la première palette est lue.
    let variantes = Tag::List(vec![
        palette.clone(),
        Tag::List(vec![
            etat_compound("minecraft:mossy_cobblestone", &[]),
            etat_compound("minecraft:air", &[]),
            etat_compound("minecraft:barrel", &[]),
        ]),
    ]);
    let mut interner = Interner::new();
    let lu = lire(
        &gz(&nbt("", &structure(("palettes", variantes)))),
        &mut interner,
    )
    .unwrap();
    assert_eq!(etat(&lu, &interner, [0, 0, 0]), "minecraft:stone");
    assert_eq!(
        lu.remarques,
        vec![Remarque::VariantesIgnorees(2), Remarque::CasesVides(2)]
    );
}

/// **Une entité accrochée que le jeu vanilla ne connaît pas** — celle d'un
/// mod : sa case ne se RECALCULE pas depuis sa position. Dans un fichier dont
/// on connaît le repère (WorldEdit), elle se traduit comme la position ; dans
/// un autre (Litematica), elle reste telle quelle, et le compte rendu la
/// nomme.
#[test]
fn une_entite_accrochee_de_mod_se_traduit_ou_se_nomme() {
    let min = [500, 70, 500];
    let affiche = |tuile: [i32; 3], pos: [f64; 3]| {
        vec![
            ("Pos".to_string(), ld(pos)),
            ("TileX".to_string(), i(tuile[0])),
            ("TileY".to_string(), i(tuile[1])),
            ("TileZ".to_string(), i(tuile[2])),
            ("Facing".to_string(), Tag::Byte(2)),
        ]
    };
    let v2 = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(4)),
        ("Height", Tag::Short(4)),
        ("Length", Tag::Short(4)),
        ("Offset", ia(min)),
        ("Palette", c(vec![("minecraft:air", i(0))])),
        ("BlockData", Tag::ByteArray(vec![0; 64])),
        (
            "Entities",
            Tag::List(vec![avec(
                affiche([502, 71, 503], [502.5, 71.5, 503.96875]),
                "Id",
                s("unmod:affiche"),
            )]),
        ),
    ]);
    let lu = lire(&gz(&nbt("Schematic", &v2)), &mut Interner::new()).unwrap();
    let m = &lu.presse.mobiles[0];
    assert_eq!(m.pos(), Some([2.5, 1.5, 3.96875]));
    assert_eq!(
        m.corps[0].tuile.as_ref().map(|t| t.v),
        Some([2, 1, 3]),
        "traduite comme la position : le repère est connu"
    );
    assert_eq!(lu.remarques, vec![]);

    let litematic = c(vec![
        ("Version", i(6)),
        ("Metadata", c(vec![])),
        (
            "Regions",
            c(vec![(
                "r",
                c(vec![
                    (
                        "BlockStatePalette",
                        Tag::List(vec![c(vec![("Name", s("minecraft:air"))])]),
                    ),
                    ("BlockStates", Tag::LongArray(vec![0, 0])),
                    (
                        "Entities",
                        Tag::List(vec![avec(
                            affiche([100_502, 71, -9_497], [2.5, 1.5, 3.96875]),
                            "id",
                            s("unmod:affiche"),
                        )]),
                    ),
                    ("Position", xyz([0, 0, 0])),
                    ("Size", xyz([4, 4, 4])),
                ]),
            )]),
        ),
    ]);
    let lu = lire(&gz(&nbt("", &litematic)), &mut Interner::new()).unwrap();
    let m = &lu.presse.mobiles[0];
    assert!(m.corps[0].tuile.is_none(), "ni recalculable ni traduisible");
    assert_eq!(
        arbre(&m.octets()).get("TileX"),
        Some(&Tag::Int(100_502)),
        "ses octets restent ceux du fichier"
    );
    assert_eq!(
        lu.remarques,
        vec![Remarque::AccrochesInconnues(vec!["unmod:affiche".into()])]
    );
}
