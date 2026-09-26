//! **Ce qu'on écrit se relit — par nous, et par un décodeur qui ne nous doit
//! rien.**
//!
//! L'aller-retour seul prouverait que l'écriture et la lecture s'accordent
//! entre elles ; c'est pourquoi chaque fichier est AUSSI relu par le décodeur
//! gelé de `tf-anvil` et confronté à ce que dit la spécification de chaque
//! format — le repère de WorldEdit, les indices à cheval de Litematica, la
//! liste de blocs du jeu. Deux erreurs indépendantes ont peu de chances de
//! coïncider.

mod common;
#[path = "../../tf-anvil/tests/common/frozen.rs"]
mod frozen;

use common::*;
use frozen::Tag;
use tf_anvil::Interner;
use tf_bench::mobiles::DV_1_18_2;
use tf_formats::{ecrire, lire, Format, Lecture, Meta, Remarque};

fn meta() -> Meta {
    Meta {
        data_version: DV_1_18_2,
        nom: "Porte de Mosslorn".into(),
        auteur: "Titi".into(),
        description: "la porte nord, avec son coffre".into(),
        date_ms: 1_758_844_800_000,
    }
}

/// Ce qu'un format ne peut pas garder, et donc ce que l'aller-retour perd :
/// l'ancre pour le `.nbt` ; les cases retenues pour les formats qui écrivent
/// le reste d'une entité dans le repère d'un AUTRE monde.
fn perd(format: Format) -> (bool, bool) {
    match format {
        Format::SpongeV2 | Format::SpongeV3 => (false, false),
        Format::Litematic => (false, true),
        Format::Structure => (true, true),
    }
}

/// **Chaque format rend ce qu'il a reçu** — la grille case par case, les
/// block entities à l'ordre de leurs champs près, et tout ce qui SITUE chaque
/// entité : position, case d'accroche, face, passagers. Relu avec un interner
/// NEUF : aucun numéro d'état ne traverse, seules les clés.
#[test]
fn chaque_format_rend_ce_qu_il_a_recu() {
    let r = riche();
    for format in Format::TOUS {
        let ecrit = ecrire(&r.presse, format, &r.interner, &meta()).unwrap();
        let mut neuf = Interner::new();
        let lu = lire(&ecrit.octets, &mut neuf).unwrap();
        let (sans_ancre, sans_souvenirs) = perd(format);

        assert_eq!(lu.presse.taille, r.presse.taille, "{format:?}");
        assert_eq!(
            cles(&lu.presse, &neuf),
            cles(&r.presse, &r.interner),
            "{format:?} : la grille"
        );
        if sans_ancre {
            assert_eq!(lu.presse.ancre, [0; 3]);
            assert_eq!(ecrit.remarques, vec![Remarque::AncrePerdue], "{format:?}");
        } else {
            assert_eq!(lu.presse.ancre, r.presse.ancre, "{format:?} : l'ancre");
            assert_eq!(ecrit.remarques, vec![], "{format:?}");
        }
        assert_eq!(lu.data_version, Some(DV_1_18_2), "{format:?}");

        let avant: Vec<_> = r.presse.entites.iter().map(be_canon).collect();
        let apres: Vec<_> = lu.presse.entites.iter().map(be_canon).collect();
        assert_eq!(apres, avant, "{format:?} : les block entities");

        assert_eq!(lu.presse.mobiles.len(), r.presse.mobiles.len());
        for (a, b) in r.presse.mobiles.iter().zip(&lu.presse.mobiles) {
            let mut attendu = situation(a);
            if sans_souvenirs {
                for s in &mut attendu {
                    s.4.clear();
                }
            }
            assert_eq!(situation(b), attendu, "{format:?} : {:?}", a.id());
            assert_eq!(
                canon(&arbre(&b.octets())),
                canon(&arbre(&a.octets())),
                "{format:?} : les octets de {:?}",
                a.id()
            );
            assert_eq!(b.data_version, Some(DV_1_18_2));
        }
        let souvenirs: usize = r
            .presse
            .mobiles
            .iter()
            .flat_map(|m| &m.corps)
            .map(|k| k.retenues.len())
            .sum();
        assert_eq!(souvenirs, 1, "le lit du villageois, et lui seul");
        // Le poste de travail, hors de l'extrait, était déjà laissé tel quel
        // dans le presse-papiers : la lecture le DIT. Sans repère connu, le
        // lit l'est aussi.
        let laisses = if sans_souvenirs { 2 } else { 1 };
        assert_eq!(
            lu.remarques,
            vec![Remarque::SouvenirsLaisses(laisses)],
            "{format:?}"
        );
        if format != Format::Structure {
            assert_eq!(lu.nom.as_deref(), Some("Porte de Mosslorn"), "{format:?}");
            assert_eq!(lu.auteur.as_deref(), Some("Titi"), "{format:?}");
        }
    }
}

