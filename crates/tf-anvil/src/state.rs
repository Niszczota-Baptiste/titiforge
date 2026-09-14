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

    /// Absorbe une table locale et rend la TABLE DE CORRESPONDANCE de ses
    /// identifiants vers ceux-ci : `table[ancien as usize]` donne le nouveau.
    ///
    /// C'est la pièce que le décodage parallèle réclame. Un interner est de
    /// l'état mutable partagé : le mettre derrière un verrou sérialiserait
    /// exactement ce qu'on parallélise. Chaque fil interne donc dans sa propre
    /// table, et la fusion se fait après — mesuré à 1,06 ms pour une région
    /// pleine, soit 3,9 % du décodage, contre × 3,80 gagnés sur 4 cœurs.
    pub fn merge_from(&mut self, other: &Interner) -> Vec<StateId> {
        (0..other.len())
            .map(|i| {
                let key = other
                    .resolve(i as StateId)
                    .expect("un identifiant d'une table doit s'y résoudre");
                self.intern(key)
            })
            .collect()
    }

    /// Applique une table de correspondance à une palette, en place.
    ///
    /// Séparé de `merge_from` exprès : une table se calcule UNE fois par
    /// chunk, et s'applique à chacune de ses sections. Les refondre
    /// ensemble ferait recalculer la table par section.
    pub fn remap_palette(table: &[StateId], palette: &mut [StateId]) {
        for id in palette.iter_mut() {
            if let Some(&neuf) = table.get(*id as usize) {
                *id = neuf;
            }
        }
    }
}
