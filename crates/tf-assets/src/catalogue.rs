//! Du pack à la table que `tf-mesh` consomme.
//!
//! C'est ici que se décide, pour chaque bloc, s'il BOUCHE sa case. La règle a
//! été payée cher dans `ExeWorldEdit`, et elle n'est pas celle qu'on croit.

use std::collections::BTreeMap;

use serde_json::Value;
use tf_mesh::forme::Cuboide;

use crate::blockstates::Blockstate;
use crate::modele::{self, ModeleResolu};
use crate::source::{Id, Source};

/// Où un pack range ses fichiers.
///
/// Un pack Minecraft et le codex extrait du site ne nomment pas leurs fichiers
/// pareil. Passer par une fonction plutôt que par un chemin en dur, c'est ce
/// qui permet de lire les deux — et un `.jar` de mod demain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// `assets/<ns>/blockstates/<nom>.json`, `assets/<ns>/models/<chemin>.json`.
    Pack,
    /// Le codex du site : un seul `blockstates.json`, et
    /// `models/block_<nom>.json` puis `render-models/block_<nom>.json`.
    Codex,
}

impl Disposition {
    pub fn chemin_modele(self, id: &Id) -> String {
        match self {
            Disposition::Pack => id.modele(),
            // Le codex aplatit : `minefield:block/chaise` → `block_chaise.json`.
            Disposition::Codex => {
                let feuille = id.chemin.rsplit('/').next().unwrap_or(&id.chemin);
                format!("models/block_{feuille}.json")
            }
        }
    }

    /// Les chemins à essayer, dans l'ordre. Le codex en a deux dossiers, et
    /// c'est le second qui porte les modèles de rendu.
    pub fn chemins_modele(self, id: &Id) -> Vec<String> {
        match self {
            Disposition::Pack => vec![id.modele()],
            Disposition::Codex => {
                let feuille = id.chemin.rsplit('/').next().unwrap_or(&id.chemin);
                vec![
                    format!("render-models/block_{feuille}.json"),
                    format!("models/block_{feuille}.json"),
                ]
            }
        }
    }
}

/// Ce qu'un bloc oppose au mailleur.
///
/// La distinction n'est pas esthétique : un cube plein bouche sa case, donc
/// masque les faces de ses voisins et se fond dans un quad glouton ; tout le
/// reste doit être dessiné cuboïde par cuboïde et ne masque rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Classement {
    Cube,
    Modele,
    Vide,
}

/// **« Un cuboïde » n'est PAS le bon critère pour « c'est un cube ».**
///
/// `grass_block` en déclare DEUX — le cube, puis la couche d'herbe teintée sur
/// les côtés — et se retrouvait classé « modèle » dans `ExeWorldEdit`. Un
/// bloc-modèle n'est pas opaque : l'herbe ne cachait donc plus rien, et sur un
/// terrain chaque bloc SOUS la surface redevenait visible. Mesuré sur un build
/// réel : 3 957 chunks, 1 281 appels de dessin, dix secondes de maillage, et un
/// sol méconnaissable.
///
/// Le critère est la présence d'un cuboïde qui **REMPLIT** le bloc. Quatre
/// blocs vanilla sont concernés, mais l'un d'eux est la surface de tout
/// terrain.
pub fn classer(m: &ModeleResolu) -> Classement {
    if m.elements.is_empty() {
        return Classement::Vide;
    }
    if indice_cube_plein(&modele::cuboides(m)).is_some() {
        Classement::Cube
    } else {
        Classement::Modele
    }
}

/// L'indice du cuboïde qui remplit le bloc. À égalité, celui qui déclare le
/// plus de faces — c'est le vrai cube, pas la couche posée par-dessus.
pub fn indice_cube_plein(cuboides: &[Cuboide]) -> Option<usize> {
    cuboides
        .iter()
        .enumerate()
        .filter(|(_, c)| c.remplit())
        .max_by_key(|(_, c)| c.faces.count_ones())
        .map(|(i, _)| i)
}

/// Un pack lu : les blockstates, et les modèles résolus.
pub struct Catalogue {
    pub disposition: Disposition,
    blockstates: BTreeMap<String, Blockstate>,
    modeles: BTreeMap<Id, ModeleResolu>,
    /// Les modèles qu'on n'a pas su résoudre, pour que le trou se VOIE.
    pub introuvables: Vec<Id>,
}

impl Catalogue {
    pub fn new(disposition: Disposition) -> Self {
        Catalogue {
            disposition,
            blockstates: BTreeMap::new(),
            modeles: BTreeMap::new(),
            introuvables: Vec::new(),
        }
    }