/// **Écrire deux fois le même extrait donne les mêmes octets** — pas de date
/// dans l'en-tête gzip, pas d'ordre de table de hachage dans la palette.
#[test]
fn l_ecriture_est_deterministe() {
    let r = riche();
    for format in Format::TOUS {
        let a = ecrire(&r.presse, format, &r.interner, &meta()).unwrap();
        let b = ecrire(&r.presse, format, &r.interner, &meta()).unwrap();
        assert_eq!(a.octets, b.octets, "{format:?}");
    }
}

// ── relu par le décodeur gelé ───────────────────────────────────────────────

/// Une clé d'état depuis une chaîne de palette Sponge — à la main, sans le
/// code du crate. Nos palettes sont écrites triées.
fn cle_sponge(s: &str) -> String {
    match s.split_once('[') {
        None => s.to_string(),
        Some((n, p)) => format!("{n}|{}", p.trim_end_matches(']')),
    }
}

/// Une clé d'état depuis un compound `{Name, Properties}`.
fn cle_compound(t: &Tag) -> String {
    let nom = t.get("Name").unwrap().as_str().unwrap().to_string();
    match t.get("Properties") {
        None => nom,
        Some(Tag::Compound(p)) => {
            let mut v: Vec<String> = p
                .iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().unwrap()))
                .collect();
            v.sort();
            format!("{nom}|{}", v.join(","))
        }
        _ => panic!("Properties n'est pas un compound"),
    }
}

/// Des varints Sponge, relus à la main.
fn varints(b: &[u8]) -> Vec<u32> {
    let (mut out, mut v, mut dec) = (Vec::new(), 0u32, 0);
    for &o in b {
        v |= ((o & 0x7f) as u32) << dec;
        if o & 0x80 == 0 {
            out.push(v);
            v = 0;
            dec = 0;
        } else {
            dec += 7;
        }
    }
    assert_eq!(dec, 0, "un varint inachevé en fin de données");
    out
}

/// `LitematicaBitArray.getAt`, porté ligne à ligne depuis le Java.
fn get_at(longs: &[i64], bits: u32, index: u64) -> u32 {
    let start_offset = index * bits as u64;
    let start_arr = (start_offset >> 6) as usize;
    let end_arr = (((index + 1) * bits as u64 - 1) >> 6) as usize;
    let start_bit = (start_offset & 0x3f) as u32;
    let max = (1u64 << bits) - 1;
    if start_arr == end_arr {
        ((longs[start_arr] as u64 >> start_bit) & max) as u32
    } else {
        let end_offset = 64 - start_bit;
        (((longs[start_arr] as u64 >> start_bit) | ((longs[end_arr] as u64) << end_offset)) & max)
            as u32
    }
}

fn nombre(t: &Tag) -> i64 {
    match t {
        Tag::Byte(v) => *v as i64,
        Tag::Short(v) => *v as i64,
        Tag::Int(v) => *v as i64,
        Tag::Long(v) => *v,
        autre => panic!("pas un nombre : {autre:?}"),
    }
}

fn triplet(t: &Tag) -> [i32; 3] {
    match t {
        Tag::IntArray(v) => [v[0], v[1], v[2]],
        Tag::List(v) => [0, 1, 2].map(|k| nombre(&v[k]) as i32),
        Tag::Compound(_) => ["x", "y", "z"].map(|k| nombre(t.get(k).unwrap()) as i32),
        autre => panic!("pas un triplet : {autre:?}"),
    }
}

fn doubles(t: &Tag) -> [f64; 3] {
    let v = t.as_list().unwrap();
    [0, 1, 2].map(|k| match &v[k] {
        Tag::Double(d) => *d,
        autre => panic!("pas un double : {autre:?}"),
    })
}

