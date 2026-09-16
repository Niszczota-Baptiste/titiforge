//! Les modèles d'un pack : `parent` résolu, `textures` résolues, cuboïdes.
//!
//! On ne peut PAS sauter au nom de texture. `grass_block` n'en a aucune qui
//! porte son nom, et surtout un bloc peut n'être pas un cube. La chaîne
//! complète `blockstates → models (parent) → textures` est la seule qui donne
//! la bonne géométrie et la bonne image.

use std::collections::BTreeMap;

use serde_json::Value;
use tf_mesh::forme::Face;
use tf_mesh::Cuboide;

use crate::source::{Id, Source, SourceError};

/// Un modèle tel qu'il est écrit, avant résolution.
#[derive(Debug, Clone, Default)]
pub struct Modele {
    pub parent: Option<Id>,
    /// `None` quand le modèle n'en déclare pas : il HÉRITE alors de son parent.
    /// Un `Some(vec![])` est différent — c'est un modèle qui déclare
    /// explicitement n'avoir aucun élément, et qui ne doit rien hériter.
    pub elements: Option<Vec<Element>>,
    /// `side` → `block_oak_log.png`, ou `#side` → `#texture`.
    pub textures: BTreeMap<String, String>,
    pub ambient_occlusion: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// En seizièmes de bloc. Minecraft autorise −16 à 32.
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub rotation: Option<Rotation>,
    pub shade: bool,
    pub faces: BTreeMap<Face, FaceDef>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub origine: [f32; 3],
    /// 0 = X, 1 = Y, 2 = Z.
    pub axe: usize,
    pub angle: f32,
    pub rescale: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FaceDef {
    /// **`None` n'est PAS `[0, 0, 0, 0]`.**
    ///
    /// Un modèle qui ne déclare pas les uv d'une face demande qu'on les
    /// DÉDUISE des bornes du cuboïde : c'est ce qui fait qu'une dalle montre la
    /// moitié basse de sa texture au lieu de la texture entière écrasée.
    /// Confondre les deux étire un point de texture sur toute la face.
    ///
    /// Mesuré sur le pack du serveur : **26,4 % des faces** ne déclarent pas
    /// d'uv. Ce n'est pas un cas limite.
    pub uv: Option<[f32; 4]>,
    /// `#side` ou `minecraft:block/stone`. Résolu par `resoudre`.
    pub texture: String,
    /// La face disparaît quand le voisin de ce côté est opaque. 28,2 % des
    /// faces du pack en portent une.
    pub cullface: Option<Face>,
    /// 0, 90, 180 ou 270.
    pub rotation: u16,
    /// Le jeu multiplie la texture par une couleur de biome. Sans lui, le sol
    /// de tout terrain sort BLANCHÂTRE — les textures teintées du jeu sont
    /// grises : `grass_block_top.png` vaut (147, 147, 147).
    pub tintindex: Option<i32>,
}

fn face_depuis(nom: &str) -> Option<Face> {
    Some(match nom {
        "west" => Face::MoinsX,
        "east" => Face::PlusX,
        "down" => Face::MoinsY,
        "up" => Face::PlusY,
        "north" => Face::MoinsZ,
        "south" => Face::PlusZ,
        _ => return None,
    })
}

/// Le nom Minecraft d'une face. Le repère : **+X = Est, +Z = Sud, +Y = Haut**.
pub const fn nom_de_face(f: Face) -> &'static str {
    match f {
        Face::MoinsX => "west",
        Face::PlusX => "east",
        Face::MoinsY => "down",
        Face::PlusY => "up",
        Face::MoinsZ => "north",
        Face::PlusZ => "south",
    }
}

fn triplet(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
    ])
}

fn quadruplet(v: &Value) -> Option<[f32; 4]> {
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    Some([
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
        a[3].as_f64()? as f32,
    ])
}

impl Modele {
    pub fn depuis_json(v: &Value) -> Modele {
        let mut m = Modele {
            parent: v.get("parent").and_then(|p| p.as_str()).map(Id::parse),
            ambient_occlusion: v.get("ambientocclusion").and_then(|b| b.as_bool()),
            ..Default::default()
        };
        if let Some(t) = v.get("textures").and_then(|t| t.as_object()) {
            for (k, val) in t {
                if let Some(s) = val.as_str() {
                    m.textures.insert(k.clone(), s.to_string());
                }
            }
        }
        if let Some(els) = v.get("elements").and_then(|e| e.as_array()) {
            m.elements = Some(els.iter().filter_map(element_depuis).collect());
        }
        m
    }
}

