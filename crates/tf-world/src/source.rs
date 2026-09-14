//! D'où viennent les régions — et la seule frontière entre `tf-world` et son
//! hôte.
//!
//! Le moteur ne sait pas s'il lit un dossier de save, une archive, un objet
//! distant ou un tampon en mémoire. C'est ce qui a permis à `we-engine` de
//! tourner à la fois dans un serveur Express et dans une application de
//! bureau, et c'est ce qui permettra un jour d'ouvrir une save sans la
//! décompresser.
//!
//! Les tests de cette frontière sont une **suite de contrat** : une nouvelle
//! source s'y branche et doit passer les mêmes assertions. Des tests écrits
//! contre une seule implémentation ne décrivent pas un contrat, ils décrivent
//! cette implémentation.

use std::collections::BTreeMap;
use std::fmt;

use crate::coords::RegionPos;

/// Une dimension du monde.
///
/// Les trois du jeu, plus celles qu'un datapack ou un mod ajoute. Les traiter
/// comme un ensemble fermé obligerait à toucher ce crate pour chaque monde
/// moddé — exactement ce que la vision interdit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
    /// `namespace:chemin`, tel qu'un datapack ou un mod la déclare.
    Custom {
        namespace: String,
        path: String,
    },
}

/// Les trois dossiers de région d'une dimension.
///
/// `entities/` existe depuis 1.17 : les entités mobiles ont quitté le chunk
/// pour un fichier à part. Une opération qui déplace des blocs devra les
/// suivre, comme elle suit les block entities — c'est le même piège que « un
/// build pivoté abandonne ses coffres », à une échelle plus grande.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Folder {
    Region,
    Entities,
    Poi,
}

impl Folder {
    pub const fn dir_name(self) -> &'static str {
        match self {
            Folder::Region => "region",
            Folder::Entities => "entities",
            Folder::Poi => "poi",
        }
    }

    pub const ALL: [Folder; 3] = [Folder::Region, Folder::Entities, Folder::Poi];
}

impl Dimension {
    /// Chemin du dossier, relatif à la racine de la save.
    ///
    /// La disposition du jeu, qui n'est pas régulière : l'overworld est à la
    /// racine, le nether et l'end sous des noms hérités (`DIM-1`, `DIM1`), et
    /// les dimensions ajoutées sous `dimensions/<namespace>/<chemin>`.
    /// L'uniformiser serait plus joli et ne lirait aucune save réelle.
    pub fn dir(&self, folder: Folder) -> String {
        let f = folder.dir_name();
        match self {
            Dimension::Overworld => f.to_string(),
            Dimension::Nether => format!("DIM-1/{f}"),
            Dimension::End => format!("DIM1/{f}"),
            Dimension::Custom { namespace, path } => {
                format!("dimensions/{namespace}/{path}/{f}")
            }
        }
    }

    /// Nom lisible, pour l'interface.
    pub fn label(&self) -> String {
        match self {
            Dimension::Overworld => "Surface".to_string(),
            Dimension::Nether => "Nether".to_string(),
            Dimension::End => "End".to_string(),
            Dimension::Custom { namespace, path } => format!("{namespace}:{path}"),
        }
    }

    pub const VANILLA: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ── erreurs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// La ressource n'existe pas. **Distinct** d'une erreur : une région
    /// absente est le cas NORMAL aux bords d'un monde, et la confondre avec un
    /// échec ferait refuser d'ouvrir une save parfaitement saine.
    NotFound,
    /// Lecture ou écriture impossible : droits, disque plein, fichier verrouillé.
    Io(String),
    /// La source ne sait pas écrire.
    ReadOnly,
    /// Chemin refusé — voir `safe_name`.
    BadName(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::NotFound => write!(f, "ressource absente"),
            SourceError::Io(m) => write!(f, "entrée/sortie : {m}"),
            SourceError::ReadOnly => write!(f, "cette source est en lecture seule"),
            SourceError::BadName(n) => write!(f, "nom de fichier refusé : « {n} »"),
        }
    }
}

impl std::error::Error for SourceError {}

pub type Result<T> = std::result::Result<T, SourceError>;

// ── le contrat ──────────────────────────────────────────────────────────────

/// Ce qu'on sait d'une région **sans l'avoir lue**.
///
/// Une save ne s'ouvre jamais en entier : plusieurs centaines de régions, des
/// dizaines de gigaoctets. On dresse la carte avec des noms et des tailles, et
/// on ne matérialise que ce qu'on regarde. `we-engine` a appris ça à ses
/// dépens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInfo {
    pub pos: RegionPos,
    /// Taille du fichier en octets. Sert à estimer sans lire — un `.mca` de
    /// 8 Kio ne contient que son en-tête, donc aucun chunk.
    pub bytes: u64,
}

impl RegionInfo {
    /// Vraie pour un fichier qui ne peut contenir aucun chunk : il n'a que son
    /// en-tête de 8 Kio, ou moins.
    pub fn is_empty(&self) -> bool {
        self.bytes <= tf_anvil::HEADER as u64
    }
}

/// Carte d'une dimension, dressée sans rien décoder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overview {
    pub regions: Vec<RegionInfo>,
}