/// **Un `.schem` v2 est ce que WorldEdit 7.2 attend** : la racine nommée
/// `Schematic`, des dimensions en shorts, le coin à l'origine et l'ancre dans
/// `WEOffset`, les block entities À PLAT avec `Id` et `Pos` relatif, les
/// entités à plat avec une position ABSOLUE — qui vaut la locale, puisque le
/// coin est à zéro.
#[test]
fn un_schem_v2_est_ce_que_worldedit_7_2_attend() {
    let r = riche();
    let ecrit = ecrire(&r.presse, Format::SpongeV2, &r.interner, &meta()).unwrap();
    let (nom, s) = relire(&ecrit.octets);
    assert_eq!(nom, "Schematic");
    assert_eq!(s.get("Version"), Some(&Tag::Int(2)));
    assert_eq!(s.get("DataVersion"), Some(&Tag::Int(DV_1_18_2)));
    assert_eq!(s.get("Width"), Some(&Tag::Short(7)));
    assert_eq!(s.get("Height"), Some(&Tag::Short(5)));
    assert_eq!(s.get("Length"), Some(&Tag::Short(6)));
    assert_eq!(triplet(s.get("Offset").unwrap()), [0, 0, 0]);
    let m = s.get("Metadata").unwrap();
    // L'origine de WorldEdit est `Offset − WEOffset` : l'ancre.
    let we = ["WEOffsetX", "WEOffsetY", "WEOffsetZ"].map(|k| nombre(m.get(k).unwrap()) as i32);
    assert_eq!(we.map(|v| -v), r.presse.ancre);

    let Tag::Compound(palette) = s.get("Palette").unwrap() else {
        panic!()
    };
    assert_eq!(s.get("PaletteMax"), Some(&Tag::Int(palette.len() as i32)));
    let mut par_indice = vec![String::new(); palette.len()];
    for (k, v) in palette {
        par_indice[nombre(v) as usize] = cle_sponge(k);
    }
    let data = varints(s.get("BlockData").unwrap().as_bytes().unwrap());
    let grille: Vec<String> = data
        .iter()
        .map(|&v| par_indice[v as usize].clone())
        .collect();
    assert_eq!(grille, cles(&r.presse, &r.interner));

    let bes = s.get("BlockEntities").unwrap().as_list().unwrap();
    assert_eq!(bes.len(), 2);
    for (be, e) in bes.iter().zip(&r.presse.entites) {
        assert_eq!(triplet(be.get("Pos").unwrap()), e.case);
        assert!(be.get("Id").is_some());
        for absent in ["id", "x", "y", "z"] {
            assert!(
                be.get(absent).is_none(),
                "{absent} ne doit pas rester à plat"
            );
        }
    }
    let ents = s.get("Entities").unwrap().as_list().unwrap();
    for (t, m) in ents.iter().zip(&r.presse.mobiles) {
        assert_eq!(t.get("Id").unwrap().as_str(), m.id());
        assert!(t.get("id").is_none());
        assert_eq!(doubles(t.get("Pos").unwrap()), m.pos().unwrap());
    }
}

/// **Un `.schem` v3 est ce que WorldEdit 7.3 attend** : une racine anonyme, le
/// compound `Schematic` dedans, `Offset` = −ancre et `WorldEdit.Origin` =
/// +ancre — le coin tombe à zéro —, les blocs sous `Blocks`, et les données
/// des block entities et des entités IMBRIQUÉES sous `Data`.
#[test]
fn un_schem_v3_est_ce_que_worldedit_7_3_attend() {
    let r = riche();
    let ecrit = ecrire(&r.presse, Format::SpongeV3, &r.interner, &meta()).unwrap();
    let (nom, racine) = relire(&ecrit.octets);
    assert_eq!(nom, "");
    let s = racine.get("Schematic").unwrap();
    assert_eq!(s.get("Version"), Some(&Tag::Int(3)));
    let offset = triplet(s.get("Offset").unwrap());
    let origine = triplet(
        s.get("Metadata")
            .unwrap()
            .get("WorldEdit")
            .unwrap()
            .get("Origin")
            .unwrap(),
    );
    assert_eq!(origine, r.presse.ancre);
    assert_eq!([0, 1, 2].map(|a| offset[a] + origine[a]), [0; 3], "le coin");
    let blocs = s.get("Blocks").unwrap();
    let Tag::Compound(palette) = blocs.get("Palette").unwrap() else {
        panic!()
    };
    let mut par_indice = vec![String::new(); palette.len()];
    for (k, v) in palette {
        par_indice[nombre(v) as usize] = cle_sponge(k);
    }
    let data = varints(blocs.get("Data").unwrap().as_bytes().unwrap());
    let grille: Vec<String> = data
        .iter()
        .map(|&v| par_indice[v as usize].clone())
        .collect();
    assert_eq!(grille, cles(&r.presse, &r.interner));
    for (be, e) in blocs
        .get("BlockEntities")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .zip(&r.presse.entites)
    {
        assert_eq!(triplet(be.get("Pos").unwrap()), e.case);
        let d = be.get("Data").unwrap();
        assert!(d.get("Items").is_some() || d.get("Text1").is_some());
    }
    for (t, m) in s
        .get("Entities")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .zip(&r.presse.mobiles)
    {
        assert_eq!(t.get("Id").unwrap().as_str(), m.id());
        assert_eq!(doubles(t.get("Pos").unwrap()), m.pos().unwrap());
        let d = t.get("Data").unwrap();
        assert!(d.get("id").is_none());
        assert!(
            d.get("UUID").is_some(),
            "le reste de l'entité voyage dans Data"
        );
    }
}

