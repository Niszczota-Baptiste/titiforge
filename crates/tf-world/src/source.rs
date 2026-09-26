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
use std::sync::RwLock;

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

    /// Code STABLE pour les formats persistés. Le discriminant de l'`enum` ne
    /// l'est pas : insérer une variante décalerait tout ce qui est déjà sur le
    /// disque d'un utilisateur, et ses annulations viseraient le mauvais
    /// dossier.
    pub const fn code(self) -> u8 {
        match self {
            Folder::Region => 0,
            Folder::Entities => 1,
            Folder::Poi => 2,
        }
    }

    pub const fn depuis_code(c: u8) -> Option<Folder> {
        match c {
            0 => Some(Folder::Region),
            1 => Some(Folder::Entities),
            2 => Some(Folder::Poi),
            _ => None,
        }
    }
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

    /// L'identifiant que le JEU écrit dans ses données — `minecraft:overworld`
    /// et non `Surface`. C'est lui qu'une position globale porte (le lit d'un
    /// villageois, son poste de travail) : comparer au nom lisible ne
    /// trouverait jamais rien.
    pub fn id(&self) -> String {
        match self {
            Dimension::Overworld => "minecraft:overworld".to_string(),
            Dimension::Nether => "minecraft:the_nether".to_string(),
            Dimension::End => "minecraft:the_end".to_string(),
            Dimension::Custom { namespace, path } => format!("{namespace}:{path}"),
        }
    }
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

    /// Vraie pour un fichier qui n'a même pas la taille de son EN-TÊTE, donc
    /// qui n'est pas un fichier de région quoi que dise son nom.
    ///
    /// La distinction avec `is_empty` compte : une région vide est normale —
    /// le jeu en écrit — alors qu'un fichier plus court que son en-tête est
    /// forcément autre chose. Ça arrive vraiment : un `.mca` de 2,7 Kio
    /// commençant par `bplist00` est un ALIAS iOS, envoyé à la place du
    /// fichier quand celui-ci n'est pas matérialisé localement. L'aperçu ne lit
    /// aucun contenu — par conception, une save ne s'ouvre jamais en entier —
    /// mais il connaît déjà la taille, donc le dire est gratuit. Sans ça
    /// l'outil annonce « 5 régions · 0,0 Mo » et laisse chercher.
    pub fn est_tronquee(&self) -> bool {
        self.bytes < tf_anvil::HEADER as u64
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

    /// Noms des charges déportées présentes, triés.
    ///
    /// Sans ça, on ne peut les atteindre qu'en devinant leur nom : une région
    /// porte 1024 chunks, donc 1024 essais par région pour trouver les zéro à
    /// deux `.mcc` qui existent vraiment. C'est aussi ce qui permet de rouvrir
    /// un staging sans perdre les charges déportées déjà écrites.
    fn external_names(&self, dim: &Dimension, folder: Folder) -> Result<Vec<String>>;

    /// Relit un petit fichier du monde qui n'est PAS une région — une
    /// métadonnée de la copie de travail, le document des composants — ou
    /// `NotFound`.
    ///
    /// **Sur la lecture, et non sur l'écriture** : la copie de travail doit
    /// relire celui de la SAVE, qu'elle ne tient qu'en lecture. Absent par
    /// défaut — une source d'archive n'en porte pas.
    fn read_meta(&self, nom: &str) -> Result<Vec<u8>> {
        let _ = nom;
        Err(SourceError::NotFound)
    }

    /// Les noms de ces fichiers, triés. Vide par défaut, pour la même raison.
    fn meta_names(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
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

    /// Retire une région. Déjà absente : c'est le résultat voulu, pas une
    /// erreur.
    ///
    /// La copie de travail s'en sert pour OUBLIER une région — la relire
    /// depuis la save quand le jeu l'a changée sous elle. Rien dans le
    /// programme ne retire une région d'une save.
    fn remove_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<()>;

    /// Un petit fichier de MÉTADONNÉES du stockage lui-même — pas une région.
    ///
    /// La copie de travail y range l'empreinte des régions de la save qu'elle
    /// recouvre : c'est ce qui lui permet de savoir, des jours plus tard,
    /// qu'une région a changé sous elle. Écrit d'un bloc, atomiquement.
    fn write_meta(&self, nom: &str, bytes: &[u8]) -> Result<()>;

    /// Retire une métadonnée. Déjà absente : c'est le résultat voulu.
    fn remove_meta(&self, nom: &str) -> Result<()>;
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
///
/// Les tampons sont derrière un verrou pour qu'elle puisse aussi être un
/// PUITS : `RegionSink` écrit à travers `&self`, comme un système de fichiers.
/// Sans ça, la moitié écriture du contrat n'aurait qu'une implémentation —
/// donc ne serait pas un contrat.
type CleRegion = (Dimension, Folder, RegionPos);
type CleExterne = (Dimension, Folder, String);

#[derive(Debug, Default)]
pub struct MemorySource {
    regions: RwLock<BTreeMap<CleRegion, Vec<u8>>>,
    externals: RwLock<BTreeMap<CleExterne, Vec<u8>>>,
    metas: RwLock<BTreeMap<String, Vec<u8>>>,
    read_only: bool,
}

impl MemorySource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Une source en lecture seule : toute écriture répond `ReadOnly`.
    pub fn en_lecture_seule() -> Self {
        MemorySource {
            read_only: true,
            ..Default::default()
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn put_region(&self, dim: Dimension, folder: Folder, pos: RegionPos, bytes: Vec<u8>) {
        self.regions
            .write()
            .unwrap()
            .insert((dim, folder, pos), bytes);
    }

    pub fn put_external(&self, dim: Dimension, folder: Folder, name: &str, bytes: Vec<u8>) {
        self.externals
            .write()
            .unwrap()
            .insert((dim, folder, name.to_string()), bytes);
    }
}

impl RegionSource for MemorySource {
    fn dimensions(&self) -> Result<Vec<Dimension>> {
        // Les charges déportées comptent : une dimension qui n'a qu'un `.mcc`
        // existe quand même, et l'oublier la rendrait invisible à la reprise.
        let mut v: Vec<Dimension> = self
            .regions
            .read()
            .unwrap()
            .keys()
            .map(|(d, ..)| d.clone())
            .collect();
        v.extend(
            self.externals
                .read()
                .unwrap()
                .keys()
                .map(|(d, ..)| d.clone()),
        );
        v.sort();
        v.dedup();
        Ok(v)
    }

    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview> {
        Ok(Overview {
            regions: self
                .regions
                .read()
                .unwrap()
                .iter()
                .filter(|((d, f, _), _)| d == dim && *f == folder)
                .map(|((_, _, pos), b)| RegionInfo {
                    pos: *pos,
                    bytes: b.len() as u64,
                })
                .collect(),
        })
    }

    fn read_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<Vec<u8>> {
        self.regions
            .read()
            .unwrap()
            .get(&(dim.clone(), folder, pos))
            .cloned()
            .ok_or(SourceError::NotFound)
    }

    fn read_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<Vec<u8>> {
        self.externals
            .read()
            .unwrap()
            .get(&(dim.clone(), folder, name.to_string()))
            .cloned()
            .ok_or(SourceError::NotFound)
    }

    fn external_names(&self, dim: &Dimension, folder: Folder) -> Result<Vec<String>> {
        Ok(self
            .externals
            .read()
            .unwrap()
            .keys()
            .filter(|(d, f, _)| d == dim && *f == folder)
            .map(|(_, _, n)| n.clone())
            .collect())
    }

    fn read_meta(&self, nom: &str) -> Result<Vec<u8>> {
        self.metas
            .read()
            .unwrap()
            .get(nom)
            .cloned()
            .ok_or(SourceError::NotFound)
    }

    fn meta_names(&self) -> Result<Vec<String>> {
        let mut v: Vec<String> = self.metas.read().unwrap().keys().cloned().collect();
        v.sort();
        Ok(v)
    }
}

impl RegionSink for MemorySource {
    fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.put_region(dim.clone(), folder, pos, bytes.to_vec());
        Ok(())
    }

    fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.put_external(dim.clone(), folder, name, bytes.to_vec());
        Ok(())
    }

    /// Supprimer ce qui n'est pas là RÉUSSIT — c'est le cas normal : un chunk
    /// qui rétrécit sous le seuil n'avait peut-être jamais débordé.
    fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.externals
            .write()
            .unwrap()
            .remove(&(dim.clone(), folder, name.to_string()));
        Ok(())
    }

    fn remove_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.regions
            .write()
            .unwrap()
            .remove(&(dim.clone(), folder, pos));
        Ok(())
    }

    fn write_meta(&self, nom: &str, bytes: &[u8]) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.metas
            .write()
            .unwrap()
            .insert(nom.to_string(), bytes.to_vec());
        Ok(())
    }

    fn remove_meta(&self, nom: &str) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        self.metas.write().unwrap().remove(nom);
        Ok(())
    }
}
