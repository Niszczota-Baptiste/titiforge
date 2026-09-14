//! Une save Minecraft posée sur un système de fichiers.
//!
//! La première implémentation du contrat `RegionSource`, et rien de plus : ce
//! fichier est le SEUL du crate qui connaisse l'existence d'un disque.

use std::fs;
use std::path::{Path, PathBuf};

use crate::coords::RegionPos;
use crate::source::{
    Dimension, Folder, LockProbe, Overview, RegionInfo, RegionSink, RegionSource, Result,
    SourceError,
};

fn io<E: std::fmt::Display>(e: E) -> SourceError {
    SourceError::Io(e.to_string())
}

/// Refuse un nom qui sortirait du dossier.
///
/// Les noms de `.mcc` sont calculés par nous — mais ils descendent de
/// coordonnées lues dans un fichier, et un fichier vient du disque d'un
/// utilisateur. Un nom qui contient `..` ou un séparateur écrirait AILLEURS
/// que dans la save, et « on contrôle l'appelant » est la phrase qu'on se dit
/// juste avant de ne plus le contrôler.
fn safe_name(name: &str) -> Result<&str> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(SourceError::BadName(name.to_string()));
    }
    Ok(name)
}

pub struct FsSource {
    root: PathBuf,
    read_only: bool,
}

impl FsSource {
    /// Ouvre une save. Le dossier doit exister ; son contenu, non — une save
    /// neuve n'a pas encore de dossier `region/`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            return Err(SourceError::NotFound);
        }
        Ok(FsSource {
            root,
            read_only: false,
        })
    }

    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, dim: &Dimension, folder: Folder) -> PathBuf {
        self.root.join(dim.dir(folder))
    }

    fn region_path(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> PathBuf {
        self.dir(dim, folder).join(pos.file_name())
    }

    /// Vraie si la save porte un `level.dat`. C'est le critère d'une save, et
    /// non la présence d'un dossier `region/` — une save neuve n'en a pas
    /// encore, et un dossier `region/` isolé n'est pas une save.
    pub fn looks_like_world(root: impl AsRef<Path>) -> bool {
        root.as_ref().join("level.dat").is_file()
    }

    /// Sonde le verrou `session.lock`.
    ///
    /// Ne jamais réduire le résultat à un booléen. Sous Windows, le jeu tient
    /// le fichier ouvert et toute tentative d'écriture échoue : la réponse est
    /// fiable. Ailleurs le verrou est CONSULTATIF — l'ouvrir réussit même
    /// quand le jeu tourne, donc un succès ne prouve rien. Affirmer « le monde
    /// est libre » sur cette base ferait écrire pendant que le jeu tourne, et
    /// perdre les deux côtés.
    pub fn probe_lock(&self) -> LockProbe {
        let f = self.root.join("session.lock");
        if !f.exists() {
            // Pas de verrou du tout : le jeu n'a jamais ouvert cette save, ou
            // l'a proprement refermée. Fiable partout.
            return LockProbe::LIBRE_SUR;
        }
        if !cfg!(windows) {
            return LockProbe::INDECIDABLE;
        }
        match fs::OpenOptions::new().write(true).open(&f) {
            Ok(_) => LockProbe::LIBRE_SUR,
            Err(_) => LockProbe::TENU,
        }
    }

    /// Écrit par fichier temporaire puis renommage.
    ///
    /// Un crash au milieu d'une écriture directe laisse un `.mca` tronqué,
    /// donc des chunks perdus. Le renommage est atomique sur tous les systèmes
    /// qui nous intéressent : soit l'ancien fichier est intact, soit le
    /// nouveau est complet, jamais un mélange des deux.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let tmp = path.with_extension(format!(
            "{}.tmp",
            path.extension().and_then(|e| e.to_str()).unwrap_or("bin")
        ));
        fs::write(&tmp, bytes).map_err(io)?;
        fs::rename(&tmp, path).map_err(|e| {
            // Le temporaire ne doit pas rester traîner si le renommage échoue.
            let _ = fs::remove_file(&tmp);
            io(e)
        })
    }
}