/// **Un `.litematic` est ce que Litematica attend** : l'air à l'indice 0, des
/// indices À CHEVAL sur les longs — relus par le `getAt` de Litematica porté
/// ligne à ligne —, la longueur exacte du tableau, les métadonnées qu'affiche
/// son navigateur, et la région qui commence à −ancre.
#[test]
fn un_litematic_est_ce_que_litematica_attend() {
    let r = riche();
    let ecrit = ecrire(&r.presse, Format::Litematic, &r.interner, &meta()).unwrap();
    let (nom, t) = relire(&ecrit.octets);
    assert_eq!(nom, "");
    assert_eq!(t.get("Version"), Some(&Tag::Int(6)));
    assert_eq!(t.get("SubVersion"), Some(&Tag::Int(1)));
    assert_eq!(t.get("MinecraftDataVersion"), Some(&Tag::Int(DV_1_18_2)));
    let m = t.get("Metadata").unwrap();
    let volume: usize = 7 * 5 * 6;
    let pleins = cles(&r.presse, &r.interner)
        .iter()
        .filter(|k| k.as_str() != "minecraft:air")
        .count();
    assert_eq!(m.get("RegionCount"), Some(&Tag::Int(1)));
    assert_eq!(m.get("TotalVolume"), Some(&Tag::Int(volume as i32)));
    assert_eq!(m.get("TotalBlocks"), Some(&Tag::Int(pleins as i32)));
    assert_eq!(triplet(m.get("EnclosingSize").unwrap()), [7, 5, 6]);
    assert_eq!(m.get("Name").unwrap().as_str(), Some("Porte de Mosslorn"));

    let Tag::Compound(regions) = t.get("Regions").unwrap() else {
        panic!()
    };
    assert_eq!(regions.len(), 1);
    let reg = &regions[0].1;
    assert_eq!(
        triplet(reg.get("Position").unwrap()),
        r.presse.ancre.map(|v| -v)
    );
    assert_eq!(triplet(reg.get("Size").unwrap()), [7, 5, 6]);
    let palette: Vec<String> = reg
        .get("BlockStatePalette")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .map(cle_compound)
        .collect();
    assert_eq!(palette[0], "minecraft:air", "l'air à l'indice 0");
    assert_eq!(palette.len(), etats().len());
    let bits = (32 - ((palette.len() - 1) as u32).leading_zeros()).max(2);
    assert_eq!(bits, 6, "le motif doit mettre des indices à cheval");
    let longs = reg.get("BlockStates").unwrap().as_longs().unwrap();
    assert_eq!(longs.len(), (volume * bits as usize).div_ceil(64));
    let grille: Vec<String> = (0..volume as u64)
        .map(|i| palette[get_at(longs, bits, i) as usize].clone())
        .collect();
    assert_eq!(grille, cles(&r.presse, &r.interner));

    // Block entities et entités sont en LOCAL : c'est le repère de la région.
    for (be, e) in reg
        .get("TileEntities")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .zip(&r.presse.entites)
    {
        assert_eq!(triplet(be), e.case);
        assert!(be.get("id").is_some());
    }
    for (t, m) in reg
        .get("Entities")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .zip(&r.presse.mobiles)
    {
        assert_eq!(doubles(t.get("Pos").unwrap()), m.pos().unwrap());
    }
    assert_eq!(
        reg.get("PendingBlockTicks")
            .unwrap()
            .as_list()
            .unwrap()
            .len(),
        0
    );
}