    /// Charge le `blockstates.json` unique du codex.
    pub fn charger_codex<S: Source + ?Sized>(&mut self, src: &S) -> Result<usize, String> {
        let octets = src.lire("blockstates.json").map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&octets).map_err(|e| e.to_string())?;
        let o = v.as_object().ok_or("blockstates.json n'est pas un objet")?;
        let mut n = 0;
        for (nom, val) in o {
            if let Some(b) = Blockstate::depuis_json(val) {
                self.blockstates.insert(nom.clone(), b);
                n += 1;
            }
        }
        Ok(n)
    }

    /// Charge le blockstate d'UN bloc, disposition `Pack`.
    pub fn charger_bloc<S: Source + ?Sized>(&mut self, src: &S, nom: &str) -> Result<(), String> {
        let id = Id::parse(nom);
        let octets = src.lire(&id.blockstate()).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&octets).map_err(|e| e.to_string())?;
        let b = Blockstate::depuis_json(&v).ok_or("ni variants ni multipart")?;
        self.blockstates.insert(nom.to_string(), b);
        Ok(())
    }

    pub fn blocs(&self) -> impl Iterator<Item = (&String, &Blockstate)> {
        self.blockstates.iter()
    }

    pub fn blockstate(&self, nom: &str) -> Option<&Blockstate> {
        self.blockstates.get(nom)
    }

    pub fn nb_blocs(&self) -> usize {
        self.blockstates.len()
    }

    pub fn nb_modeles(&self) -> usize {
        self.modeles.len()
    }

    /// Résout tous les modèles cités par les blockstates chargés.
    ///
    /// Les modèles sont partagés : un pack déclare `oak_slab` deux fois (haute
    /// et basse) et vingt escaliers renvoient au même parent. Les résoudre par
    /// ÉTAT les relirait des milliers de fois.
    pub fn resoudre_modeles<S: Source + ?Sized>(&mut self, src: &S) {
        let mut voulus: Vec<Id> = self
            .blockstates
            .values()
            .flat_map(|b| b.modeles())
            .map(|v| v.modele.clone())
            .collect();
        voulus.sort();
        voulus.dedup();
        for id in voulus {
            if self.modeles.contains_key(&id) {
                continue;
            }
            let mut trouve = None;
            for chemin in self.disposition.chemins_modele(&id) {
                let f = move |_: &Id| chemin.clone();
                if let Ok(m) = modele::resoudre(src, &id, &f) {
                    trouve = Some(m);
                    break;
                }
            }
            match trouve {
                Some(m) => {
                    self.modeles.insert(id, m);
                }
                None => self.introuvables.push(id),
            }
        }
    }

    pub fn modele(&self, id: &Id) -> Option<&ModeleResolu> {
        self.modeles.get(id)
    }

    /// Le premier modèle d'un bloc — celui qui décide de sa forme.
    pub fn modele_de(&self, nom: &str) -> Option<&ModeleResolu> {
        let b = self.blockstates.get(nom)?;
        b.modeles().iter().find_map(|v| self.modeles.get(&v.modele))
    }

    /// La forme d'un bloc, pour le mailleur.
    pub fn classement(&self, nom: &str) -> Option<Classement> {
        self.modele_de(nom).map(classer)
    }

    /// Recense les formes du catalogue. Sert à vérifier qu'un chargement n'a
    /// pas laissé de trous : **un recensement qui laisse 21 % de trous ne dit
    /// rien**, et c'est arrivé.
    pub fn recensement(&self) -> BTreeMap<Classement, usize> {
        let mut out = BTreeMap::new();
        for nom in self.blockstates.keys() {
            if let Some(c) = self.classement(nom) {
                *out.entry(c).or_insert(0) += 1;
            }
        }
        out
    }
}

/// Découpe une clé d'état `nom|k=v,k=v` en son nom et ses propriétés.
///
/// C'est la clé d'identité du dépôt entier (`tf_anvil::state_key`). Une
/// seconde règle ici ferait chercher le même bloc sous deux noms.
pub fn decouper(cle: &str) -> (&str, Vec<(String, String)>) {
    match cle.split_once('|') {
        None => (cle, Vec::new()),
        Some((nom, props)) => (
            nom,
            props
                .split(',')
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    }
}

/// Bâtit la table que `tf-mesh` consomme, dans l'ordre de l'interner.
///
/// `translucide` décide si un cube plein BOUCHE vraiment sa case. Un pack ne
/// le dit pas : le verre remplit son bloc et ne doit pourtant masquer personne.
/// La réponse est dans l'ALPHA de sa texture, que ce crate ne lit pas encore —
/// d'où cette couture, plutôt qu'une liste de noms en dur qui ne couvrirait
/// aucun bloc `minefield:*`.
pub fn table_formes(
    cat: &Catalogue,
    cles: impl Iterator<Item = String>,
    translucide: &dyn Fn(&str) -> bool,
) -> tf_mesh::TableFormes {
    let mut t = tf_mesh::TableFormes::new();
    for cle in cles {
        let (nom, etat) = decouper(&cle);
        if nom == "minecraft:air" || nom == "minecraft:cave_air" || nom == "minecraft:void_air" {
            t.pousser(true, false, Vec::new());
            continue;
        }
        let Some(bs) = cat.blockstate(nom) else {
            // Un bloc que le pack ne connaît pas : ni air, ni opaque, ni
            // géométrie. Le supposer opaque effacerait des faces réelles.
            t.pousser(false, false, Vec::new());
            continue;
        };
        // L'état décide du MODÈLE : un escalier tourné n'a pas la même
        // géométrie, et prendre le premier modèle déclaré les dessinerait tous
        // dans la même direction.
        let mut cub: Vec<Cuboide> = Vec::new();
        let mut plein = false;
        for v in bs.pour(&etat) {
            let Some(m) = cat.modele(&v.modele) else {
                continue;
            };
            let c = modele::cuboides(m);
            plein |= indice_cube_plein(&c).is_some();
            cub.extend(c);
        }
        if cub.is_empty() {
            t.pousser(true, false, Vec::new());
        } else if plein && !translucide(nom) {
            // Un cube plein opaque passe par la passe gloutonne, qui n'a pas
            // besoin de sa géométrie.
            t.pousser(false, true, Vec::new());
        } else {
            t.pousser(false, false, cub);
        }
    }
    t
}
