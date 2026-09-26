//! **Un fichier d'échange vient d'ailleurs** — d'un site, d'un ami, d'une
//! version de l'outil qu'on n'a jamais vue. Aucun ne doit faire paniquer
//! l'application, ni lui faire allouer des gigaoctets pour quelques octets.

mod common;
#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use std::io::{Read, Write};

use common::*;
use frozen::Tag;
use tf_anvil::Interner;
use tf_bench::mobiles::DV_1_18_2;
use tf_formats::{ecrire, lire, Erreur, Format, Meta};

fn meta() -> Meta {
    Meta {
        data_version: DV_1_18_2,
        nom: "essai".into(),
        auteur: String::new(),
        description: String::new(),
        date_ms: 0,
    }
}

/// Les octets NBT, décompressés, de l'extrait riche dans un format.
fn nu(format: Format) -> Vec<u8> {
    let r = riche();
    let ecrit = ecrire(&r.presse, format, &r.interner, &meta()).unwrap();
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&ecrit.octets[..])
        .read_to_end(&mut out)
        .unwrap();
    out
}

/// **Les formats d'avant 1.13 sont NOMMÉS**, pas pris pour des fichiers
/// corrompus : ils numérotent leurs blocs, et le dire permet de savoir quoi
/// faire.
#[test]
fn les_formats_d_avant_1_13_sont_nommes() {
    let mcedit = c(vec![
        ("Width", Tag::Short(2)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Materials", s("Alpha")),
        ("Blocks", Tag::ByteArray(vec![1, 4])),
        ("Data", Tag::ByteArray(vec![0, 0])),
        ("Entities", Tag::List(vec![])),
        ("TileEntities", Tag::List(vec![])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("Schematic", &mcedit)), &mut Interner::new()),
        Err(Erreur::Ancien(_))
    ));
    let litematic_v3 = c(vec![
        ("Version", i(3)),
        ("Regions", c(vec![])),
        ("Metadata", c(vec![])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("", &litematic_v3)), &mut Interner::new()),
        Err(Erreur::Ancien(_))
    ));
    let sponge_v4 = c(vec![
        ("Version", i(4)),
        ("Width", Tag::Short(1)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Palette", c(vec![("minecraft:air", i(0))])),
    ]);
    assert_eq!(
        lire(&gz(&nbt("Schematic", &sponge_v4)), &mut Interner::new()),
        Err(Erreur::Version {
            format: ".schem (Sponge)",
            version: 4
        })
    );
}

/// Un NBT qui n'est d'aucun format, et ce qui n'est pas du NBT du tout.
#[test]
fn ce_qui_n_est_pas_un_format_d_echange_est_refuse() {
    let level = c(vec![("Data", c(vec![("LevelName", s("Minefield"))]))]);
    assert_eq!(
        lire(&gz(&nbt("", &level)), &mut Interner::new()),
        Err(Erreur::Inconnu)
    );
    for octets in [
        &b""[..],
        b"PK\x03\x04",
        b"\x1f\x8b\x08\x00garbage",
        b"\x0a\x00",
    ] {
        assert_eq!(
            lire(octets, &mut Interner::new()),
            Err(Erreur::Illisible),
            "{octets:?}"
        );
    }
}

