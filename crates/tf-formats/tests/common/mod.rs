//! Ce que les tests des formats partagent : un ÉCRIVAIN NBT indépendant — pour
//! produire les fichiers comme WorldEdit, Litematica et le jeu les écrivent —
//! un presse-papiers riche, et la comparaison de deux compounds à l'ordre des
//! champs près.
//!
//! L'écrivain construit un ARBRE puis le sérialise : la méthode opposée à
//! celle du crate, qui écrit au fil de l'eau et recopie des octets. Il ne doit
//! rien importer de `tf_formats` ni de `tf_nbt` — sinon un fichier « écrit par
//! WorldEdit » serait écrit par nous.

#![allow(dead_code)]

use std::io::{Read, Write};

use crate::frozen::{parse_nbt, Tag};
use tf_anvil::entites::Entite;
use tf_anvil::mobiles::Mobile;
use tf_anvil::{Interner, StateId};
use tf_bench::mobiles::{ecrire, Occupant, Trait, DV_1_18_2};
use tf_ops::Presse;

// ── un écrivain NBT indépendant ─────────────────────────────────────────────

fn id(t: &Tag) -> u8 {
    match t {
        Tag::End => 0,
        Tag::Byte(_) => 1,
        Tag::Short(_) => 2,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::Float(_) => 5,
        Tag::Double(_) => 6,
        Tag::ByteArray(_) => 7,
        Tag::Str(_) => 8,
        Tag::List(_) => 9,
        Tag::Compound(_) => 10,
        Tag::IntArray(_) => 11,
        Tag::LongArray(_) => 12,
    }
}

