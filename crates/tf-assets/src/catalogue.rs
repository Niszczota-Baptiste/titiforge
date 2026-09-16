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
    /// Les chemins d'une TEXTURE, dans l'ordre. Un modèle la nomme
    /// `minefield:block/pierre` ou simplement `block_pierre.png`, selon le
    /// pack.
    pub fn chemins_texture(self, nom: &str) -> Vec<String> {
        let id = Id::parse(nom);
        match self {
            Disposition::Pack => vec![id.texture()],
            Disposition::Codex => {
                let feuille = id.chemin.rsplit('/').next().unwrap_or(&id.chemin);
                let feuille = feuille.strip_suffix(".png").unwrap_or(feuille);
                let nu = feuille.strip_prefix("block_").unwrap_or(feuille);
                vec![
                    format!("render-textures/block_{nu}.png"),
                    format!("model-textures/block_{nu}.png"),
                    format!("render-textures/{feuille}.png"),
                    format!("model-textures/{feuille}.png"),
                ]
            }
        }
    }

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

    /// Charge TOUT ce qu'un pack Minecraft déclare, namespace par namespace.
    ///
    /// Un pack n'a pas de `blockstates.json` global : chaque bloc a son
    /// fichier, sous `assets/<ns>/blockstates/`. On ne peut donc pas savoir
    /// d'avance quoi charger — il faut LISTER, et c'est la seule raison pour
    /// laquelle `Source` sait le faire.
    ///
    /// Les namespaces sortent de ce qui est là, jamais d'une liste écrite à la
    /// main : le pack d'un serveur en apporte un que personne n'a prévu, et
    /// c'est exactement le cas qui compte ici.
    pub fn charger_pack<S: Source + ?Sized>(&mut self, src: &S) -> Result<usize, String> {
        let mut n = 0;
        for chemin in src.lister("assets/") {
            let Some(reste) = chemin.strip_prefix("assets/") else {
                continue;
            };
            let mut morceaux = reste.splitn(3, '/');
            let (Some(ns), Some("blockstates"), Some(feuille)) =
                (morceaux.next(), morceaux.next(), morceaux.next())
            else {
                continue;
            };
            // `blockstates/` est plat dans un pack : un `/` de plus veut dire
            // qu'on regarde autre chose.
            let Some(nom) = feuille.strip_suffix(".json").filter(|f| !f.contains('/')) else {
                continue;
            };
            if self.charger_bloc(src, &format!("{ns}:{nom}")).is_ok() {
                n += 1;
            }
        }
        if n == 0 {
            return Err("aucun blockstate sous `assets/*/blockstates/`".into());
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
        let (forme, _) = forme_et_habillage(cat, None, None, &cle, translucide);
        t.pousser(forme.air, forme.opaque, forme.cuboides);
        let _ = &t;
    }
    t
}

/// Ce qu'un état oppose au mailleur, avant d'entrer dans la table.
pub struct Forme {
    pub air: bool,
    pub opaque: bool,
    pub cuboides: Vec<Cuboide>,
}

/// La forme d'un état ET l'habillage de chacun de ses cuboïdes, **d'un seul
/// parcours**.
///
/// Les deux se déduisent de la même liste de variantes, dans le même ordre :
/// `habillage.cuboides[i]` habille `forme.cuboides[i]`. Les produire
/// séparément laisserait ce couplage implicite — « le n-ième de l'un va avec
/// le n-ième de l'autre » — et c'est le genre de contrat qui se casse à la
/// première optimisation, sans bruit.
///
/// `atlas` et `teintes` sont facultatifs : le mailleur n'a aucun besoin de
/// savoir qu'un atlas existe, et les benchs le construisent sans pack.
fn forme_et_habillage(
    cat: &Catalogue,
    atlas: Option<&crate::atlas::Atlas>,
    teintes: Option<&crate::apparence::Teintes>,
    cle: &str,
    translucide: &dyn Fn(&str) -> bool,
) -> (Forme, crate::apparence::Habillage) {
    use crate::apparence::{Apparence, Habillage};
    let vide = |air: bool| {
        (
            Forme {
                air,
                opaque: false,
                cuboides: Vec::new(),
            },
            Habillage::default(),
        )
    };
    let (nom, etat) = decouper(cle);
    if nom == "minecraft:air" || nom == "minecraft:cave_air" || nom == "minecraft:void_air" {
        return vide(true);
    }
    let Some(bs) = cat.blockstate(nom) else {
        // Un bloc que le pack ne connaît pas : ni air, ni opaque, ni
        // géométrie. Le supposer opaque effacerait des faces réelles.
        return vide(false);
    };
    // L'état décide du MODÈLE : un escalier tourné n'a pas la même géométrie,
    // et prendre le premier modèle déclaré les dessinerait tous dans la même
    // direction.
    //
    // Et le modèle ne suffit pas : un pack ne décrit pas seize escaliers, il
    // en décrit UN et le TOURNE. La rotation vit sur la variante, et l'oublier
    // dessinait tous les escaliers d'un build vers l'est — relevé sur une
    // vraie save, avec un `mushroom_stem` dont les six parts se superposaient
    // en un seul plan, à lui seul 38 % de la passe de modèles.
    let couleur = teintes.map(|t| t.pour(nom));
    let mut cub: Vec<Cuboide> = Vec::new();
    let mut hab: Vec<[Apparence; 6]> = Vec::new();
    let mut plein = false;
    let mut cube = [Apparence::default(); 6];
    for v in bs.pour(&etat) {
        let Some(m) = cat.modele(&v.modele) else {
            continue;
        };
        let a = crate::rotation::axes(v.x, v.y);
        let c = crate::rotation::tourner(modele::cuboides(m), v.x, v.y);
        if let Some(i) = indice_cube_plein(&c) {
            if !plein {
                // Le premier cuboïde qui REMPLIT la case habille le cube. « Le
                // premier élément » prendrait la couche d'herbe transparente de
                // `grass_block` au lieu du cube lui-même.
                if let (Some(atlas), Some(e)) = (atlas, m.elements.get(i)) {
                    cube = crate::apparence::habiller(e, a, atlas, couleur);
                }
            }
            plein = true;
        }
        if let Some(atlas) = atlas {
            for e in &m.elements {
                hab.push(crate::apparence::habiller(e, a, atlas, couleur));
            }
        }
        cub.extend(c);
    }
    if cub.is_empty() {
        return vide(true);
    }
    if plein && !translucide(nom) {
        // Un cube plein opaque passe par la passe gloutonne, qui n'a pas
        // besoin de sa géométrie.
        return (
            Forme {
                air: false,
                opaque: true,
                cuboides: Vec::new(),
            },
            Habillage {
                cube,
                cuboides: Vec::new(),
            },
        );
    }
    debug_assert!(
        atlas.is_none() || hab.len() == cub.len(),
        "un habillage par cuboïde, sinon la correspondance est fausse"
    );
    (
        Forme {
            air: false,
            opaque: false,
            cuboides: cub,
        },
        Habillage {
            cube,
            cuboides: hab,
        },
    )
}

