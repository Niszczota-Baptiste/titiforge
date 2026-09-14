//! `blockstates/*.json` : quel modèle pour quel état.
//!
//! Deux formes, et **sauter la seconde fausse tout recensement** : mesuré sur
//! le pack du serveur, un tiers des blocs `minefield:*` déclarent leurs modèles
//! en `multipart`. En ne lisant que `variants`, 353 blocs sur 1 678 restaient
//! non résolus, et la part de blocs-modèles sortait à 50 % au lieu de 66,8 %.

use serde_json::Value;

use crate::source::Id;

/// Un modèle posé, avec sa rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Variante {
    pub modele: Id,
    /// Rotation autour de X, en degrés (0, 90, 180, 270).
    pub x: u16,
    /// Rotation autour de Y.
    pub y: u16,
    /// Les uv suivent la rotation du bloc au lieu de tourner avec lui.
    pub uvlock: bool,
    /// Poids d'un tirage aléatoire, quand plusieurs modèles sont proposés.
    pub poids: u32,
}

impl Variante {
    fn depuis_json(v: &Value) -> Option<Variante> {
        Some(Variante {
            modele: Id::parse(v.get("model")?.as_str()?),
            x: v.get("x").and_then(|n| n.as_u64()).unwrap_or(0) as u16,
            y: v.get("y").and_then(|n| n.as_u64()).unwrap_or(0) as u16,
            uvlock: v.get("uvlock").and_then(|b| b.as_bool()).unwrap_or(false),
            poids: v.get("weight").and_then(|n| n.as_u64()).unwrap_or(1) as u32,
        })
    }

    /// Une liste, ou un objet seul. Les deux sont valides dans un pack.
    fn liste(v: &Value) -> Vec<Variante> {
        match v {
            Value::Array(a) => a.iter().filter_map(Variante::depuis_json).collect(),
            _ => Variante::depuis_json(v).into_iter().collect(),
        }
    }
}

/// Ce qu'un `blockstates/*.json` déclare.
#[derive(Debug, Clone, PartialEq)]
pub enum Blockstate {
    /// `"facing=north,half=top"` → des modèles. La clé vide est le cas sans
    /// propriété.
    Variants(Vec<(String, Vec<Variante>)>),
    /// Des règles qui s'ajoutent : un mur, une clôture, une vitre reliée.
    Multipart(Vec<Regle>),
}

/// Une règle `multipart` : « quand telles propriétés, ajouter ces modèles ».
#[derive(Debug, Clone, PartialEq)]
pub struct Regle {
    /// Conditions `propriété → valeurs acceptées`. Vide = toujours.
    /// Plusieurs valeurs séparées par `|`, comme dans le format.
    pub quand: Vec<(String, Vec<String>)>,
    /// `OR` : la règle s'applique si l'une des alternatives passe.
    pub ou: Vec<Vec<(String, Vec<String>)>>,
    pub modeles: Vec<Variante>,
}

fn conditions(v: &Value) -> Vec<(String, Vec<String>)> {
    let Some(o) = v.as_object() else {
        return Vec::new();
    };
    o.iter()
        .filter(|(k, _)| *k != "OR" && *k != "AND")
        .filter_map(|(k, val)| {
            let s = match val {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some((k.clone(), s.split('|').map(str::to_string).collect()))
        })
        .collect()
}

impl Blockstate {
    pub fn depuis_json(v: &Value) -> Option<Blockstate> {
        if let Some(o) = v.get("variants").and_then(|x| x.as_object()) {
            return Some(Blockstate::Variants(
                o.iter()
                    .map(|(k, val)| (k.clone(), Variante::liste(val)))
                    .collect(),
            ));
        }
        if let Some(a) = v.get("multipart").and_then(|x| x.as_array()) {
            return Some(Blockstate::Multipart(
                a.iter()
                    .map(|r| {
                        let quand_val = r.get("when");
                        Regle {
                            quand: quand_val.map(conditions).unwrap_or_default(),
                            ou: quand_val
                                .and_then(|w| w.get("OR"))
                                .and_then(|o| o.as_array())
                                .map(|a| a.iter().map(conditions).collect())
                                .unwrap_or_default(),
                            modeles: r.get("apply").map(Variante::liste).unwrap_or_default(),
                        }
                    })
                    .collect(),
            ));
        }
        None
    }

    /// Tous les modèles cités, dans l'ordre de déclaration.
    ///
    /// Sert au recensement d'un catalogue : on veut savoir de quoi un bloc est
    /// fait, sans avoir à résoudre chaque état.
    pub fn modeles(&self) -> Vec<&Variante> {
        match self {
            Blockstate::Variants(v) => v.iter().flat_map(|(_, m)| m.iter()).collect(),
            Blockstate::Multipart(r) => r.iter().flat_map(|x| x.modeles.iter()).collect(),
        }
    }

    /// Les modèles qui s'appliquent à un état donné.
    ///
    /// `etat` est la liste `(propriété, valeur)` du bloc. En `variants`, la clé
    /// qui correspond ; en `multipart`, toutes les règles qui passent.
    pub fn pour(&self, etat: &[(String, String)]) -> Vec<&Variante> {
        match self {
            Blockstate::Variants(v) => {
                for (cle, modeles) in v {
                    if cle_correspond(cle, etat) {
                        return modeles.iter().collect();
                    }
                }
                // Aucune clé ne correspond : on prend la première déclarée
                // plutôt que rien. Un bloc sans modèle est invisible, ce qui
                // est pire qu'un bloc dans la mauvaise orientation.
                v.first()
                    .map(|(_, m)| m.iter().collect())
                    .unwrap_or_default()
            }
            Blockstate::Multipart(regles) => regles
                .iter()
                .filter(|r| regle_passe(r, etat))
                .flat_map(|r| r.modeles.iter())
                .collect(),
        }
    }
}

/// `"facing=north,half=top"` contre l'état. La clé VIDE correspond à tout.
fn cle_correspond(cle: &str, etat: &[(String, String)]) -> bool {
    if cle.is_empty() {
        return true;
    }
    cle.split(',').all(|p| {
        let Some((k, v)) = p.split_once('=') else {
            return false;
        };
        etat.iter().any(|(ek, ev)| ek == k && ev == v)
    })
}

fn conditions_passent(c: &[(String, Vec<String>)], etat: &[(String, String)]) -> bool {
    c.iter().all(|(k, valeurs)| {
        etat.iter()
            .any(|(ek, ev)| ek == k && valeurs.iter().any(|v| v == ev))
    })
}

fn regle_passe(r: &Regle, etat: &[(String, String)]) -> bool {
    if !r.ou.is_empty() && !r.ou.iter().any(|c| conditions_passent(c, etat)) {
        return false;
    }
    conditions_passent(&r.quand, etat)
}