fn element_depuis(v: &Value) -> Option<Element> {
    let from = triplet(v.get("from")?)?;
    let to = triplet(v.get("to")?)?;
    let rotation = v.get("rotation").and_then(|r| {
        let axe = match r.get("axis")?.as_str()? {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            _ => return None,
        };
        Some(Rotation {
            origine: r.get("origin").and_then(triplet).unwrap_or([8.0, 8.0, 8.0]),
            axe,
            angle: r.get("angle")?.as_f64()? as f32,
            rescale: r.get("rescale").and_then(|b| b.as_bool()).unwrap_or(false),
        })
    });
    let mut faces = BTreeMap::new();
    if let Some(fs) = v.get("faces").and_then(|f| f.as_object()) {
        for (nom, fd) in fs {
            let Some(face) = face_depuis(nom) else {
                continue;
            };
            let Some(texture) = fd.get("texture").and_then(|t| t.as_str()) else {
                continue;
            };
            faces.insert(
                face,
                FaceDef {
                    uv: fd.get("uv").and_then(quadruplet),
                    texture: texture.to_string(),
                    cullface: fd
                        .get("cullface")
                        .and_then(|c| c.as_str())
                        .and_then(face_depuis),
                    rotation: fd.get("rotation").and_then(|r| r.as_u64()).unwrap_or(0) as u16,
                    tintindex: fd
                        .get("tintindex")
                        .and_then(|t| t.as_i64())
                        .map(|t| t as i32),
                },
            );
        }
    }
    Some(Element {
        from,
        to,
        rotation,
        shade: v.get("shade").and_then(|b| b.as_bool()).unwrap_or(true),
        faces,
    })
}

/// Un modèle dont la chaîne de parents est aplatie et les `#variables`
/// résolues.
#[derive(Debug, Clone, Default)]
pub struct ModeleResolu {
    pub elements: Vec<Element>,
    pub ambient_occlusion: bool,
    /// Ce qui a servi, pour le diagnostic.
    pub chaine: Vec<Id>,
}

/// Profondeur maximale d'une chaîne de parents.
///
/// Un pack vient du disque d'un utilisateur : `a` parent de `b` parent de `a`
/// ferait boucler la résolution jusqu'à la mort du processus, sans message.
/// La chaîne vanilla la plus longue fait quatre maillons.
pub const PROFONDEUR_MAX: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeleError {
    /// Le JSON ne se lit pas.
    Illisible(String),
    /// Chaîne de parents trop longue, ou cycle.
    Boucle(String),
    Source(SourceError),
}