/// **Un `.nbt` est ce qu'un bloc de structure écrit** : une LISTE de blocs,
/// l'air compris, chacun avec sa case et son état ; la block entity d'une case
/// sans ses coordonnées mais avec son `id` ; les entités avec `pos`,
/// `blockPos` — la case d'ACCROCHE pour un tableau — et `nbt`.
#[test]
fn un_nbt_est_ce_qu_un_bloc_de_structure_ecrit() {
    let r = riche();
    let ecrit = ecrire(&r.presse, Format::Structure, &r.interner, &meta()).unwrap();
    let (_, t) = relire(&ecrit.octets);
    assert_eq!(triplet(t.get("size").unwrap()), [7, 5, 6]);
    assert_eq!(t.get("DataVersion"), Some(&Tag::Int(DV_1_18_2)));
    let palette: Vec<String> = t
        .get("palette")
        .unwrap()
        .as_list()
        .unwrap()
        .iter()
        .map(cle_compound)
        .collect();
    let blocs = t.get("blocks").unwrap().as_list().unwrap();
    assert_eq!(
        blocs.len(),
        7 * 5 * 6,
        "l'air est écrit : la structure creuse sa place"
    );
    let mut grille = vec![String::new(); blocs.len()];
    let mut avec_nbt = 0;
    for b in blocs {
        let [x, y, z] = triplet(b.get("pos").unwrap());
        let i = r.presse.index(x as u32, y as u32, z as u32).unwrap();
        grille[i] = palette[nombre(b.get("state").unwrap()) as usize].clone();
        if let Some(n) = b.get("nbt") {
            avec_nbt += 1;
            assert!(n.get("id").is_some());
            for absent in ["x", "y", "z"] {
                assert!(n.get(absent).is_none());
            }
        }
    }
    assert_eq!(grille, cles(&r.presse, &r.interner));
    assert_eq!(avec_nbt, 2);
    let ents = t.get("entities").unwrap().as_list().unwrap();
    assert_eq!(ents.len(), r.presse.mobiles.len());
    let tableau = ents
        .iter()
        .find(|e| e.get("nbt").unwrap().get("id").unwrap().as_str() == Some("minecraft:painting"))
        .unwrap();
    assert_eq!(
        triplet(tableau.get("blockPos").unwrap()),
        [6, 1, 2],
        "la case d'accroche"
    );
}

/// Une palette de TROIS CENTS états — neuf bits, qu'aucun motif courant
/// n'atteint — traverse aussi : les bornes du rangement à cheval ne sont pas
/// celles des petites palettes.
#[test]
fn une_grande_palette_traverse() {
    let mut interner = Interner::new();
    let ids: Vec<_> = (0..300)
        .map(|k| interner.intern(&format!("minefield:bloc_{k}")))
        .collect();
    let taille = [17, 3, 11];
    let blocs = (0..17 * 3 * 11)
        .map(|i: usize| ids[(i * 7919) % 300])
        .collect();
    let presse = tf_ops::Presse {
        taille,
        blocs,
        ancre: [0, 0, 0],
        entites: vec![],
        mobiles: vec![],
    };
    for format in Format::TOUS {
        let ecrit = ecrire(&presse, format, &interner, &meta()).unwrap();
        let mut neuf = Interner::new();
        let lu = lire(&ecrit.octets, &mut neuf).unwrap();
        assert_eq!(
            cles(&lu.presse, &neuf),
            cles(&presse, &interner),
            "{format:?}"
        );
    }
}

/// La lecture dit ce qu'elle a lu.
#[test]
fn la_lecture_dit_ce_qu_elle_a_lu() {
    let r = riche();
    let attendu = [
        (Format::SpongeV2, Lecture::Sponge { version: 2 }),
        (Format::SpongeV3, Lecture::Sponge { version: 3 }),
        (
            Format::Litematic,
            Lecture::Litematic {
                version: 6,
                regions: 1,
            },
        ),
        (Format::Structure, Lecture::Structure),
    ];
    for (format, lecture) in attendu {
        let ecrit = ecrire(&r.presse, format, &r.interner, &meta()).unwrap();
        let lu = lire(&ecrit.octets, &mut Interner::new()).unwrap();
        assert_eq!(lu.lecture, lecture);
    }
}

/// Un monde à COMPOSANTS (1.20.5 et suivants) s'écrit en `.litematic` v7 :
/// c'est ce qui dit à Litematica que les objets des coffres sont déjà sous la
/// nouvelle forme. La frontière est 1.20.5, `DataVersion` 3837 — un cran
/// avant, c'est encore la v6 que Litematica 1.18 sait ouvrir.
#[test]
fn un_monde_a_composants_s_ecrit_en_v7() {
    let r = riche();
    for (dv, version) in [(3836, 6), (3837, 7), (3953, 7)] {
        let mut m = meta();
        m.data_version = dv;
        let ecrit = ecrire(&r.presse, Format::Litematic, &r.interner, &m).unwrap();
        let (_, t) = relire(&ecrit.octets);
        assert_eq!(t.get("Version"), Some(&Tag::Int(version)), "{dv}");
        assert_eq!(t.get("MinecraftDataVersion"), Some(&Tag::Int(dv)));
    }
}
