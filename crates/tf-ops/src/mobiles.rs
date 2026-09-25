//! Les ENTITÉS suivent les builds : cadres, tableaux, porte-armures, bêtes.
//!
//! Depuis 1.17 elles vivent dans `entities/`, un dossier à part : rien ne les
//! fait suivre les blocs, exactement comme le contenu d'un coffre. Déplacer un
//! bâtiment sans elles laisse ses cadres flotter à l'ancienne place, et le
//! copier laisse sa copie nue. C'est le piège « ce qui est ancré dans le monde
//! doit suivre les blocs qui le portent », au troisième type de donnée.
//!
//! Trois décisions, chacune pour une raison qu'on paierait sinon :
//!
//! 1. **Une copie reçoit de NOUVEAUX `UUID`.** Le jeu refuse une entité dont
//!    l'`UUID` existe déjà : il la jette au chargement, avec un simple
//!    avertissement dans son journal. Garder l'`UUID` d'une copie ferait donc
//!    disparaître l'original OU la copie, selon celui des deux chunks qui se
//!    charge en second — une perte silencieuse, et au hasard. Un déplacement,
//!    lui, garde le sien : c'est la même entité.
//! 2. **Le nouvel `UUID` se hache sur l'ancien et la POSITION d'arrivée**,
//!    jamais sur un générateur : rejouable, indépendant de l'ordre de
//!    parcours (invariant n° 5), et deux collages au même endroit rendent les
//!    mêmes entités — que la pose REMPLACE au lieu de les doubler, comme elle
//!    remplace un coffre sur sa case.
//! 3. **Une entité accrochée suit le bloc qui la PORTE**, pas la case où elle
//!    flotte. Un cadre sur la face extérieure d'un mur occupe une case HORS du
//!    bâtiment : le choisir par sa position laisserait tous les cadres de
//!    façade derrière un mur déplacé.
//!
//! Et une règle qui les tient toutes : **ce qu'on ne sait pas transformer est
//! NOMMÉ** (`Approche`), jamais deviné ni tu. Une pose de porte-armure
//! reflétée, un tableau de mod dont on ne connaît pas la largeur : ce sont
//! des limites, et elles se lisent dans le compte rendu.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use tf_anvil::chunk::{splice, Edit};
use tf_anvil::codec::{deflate_level, inflate};
use tf_anvil::mobiles::{balayer_chunk, chunk_neuf, edition_mobiles, ChunkMobiles, Corps, Mobile};
use tf_anvil::region::{external_file_name, read, write, Compression, RawChunk, Region};
use tf_blocks::Transfo;
use tf_nbt::Span;
use tf_world::coords::{BBox, BlockPos, ChunkPos, RegionPos};
use tf_world::journal::{ChunkPatch, Cible};
use tf_world::source::{Dimension, Folder, RegionSource, SourceError};
use tf_world::staging::{RegionStore, Staging};

use crate::edition::{chunks_de, regions_a_visiter, Erreur, RapportRegion, NIVEAU_STAGING};
use crate::presse::TransfoBoite;

// ── ce que les entités veulent dire ─────────────────────────────────────────

/// Le pas des directions 3D, dans l'ordre du jeu : bas, haut, nord, sud,
/// ouest, est — la valeur que `Facing` porte pour un cadre.
pub const PAS_3D: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// Le pas des directions 2D, dans l'ordre du jeu : sud, ouest, nord, est — la
/// valeur que `Facing` porte pour un tableau.
pub const PAS_2D: [[i32; 3]; 4] = [[0, 0, 1], [-1, 0, 0], [0, 0, -1], [1, 0, 0]];

/// Comment lire le `Facing` d'une entité. Le même octet ne veut pas dire la
/// même chose selon l'`id` : 2 est le NORD pour un cadre et le NORD pour un
/// tableau, mais 3 est le SUD pour l'un et l'EST pour l'autre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Genre {
    Trois,
    Deux,
}

fn genre(id: Option<&str>) -> Option<Genre> {
    match id? {
        "minecraft:item_frame" | "minecraft:glow_item_frame" => Some(Genre::Trois),
        "minecraft:painting" => Some(Genre::Deux),
        _ => None,
    }
}

fn pas_de(g: Genre, f: i8) -> Option<[i32; 3]> {
    let i = usize::try_from(f).ok()?;
    match g {
        Genre::Trois => PAS_3D.get(i).copied(),
        Genre::Deux => PAS_2D.get(i).copied(),
    }
}