fn chaine(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u16).to_be_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn charge(o: &mut Vec<u8>, t: &Tag) {
    match t {
        Tag::End => {}
        Tag::Byte(v) => o.push(*v as u8),
        Tag::Short(v) => o.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => o.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => o.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => o.extend_from_slice(&v.to_be_bytes()),
        Tag::Double(v) => o.extend_from_slice(&v.to_be_bytes()),
        Tag::ByteArray(v) => {
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            o.extend_from_slice(v);
        }
        Tag::Str(s) => chaine(o, s),
        Tag::List(v) => {
            o.push(v.first().map_or(0, id));
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for e in v {
                charge(o, e);
            }
        }
        Tag::Compound(v) => {
            for (k, e) in v {
                o.push(id(e));
                chaine(o, k);
                charge(o, e);
            }
            o.push(0);
        }
        Tag::IntArray(v) => {
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                o.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                o.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

/// Un fichier NBT : racine nommée, NON compressé.
pub fn nbt(nom: &str, racine: &Tag) -> Vec<u8> {
    let mut o = vec![10];
    chaine(&mut o, nom);
    charge(&mut o, racine);
    o
}

/// Le même, en gzip — ce qu'écrivent les trois outils.
pub fn gz(octets: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(octets).unwrap();
    e.finish().unwrap()
}

/// Décompresse et relit par le décodeur GELÉ.
pub fn relire(octets: &[u8]) -> (String, Tag) {
    let mut nu = Vec::new();
    flate2::read::GzDecoder::new(octets)
        .read_to_end(&mut nu)
        .expect("l'écriture doit être du gzip");
    parse_nbt(&nu)
}

/// La charge d'un compound, `TAG_End` compris — ce que portent `Entite::nbt`
/// et `Mobile::nbt`.
pub fn charge_de(t: &Tag) -> Vec<u8> {
    let mut o = Vec::new();
    charge(&mut o, t);
    o
}

/// Un compound relu depuis sa charge.
pub fn arbre(charge: &[u8]) -> Tag {
    let mut b = vec![10, 0, 0];
    b.extend_from_slice(charge);
    parse_nbt(&b).1
}

// ── des raccourcis pour écrire des arbres ───────────────────────────────────

pub fn c(champs: Vec<(&str, Tag)>) -> Tag {
    Tag::Compound(
        champs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}
pub fn s(v: &str) -> Tag {
    Tag::Str(v.to_string())
}
pub fn i(v: i32) -> Tag {
    Tag::Int(v)
}
pub fn ia(v: [i32; 3]) -> Tag {
    Tag::IntArray(v.to_vec())
}
pub fn li(v: [i32; 3]) -> Tag {
    Tag::List(v.iter().map(|&x| Tag::Int(x)).collect())
}
pub fn ld(v: [f64; 3]) -> Tag {
    Tag::List(v.iter().map(|&x| Tag::Double(x)).collect())
}
pub fn xyz(v: [i32; 3]) -> Tag {
    c(vec![("x", i(v[0])), ("y", i(v[1])), ("z", i(v[2]))])
}

/// Un compound aux champs TRIÉS, récursivement : deux compounds égaux à
/// l'ordre des champs près le deviennent tout court. NBT ne donne aucun sens
/// à cet ordre.
pub fn canon(t: &Tag) -> Tag {
    match t {
        Tag::Compound(v) => {
            let mut v: Vec<_> = v.iter().map(|(k, t)| (k.clone(), canon(t))).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            Tag::Compound(v)
        }
        Tag::List(v) => Tag::List(v.iter().map(canon).collect()),
        autre => autre.clone(),
    }
}

// ── un presse-papiers riche ─────────────────────────────────────────────────

/// Des états de chaque sorte : l'air, des blocs nus, des blocs à propriétés,
/// un `minefield:*` — et assez de monde pour que Litematica range ses indices
/// À CHEVAL sur deux longs (40 états : 6 bits, et 64 n'est pas un multiple
/// de 6).
pub fn etats() -> Vec<String> {
    let mut v = vec![
        "minecraft:air".to_string(),
        "minecraft:stone".to_string(),
        "minecraft:oak_stairs|facing=east,half=bottom,shape=straight,waterlogged=false".into(),
        "minecraft:oak_stairs|facing=north,half=top,shape=outer_left,waterlogged=true".into(),
        "minecraft:chest|facing=west,type=single,waterlogged=false".into(),
        "minecraft:oak_sign|rotation=4,waterlogged=false".into(),
        "minefield:chaise_chene|facing=south".into(),
        "minefield:lanterne_ruines".into(),
    ];
    for couleur in [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ] {
        v.push(format!("minecraft:{couleur}_wool"));
        v.push(format!("minecraft:{couleur}_terracotta"));
    }
    v
}

pub struct Riche {
    pub interner: Interner,
    pub presse: Presse,
}

pub const TAILLE: [u32; 3] = [7, 5, 6];

/// Un hachage de position, pour un motif sans régularité.
fn melange(x: u32, y: u32, z: u32) -> u32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ z.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Une block entity comme `copier` les range : ses octets, ses coordonnées
/// réécrites à la case LOCALE.
pub fn block_entity(champs: Tag, case: [i32; 3]) -> Entite {
    let Tag::Compound(mut v) = champs else {
        panic!("un compound")
    };
    v.push(("x".into(), Tag::Int(case[0])));
    v.push(("y".into(), Tag::Int(case[1])));
    v.push(("z".into(), Tag::Int(case[2])));
    let nbt = charge_de(&Tag::Compound(v));
    let e = Entite::depuis_compound(nbt).unwrap().unwrap();
    assert_eq!(e.case, case);
    e
}

/// Une entité comme `copier` la range : en LOCAL, et sans les cases retenues
/// qui tombent hors de l'extrait — elles gardent leurs octets.
pub fn entite(o: &Occupant) -> Mobile {
    let mut m = Mobile::depuis_compound(ecrire(o), Some(DV_1_18_2)).unwrap();
    for k in &mut m.corps {
        k.retenues.retain(|r| {
            r.dimension.is_none() && (0..3).all(|a| r.v[a] >= 0 && (r.v[a] as u32) < TAILLE[a])
        });
    }
    m
}

pub fn riche() -> Riche {
    let mut interner = Interner::new();
    let ids: Vec<StateId> = etats().iter().map(|k| interner.intern(k)).collect();
    let mut blocs = Vec::new();
    for y in 0..TAILLE[1] {
        for z in 0..TAILLE[2] {
            for x in 0..TAILLE[0] {
                // Un quart d'air, le reste réparti sur tous les états.
                let h = melange(x, y, z);
                blocs.push(if h.is_multiple_of(4) {
                    ids[0]
                } else {
                    ids[(h as usize / 4) % ids.len()]
                });
            }
        }
    }
    let mut presse = Presse {
        taille: TAILLE,
        blocs,
        ancre: [-3, 1, 10],
        entites: Vec::new(),
        mobiles: Vec::new(),
    };
    // Le coffre et le panneau sont posés là où sont leurs blocs.
    let coffre = [2, 1, 3];
    let panneau = [4, 2, 0];
    let i = presse.index(2, 1, 3).unwrap();
    presse.blocs[i] = ids[4];
    let i = presse.index(4, 2, 0).unwrap();
    presse.blocs[i] = ids[5];
    presse.entites.push(block_entity(
        c(vec![
            ("id", s("minecraft:chest")),
            (
                "Items",
                Tag::List(vec![c(vec![
                    ("Slot", Tag::Byte(0)),
                    ("id", s("minecraft:diamond")),
                    ("Count", Tag::Byte(5)),
                    (
                        "tag",
                        c(vec![(
                            "display",
                            c(vec![("Name", s("{\"text\":\"Épée de 경비원\"}"))]),
                        )]),
                    ),
                ])]),
            ),
            ("Lock", s("")),
        ]),
        coffre,
    ));
    presse.entites.push(block_entity(
        c(vec![
            ("id", s("minecraft:sign")),
            ("Text1", s("{\"text\":\"Bienvenue à Minefield\"}")),
            ("Color", s("black")),
            ("GlowingText", Tag::Byte(0)),
        ]),
        panneau,
    ));
    presse
        .entites
        .sort_by_key(|e| (e.case[1], e.case[2], e.case[0]));

    // Un cadre au mur, un tableau PAIR, un porte-armure posé, un cochon monté
    // par un zombie, un villageois qui dort — son lit dans l'extrait, un
    // souvenir de poste de travail hors de lui.
    presse.mobiles = vec![
        entite(&Occupant::cadre(
            [1, 2, 4],
            3,
            "minecraft:map",
            3,
            [1, 1, 1, 1],
        )),
        entite(&Occupant::tableau(
            [6, 1, 2],
            1,
            "minecraft:skeleton",
            [2, 2, 2, 2],
        )),
        entite(
            &Occupant::nouveau("minecraft:armor_stand", [3.5, 1.0, 2.5], 45.0, [3, 3, 3, 3])
                .avec(Trait::Pose),
        ),
        entite(
            &Occupant::nouveau("minecraft:pig", [0.5, 1.0, 5.5], -90.0, [4, 4, 4, 4]).portant(
                Occupant::nouveau("minecraft:zombie", [0.5, 1.8, 5.5], -90.0, [5, 5, 5, 5]),
            ),
        ),
        entite(
            &Occupant::nouveau("minecraft:villager", [5.5, 1.5625, 4.5], 0.0, [6, 6, 6, 6])
                .avec(Trait::Dort([5, 1, 4]))
                .avec(Trait::Souvenir(
                    "minecraft:job_site",
                    [500, 64, 500],
                    "minecraft:overworld",
                )),
        ),
    ];
    Riche { interner, presse }
}

/// Les clés d'un extrait, case par case : la grille dans un repère
/// d'états qui ne dépend d'aucun interner.
pub fn cles(p: &Presse, interner: &Interner) -> Vec<String> {
    p.blocs
        .iter()
        .map(|&id| interner.resolve(id).unwrap().to_string())
        .collect()
}

/// Une block entity sous forme comparable : sa case, et son compound à
/// l'ordre des champs près.
pub fn be_canon(e: &Entite) -> ([i32; 3], Tag) {
    (e.case, canon(&arbre(&e.octets())))
}

/// Ce qui SITUE chaque corps d'une entité : id, position, case d'accroche,
/// face, cases retenues suivies.
pub type Situation = (
    Option<String>,
    Option<[f64; 3]>,
    Option<[i32; 3]>,
    Option<i8>,
    Vec<[i32; 3]>,
);

pub fn situation(m: &Mobile) -> Vec<Situation> {
    m.corps
        .iter()
        .map(|k| {
            (
                k.id.clone(),
                k.pos.map(|p| p.v),
                k.tuile.as_ref().map(|t| t.v),
                k.facing.map(|f| f.v),
                k.retenues.iter().map(|r| r.v).collect(),
            )
        })
        .collect()
}