/// **Un format se reconnaît à ses OCTETS** : le même `.litematic` se lit en
/// gzip, en zlib et en NBT nu.
#[test]
fn un_format_se_reconnait_a_ses_octets() {
    let brut = nu(Format::Litematic);
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    zlib.write_all(&brut).unwrap();
    let zlib = zlib.finish().unwrap();
    let a = lire(&brut, &mut Interner::new()).unwrap();
    let b = lire(&zlib, &mut Interner::new()).unwrap();
    let c = lire(&gz(&brut), &mut Interner::new()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
}

/// **Aucun fichier tronqué ne fait paniquer** — à CHAQUE longueur, pour les
/// quatre formats : c'est un fichier de moins de deux kilo-octets, on peut
/// tous les essayer.
#[test]
fn aucun_fichier_tronque_ne_fait_paniquer() {
    for format in Format::TOUS {
        let brut = nu(format);
        for n in 0..brut.len() {
            assert!(
                lire(&brut[..n], &mut Interner::new()).is_err(),
                "{format:?} tronqué à {n} octets sur {} se lit",
                brut.len()
            );
        }
    }
}

/// **Aucune corruption ne fait paniquer** : des milliers d'octets changés au
/// hasard, deux par fichier. Le résultat peut être une erreur ou un extrait —
/// jamais un plantage, jamais une allocation démesurée.
#[test]
fn aucune_corruption_ne_fait_paniquer() {
    let mut graine = 0x853c_49e6_748f_ea9bu64;
    let mut tirer = move || {
        graine ^= graine << 13;
        graine ^= graine >> 7;
        graine ^= graine << 17;
        graine
    };
    let mut lus = 0;
    for format in Format::TOUS {
        let brut = nu(format);
        for _ in 0..1_500 {
            let mut v = brut.clone();
            for _ in 0..2 {
                let i = (tirer() % v.len() as u64) as usize;
                v[i] = tirer() as u8;
            }
            if lire(&v, &mut Interner::new()).is_ok() {
                lus += 1;
            }
        }
    }
    // Beaucoup d'octets sont des blocs ou du texte : leur corruption donne
    // encore un fichier lisible. Si rien ne se lisait, le test ne prouverait
    // que la solidité du refus.
    assert!(lus > 1_000, "{lus}");
}

/// **Une boîte géante est refusée AVANT d'allouer.**
#[test]
fn une_boite_geante_est_refusee_sans_allouer() {
    // 65 535 de côté — des shorts à −1 — et trois octets de données.
    let geant = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(-1)),
        ("Height", Tag::Short(-1)),
        ("Length", Tag::Short(-1)),
        ("Palette", c(vec![("minecraft:air", i(0))])),
        ("BlockData", Tag::ByteArray(vec![0, 0, 0])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("Schematic", &geant)), &mut Interner::new()),
        Err(Erreur::TropGros { .. })
    ));
    // Cent millions de cases — sous le plafond — et dix octets : la grille
    // n'est pas réservée, les données manquent.
    let creux = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(1000)),
        ("Height", Tag::Short(100)),
        ("Length", Tag::Short(1000)),
        ("Palette", c(vec![("minecraft:air", i(0))])),
        ("BlockData", Tag::ByteArray(vec![0; 10])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("Schematic", &creux)), &mut Interner::new()),
        Err(Erreur::Incoherent(_))
    ));
    // Deux régions d'une case, à vingt mille blocs l'une de l'autre.
    let region = |p: [i32; 3]| {
        c(vec![
            (
                "BlockStatePalette",
                Tag::List(vec![c(vec![("Name", s("minecraft:stone"))])]),
            ),
            ("BlockStates", Tag::LongArray(vec![0])),
            ("Position", xyz(p)),
            ("Size", xyz([1, 1, 1])),
        ])
    };
    let eloignees = c(vec![
        ("Version", i(6)),
        ("Metadata", c(vec![])),
        (
            "Regions",
            c(vec![
                ("a", region([0, 0, 0])),
                ("b", region([20_000, 0, 20_000])),
            ]),
        ),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("", &eloignees)), &mut Interner::new()),
        Err(Erreur::Incoherent(_))
    ));
    // Une structure qui annonce un kilomètre carré et ne décrit rien.
    let vide = c(vec![
        ("size", li([1000, 384, 1000])),
        ("palette", Tag::List(vec![])),
        ("blocks", Tag::List(vec![])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("", &vide)), &mut Interner::new()),
        Err(Erreur::TropDeCases { .. })
    ));
}

/// Un indice hors palette, des données en trop, une palette à trous : ce
/// sont des fichiers FAUX, et ils sont refusés plutôt que devinés.
#[test]
fn un_fichier_incoherent_est_refuse_pas_devine() {
    let base = |palette: Tag, data: Vec<u8>| {
        c(vec![
            ("Version", i(2)),
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(1)),
            ("Length", Tag::Short(1)),
            ("Palette", palette),
            ("BlockData", Tag::ByteArray(data)),
        ])
    };
    let deux = || c(vec![("minecraft:air", i(0)), ("minecraft:stone", i(1))]);
    for (quoi, t) in [
        ("indice hors palette", base(deux(), vec![0, 2])),
        ("données en trop", base(deux(), vec![0, 1, 1])),
        ("données manquantes", base(deux(), vec![0])),
        (
            "palette à trous",
            base(
                c(vec![("minecraft:air", i(0)), ("minecraft:stone", i(5))]),
                vec![0, 0],
            ),
        ),
        (
            "indice en double",
            base(
                c(vec![("minecraft:air", i(0)), ("minecraft:stone", i(0))]),
                vec![0, 0],
            ),
        ),
        ("palette vide", base(c(vec![]), vec![0, 0])),
    ] {
        assert!(
            matches!(
                lire(&gz(&nbt("Schematic", &t)), &mut Interner::new()),
                Err(Erreur::Incoherent(_))
            ),
            "{quoi}"
        );
    }
}