/// La largeur, en blocs, d'un motif de tableau VANILLA (1.18 à 1.21).
///
/// Seule sa PARITÉ sert : le jeu décale d'un demi-bloc le centre d'un tableau
/// de largeur paire, vers la gauche de qui le regarde — et un miroir met la
/// gauche à droite. Un motif qu'on ne connaît pas rend `None`, et le tableau
/// est alors ANNONCÉ comme approché plutôt que décalé au jugé.
pub fn largeur_motif(motif: &str) -> Option<u32> {
    let nom = motif.strip_prefix("minecraft:")?;
    Some(match nom {
        "kebab" | "aztec" | "alban" | "aztec2" | "bomb" | "plant" | "wasteland" | "wanderer"
        | "graham" | "meditative" | "prairie_ride" => 1,
        "pool" | "courbet" | "sea" | "sunset" | "creebet" | "match" | "bust" | "stage" | "void"
        | "skull_and_roses" | "wither" | "earth" | "wind" | "water" | "fire" | "baroque"
        | "humble" => 2,
        "backyard" | "bouquet" | "cavebird" | "cotan" | "endboss" | "fern" | "owlemons"
        | "pond" | "sunflowers" | "tides" => 3,
        "fighters" | "pointer" | "pigscene" | "burning_skull" | "skeleton" | "donkey_kong"
        | "unpacked" | "changing" | "finding" | "lowmist" | "orb" | "passage" => 4,
        _ => return None,
    })
}

// ── les transformations, pures ──────────────────────────────────────────────

/// Un POINT continu d'une boîte de taille `taille`, après transformation.
///
/// La formule des CASES (`TransfoBoite::case_apres`), prise sur leurs bords :
/// la case `x` couvre `[x, x + 1)`, et son centre doit arriver au centre de la
/// case transformée. `sz − 1 − z` sur les cases devient donc `sz − z` sur les
/// points — l'oublier décalerait chaque entité d'un bloc par rapport à son
/// build, ce qui se lit « le porte-armure est dans le mur ».
pub fn position_apres(t: Transfo, [x, y, z]: [f64; 3], [sx, _, sz]: [u32; 3]) -> [f64; 3] {
    let (sx, sz) = (sx as f64, sz as f64);
    let (ax, az) = match t {
        Transfo::Rot90 => (sz - z, x),
        Transfo::Rot180 => (sx - x, sz - z),
        Transfo::Rot270 => (z, sx - x),
        Transfo::MiroirX => (sx - x, z),
        Transfo::MiroirZ => (x, sz - z),
    };
    [ax, y, az]
}

/// Un VECTEUR — une vitesse, une direction — après transformation : la même
/// chose sans la translation.
pub fn vecteur_apres(t: Transfo, [x, y, z]: [f64; 3]) -> [f64; 3] {
    let (ax, az) = match t {
        Transfo::Rot90 => (-z, x),
        Transfo::Rot180 => (-x, -z),
        Transfo::Rot270 => (z, -x),
        Transfo::MiroirX => (-x, z),
        Transfo::MiroirZ => (x, -z),
    };
    [ax, y, az]
}

fn pas_apres(t: Transfo, [x, y, z]: [i32; 3]) -> [i32; 3] {
    let (ax, az) = match t {
        Transfo::Rot90 => (-z, x),
        Transfo::Rot180 => (-x, -z),
        Transfo::Rot270 => (z, -x),
        Transfo::MiroirX => (-x, z),
        Transfo::MiroirZ => (x, -z),
    };
    [ax, y, az]
}

/// Le lacet d'une entité après transformation, en degrés, ramené dans
/// `[−180, 180)` comme le fait `Mth.wrapDegrees`.
///
/// Le repère du jeu : lacet 0 regarde le SUD (+Z), 90 l'OUEST (−X) — la
/// direction est `(−sin θ, cos θ)` sur `(x, z)`. Un quart de tour qui envoie
/// +X sur +Z ajoute donc 90° ; un miroir inverse le sens.
pub fn lacet_apres(t: Transfo, lacet: f32) -> f32 {
    let l = lacet as f64;
    let n = match t {
        Transfo::Rot90 => l + 90.0,
        Transfo::Rot180 => l + 180.0,
        Transfo::Rot270 => l + 270.0,
        Transfo::MiroirX => -l,
        Transfo::MiroirZ => 180.0 - l,
    };
    let r = n.rem_euclid(360.0);
    (if r >= 180.0 { r - 360.0 } else { r }) as f32
}

/// Une face 3D (`Facing` d'un cadre, `AttachFace` d'un shulker) après
/// transformation. `None` pour une valeur hors de 0..5, qui reste alors telle
/// quelle.
pub fn face3_apres(t: Transfo, f: i8) -> Option<i8> {
    let q = pas_apres(t, pas_de(Genre::Trois, f)?);
    PAS_3D.iter().position(|r| *r == q).map(|i| i as i8)
}

/// Une face 2D (`Facing` d'un tableau) après transformation.
pub fn face2_apres(t: Transfo, f: i8) -> Option<i8> {
    let q = pas_apres(t, pas_de(Genre::Deux, f)?);
    PAS_2D.iter().position(|r| *r == q).map(|i| i as i8)
}