impl RegionSource for FsSource {
    fn dimensions(&self) -> Result<Vec<Dimension>> {
        let mut out = Vec::new();
        for d in Dimension::VANILLA {
            if self.dir(&d, Folder::Region).is_dir() {
                out.push(d);
            }
        }
        // Les dimensions ajoutées : `dimensions/<namespace>/<chemin>`. Le
        // chemin peut avoir plusieurs segments, d'où la descente récursive
        // jusqu'à trouver un dossier `region`.
        let base = self.root.join("dimensions");
        if base.is_dir() {
            for ns in lire_dossiers(&base)? {
                let namespace = nom(&ns);
                descendre(&ns, &mut Vec::new(), &namespace, &mut out)?;
            }
        }
        Ok(out)
    }

    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview> {
        let dir = self.dir(dim, folder);
        if !dir.is_dir() {
            // Un dossier absent n'est pas une erreur : une save peut n'avoir
            // jamais généré de `poi/`, et refuser de l'ouvrir pour ça serait
            // absurde.
            return Ok(Overview::default());
        }
        let mut regions = Vec::new();
        for e in fs::read_dir(&dir).map_err(io)? {
            let e = e.map_err(io)?;
            let Some(nom) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Some((x, z)) = tf_anvil::region_coords_from_name(&nom) else {
                continue; // pas un fichier de région : on l'ignore, on n'échoue pas
            };
            let bytes = e.metadata().map_err(io)?.len();
            regions.push(RegionInfo {
                pos: RegionPos::new(x, z),
                bytes,
            });
        }
        regions.sort_by_key(|r| (r.pos.z, r.pos.x));
        Ok(Overview { regions })
    }

    fn read_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<Vec<u8>> {
        let p = self.region_path(dim, folder, pos);
        match fs::read(&p) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SourceError::NotFound),
            Err(e) => Err(io(e)),
        }
    }

    fn read_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<Vec<u8>> {
        let p = self.dir(dim, folder).join(safe_name(name)?);
        match fs::read(&p) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SourceError::NotFound),
            Err(e) => Err(io(e)),
        }
    }

    fn external_names(&self, dim: &Dimension, folder: Folder) -> Result<Vec<String>> {
        let dir = self.dir(dim, folder);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for e in fs::read_dir(&dir).map_err(io)? {
            let e = e.map_err(io)?;
            let Some(nom) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            // Le NOM est le critère, et il est reconstruit depuis les
            // coordonnées qu'on en lit : `c.3.-4.mcc` passe, `c.03.-4.mcc` non.
            // Accepter un nom approchant ferait écrire un fichier que Minecraft
            // ne relirait jamais.
            let Some((x, z)) = external_coords_from_name(&nom) else {
                continue;
            };
            if tf_anvil::external_file_name(x, z) == nom {
                out.push(nom);
            }
        }
        out.sort();
        Ok(out)
    }
}

/// `c.X.Z.mcc` → `(X, Z)`. `None` pour tout le reste.
fn external_coords_from_name(nom: &str) -> Option<(i32, i32)> {
    let reste = nom.strip_prefix("c.")?.strip_suffix(".mcc")?;
    let (x, z) = reste.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

impl RegionSink for FsSource {
    fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()> {
        self.write_atomic(&self.region_path(dim, folder, pos), bytes)
    }

    fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        let p = self.dir(dim, folder).join(safe_name(name)?);
        self.write_atomic(&p, bytes)
    }

    fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()> {
        if self.read_only {
            return Err(SourceError::ReadOnly);
        }
        let p = self.dir(dim, folder).join(safe_name(name)?);
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            // Déjà absent : c'est le résultat voulu, pas une erreur.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }
}

fn nom(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

fn lire_dossiers(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).map_err(io)? {
        let e = e.map_err(io)?;
        if e.path().is_dir() {
            out.push(e.path());
        }
    }
    out.sort();
    Ok(out)
}

/// Descend sous `dimensions/<namespace>/` jusqu'aux dossiers qui contiennent
/// un `region`. Le chemin d'une dimension peut avoir plusieurs segments.
fn descendre(
    dir: &Path,
    segments: &mut Vec<String>,
    namespace: &str,
    out: &mut Vec<Dimension>,
) -> Result<()> {
    // Profondeur bornée : une arborescence pathologique ne doit pas faire
    // déborder la pile, et aucune dimension réelle n'a huit segments.
    if segments.len() >= 8 {
        return Ok(());
    }
    for sous in lire_dossiers(dir)? {
        let n = nom(&sous);
        if n == Folder::Region.dir_name() && !segments.is_empty() {
            out.push(Dimension::Custom {
                namespace: namespace.to_string(),
                path: segments.join("/"),
            });
            continue;
        }
        segments.push(n);
        descendre(&sous, segments, namespace, out)?;
        segments.pop();
    }
    Ok(())
}