impl std::fmt::Display for ModeleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModeleError::Illisible(n) => write!(f, "modèle illisible : {n}"),
            ModeleError::Boucle(n) => write!(
                f,
                "chaîne de parents trop longue ou circulaire, à partir de {n}"
            ),
            ModeleError::Source(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ModeleError {}

impl From<SourceError> for ModeleError {
    fn from(e: SourceError) -> Self {
        ModeleError::Source(e)
    }
}

/// Lit un modèle et résout sa chaîne.
///
/// `chemin` dit où chercher : un pack peut nommer ses modèles autrement qu'en
/// `assets/<ns>/models/<id>.json`, et le codex du site en est un exemple.
pub fn resoudre<S: Source + ?Sized>(
    src: &S,
    id: &Id,
    chemins: &dyn Fn(&Id) -> Vec<String>,
) -> std::result::Result<ModeleResolu, ModeleError> {
    let mut chaine = Vec::new();
    let mut elements: Option<Vec<Element>> = None;
    let mut textures: BTreeMap<String, String> = BTreeMap::new();
    let mut ao: Option<bool> = None;
    let mut courant = Some(id.clone());

    while let Some(c) = courant {
        if chaine.len() >= PROFONDEUR_MAX || chaine.contains(&c) {
            return Err(ModeleError::Boucle(c.to_string()));
        }
        // Les chemins du MAILLON COURANT, pas ceux de la racine. Un modèle
        // vanilla nomme son parent (`block/dirt` → `block/cube_all` →
        // `block/cube`), et chercher le parent à l'adresse de l'enfant relit
        // le même fichier — la chaîne se referme sur elle-même et se solde en
        // `Boucle`. Résultat : ZÉRO modèle résolu sur un vrai `.jar`, alors
        // que le codex, dont les modèles sont aplatis, ne montrait rien.
        //
        // Plusieurs chemins parce qu'un codex range ses modèles dans deux
        // dossiers ; le premier qui répond l'emporte.
        let candidats = chemins(&c);
        let octets = candidats
            .iter()
            .find_map(|p| src.lire(p).ok())
            .ok_or_else(|| {
                ModeleError::Source(SourceError::Absent(
                    candidats.first().cloned().unwrap_or_else(|| c.to_string()),
                ))
            })?;
        let v: Value =
            serde_json::from_slice(&octets).map_err(|_| ModeleError::Illisible(c.to_string()))?;
        let m = Modele::depuis_json(&v);
        chaine.push(c);

        // L'ENFANT gagne : on ne remplace que ce qui n'est pas déjà décidé.
        if elements.is_none() {
            elements = m.elements;
        }
        for (k, val) in m.textures {
            textures.entry(k).or_insert(val);
        }
        if ao.is_none() {
            ao = m.ambient_occlusion;
        }
        courant = m.parent;
    }

    let mut elements = elements.unwrap_or_default();
    for e in &mut elements {
        for fd in e.faces.values_mut() {
            fd.texture = resoudre_variable(&fd.texture, &textures);
        }
    }
    Ok(ModeleResolu {
        elements,
        ambient_occlusion: ao.unwrap_or(true),
        chaine,
    })
}

/// `#side` → la valeur de `side`, en suivant les renvois.
///
/// Un pack chaîne les variables : `#all` → `#texture` → `block/stone`. Sans
/// suivre, la face porterait le nom d'une autre variable et aucune texture ne
/// serait trouvée.
fn resoudre_variable(t: &str, textures: &BTreeMap<String, String>) -> String {
    let mut courant = t.to_string();
    for _ in 0..PROFONDEUR_MAX {
        let Some(cle) = courant.strip_prefix('#') else {
            return courant;
        };
        match textures.get(cle) {
            Some(v) if *v != courant => courant = v.clone(),
            // Une variable qui ne se résout pas reste telle quelle : la
            // remplacer par du vide ferait chercher une texture nommée « » et
            // le message dirait « texture absente : » au lieu de nommer la
            // variable fautive.
            _ => return courant,
        }
    }
    courant
}

/// Les cuboïdes d'un modèle, pour le mailleur.
///
/// C'est ici que se décide ce que `tf-mesh` verra, et ces deux règles sont
/// payées cher :
///
/// - **`cull` ne porte que les faces qui déclarent `cullface`**, pas toutes
///   celles qui sont à ras. Une face à ras sans `cullface` doit rester
///   dessinée quoi qu'il y ait à côté.
/// - **Une face non déclarée n'est pas dessinée.** Un modèle qui n'a que
///   `north` et `south` est une plante en croix, pas un cube.
pub fn cuboides(m: &ModeleResolu) -> Vec<Cuboide> {
    m.elements
        .iter()
        .map(|e| {
            let mut faces = 0u8;
            let mut cull = 0u8;
            for (f, fd) in &e.faces {
                faces |= f.bit();
                if fd.cullface == Some(*f) {
                    cull |= f.bit();
                }
            }
            // `from` et `to` peuvent être dans l'ordre inverse : un modèle
            // écrit à la main n'est pas obligé de les ranger.
            let mut min = e.from;
            let mut max = e.to;
            for k in 0..3 {
                if min[k] > max[k] {
                    std::mem::swap(&mut min[k], &mut max[k]);
                }
            }
            Cuboide {
                min,
                max,
                faces,
                cull,
            }
        })
        .collect()
}

/// Les uv d'une face, DÉDUITES du cuboïde quand le modèle n'en déclare pas.
///
/// C'est la règle que Minecraft applique : la face prend la portion de texture
/// qui correspond à sa position dans le bloc. Une dalle montre donc la moitié
/// basse de sa texture, et non la texture entière écrasée sur 8 seizièmes.
pub fn uv_de(e: &Element, f: Face, fd: &FaceDef) -> [f32; 4] {
    if let Some(uv) = fd.uv {
        return uv;
    }
    let (from, to) = (e.from, e.to);
    match f {
        Face::MoinsY | Face::PlusY => [from[0], from[2], to[0], to[2]],
        Face::MoinsZ | Face::PlusZ => [16.0 - to[0], 16.0 - to[1], 16.0 - from[0], 16.0 - from[1]],
        Face::MoinsX | Face::PlusX => [from[2], 16.0 - to[1], to[2], 16.0 - from[1]],
    }
}