/// La rotation d'un objet DANS son cadre (`ItemRotation`, 0..7), après
/// transformation du monde.
///
/// Tirée de la façon dont le jeu DESSINE un cadre (`ItemFrameRenderer`) : le
/// cadre est tourné selon sa face, puis l'objet de `r × 45°` dans son plan —
/// par quarts de tour pour une carte, qui ne lit que `r % 4`. D'où trois cas :
///
/// * au MUR, une rotation du monde emporte le cadre avec l'objet : rien ne
///   change ; un miroir inverse le sens de rotation ;
/// * au SOL, l'objet tourne autour de la verticale, dans le même sens que le
///   monde vu d'en haut ;
/// * au PLAFOND, dans le sens contraire — le cadre est retourné.
///
/// **Jamais vu en jeu à l'écriture de ces lignes** : dérivé du code de rendu,
/// vérifié contre lui par un calcul de matrices indépendant (test), et à
/// confirmer en posant un cadre au sol dans une vraie partie.
pub fn rotation_objet_apres(t: Transfo, r: i8, face: i8, carte: bool) -> i8 {
    let quart: i32 = if carte { 1 } else { 2 };
    let demi = 2 * quart;
    let r = r as i32;
    let n = match (face, t) {
        (2..=5, Transfo::MiroirX | Transfo::MiroirZ) => -r,
        (2..=5, _) => r,
        (1, Transfo::Rot90) | (0, Transfo::Rot270) => r + quart,
        (1, Transfo::Rot270) | (0, Transfo::Rot90) => r - quart,
        (0 | 1, Transfo::Rot180) => r + demi,
        (0 | 1, Transfo::MiroirX) => -r,
        (0 | 1, Transfo::MiroirZ) => demi - r,
        // Une face hors de 0..5 : on ne sait pas où est le cadre, on ne touche
        // pas à ce qu'il porte.
        _ => r,
    };
    n.rem_euclid(8) as i8
}

/// Ce qu'une transformation n'a pas su porter EXACTEMENT, nommé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approche {
    pub id: String,
    pub raison: &'static str,
}

/// Transforme une entité rangée en LOCAL dans une boîte de taille `taille`.
///
/// Tout ce qui la situe tourne ensemble — position, lacet, vitesse, case
/// d'accroche, face, souvenirs — et ses passagers avec elle. Ce qui ne se
/// transforme pas exactement est ajouté à `approches`.
pub fn transformer_mobile(
    m: &Mobile,
    t: Transfo,
    taille: [u32; 3],
    approches: &mut Vec<Approche>,
) -> Mobile {
    let mut n = m.clone();
    for k in &mut n.corps {
        let id = k.id.clone().unwrap_or_default();
        let g = genre(k.id.as_deref());
        let mut approche = |raison: &'static str| {
            approches.push(Approche {
                id: id.clone(),
                raison,
            })
        };

        if let Some(p) = &mut k.pos {
            p.v = position_apres(t, p.v, taille);
        }
        if let Some(r) = &mut k.rotation {
            r.v[0] = lacet_apres(t, r.v[0]);
        }
        if let Some(v) = &mut k.motion {
            v.v = vecteur_apres(t, v.v);
        }

        let face_avant = k.facing.map(|f| f.v);
        if let Some(f) = &mut k.facing {
            let neuve = match g {
                Some(Genre::Trois) => face3_apres(t, f.v),
                Some(Genre::Deux) => face2_apres(t, f.v),
                None => None,
            };
            match neuve {
                Some(v) => f.v = v,
                None => approche("orientation qu'on ne sait pas lire, laissée telle quelle"),
            }
        }
        if let Some(a) = &mut k.attache {
            match face3_apres(t, a.v) {
                Some(v) => a.v = v,
                None => approche("face d'accroche hors de 0..5, laissée telle quelle"),
            }
        }
        if let (Some(ro), Some(Genre::Trois), Some(f)) = (&mut k.rotation_objet, g, face_avant) {
            let carte = k.objet.as_deref() == Some("minecraft:filled_map");
            ro.v = rotation_objet_apres(t, ro.v, f, carte);
        }

        if let Some(tu) = &mut k.tuile {
            tu.v = t.point_apres(tu.v, taille);
            // Sous miroir, la gauche d'un tableau passe à droite : un tableau
            // de largeur PAIRE, dont le jeu décale le centre d'un demi-bloc
            // vers sa gauche, doit reculer son ancre d'un bloc pour couvrir
            // les mêmes cases. Une rotation, elle, emporte sa gauche avec lui.
            if t.est_miroir() && g == Some(Genre::Deux) {
                if let Some(f) = k.facing {
                    match k.motif.as_deref().and_then(largeur_motif) {
                        Some(l) if l % 2 == 0 => {
                            let gauche = PAS_2D[((f.v as usize) + 3) % 4];
                            tu.v[0] -= gauche[0];
                            tu.v[2] -= gauche[2];
                        }
                        Some(_) => {}
                        None => {
                            approche("motif de tableau inconnu : décalage de miroir non appliqué")
                        }
                    }
                }
            }
        }
        for r in &mut k.retenues {
            r.v = t.point_apres(r.v, taille);
        }
        if k.pose && t.est_miroir() {
            approche("pose de porte-armure : les membres ne sont pas reflétés");
        }
    }
    n
}