/// Un état illisible devient de l'air, et le compte rendu le NOMME.
#[test]
fn un_etat_illisible_devient_de_l_air_et_se_dit() {
    let t = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(2)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        (
            "Palette",
            c(vec![
                ("minecraft:stone", i(0)),
                ("minecraft:oak_log[axis=y", i(1)),
            ]),
        ),
        ("BlockData", Tag::ByteArray(vec![0, 1])),
    ]);
    let mut interner = Interner::new();
    let lu = lire(&gz(&nbt("Schematic", &t)), &mut interner).unwrap();
    assert_eq!(
        cles(&lu.presse, &interner),
        vec!["minecraft:stone", "minecraft:air"]
    );
    assert_eq!(
        lu.remarques,
        vec![tf_formats::Remarque::EtatsIllisibles(vec![
            "minecraft:oak_log[axis=y".into()
        ])]
    );
}

/// Un extrait trop grand pour ce que le format sait décrire est refusé à
/// l'ÉCRITURE — avec le nom du format, pas un fichier tronqué.
#[test]
fn un_extrait_trop_grand_pour_le_format_est_refuse() {
    let mut interner = Interner::new();
    let pierre = interner.intern("minecraft:stone");
    let long = tf_ops::Presse::uniforme([70_000, 1, 1], pierre);
    for format in [Format::SpongeV2, Format::SpongeV3] {
        assert!(matches!(
            ecrire(&long, format, &interner, &meta()),
            Err(Erreur::TropGrandPourLeFormat { .. })
        ));
    }
    assert!(ecrire(&long, Format::Litematic, &interner, &meta()).is_ok());
    let gros = tf_ops::Presse::uniforme([300, 300, 300], pierre);
    assert!(matches!(
        ecrire(&gros, Format::Structure, &interner, &meta()),
        Err(Erreur::TropDeCases { .. })
    ));
    // Un état que l'interner ne connaît pas ne s'écrit pas.
    let inconnu = tf_ops::Presse::uniforme([1, 1, 1], 999);
    assert_eq!(
        ecrire(&inconnu, Format::Litematic, &interner, &meta()),
        Err(Erreur::EtatInconnu(999))
    );
}

/// Un champ en DOUBLE — un NBT invalide, mais qui existe : le dernier gagne,
/// comme dans la table de hachage du jeu.
#[test]
fn un_champ_en_double_se_lit_comme_le_jeu_le_lit() {
    let t = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(1)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Width", Tag::Short(2)),
        ("Palette", c(vec![("minecraft:stone", i(0))])),
        ("BlockData", Tag::ByteArray(vec![0, 0])),
    ]);
    let lu = lire(&gz(&nbt("Schematic", &t)), &mut Interner::new()).unwrap();
    assert_eq!(lu.presse.taille, [2, 1, 1]);
}