/// La table du mailleur ET l'habillage, d'un seul parcours.
///
/// C'est ce que consomme le rendu : le premier dit ce qu'un bloc OPPOSE, le
/// second à quoi il RESSEMBLE, et les deux sont indexés par `StateId`.
pub fn table_rendu(
    cat: &Catalogue,
    atlas: &crate::atlas::Atlas,
    teintes: &crate::apparence::Teintes,
    cles: impl Iterator<Item = String>,
    translucide: &dyn Fn(&str) -> bool,
) -> (tf_mesh::TableFormes, Vec<crate::apparence::Habillage>) {
    let mut t = tf_mesh::TableFormes::new();
    let mut h = Vec::new();
    for cle in cles {
        let (forme, hab) = forme_et_habillage(cat, Some(atlas), Some(teintes), &cle, translucide);
        t.pousser(forme.air, forme.opaque, forme.cuboides);
        h.push(hab);
    }
    (t, h)
}

/// Tous les noms de texture cités par les modèles résolus.
///
/// Sert à bâtir le tableau d'atlas : on ne lit que ce qui est RÉFÉRENCÉ. Un
/// pack contient des textures d'objets, d'interface et d'entités dont aucun
/// bloc ne se sert — les charger toutes multiplierait la mémoire de l'atlas
/// pour rien.
pub fn textures_citees(cat: &Catalogue) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (nom, _) in cat.blocs() {
        let Some(bs) = cat.blockstate(nom) else {
            continue;
        };
        for var in bs.modeles() {
            let Some(m) = cat.modele(&var.modele) else {
                continue;
            };
            for e in &m.elements {
                for fd in e.faces.values() {
                    // Une variable non résolue n'est pas une texture : aller la
                    // chercher produirait une « absente » qui nommerait `#side`
                    // au lieu du vrai trou.
                    if !fd.texture.starts_with('#') {
                        v.push(fd.texture.clone());
                    }
                }
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Les blocs qui ne BOUCHENT pas leur case malgré un cuboïde qui la remplit.
///
/// C'est la réponse à la question qu'un pack ne pose nulle part : le verre
/// remplit son bloc et ne doit masquer personne. Elle se DÉRIVE de l'alpha des
/// textures, jamais d'une liste de noms — une liste ne couvrirait aucun des
/// 1 678 blocs `minefield:*`.
///
/// **Mais pas de n'importe quelle texture.** « Une texture transparente
/// quelque part » classait 864 blocs sur 2 560 comme translucides, dont
/// `grass_block` : sa couche d'herbe est transparente sur les côtés, et le
/// bloc serait devenu non opaque. C'est exactement le piège qui a coûté à
/// `ExeWorldEdit` un sol méconnaissable et 1 281 appels de dessin, repris sous
/// une autre forme.
///
/// Seules comptent les faces du cuboïde qui REMPLIT la case, et seulement
/// celles qui sont à ras du bord : c'est par elles qu'on verrait au travers.
/// Ce qui est posé par-dessus ne rend pas le bloc transparent.
pub fn blocs_translucides(
    cat: &Catalogue,
    atlas: &crate::Atlas,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (nom, _) in cat.blocs() {
        let Some(m) = cat.modele_de(nom) else {
            continue;
        };
        let cub = modele::cuboides(m);
        let Some(i) = indice_cube_plein(&cub) else {
            // Pas un cube plein : la question ne se pose pas, il passe déjà par
            // la passe de modèles et ne masque rien.
            continue;
        };
        let e = &m.elements[i];
        let troue = e.faces.iter().any(|(f, fd)| {
            cub[i].au_bord(*f)
                && atlas
                    .couche(&fd.texture)
                    .and_then(|c| atlas.couches.get(c as usize))
                    .is_some_and(|c| c.transparente)
        });
        if troue {
            out.insert(nom.clone());
        }
    }
    out
}