/// Un `UUID` neuf, haché sur l'ancien et la position d'ARRIVÉE.
///
/// Version 4 et variante IETF, comme `Mth.createInsecureUUID` : le jeu ne le
/// vérifie pas, mais un outil tiers qui lirait la save n'y verra rien
/// d'étrange.
pub fn uuid_derive(ancien: [i32; 4], pos: [f64; 3]) -> [i32; 4] {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    for x in ancien {
        h = melanger(h ^ (x as u32 as u64));
    }
    for x in pos {
        h = melanger(h ^ x.to_bits());
    }
    let hi = (h & !0xF000) | 0x4000;
    let lo = (melanger(h ^ 0xD1B5_4A32_D192_ED03) & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    [(hi >> 32) as i32, hi as i32, (lo >> 32) as i32, lo as i32]
}

/// La finale de splitmix64 : chaque bit d'entrée touche chaque bit de sortie.
fn melanger(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ── appartenance ────────────────────────────────────────────────────────────

/// La case d'une position, si elle en a une. Une position infinie ou `NaN`
/// n'est nulle part : `NaN as i32` vaut 0, et l'entité serait prise dans toute
/// sélection qui contient l'origine.
fn case_de(p: [f64; 3]) -> Option<[i32; 3]> {
    if !p.iter().all(|x| x.is_finite() && x.abs() < 1.0e9) {
        return None;
    }
    Some([
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    ])
}

/// La case qui décide si une entité est DANS une sélection.
///
/// Accrochée, c'est le bloc qui la PORTE : sa case reculée d'un pas vers le
/// mur (ou la clôture elle-même pour un nœud de laisse, qui n'a pas de face).
/// Sinon, la case où elle se tient — c'est ce que fait WorldEdit.
fn rattachement(k: &Corps) -> Option<[i32; 3]> {
    if let Some(t) = &k.tuile {
        let pas = match (genre(k.id.as_deref()), k.facing) {
            (Some(g), Some(f)) => pas_de(g, f.v).unwrap_or([0; 3]),
            _ => [0; 3],
        };
        return Some([t.v[0] - pas[0], t.v[1] - pas[1], t.v[2] - pas[2]]);
    }
    case_de(k.pos?.v)
}

fn dans(sel: &BBox, c: [i32; 3]) -> bool {
    sel.contains(BlockPos::new(c[0], c[1], c[2]))
}

/// Le chunk où une entité est rangée : celui de sa position, comme le fait le
/// jeu (`blockPosition`). Sa case d'accroche sert de repli à une entité
/// accrochée sans `Pos`.
fn chunk_de(k: &Corps) -> Option<ChunkPos> {
    let c = k
        .pos
        .and_then(|p| case_de(p.v))
        .or(k.tuile.as_ref().map(|t| t.v))?;
    Some(BlockPos::new(c[0], c[1], c[2]).chunk())
}

// ── monde ⇄ local ───────────────────────────────────────────────────────────

/// MONDE → LOCAL, au coin `o` de la sélection.
///
/// **Une position retenue ne suit que si le BLOC qu'elle désigne suit** : le
/// lit d'un villageois qu'on déplace avec sa maison, oui ; son poste de
/// travail resté dans l'atelier d'en face, non. Celles qui restent sont
/// retirées de la liste — elles gardent leurs octets, donc leurs coordonnées
/// monde, et désignent toujours le bon bloc.
fn vers_local(m: &mut Mobile, sel: &BBox, dimension: &str) {
    let o = [sel.min.x, sel.min.y, sel.min.z];
    for k in &mut m.corps {
        if let Some(p) = &mut k.pos {
            decaler_point(&mut p.v, o, -1);
        }
        if let Some(t) = &mut k.tuile {
            decaler_case(&mut t.v, o, -1);
        }
        k.retenues
            .retain(|r| dans(sel, r.v) && r.dimension.as_deref().is_none_or(|d| d == dimension));
        for r in &mut k.retenues {
            decaler_case(&mut r.v, o, -1);
        }
    }
}

/// LOCAL → MONDE, le coin de la boîte à `coin`.
fn vers_monde(m: &Mobile, coin: BlockPos) -> Mobile {
    let o = [coin.x, coin.y, coin.z];
    let mut n = m.clone();
    for k in &mut n.corps {
        if let Some(p) = &mut k.pos {
            decaler_point(&mut p.v, o, 1);
        }
        for c in k.tuile.iter_mut().chain(k.retenues.iter_mut()) {
            decaler_case(&mut c.v, o, 1);
        }
    }
    n
}

/// `p ± o`, pour une position continue.
fn decaler_point(p: &mut [f64; 3], o: [i32; 3], signe: i32) {
    for (x, d) in p.iter_mut().zip(o) {
        *x += (signe * d) as f64;
    }
}

/// `c ± o`, pour une case. En arithmétique modulaire : une case forgée à
/// `i32::MIN` ne doit pas faire paniquer l'opération d'un utilisateur — elle
/// n'est de toute façon pas dans la sélection, donc jamais relue.
fn decaler_case(c: &mut [i32; 3], o: [i32; 3], signe: i32) {
    for (x, d) in c.iter_mut().zip(o) {
        *x = x.wrapping_add(d.wrapping_mul(signe));
    }
}

/// Donne un `UUID` neuf à chaque entité d'un lot, passagers compris — et
/// fait suivre les laisses qui désignent une entité DU LOT.
///
/// Le lot entier d'abord, puis les références : un lama de marchand tenu par
/// son marchand doit l'être par la COPIE du marchand, pas par l'original resté
/// à l'autre bout du monde (le jeu casserait la laisse au premier pas).
fn renouveler(lot: &mut [Mobile]) {
    let mut vers: HashMap<[i32; 4], [i32; 4]> = HashMap::new();
    for m in lot.iter_mut() {
        for k in &mut m.corps {
            let (Some(u), Some(p)) = (&mut k.uuid, k.pos) else {
                continue;
            };
            let neuf = uuid_derive(u.v, p.v);
            vers.insert(u.v, neuf);
            u.v = neuf;
        }
    }
    for m in lot.iter_mut() {
        for k in &mut m.corps {
            if let Some(l) = &mut k.laisse_uuid {
                if let Some(n) = vers.get(&l.v) {
                    l.v = *n;
                }
            }
        }
    }
}

// ── relever ─────────────────────────────────────────────────────────────────

/// D'où vient une entité relevée : son chunk, et son rang dans la liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Source {
    chunk: ChunkPos,
    rang: usize,
}

/// Les entités de la sélection, en LOCAL, avec l'entrée d'où chacune vient.
///
/// Les chunks lus débordent d'un bloc : un cadre accroché à la face
/// extérieure du dernier mur de la sélection est rangé dans la case d'à côté,
/// donc parfois dans le chunk d'à côté.
fn relever<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    sel: &BBox,
) -> Result<Vec<(Mobile, Source)>, Erreur> {
    let large = BBox::new(
        BlockPos::new(sel.min.x - 1, sel.min.y - 1, sel.min.z - 1),
        BlockPos::new(sel.max.x + 1, sel.max.y + 1, sel.max.z + 1),
    );
    let dimension = dim.id();
    let mut out = Vec::new();
    for pos in regions_a_visiter(staging, dim, Folder::Entities, &large)? {
        let octets = match staging.read_region(dim, Folder::Entities, pos) {
            Ok(b) => b,
            Err(SourceError::NotFound) => continue,
            Err(e) => return Err(e.into()),
        };
        let mut region = read(&octets, pos.x, pos.z)?;
        for cpos in chunks_de(&large, pos) {
            let Some(avant) = inflater(staging, dim, &mut region, cpos)? else {
                continue;
            };
            let chunk = balayer_chunk(&avant)?;
            for (rang, e) in chunk.entrees.iter().enumerate() {
                let Some(c) = e.corps.first().and_then(rattachement) else {
                    continue;
                };
                if !dans(sel, c) {
                    continue;
                }
                let mut m = Mobile::depuis(&avant, e, chunk.data_version);
                vers_local(&mut m, sel, &dimension);
                out.push((m, Source { chunk: cpos, rang }));
            }
        }
    }
    // L'ordre du ramassage dépend du parcours des régions ; celui de l'extrait
    // ne doit dépendre de rien.
    out.sort_by_key(|(_, s)| (s.chunk.z, s.chunk.x, s.rang));
    Ok(out)
}

