//! La copie de travail non destructive.
//!
//! **Invariant n° 1 : on ne touche jamais au fichier source.** Toute
//! modification va dans une couche par-dessus, et la save d'origine reste
//! intacte tant qu'on n'a pas explicitement demandé à écrire.
//!
//! Le staging n'est pas un nouveau genre de stockage : c'est la **composition**
//! de deux sources. La source d'origine, ouverte en lecture seule, et une
//! couche qui reçoit tout ce qu'on écrit. Lire, c'est regarder la couche
//! d'abord et retomber sur la source. Cette composition marche donc avec
//! n'importe quelle implémentation du contrat — un dossier sur disque, une
//! carte en mémoire, une archive un jour.

use std::collections::BTreeSet;
use std::sync::RwLock;

use crate::coords::RegionPos;
use crate::source::{
    Dimension, Folder, LockProbe, Overview, RegionInfo, RegionSink, RegionSource, Result,
    SourceError,
};

/// Une source qui sait aussi écrire. `dyn RegionSource + RegionSink` n'existe
/// pas en Rust ; ce trait vide le rend possible.
pub trait RegionStore: RegionSource + RegionSink {}
impl<T: RegionSource + RegionSink> RegionStore for T {}

type Cle = (Dimension, Folder, RegionPos);
type CleExterne = (Dimension, Folder, String);

pub struct Staging<S: RegionSource, O: RegionStore> {
    source: S,
    overlay: O,
    /// Ce que la couche porte. Redondant avec `overlay.overview()`, et c'est
    /// voulu : lister une couche sur disque coûte un parcours de dossier par
    /// dimension, et on a besoin de cette réponse à chaque lecture.
    ecrits: RwLock<BTreeSet<Cle>>,
    /// Charges déportées SUPPRIMÉES.
    ///
    /// Sans elles, effacer un `.mcc` du staging ferait réapparaître celui de la
    /// source à la lecture suivante — la suppression serait silencieusement
    /// annulée, et un chunk surdimensionné ressusciterait avec son ancien
    /// contenu.
    pierres_tombales: RwLock<BTreeSet<CleExterne>>,
}

/// Ce qu'une écriture dans la save a fait.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub regions_ecrites: usize,
    pub externes_ecrites: usize,
    pub externes_supprimees: usize,
}