/// Des block entities hors de la boîte, ou deux sur une même case : les
/// unes sont ignorées, des autres la dernière gagne — et les deux se DISENT.
#[test]
fn des_block_entities_mal_placees_se_disent() {
    let be = |p: [i32; 3], nom: &str| {
        c(vec![
            ("Id", s("minecraft:chest")),
            ("Pos", ia(p)),
            ("CustomName", s(nom)),
        ])
    };
    let t = c(vec![
        ("Version", i(2)),
        ("Width", Tag::Short(2)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Palette", c(vec![("minecraft:chest", i(0))])),
        ("BlockData", Tag::ByteArray(vec![0, 0])),
        (
            "BlockEntities",
            Tag::List(vec![
                be([0, 0, 0], "premier"),
                be([5, 0, 0], "dehors"),
                be([0, 0, 0], "second"),
                c(vec![("Id", s("minecraft:chest"))]), // sans position
            ]),
        ),
    ]);
    let lu = lire(&gz(&nbt("Schematic", &t)), &mut Interner::new()).unwrap();
    assert_eq!(lu.presse.entites.len(), 1);
    assert_eq!(
        arbre(&lu.presse.entites[0].octets()).get("CustomName"),
        Some(&s("second"))
    );
    assert_eq!(
        lu.remarques,
        vec![
            tf_formats::Remarque::BlockEntitiesIgnorees(2),
            tf_formats::Remarque::BlockEntitiesEnDouble(1),
        ]
    );
}

/// Des indices Litematica qui ne couvrent pas la région sont refusés — sans
/// quoi la lecture déborderait du tableau.
#[test]
fn des_indices_litematica_trop_courts_sont_refuses() {
    let region = c(vec![
        (
            "BlockStatePalette",
            Tag::List(vec![c(vec![("Name", s("minecraft:air"))])]),
        ),
        // 40 cases de 2 bits : il faut 2 longs.
        ("BlockStates", Tag::LongArray(vec![0])),
        ("Position", xyz([0, 0, 0])),
        ("Size", xyz([10, 2, 2])),
    ]);
    let t = c(vec![
        ("Version", i(6)),
        ("Metadata", c(vec![])),
        ("Regions", c(vec![("r", region)])),
    ]);
    assert!(matches!(
        lire(&gz(&nbt("", &t)), &mut Interner::new()),
        Err(Erreur::Incoherent(_))
    ));
}

/// Un `.schem` v1 — la première version de Sponge, sans `DataVersion`, dont
/// les block entities s'appellent encore `TileEntities` et portent un
/// `ContentVersion` qui n'appartient pas au jeu.
#[test]
fn un_schem_v1_se_lit() {
    let t = c(vec![
        ("Version", i(1)),
        (
            "Metadata",
            c(vec![
                ("WEOffsetX", i(-1)),
                ("WEOffsetY", i(0)),
                ("WEOffsetZ", i(-2)),
            ]),
        ),
        ("Width", Tag::Short(2)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Offset", ia([10, 20, 30])),
        ("PaletteMax", i(2)),
        (
            "Palette",
            c(vec![
                (
                    "minecraft:chest[facing=north,type=single,waterlogged=false]",
                    i(0),
                ),
                ("minecraft:air", i(1)),
            ]),
        ),
        ("BlockData", Tag::ByteArray(vec![0, 1])),
        (
            "TileEntities",
            Tag::List(vec![c(vec![
                ("ContentVersion", i(1)),
                ("Id", s("minecraft:chest")),
                ("Pos", ia([0, 0, 0])),
                ("Items", Tag::List(vec![])),
            ])]),
        ),
    ]);
    let lu = lire(&gz(&nbt("Schematic", &t)), &mut Interner::new()).unwrap();
    assert_eq!(lu.lecture, tf_formats::Lecture::Sponge { version: 1 });
    assert_eq!(lu.data_version, None);
    assert_eq!(lu.presse.ancre, [1, 0, 2]);
    let be = arbre(&lu.presse.entites[0].octets());
    assert!(be.get("ContentVersion").is_none(), "pas un champ du jeu");
    assert!(be.get("Items").is_some());
    assert_eq!(be.get("id"), Some(&s("minecraft:chest")));
}

/// Une entité dont les données ne portent PAS de position — un `.schem` v3
/// écrit par un autre outil — reçoit celle que le fichier donne à côté : sans
/// elle, le jeu la poserait à l'origine du monde.
#[test]
fn une_entite_sans_position_recoit_celle_du_fichier() {
    let t = c(vec![
        ("Version", i(3)),
        ("DataVersion", i(2975)),
        ("Width", Tag::Short(1)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        (
            "Blocks",
            c(vec![
                ("Palette", c(vec![("minecraft:air", i(0))])),
                ("Data", Tag::ByteArray(vec![0])),
            ]),
        ),
        (
            "Entities",
            Tag::List(vec![c(vec![
                ("Id", s("minecraft:armor_stand")),
                ("Pos", ld([0.5, 0.0, 0.25])),
                ("Data", c(vec![("Invisible", Tag::Byte(1))])),
            ])]),
        ),
    ]);
    let lu = lire(
        &gz(&nbt("", &c(vec![("Schematic", t)]))),
        &mut Interner::new(),
    )
    .unwrap();
    let m = &lu.presse.mobiles[0];
    assert_eq!(m.pos(), Some([0.5, 0.0, 0.25]));
    let a = arbre(&m.octets());
    assert_eq!(a.get("id"), Some(&s("minecraft:armor_stand")));
    assert_eq!(a.get("Invisible"), Some(&Tag::Byte(1)));
    assert_eq!(a.get("Pos"), Some(&ld([0.5, 0.0, 0.25])));
}