/// La charge inflatée d'un chunk, déportée comprise. `None` s'il n'existe pas.
fn inflater<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    region: &mut Region<'_>,
    cpos: ChunkPos,
) -> Result<Option<Vec<u8>>, Erreur> {
    let (lx, lz) = (cpos.x.rem_euclid(32), cpos.z.rem_euclid(32));
    let Some(brut) = region.get_mut(lx, lz) else {
        return Ok(None);
    };
    // Une charge déportée arrive VIDE : l'oublier ferait lire un chunk vide,
    // et la pose l'écraserait.
    if brut.needs_external() {
        let charge =
            staging.read_external(dim, Folder::Entities, &external_file_name(cpos.x, cpos.z))?;
        brut.resolve_external(charge);
    }
    if brut.payload.is_empty() {
        return Ok(None);
    }
    Ok(Some(inflate(&brut.payload, brut.compression)?))
}

/// `//copy` pour les entités : celles de la sélection, en LOCAL.
///
/// N'écrit rien, comme `//copy`.
pub fn copier_mobiles<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    sel: &BBox,
) -> Result<Vec<Mobile>, Erreur> {
    Ok(relever(staging, dim, sel)?
        .into_iter()
        .map(|(m, _)| m)
        .collect())
}

// ── poser ───────────────────────────────────────────────────────────────────