/// Pourquoi une écriture a été refusée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    /// Minecraft tient le monde. On refuse — écrire pendant que le jeu tourne
    /// perd les deux côtés.
    WorldLocked,
    /// La sonde n'a pas pu conclure et l'appelant n'a pas confirmé. Sous Linux
    /// et macOS le verrou est consultatif : un succès d'ouverture ne prouve
    /// RIEN. C'est à l'utilisateur de trancher, pas à nous.
    LockUnknown,
    /// La sauvegarde préalable a échoué. On n'écrit pas : une écriture sans
    /// filet est exactement ce que l'invariant interdit.
    BackupFailed(String),
    Source(SourceError),
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::WorldLocked => {
                write!(
                    f,
                    "Minecraft tient ce monde ouvert — fermez le jeu avant d'écrire"
                )
            }
            CommitError::LockUnknown => write!(
                f,
                "impossible de savoir si Minecraft tient ce monde : confirmez que le jeu est fermé"
            ),
            CommitError::BackupFailed(m) => write!(f, "la sauvegarde préalable a échoué : {m}"),
            CommitError::Source(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CommitError {}

impl From<SourceError> for CommitError {
    fn from(e: SourceError) -> Self {
        CommitError::Source(e)
    }
}

impl<S: RegionSource, O: RegionStore> Staging<S, O> {
    pub fn new(source: S, overlay: O) -> Self {
        Staging {
            source,
            overlay,
            ecrits: RwLock::new(BTreeSet::new()),
            pierres_tombales: RwLock::new(BTreeSet::new()),
        }
    }

    /// Reprend un staging qui existe déjà sur disque : la couche est relue pour
    /// savoir ce qu'elle contient.
    ///
    /// Sans ça, rouvrir un projet ferait croire que rien n'a été modifié, et la
    /// première lecture retomberait sur la source — donc annulerait
    /// silencieusement tout le travail en cours.
    /// Les **pierres tombales ne survivent pas** à une reprise : une
    /// suppression non écrite ne laisse rien sur le disque, et inventer un
    /// journal pour elles ici doublerait celui de la phase 1.2. Conséquence
    /// assumée et testée : rouvrir un staging ressuscite une charge déportée
    /// qu'on avait supprimée sans écrire dans la save.
    pub fn reopen(source: S, overlay: O) -> Result<Self> {
        let mut ecrits = BTreeSet::new();
        for dim in overlay.dimensions()? {
            for folder in Folder::ALL {
                for r in overlay.overview(&dim, folder)?.regions {
                    ecrits.insert((dim.clone(), folder, r.pos));
                }
            }
        }
        Ok(Staging {
            source,
            overlay,
            ecrits: RwLock::new(ecrits),
            pierres_tombales: RwLock::new(BTreeSet::new()),
        })
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn overlay(&self) -> &O {
        &self.overlay
    }

    /// Vraie si cette région a été modifiée depuis l'ouverture.
    pub fn is_dirty(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> bool {
        self.ecrits
            .read()
            .unwrap()
            .contains(&(dim.clone(), folder, pos))
    }

    /// Tout ce que le staging porte, dans un ordre stable.
    pub fn touched(&self) -> Vec<Cle> {
        self.ecrits.read().unwrap().iter().cloned().collect()
    }

    pub fn is_clean(&self) -> bool {
        self.ecrits.read().unwrap().is_empty() && self.pierres_tombales.read().unwrap().is_empty()
    }

    /// Jette tout le travail en cours et revient à la source.
    ///
    /// Ne supprime PAS les fichiers de la couche : le staging redevient
    /// simplement transparent. Effacer serait plus propre sur le disque et
    /// irréversible pour l'utilisateur — le mauvais échange.
    pub fn discard(&self) {
        self.ecrits.write().unwrap().clear();
        self.pierres_tombales.write().unwrap().clear();
    }

    // ── écriture dans la couche ─────────────────────────────────────────────

    pub fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()> {
        self.overlay.write_region(dim, folder, pos, bytes)?;
        self.ecrits
            .write()
            .unwrap()
            .insert((dim.clone(), folder, pos));
        Ok(())
    }

    pub fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.overlay.write_external(dim, folder, name, bytes)?;
        // Réécrire annule une suppression antérieure. Sans ça, on écrirait un
        // fichier que la lecture suivante refuserait de voir.
        self.pierres_tombales
            .write()
            .unwrap()
            .remove(&(dim.clone(), folder, name.to_string()));
        Ok(())
    }

    pub fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()> {
        self.overlay.remove_external(dim, folder, name)?;
        self.pierres_tombales
            .write()
            .unwrap()
            .insert((dim.clone(), folder, name.to_string()));
        Ok(())
    }

    // ── écriture dans la SAVE ───────────────────────────────────────────────

    /// Écrit le staging dans la save.
    ///
    /// La sonde de verrou et la sauvegarde sont exigées **en paramètre**, et ce
    /// n'est pas de la politesse : l'ordre EST l'invariant. Une sauvegarde
    /// prise après la première écriture ne sauvegarde plus rien, et
    /// `we-engine` a un test qui l'exige explicitement. En les faisant passer
    /// par ici, l'ordre ne peut plus être inversé par un appelant distrait.
    ///
    /// `confirme_sans_verrou` laisse l'utilisateur trancher quand la sonde ne
    /// peut pas conclure — sous Linux et macOS, le verrou est consultatif et un
    /// succès d'ouverture ne prouve rien. Décider à sa place ferait écrire
    /// pendant que le jeu tourne.
    pub fn commit(
        &self,
        sink: &dyn RegionSink,
        lock: LockProbe,
        confirme_sans_verrou: bool,
        sauvegarder: &mut dyn FnMut() -> std::result::Result<(), String>,
    ) -> std::result::Result<CommitReport, CommitError> {
        // 1. Refuser si le jeu tient le monde.
        if lock.locked {
            return Err(CommitError::WorldLocked);
        }
        if !lock.reliable && !confirme_sans_verrou {
            return Err(CommitError::LockUnknown);
        }

        // 2. Sauvegarder AVANT d'écrire quoi que ce soit.
        sauvegarder().map_err(CommitError::BackupFailed)?;

        // 3. Écrire.
        let mut rapport = CommitReport::default();
        for (dim, folder, pos) in self.touched() {
            let bytes = self.overlay.read_region(&dim, folder, pos)?;
            sink.write_region(&dim, folder, pos, &bytes)?;
            rapport.regions_ecrites += 1;
        }
        for (dim, folder, name) in self.pierres_tombales.read().unwrap().iter() {
            sink.remove_external(dim, *folder, name)?;
            rapport.externes_supprimees += 1;
        }
        for dim in self.overlay.dimensions()? {
            for folder in Folder::ALL {
                for nom in self.overlay.external_names(&dim, folder)? {
                    // Une charge déportée supprimée PUIS réécrite n'est plus
                    // une pierre tombale — `write_external` l'a retirée.
                    let b = self.overlay.read_external(&dim, folder, &nom)?;
                    sink.write_external(&dim, folder, &nom, &b)?;
                    rapport.externes_ecrites += 1;
                }
            }
        }
        Ok(rapport)
    }
}

// ── lecture : la couche d'abord, la source ensuite ──────────────────────────

impl<S: RegionSource, O: RegionStore> RegionSource for Staging<S, O> {
    fn dimensions(&self) -> Result<Vec<Dimension>> {
        let mut v = self.source.dimensions()?;
        v.extend(self.overlay.dimensions()?);
        v.sort();
        v.dedup();
        Ok(v)
    }

    /// Union des deux cartes. La couche l'emporte sur la source pour une même
    /// région : c'est la version courante.
    fn overview(&self, dim: &Dimension, folder: Folder) -> Result<Overview> {
        let base = self.source.overview(dim, folder)?;
        let dessus = self.overlay.overview(dim, folder)?;
        let ecrits = self.ecrits.read().unwrap();

        let mut regions: Vec<RegionInfo> = base
            .regions
            .into_iter()
            .filter(|r| !ecrits.contains(&(dim.clone(), folder, r.pos)))
            .collect();
        for r in dessus.regions {
            if ecrits.contains(&(dim.clone(), folder, r.pos)) {
                regions.push(r);
            }
        }
        regions.sort_by_key(|r| (r.pos.z, r.pos.x));
        Ok(Overview { regions })
    }

    fn read_region(&self, dim: &Dimension, folder: Folder, pos: RegionPos) -> Result<Vec<u8>> {
        if self.is_dirty(dim, folder, pos) {
            return self.overlay.read_region(dim, folder, pos);
        }
        self.source.read_region(dim, folder, pos)
    }

    fn read_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<Vec<u8>> {
        let cle = (dim.clone(), folder, name.to_string());
        if self.pierres_tombales.read().unwrap().contains(&cle) {
            // Supprimée dans le staging : ne PAS retomber sur la source.
            return Err(SourceError::NotFound);
        }
        match self.overlay.read_external(dim, folder, name) {
            Ok(b) => Ok(b),
            Err(SourceError::NotFound) => self.source.read_external(dim, folder, name),
            Err(e) => Err(e),
        }
    }

    /// Union des deux, moins les pierres tombales.
    fn external_names(&self, dim: &Dimension, folder: Folder) -> Result<Vec<String>> {
        let tombes = self.pierres_tombales.read().unwrap();
        let mut v = self.source.external_names(dim, folder)?;
        v.extend(self.overlay.external_names(dim, folder)?);
        v.sort();
        v.dedup();
        v.retain(|n| !tombes.contains(&(dim.clone(), folder, n.clone())));
        Ok(v)
    }
}
