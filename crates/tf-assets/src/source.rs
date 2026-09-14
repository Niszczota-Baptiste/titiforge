//! D'où viennent les ressources — et **on ne redistribue rien**.
//!
//! Embarquer les assets de Mojang dans un installeur exposerait celui qui le
//! diffuse : l'EULA l'interdit. On lit l'installation de l'utilisateur, comme
//! WorldPainter, Amulet et Litematica. Et le résultat est MEILLEUR qu'un pack
//! embarqué : le dossier d'un launcher contient aussi les packs du SERVEUR,
//! donc les blocs custom arrivent avec leurs textures sans rien demander.
//!
//! Une source est un dossier ou une archive. Le reste du crate ne sait pas
//! lequel — même frontière que le contrat de région du monde.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    Absent(String),
    Io(String),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceError::Absent(n) => write!(f, "ressource absente : {n}"),
            SourceError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for SourceError {}

pub type Result<T> = std::result::Result<T, SourceError>;

/// Ce que le crate demande à son hôte.
///
/// Les chemins sont ceux d'un pack : `assets/<namespace>/blockstates/<nom>.json`.
pub trait Source: Send + Sync {
    /// Les octets d'une entrée, ou `Absent`.
    fn lire(&self, chemin: &str) -> Result<Vec<u8>>;

    /// Un nom lisible, pour les messages.
    fn nom(&self) -> &str;
}

/// Un dossier de pack déjà dépaqueté.
#[derive(Debug, Clone)]
pub struct Dossier {
    racine: PathBuf,
    nom: String,
}

impl Dossier {
    pub fn ouvrir(racine: impl AsRef<Path>) -> Result<Self> {
        let racine = racine.as_ref().to_path_buf();
        if !racine.is_dir() {
            return Err(SourceError::Absent(racine.display().to_string()));
        }
        let nom = racine
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("pack")
            .to_string();
        Ok(Dossier { racine, nom })
    }
}

/// Refuse un chemin qui sortirait de la racine.
///
/// Un pack vient du disque d'un utilisateur, et un modèle peut nommer son
/// parent. `../../../../etc/passwd` est un nom de parent parfaitement bien
/// formé.
fn sous_la_racine(chemin: &str) -> bool {
    !chemin.is_empty()
        && !chemin.starts_with('/')
        && !chemin.contains('\\')
        && !chemin.contains('\0')
        && chemin
            .split('/')
            .all(|s| s != ".." && s != "." && !s.is_empty())
}

impl Source for Dossier {
    fn lire(&self, chemin: &str) -> Result<Vec<u8>> {
        if !sous_la_racine(chemin) {
            return Err(SourceError::Absent(chemin.to_string()));
        }
        let p = self.racine.join(chemin);
        match fs::read(&p) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(SourceError::Absent(chemin.to_string()))
            }
            Err(e) => Err(SourceError::Io(e.to_string())),
        }
    }

    fn nom(&self) -> &str {
        &self.nom
    }
}

/// Plusieurs sources, la PREMIÈRE qui répond l'emporte.
///
/// C'est comme ça qu'un pack désigné par l'utilisateur recouvre le pack du
/// serveur, qui recouvre le jeu. L'ordre est celui de la liste, et il est
/// explicite : une pile qui chercherait « au mieux » rendrait le résultat
/// imprévisible selon ce qui traîne sur le disque.
pub struct Pile {
    sources: Vec<Box<dyn Source>>,
    nom: String,
}

impl Pile {
    pub fn new(sources: Vec<Box<dyn Source>>) -> Self {
        let nom = sources
            .iter()
            .map(|s| s.nom())
            .collect::<Vec<_>>()
            .join(" > ");
        Pile { sources, nom }
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

impl Source for Pile {
    fn lire(&self, chemin: &str) -> Result<Vec<u8>> {
        for s in &self.sources {
            match s.lire(chemin) {
                Ok(b) => return Ok(b),
                Err(SourceError::Absent(_)) => continue,
                // Une erreur d'ENTRÉE-SORTIE n'est pas une absence : passer à
                // la source suivante masquerait un disque en train de mourir,
                // et l'utilisateur verrait « texture manquante » au lieu de la
                // vraie panne.
                Err(e) => return Err(e),
            }
        }
        Err(SourceError::Absent(chemin.to_string()))
    }

    fn nom(&self) -> &str {
        &self.nom
    }
}

/// Un identifiant `namespace:chemin`, avec `minecraft` par défaut.
///
/// Une seule règle dans tout le crate. Deux règles différentes feraient
/// chercher le même modèle à deux endroits — et l'une des deux le trouverait.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id {
    pub namespace: String,
    pub chemin: String,
}

impl Id {
    pub fn parse(s: &str) -> Id {
        match s.split_once(':') {
            Some((ns, ch)) => Id {
                namespace: ns.to_string(),
                chemin: ch.to_string(),
            },
            None => Id {
                namespace: "minecraft".to_string(),
                chemin: s.to_string(),
            },
        }
    }

    /// Chemin d'un blockstate dans le pack.
    pub fn blockstate(&self) -> String {
        format!("assets/{}/blockstates/{}.json", self.namespace, self.chemin)
    }

    /// Chemin d'un modèle. Un modèle se nomme `namespace:block/xxx`, donc le
    /// dossier `models/` s'intercale.
    pub fn modele(&self) -> String {
        format!("assets/{}/models/{}.json", self.namespace, self.chemin)
    }

    pub fn texture(&self) -> String {
        format!("assets/{}/textures/{}.png", self.namespace, self.chemin)
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.chemin)
    }
}