/// Une entité à poser, en MONDE, avec ses `UUID` définitifs.
struct Arrivee {
    mobile: Mobile,
    chunk: ChunkPos,
    /// Pour un déplacement : l'entrée à retirer, SI l'arrivée est acceptée.
    source: Option<Source>,
}

/// Pose les entités d'un extrait, son coin à `coin`, avec des `UUID` NEUFS.
///
/// C'est le pendant de `//paste` : l'original reste où il est, la copie est
/// une autre entité. Deux collages identiques rendent les mêmes `UUID`, et la
/// pose remplace alors au lieu de doubler.
pub fn poser_mobiles<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    mobiles: &[Mobile],
    coin: BlockPos,
) -> Result<RapportRegion, Erreur> {
    let mut lot: Vec<Mobile> = mobiles.iter().map(|m| vers_monde(m, coin)).collect();
    renouveler(&mut lot);
    let arrivees = lot
        .into_iter()
        .filter_map(|mobile| {
            let chunk = chunk_de(mobile.corps.first()?)?;
            Some(Arrivee {
                mobile,
                chunk,
                source: None,
            })
        })
        .collect();
    ecrire(staging, dim, arrivees)
}

/// `//move` pour les entités : celles de la sélection, décalées de `d`,
/// `UUID` GARDÉS — c'est la même entité.
///
/// Une entité qui ne peut pas être posée à l'arrivée (pas de terrain, ou un
/// chunk d'une autre version) RESTE à sa place : on ne retire une entrée que
/// si son arrivée est acceptée. La perdre en route serait pire que de la
/// laisser derrière, et le compte rendu le dit.
pub fn deplacer_mobiles<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    sel: &BBox,
    d: [i32; 3],
) -> Result<RapportRegion, Erreur> {
    let coin = BlockPos::new(sel.min.x + d[0], sel.min.y + d[1], sel.min.z + d[2]);
    let arrivees = relever(staging, dim, sel)?
        .into_iter()
        .filter_map(|(m, source)| {
            let mobile = vers_monde(&m, coin);
            let chunk = chunk_de(mobile.corps.first()?)?;
            Some(Arrivee {
                mobile,
                chunk,
                source: Some(source),
            })
        })
        .collect();
    ecrire(staging, dim, arrivees)
}

/// Pourquoi une arrivée est refusée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refus {
    /// Le chunk de BLOCS d'arrivée n'existe pas. Un collage n'engendre pas de
    /// terrain ; y poser une entité la ferait apparaître, le jour où le jeu
    /// le générera, debout dans une forêt qui n'a rien à voir.
    SansTerrain,
    /// Le chunk d'arrivée porte un autre `DataVersion` que celui d'où vient
    /// l'entité. Ses octets ont la forme de SA version ; les mêler à ceux d'une
    /// autre ferait sauter ou rejouer des conversions du jeu — en 1.20.5,
    /// celle de tous les objets.
    AutreVersion,
}

