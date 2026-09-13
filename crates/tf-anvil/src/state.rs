//! Identité d'un état de bloc, et son internement.
//!
//! Un état est un `u32`, jamais une chaîne. Comparer deux blocs devient un `==`
//! d'entier au lieu d'une fabrication de clé texte — le piège que `we-engine` a
//! payé deux fois : `propsKey` à 18 % du temps d'écriture, puis un `findIndex`
//! qui refabriquait la clé de CHAQUE entrée de palette à chaque bloc, à 44 %.

use std::collections::HashMap;

pub type StateId = u32;

/// Clé canonique d'un état : le nom seul s'il n'a pas de propriétés, sinon
/// `nom|k=v,k=v` **trié**.
///
/// Une seule règle dans tout le dépôt. Deux règles différentes pour le même
/// bloc dédoubleraient les palettes, et un `//replace` visant un état exact en
/// raterait la moitié — sans la moindre erreur à l'écran.
///
/// Les deux formes ne peuvent pas se confondre : un identifiant Minecraft est
/// `namespace:id` et ne contient jamais de barre verticale.
pub fn state_key(name: &str, props: &mut [(String, String)]) -> String {
    if props.is_empty() {
        return name.to_string();
    }
    props.sort_by(|a, b| a.0.cmp(&b.0));
    let extra: usize = props.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
    let mut s = String::with_capacity(name.len() + extra);
    s.push_str(name);
    s.push('|');
    for (i, (k, v)) in props.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(k);
        s.push('=');
        s.push_str(v);
    }
    s
}

/// Défait `state_key`. Rend `(nom, [(clé, valeur)])`.
///
/// Une propriété sans `=` est ignorée plutôt que devinée : mieux vaut perdre
/// une propriété malformée que d'en inventer une, parce qu'une propriété
/// inventée se réécrit dans la save de l'utilisateur.
pub fn split_key(key: &str) -> (&str, Vec<(String, String)>) {
    match key.split_once('|') {
        None => (key, Vec::new()),
        Some((name, rest)) => {
            let props = rest
                .split(',')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            (name, props)
        }
    }
}

/// Table d'internement des états. `intern` est idempotent.
#[derive(Default)]
pub struct Interner {
    map: HashMap<Box<str>, StateId>,
    names: Vec<Box<str>>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, key: &str) -> StateId {
        if let Some(&id) = self.map.get(key) {
            return id;
        }
        let id = u32::try_from(self.names.len()).expect("plus de 4 milliards d'états distincts");
        let boxed: Box<str> = key.into();
        self.names.push(boxed.clone());
        self.map.insert(boxed, id);
        id
    }

    /// Interne sans créer : sert à demander « cet état existe-t-il ? » sans
    /// polluer la table, ce qu'un `//replace` fait pour chaque section.
    pub fn get(&self, key: &str) -> Option<StateId> {
        self.map.get(key).copied()
    }

    pub fn resolve(&self, id: StateId) -> Option<&str> {
        self.names.get(id as usize).map(|b| &**b)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}