impl Overview {
    pub fn total_bytes(&self) -> u64 {
        self.regions.iter().map(|r| r.bytes).sum()
    }

    /// Boîte des régions présentes, en coordonnées de région.
    /// `None` si la dimension est vide.
    pub fn bounds(&self) -> Option<(RegionPos, RegionPos)> {
        let mut it = self.regions.iter().filter(|r| !r.is_empty());
        let first = it.next()?;
        let (mut min, mut max) = (first.pos, first.pos);
        for r in it {
            min = RegionPos::new(min.x.min(r.pos.x), min.z.min(r.pos.z));
            max = RegionPos::new(max.x.max(r.pos.x), max.z.max(r.pos.z));
        }
        Some((min, max))
    }

    /// Les régions qui contiennent au moins un chunk, des plus grosses aux
    /// plus petites — l'ordre dans lequel on veut les montrer.
    pub fn non_empty(&self) -> Vec<&RegionInfo> {
        let mut v: Vec<&RegionInfo> = self.regions.iter().filter(|r| !r.is_empty()).collect();
        v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.pos.cmp(&b.pos)));
        v
    }
}

/// Lecture. La seule chose que `tf-world` exige de son hôte.
pub trait RegionSource: Send + Sync {
    /// Dimensions réellement présentes. Une save vanilla en a trois ; une save
    /// moddée peut en avoir trente.
    fn dimensions(&self) -> Result<Vec<Dimension>>;

    /// Carte d'un dossier, **sans lire aucun contenu**.
    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview>;

    /// Octets d'un fichier de région, ou `NotFound`.
    fn read_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<Vec<u8>>;

    /// Charge déportée d'un chunk surdimensionné (`c.X.Z.mcc`).
    fn read_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<Vec<u8>>;
}

/// Écriture. Séparée de la lecture : une source d'archive, ou une save montée
/// en lecture seule, implémente la première sans la seconde.
pub trait RegionSink: Send + Sync {
    fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()>;
    fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()>;
    fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()>;
}

/// État du verrou `session.lock` d'une save.
///
/// **Ne jamais réduire ça à un booléen.** Le verrou n'est détectable que sous
/// Windows, où l'ouverture d'un fichier tenu échoue. Ailleurs il est
/// consultatif : une ouverture réussie ne prouve RIEN, et affirmer « le monde
/// est libre » sur cette base ferait écrire pendant que le jeu tourne — donc
/// perdre les deux côtés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockProbe {
    pub locked: bool,
    /// Faux quand le système ne permet pas de conclure. L'interface doit alors
    /// demander à l'utilisateur, pas décider à sa place.
    pub reliable: bool,
}

impl LockProbe {
    pub const LIBRE_SUR: LockProbe = LockProbe {
        locked: false,
        reliable: true,
    };
    pub const INDECIDABLE: LockProbe = LockProbe {
        locked: false,
        reliable: false,
    };
    pub const TENU: LockProbe = LockProbe {
        locked: true,
        reliable: true,
    };

    /// Vraie seulement si on SAIT que le monde est libre.
    pub fn surement_libre(&self) -> bool {
        self.reliable && !self.locked
    }
}

// ── une source en mémoire ───────────────────────────────────────────────────

/// Source en mémoire. Sert aux tests, et prouve que la frontière en est
/// vraiment une : si le contrat ne tenait que pour un système de fichiers, ce
/// ne serait pas un contrat.
#[derive(Debug, Default)]
pub struct MemorySource {
    regions: BTreeMap<(Dimension, Folder, i32, i32), Vec<u8>>,
    externals: BTreeMap<(Dimension, Folder, String), Vec<u8>>,
    pub read_only: bool,
}

impl MemorySource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_region(&mut self, dim: Dimension, folder: Folder, pos: RegionPos, bytes: Vec<u8>) {
        self.regions.insert((dim, folder, pos.x, pos.z), bytes);
    }

    pub fn put_external(&mut self, dim: Dimension, folder: Folder, name: &str, bytes: Vec<u8>) {
        self.externals
            .insert((dim, folder, name.to_string()), bytes);
    }
}

impl RegionSource for MemorySource {
    fn dimensions(&self) -> Result<Vec<Dimension>> {
        let mut v: Vec<Dimension> = self.regions.keys().map(|(d, ..)| d.clone()).collect();
        v.sort();
        v.dedup();
        Ok(v)
    }

    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview> {
        Ok(Overview {
            regions: self
                .regions
                .iter()
                .filter(|((d, f, ..), _)| d == dim && *f == folder)
                .map(|((_, _, x, z), b)| RegionInfo {
                    pos: RegionPos::new(*x, *z),
                    bytes: b.len() as u64,
                })
                .collect(),
        })
    }

    fn read_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<Vec<u8>> {
        self.regions
            .get(&(dim.clone(), folder, pos.x, pos.z))
            .cloned()
            .ok_or(SourceError::NotFound)
    }

    fn read_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<Vec<u8>> {
        self.externals
            .get(&(dim.clone(), folder, name.to_string()))
            .cloned()
            .ok_or(SourceError::NotFound)
    }
}