/// La jonction : décider, retirer, poser, journaliser.
///
/// Deux passes, parce qu'un déplacement ne retire une entrée que si son
/// arrivée est acceptée, et que l'arrivée peut être dans une autre région que
/// la source. La première lit et DÉCIDE ; la seconde écrit.
fn ecrire<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    arrivees: Vec<Arrivee>,
) -> Result<RapportRegion, Erreur> {
    let mut rap = RapportRegion::default();
    if arrivees.is_empty() {
        return Ok(rap);
    }

    // ── 1. décider
    let mut par_region: BTreeMap<RegionPos, Vec<usize>> = BTreeMap::new();
    for (i, a) in arrivees.iter().enumerate() {
        par_region.entry(a.chunk.region()).or_default().push(i);
    }
    let mut acceptees = vec![false; arrivees.len()];
    // Le `DataVersion` d'un chunk CRÉÉ : celui de sa première arrivée.
    let mut versions_neuves: HashMap<ChunkPos, i32> = HashMap::new();
    for (&pos, idx) in &par_region {
        let blocs = lire(staging, dim, Folder::Region, pos)?;
        let blocs = match &blocs {
            Some(b) => Some(read(b, pos.x, pos.z)?),
            None => None,
        };
        let ents = lire(staging, dim, Folder::Entities, pos)?;
        let mut ents = match &ents {
            Some(b) => read(b, pos.x, pos.z)?,
            None => Region::vide(pos.x, pos.z),
        };
        let mut versions: HashMap<ChunkPos, Option<Option<i32>>> = HashMap::new();
        for &i in idx {
            let a = &arrivees[i];
            let (lx, lz) = (a.chunk.x.rem_euclid(32), a.chunk.z.rem_euclid(32));
            let terrain = blocs.as_ref().is_some_and(|r| r.get(lx, lz).is_some());
            let refus = if !terrain {
                Some(Refus::SansTerrain)
            } else {
                // `Some(dv)` : le chunk existe et porte ce DataVersion.
                // `None` : il n'existe pas, on le créera.
                let v = match versions.get(&a.chunk) {
                    Some(v) => *v,
                    None => {
                        let v = match inflater(staging, dim, &mut ents, a.chunk)? {
                            Some(b) => Some(balayer_chunk(&b)?.data_version),
                            None => None,
                        };
                        versions.insert(a.chunk, v);
                        v
                    }
                };
                let dv = a.mobile.data_version;
                match v {
                    Some(existant) => (existant.is_some() && dv.is_some() && existant != dv)
                        .then_some(Refus::AutreVersion),
                    None => match (versions_neuves.get(&a.chunk), dv) {
                        (_, None) => Some(Refus::AutreVersion),
                        (Some(&premiere), Some(dv)) if premiere != dv => Some(Refus::AutreVersion),
                        (Some(_), Some(_)) => None,
                        (None, Some(dv)) => {
                            versions_neuves.insert(a.chunk, dv);
                            None
                        }
                    },
                }
            };
            match refus {
                None => acceptees[i] = true,
                Some(Refus::SansTerrain) => rap.mobiles_sans_terrain += 1,
                Some(Refus::AutreVersion) => rap.mobiles_autre_version += 1,
            }
        }
    }

    // ── 2. écrire
    // Ce que chaque chunk perd, reçoit à la place d'une entrée, et reçoit en plus.
    let mut retraits: BTreeSet<Source> = BTreeSet::new();
    let mut en_place: HashMap<Source, usize> = HashMap::new();
    let mut entrants: BTreeMap<ChunkPos, Vec<usize>> = BTreeMap::new();
    for (i, a) in arrivees.iter().enumerate() {
        if !acceptees[i] {
            continue;
        }
        rap.mobiles_poses += 1;
        match a.source {
            // **Une entité qui reste dans son chunk garde sa PLACE dans la
            // liste.** Ajoutée à la fin, elle réordonnerait la liste : mêmes
            // entrées, autres octets, donc un correctif pour zéro changement —
            // la faute que les coffres ont déjà payée.
            Some(s) if s.chunk == a.chunk => {
                rap.mobiles_retires += 1;
                en_place.insert(s, i);
            }
            Some(s) => {
                rap.mobiles_retires += 1;
                retraits.insert(s);
                entrants.entry(a.chunk).or_default().push(i);
            }
            None => entrants.entry(a.chunk).or_default().push(i),
        }
    }
    let mut chunks: BTreeSet<ChunkPos> = entrants.keys().copied().collect();
    chunks.extend(retraits.iter().map(|s| s.chunk));
    chunks.extend(en_place.keys().map(|s| s.chunk));
    let mut par_region: BTreeMap<RegionPos, Vec<ChunkPos>> = BTreeMap::new();
    for c in chunks {
        par_region.entry(c.region()).or_default().push(c);
    }

    for (pos, cs) in par_region {
        let octets = lire(staging, dim, Folder::Entities, pos)?;
        let mut region = match &octets {
            Some(b) => read(b, pos.x, pos.z)?,
            None => Region::vide(pos.x, pos.z),
        };
        let mut modifie = false;
        for c in cs {
            let index = c.index_in_region() as u16;
            let cible = Cible {
                dim: dim.clone(),
                folder: Folder::Entities,
                region: pos,
                chunk: index,
            };
            let entre: &[usize] = entrants.get(&c).map(Vec::as_slice).unwrap_or(&[]);
            let (avant, chunk) = match inflater(staging, dim, &mut region, c)? {
                Some(b) => {
                    let ch = balayer_chunk(&b)?;
                    (b, Some(ch))
                }
                None => (Vec::new(), None),
            };
            let voulues = liste_voulue(
                &avant,
                chunk.as_ref(),
                c,
                &arrivees,
                entre,
                &en_place,
                &retraits,
            );
            let (apres, edits) = match &chunk {
                Some(ch) => match edition_mobiles(&avant, ch, &voulues) {
                    Some(e) => {
                        let mut edits = vec![e];
                        (splice(&avant, &mut edits)?, edits)
                    }
                    None => continue,
                },
                None => {
                    if voulues.is_empty() {
                        continue;
                    }
                    // Le jeu ne garde pas de chunk d'entités vide : celui-ci
                    // naît de ce qu'on y pose. Le correctif part de RIEN, donc
                    // l'annuler le fait disparaître au lieu de laisser une
                    // coquille vide.
                    let dv = versions_neuves.get(&c).copied().unwrap_or_default();
                    let nbt = chunk_neuf(dv, c.x, c.z, &voulues);
                    let edits = vec![Edit {
                        span: Span { start: 0, end: 0 },
                        bytes: nbt.clone(),
                    }];
                    (nbt, edits)
                }
            };
            let patch = ChunkPatch::record(cible, &avant, &apres, &edits)?;
            let (lx, lz) = (c.x.rem_euclid(32), c.z.rem_euclid(32));
            let compression = region
                .get(lx, lz)
                .map(|b| b.compression)
                .unwrap_or(Compression::Zlib);
            let charge = Cow::Owned(deflate_level(&apres, compression, NIVEAU_STAGING)?);
            match region.get_mut(lx, lz) {
                Some(brut) => brut.payload = charge,
                None => {
                    region.slots[index as usize] = Some(RawChunk {
                        index,
                        timestamp: 0,
                        compression,
                        payload: charge,
                        external: false,
                    })
                }
            }
            rap.patches.push(patch);
            modifie = true;
        }
        if modifie {
            ecrire_region(staging, dim, pos, &region)?;
        }
    }
    Ok(rap)
}

/// La liste d'entités voulue pour le chunk `c`.
///
/// Dans l'ordre : chaque entrée existante reste à sa place — remplacée par
/// l'entité qui y revient (un déplacement dans le même chunk), ou par
/// l'arrivée qui porte son `UUID` (un second collage identique), ou retirée
/// si elle part ; puis les arrivées restantes, dans l'ordre du lot.
fn liste_voulue(
    avant: &[u8],
    chunk: Option<&ChunkMobiles>,
    c: ChunkPos,
    arrivees: &[Arrivee],
    entre: &[usize],
    en_place: &HashMap<Source, usize>,
    retraits: &BTreeSet<Source>,
) -> Vec<Vec<u8>> {
    // Rangs DANS `entre` : le lot peut compter des milliers d'arrivées, ce
    // chunk n'en reçoit que quelques-unes.
    let mut par_uuid: HashMap<[i32; 4], usize> = HashMap::new();
    for (j, &i) in entre.iter().enumerate() {
        if let Some(u) = arrivees[i].mobile.uuid() {
            par_uuid.entry(u).or_insert(j);
        }
    }
    let mut pris = vec![false; entre.len()];
    let mut voulues = Vec::new();
    if let Some(ch) = chunk {
        for (rang, e) in ch.entrees.iter().enumerate() {
            let s = Source { chunk: c, rang };
            if let Some(&i) = en_place.get(&s) {
                voulues.push(arrivees[i].mobile.octets());
                continue;
            }
            if retraits.contains(&s) {
                continue;
            }
            let uuid = e.corps.first().and_then(|k| k.uuid).map(|u| u.v);
            if let Some(j) = uuid.and_then(|u| par_uuid.remove(&u)) {
                pris[j] = true;
                voulues.push(arrivees[entre[j]].mobile.octets());
                continue;
            }
            voulues.push(e.span.slice(avant).to_vec());
        }
    }
    for (j, &i) in entre.iter().enumerate() {
        if !pris[j] {
            voulues.push(arrivees[i].mobile.octets());
        }
    }
    voulues
}

/// Une région du staging, ou `None` si elle n'existe nulle part.
fn lire<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    pos: RegionPos,
) -> Result<Option<Vec<u8>>, Erreur> {
    match staging.read_region(dim, folder, pos) {
        Ok(b) => Ok(Some(b)),
        Err(SourceError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn ecrire_region<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    pos: RegionPos,
    region: &Region<'_>,
) -> Result<(), Erreur> {
    let out = write(region)?;
    staging.write_region(dim, Folder::Entities, pos, &out.region)?;
    for f in out.external {
        staging.write_external(dim, Folder::Entities, &f.name, &f.bytes)?;
    }
    for n in out.removed_external {
        staging.remove_external(dim, Folder::Entities, &n)?;
    }
    Ok(())
}
