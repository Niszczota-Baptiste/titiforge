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
//!
//! ## La save peut changer sous la copie
//!
//! La couche garde des régions ENTIÈRES, et une séance peut durer des jours.
//! Chaque région recouverte garde donc l'empreinte de la save au moment où la
//! couche s'en est écartée — sa BASE — et trois empreintes suffisent à dire
//! où elle en est ([`classer`]) : à jour, en attente d'écriture, périmée, ou
//! en conflit. Une écriture qui effacerait ce que le joueur a fait en jeu est
//! refusée avant la sauvegarde.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

use crate::coords::RegionPos;
use crate::journal::octets::{R, W};
use crate::journal::{ecrire_dimension, lire_dimension};
use crate::source::{
    Dimension, Folder, LockProbe, Overview, RegionInfo, RegionSink, RegionSource, Result,
    SourceError,
};

/// Une source qui sait aussi écrire. `dyn RegionSource + RegionSink` n'existe
/// pas en Rust ; ce trait vide le rend possible.
pub trait RegionStore: RegionSource + RegionSink {}
impl<T: RegionSource + RegionSink> RegionStore for T {}

/// Une région : sa dimension, son dossier, sa position.
pub type Cle = (Dimension, Folder, RegionPos);
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
    /// contenu. Rangées à côté de la couche avec les bases : elles survivent à
    /// une reprise.
    pierres_tombales: RwLock<BTreeSet<CleExterne>>,
    /// **La BASE de chaque région de la save que la couche recouvre** :
    /// l'empreinte de la save au moment où la couche s'en est écartée, ou à la
    /// dernière écriture dans la save. `None` : la région n'existait pas.
    ///
    /// La couche garde des régions ENTIÈRES. Si le joueur joue dans le monde
    /// pendant ou entre deux séances, écrire la couche remettrait ces régions
    /// telles que titiforge les avait lues, et tout ce qui a été fait en jeu
    /// y serait perdu. La base est ce qui permet de le voir (voir [`classer`]).
    bases: RwLock<BTreeMap<Cle, Option<u64>>>,
}

/// Le nom de la métadonnée qui porte les bases et les pierres tombales.
const META_COUCHE: &str = "couche";

/// **Où en est une région de la copie de travail, par rapport à la save.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtatRegion {
    /// La copie et la save portent la même chose : rien à écrire.
    AJour,
    /// La copie porte du travail que la save n'a pas, et la save n'a pas
    /// bougé depuis : c'est ce qu'une écriture écrira.
    EnAttente,
    /// La save a changé, et la copie n'a rien à y ajouter : elle est
    /// PÉRIMÉE. La garder montrerait le monde d'avant, et l'éditer
    /// fabriquerait un conflit — elle se relit depuis la save
    /// ([`Staging::rafraichir`]).
    Perimee,
    /// La save a changé ET la copie porte du travail dessus. Écrire
    /// effacerait l'un des deux : on refuse.
    EnConflit,
}

impl EtatRegion {
    /// La copie porte-t-elle du travail que la save n'a pas ? C'est ce qu'on
    /// perdrait en la jetant.
    pub fn porte_du_travail(self) -> bool {
        matches!(self, EtatRegion::EnAttente | EtatRegion::EnConflit)
    }
}

/// **La table de vérité**, à partir de trois empreintes : la `base` (ce que
/// la save portait quand la copie s'en est écartée — `None` si on ne le sait
/// pas), la `save` d'aujourd'hui, et la `copie`.
///
/// Isolée pour se tester seule : c'est elle qui décide si une écriture
/// efface le travail de quelqu'un.
///
/// - `save == copie` : **à jour**, quelle que soit la base. C'est aussi ce qui
///   rattrape une écriture interrompue — la save porte déjà ce qu'on allait
///   y mettre, la réécrire ne perdrait rien.
/// - base inconnue : **en conflit**. On ne sait pas si la save a bougé, donc
///   on ne réécrit pas.
/// - `base == save` : la save n'a pas bougé, la copie oui — **en attente**.
/// - `base == copie` : la copie n'a pas bougé, la save oui — **périmée**.
/// - sinon, les deux ont bougé : **en conflit**.
pub fn classer(base: Option<Option<u64>>, save: Option<u64>, copie: Option<u64>) -> EtatRegion {
    if save == copie {
        return EtatRegion::AJour;
    }
    match base {
        None => EtatRegion::EnConflit,
        Some(b) if b == save => EtatRegion::EnAttente,
        Some(b) if b == copie => EtatRegion::Perimee,
        Some(_) => EtatRegion::EnConflit,
    }
}

/// Ce qu'une écriture dans la save a fait.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub regions_ecrites: usize,
    pub externes_ecrites: usize,
    pub externes_supprimees: usize,
    /// Les fichiers du monde qui ne sont pas des régions — le document des
    /// composants — écrits ou retirés.
    pub fichiers_ecrits: usize,
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
    /// **La save a changé sous des régions où la copie porte du travail** —
    /// le joueur y a joué, une sauvegarde a été restaurée. Écrire remettrait
    /// ces régions telles que titiforge les avait lues, et effacerait ce qui
    /// y a été fait depuis. Refusé AVANT la sauvegarde et avant la moindre
    /// écriture.
    SaveModifiee(Vec<Cle>),
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
            CommitError::SaveModifiee(regions) => {
                let noms: Vec<String> = regions.iter().take(4).map(nom_de_region).collect();
                write!(
                    f,
                    "la save a changé depuis que titiforge a lu {} région(s) où la copie \
                     de travail porte des modifications ({}{}) — une partie jouée \
                     entre-temps ? Écrire effacerait l'un ou l'autre : rien n'a été écrit",
                    regions.len(),
                    noms.join(", "),
                    if regions.len() > noms.len() {
                        ", …"
                    } else {
                        ""
                    }
                )
            }
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

/// `region/r.0.0.mca`, `DIM-1/entities/r.2.-1.mca` — ce qu'on dit à
/// l'utilisateur, qui peut aller le regarder.
pub fn nom_de_region(cle: &Cle) -> String {
    let (d, dossier, p) = cle;
    format!("{}/r.{}.{}.mca", d.dir(*dossier), p.x, p.z)
}

impl<S: RegionSource, O: RegionStore> Staging<S, O> {
    pub fn new(source: S, overlay: O) -> Self {
        Staging {
            source,
            overlay,
            ecrits: RwLock::new(BTreeSet::new()),
            pierres_tombales: RwLock::new(BTreeSet::new()),
            bases: RwLock::new(BTreeMap::new()),
        }
    }

    /// Reprend un staging qui existe déjà sur disque : la couche est relue pour
    /// savoir ce qu'elle contient, et ses métadonnées pour savoir d'où elle
    /// part.
    ///
    /// Sans la relecture, rouvrir un projet ferait croire que rien n'a été
    /// modifié, et la première lecture retomberait sur la source — donc
    /// annulerait silencieusement tout le travail en cours.
    ///
    /// Des métadonnées absentes ou illisibles ne font pas échouer : chaque
    /// région de la couche a alors une base INCONNUE, donc se classe en
    /// conflit dès que la save diffère — dans le doute, on ne réécrit pas.
    pub fn reopen(source: S, overlay: O) -> Result<Self> {
        let mut ecrits = BTreeSet::new();
        for dim in dimensions_de(&overlay)? {
            for folder in Folder::ALL {
                for r in overlay.overview(&dim, folder)?.regions {
                    ecrits.insert((dim.clone(), folder, r.pos));
                }
            }
        }
        let (bases, tombes) = overlay
            .read_meta(META_COUCHE)
            .ok()
            .and_then(|b| decoder_etat(&b))
            .unwrap_or_default();
        Ok(Staging {
            source,
            overlay,
            ecrits: RwLock::new(ecrits),
            pierres_tombales: RwLock::new(tombes),
            bases: RwLock::new(bases),
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

    // ── la save sous la couche ──────────────────────────────────────────────

    /// Les régions que la couche recouvre : celles qu'elle porte, celles
    /// dont elle a noté la base, celles dont elle porte une charge déportée.
    ///
    /// Les pierres tombales n'ont pas à être parcourues : `remove_external`
    /// note la base de leur région avant de les poser, et les deux se rangent
    /// ensemble. Le dernier cas ne sert qu'à une couche reprise SANS ses
    /// métadonnées — une charge qu'elle porte seule doit quand même se voir.
    fn recouvertes(&self) -> Result<BTreeSet<Cle>> {
        let mut out = self.ecrits.read().unwrap().clone();
        out.extend(self.bases.read().unwrap().keys().cloned());
        for d in dimensions_de(&self.overlay)? {
            for f in Folder::ALL {
                for n in self.overlay.external_names(&d, f)? {
                    if let Some(p) = region_du_mcc(&n) {
                        out.insert((d.clone(), f, p));
                    }
                }
            }
        }
        Ok(out)
    }

    /// **L'état de chaque région recouverte**, dans un ordre stable.
    ///
    /// Relit la save ET la copie de chacune : c'est le prix d'une réponse
    /// vraie, payé à l'écriture, à la reprise et à la fermeture — jamais par
    /// opération.
    pub fn etats(&self) -> Result<Vec<(Cle, EtatRegion)>> {
        Ok(self
            .etats_detailles()?
            .into_iter()
            .map(|(c, e, _)| (c, e))
            .collect())
    }

    /// Les états, et l'empreinte de la copie de chacune.
    ///
    /// **« À jour » se juge au CONTENU**, pas aux octets du fichier. Une
    /// annulation rend un chunk identique à ce qu'il était, mais recompressé :
    /// jugée aux octets, la région aurait l'air modifiée pour toujours — une
    /// séance « éditer puis tout annuler » survivait à la fermeture, et le
    /// jeu touchant ensuite la région fabriquait un conflit avec… rien.
    fn etats_detailles(&self) -> Result<Vec<(Cle, EtatRegion, Option<u64>)>> {
        let bases = self.bases.read().unwrap().clone();
        let mut out = Vec::new();
        for cle in self.recouvertes()? {
            let save = signature(&self.source, &cle.0, cle.1, cle.2)?;
            let copie = signature(self, &cle.0, cle.1, cle.2)?;
            let mut e = classer(bases.get(&cle).copied(), save, copie);
            if e != EtatRegion::AJour && meme_contenu(&self.source, self, &cle)? {
                e = EtatRegion::AJour;
            }
            out.push((cle, e, copie));
        }
        Ok(out)
    }

    // ── les fichiers du monde qui ne sont pas des régions ───────────────────

    /// **Un fichier du monde qui n'est pas une région** — le document des
    /// composants : celui de la copie s'il y en a un, sinon celui de la save,
    /// sinon `NotFound`. Un document VIDE dans la copie en est un : c'est
    /// l'état d'un document dont on a tout retiré, qui cache celui de la save.
    ///
    /// La métadonnée interne de la copie (`couche`) n'en est pas un : elle ne
    /// se lit pas par ici, et ne partira jamais dans la save.
    pub fn lire_fichier(&self, nom: &str) -> Result<Vec<u8>> {
        if nom == META_COUCHE {
            return Err(SourceError::BadName(nom.to_string()));
        }
        match self.overlay.read_meta(nom) {
            Err(SourceError::NotFound) => self.source.read_meta(nom),
            autre => autre,
        }
    }

    /// L'écrit dans la COPIE. La save ne le reçoit qu'à l'écriture, après la
    /// sauvegarde — exactement comme une région.
    pub fn ecrire_fichier(&self, nom: &str, octets: &[u8]) -> Result<()> {
        if nom == META_COUCHE {
            return Err(SourceError::BadName(nom.to_string()));
        }
        self.overlay.write_meta(nom, octets)
    }

    /// **Les fichiers que la copie porte AUTREMENT que la save** : du travail,
    /// au même titre qu'une région en attente — une séance qui n'aurait que
    /// lui ne s'efface pas en se fermant. Absent et vide se valent : un
    /// document vidé dans la copie, quand la save n'en a pas, n'attend rien.
    pub fn fichiers_en_attente(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for nom in self.overlay.meta_names()? {
            if nom == META_COUCHE {
                continue;
            }
            let copie = self.overlay.read_meta(&nom)?;
            let save = match self.source.read_meta(&nom) {
                Ok(b) => b,
                Err(SourceError::NotFound) => Vec::new(),
                Err(e) => return Err(e),
            };
            if copie != save {
                out.push(nom);
            }
        }
        Ok(out)
    }

    /// Rend à la save les fichiers que la copie porte à l'IDENTIQUE — après
    /// une annulation, ou une écriture. Rend combien.
    pub fn alleger_fichiers(&self) -> Result<usize> {
        let attente = self.fichiers_en_attente()?;
        let mut n = 0;
        for nom in self.overlay.meta_names()? {
            if nom != META_COUCHE && !attente.contains(&nom) {
                self.overlay.remove_meta(&nom)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// **Rend à la save les régions que la copie porte à l'identique**, et
    /// celles qu'elle porte périmées : ni les unes ni les autres n'ont de
    /// travail à perdre. Rend combien.
    ///
    /// La copie ne garde ainsi que ce qui ATTEND. C'est ce qui évite les
    /// conflits sans objet : une région gardée sans raison, que le jeu change
    /// ensuite, ne se distinguerait plus d'une région où l'on a travaillé.
    pub fn alleger(&self) -> Result<usize> {
        let rendre: Vec<Cle> = self
            .etats()?
            .into_iter()
            .filter(|(_, e)| matches!(e, EtatRegion::AJour | EtatRegion::Perimee))
            .map(|(c, _)| c)
            .collect();
        self.rafraichir_regions(&rendre)?;
        Ok(rendre.len())
    }

    /// Rend à la save celles de ces régions que la copie porte à l'identique
    /// — après une annulation, typiquement, qui en ramène le contenu. Le
    /// prix est celui des seuls chunks qui diffèrent : une région qui porte
    /// encore du travail s'arrête au premier.
    pub fn alleger_regions(&self, cles: &[Cle]) -> Result<usize> {
        let recouvertes = self.recouvertes()?;
        let mut rendre = Vec::new();
        for cle in cles {
            if !recouvertes.contains(cle) {
                continue;
            }
            let (d, f, p) = (&cle.0, cle.1, cle.2);
            if signature(&self.source, d, f, p)? == signature(self, d, f, p)?
                || meme_contenu(&self.source, self, cle)?
            {
                rendre.push(cle.clone());
            }
        }
        self.rafraichir_regions(&rendre)?;
        Ok(rendre.len())
    }

    /// **Jette la copie d'une région** : la vue retombe sur la save.
    ///
    /// Ce qu'elle portait est PERDU. À n'appeler que sur une région
    /// [`EtatRegion::Perimee`] — elle n'a rien à perdre — ou pour abandonner
    /// exprès.
    ///
    /// Les métadonnées d'abord, les fichiers ensuite. Un arrêt entre les deux
    /// laisse une région sans base, que la reprise tiendra pour douteuse ;
    /// l'ordre inverse laisserait une pierre tombale qui cacherait une charge
    /// déportée de la save d'AUJOURD'HUI — un chunk illisible dans la vue.
    pub fn rafraichir(&self, cle: &Cle) -> Result<()> {
        self.rafraichir_regions(std::slice::from_ref(cle))
    }

    /// [`Staging::rafraichir`] pour plusieurs régions, en un seul rangement
    /// des métadonnées.
    pub fn rafraichir_regions(&self, cles: &[Cle]) -> Result<()> {
        if cles.is_empty() {
            return Ok(());
        }
        {
            let mut bases = self.bases.write().unwrap();
            let mut tombes = self.pierres_tombales.write().unwrap();
            for cle in cles {
                let (dim, folder, pos) = (&cle.0, cle.1, cle.2);
                bases.remove(cle);
                tombes.retain(|(d, f, n)| {
                    !(d == dim && *f == folder && region_du_mcc(n) == Some(pos))
                });
            }
        }
        self.persister()?;
        for cle in cles {
            let (dim, folder, pos) = (&cle.0, cle.1, cle.2);
            for n in self.overlay.external_names(dim, folder)? {
                if region_du_mcc(&n) == Some(pos) {
                    self.overlay.remove_external(dim, folder, &n)?;
                }
            }
            self.overlay.remove_region(dim, folder, pos)?;
            self.ecrits.write().unwrap().remove(cle);
        }
        Ok(())
    }

    /// Note la base d'une région si c'est la première fois que la couche s'en
    /// écarte. Rend vrai si la table a changé — elle est alors à ranger.
    ///
    /// Une région que la couche porte déjà SANS base connue (une couche
    /// reprise sans ses métadonnées) garde sa base inconnue : la noter
    /// maintenant affirmerait que la copie part de la save d'aujourd'hui,
    /// alors qu'elle part peut-être d'une save d'il y a une semaine.
    fn noter_base(&self, cle: &Cle) -> Result<bool> {
        if self.bases.read().unwrap().contains_key(cle) || self.ecrits.read().unwrap().contains(cle)
        {
            return Ok(false);
        }
        let b = signature(&self.source, &cle.0, cle.1, cle.2)?;
        self.bases.write().unwrap().insert(cle.clone(), b);
        Ok(true)
    }

    fn persister(&self) -> Result<()> {
        let b = encoder_etat(
            &self.bases.read().unwrap(),
            &self.pierres_tombales.read().unwrap(),
        );
        self.overlay.write_meta(META_COUCHE, &b)
    }

    // ── écriture dans la couche ─────────────────────────────────────────────

    /// La base est notée et RANGÉE avant l'écriture : un arrêt brutal entre
    /// les deux laisse une base sans région écrite, ce qui ne gêne rien ;
    /// l'inverse laisserait une région sans base, que la reprise devrait
    /// tenir pour douteuse.
    pub fn write_region(
        &self,
        dim: &Dimension,
        folder: Folder,
        pos: RegionPos,
        bytes: &[u8],
    ) -> Result<()> {
        let cle = (dim.clone(), folder, pos);
        if self.noter_base(&cle)? {
            self.persister()?;
        }
        self.overlay.write_region(dim, folder, pos, bytes)?;
        self.ecrits.write().unwrap().insert(cle);
        Ok(())
    }

    pub fn write_external(
        &self,
        dim: &Dimension,
        folder: Folder,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if let Some(pos) = region_du_mcc(name) {
            if self.noter_base(&(dim.clone(), folder, pos))? {
                self.persister()?;
            }
        }
        self.overlay.write_external(dim, folder, name, bytes)?;
        // Réécrire annule une suppression antérieure. Sans ça, on écrirait un
        // fichier que la lecture suivante refuserait de voir.
        let etait =
            self.pierres_tombales
                .write()
                .unwrap()
                .remove(&(dim.clone(), folder, name.to_string()));
        if etait {
            self.persister()?;
        }
        Ok(())
    }

    /// La pierre tombale est rangée AVANT que le fichier ne parte : un arrêt
    /// entre les deux laisse un fichier caché, jamais une charge de la save
    /// qui réapparaîtrait.
    pub fn remove_external(&self, dim: &Dimension, folder: Folder, name: &str) -> Result<()> {
        if let Some(pos) = region_du_mcc(name) {
            self.noter_base(&(dim.clone(), folder, pos))?;
        }
        self.pierres_tombales
            .write()
            .unwrap()
            .insert((dim.clone(), folder, name.to_string()));
        self.persister()?;
        self.overlay.remove_external(dim, folder, name)
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
    ///
    /// **N'écrit que ce qui est EN ATTENTE.** Une région déjà écrite et pas
    /// retouchée depuis ne se réécrit pas ; une région périmée surtout pas —
    /// ce serait remettre la save d'avant.
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

        // 2. Refuser si la save a changé sous du travail. AVANT la sauvegarde :
        // on n'en prend pas une pour une écriture qui n'aura pas lieu.
        let etats = self.etats_detailles()?;
        let conflits: Vec<Cle> = etats
            .iter()
            .filter(|(_, e, _)| *e == EtatRegion::EnConflit)
            .map(|(c, _, _)| c.clone())
            .collect();
        if !conflits.is_empty() {
            return Err(CommitError::SaveModifiee(conflits));
        }

        // 3. Sauvegarder AVANT d'écrire quoi que ce soit.
        sauvegarder().map_err(CommitError::BackupFailed)?;

        // 4. Écrire ce qui est en attente, et seulement ça. Une charge
        // déportée dont le nom ne dit pas la région s'écrit toujours, comme
        // avant : on ne sait pas la classer.
        let a_ecrire: BTreeSet<Cle> = etats
            .iter()
            .filter(|(_, e, _)| *e == EtatRegion::EnAttente)
            .map(|(c, _, _)| c.clone())
            .collect();
        let concerne = |d: &Dimension, f: Folder, nom: &str| match region_du_mcc(nom) {
            Some(p) => a_ecrire.contains(&(d.clone(), f, p)),
            None => true,
        };
        let mut rapport = CommitReport::default();
        let ecrits = self.ecrits.read().unwrap().clone();
        for (dim, folder, pos) in a_ecrire.iter().filter(|c| ecrits.contains(c)) {
            let bytes = self.overlay.read_region(dim, *folder, *pos)?;
            sink.write_region(dim, *folder, *pos, &bytes)?;
            rapport.regions_ecrites += 1;
        }
        let tombes = self.pierres_tombales.read().unwrap().clone();
        for (dim, folder, name) in &tombes {
            if concerne(dim, *folder, name) {
                sink.remove_external(dim, *folder, name)?;
                rapport.externes_supprimees += 1;
            }
        }
        for dim in dimensions_de(&self.overlay)? {
            for folder in Folder::ALL {
                for nom in self.overlay.external_names(&dim, folder)? {
                    // Une charge cachée par une pierre tombale est SUPPRIMÉE :
                    // la réécrire annulerait la suppression. `write_external`
                    // retire la pierre quand on la réécrit pour de bon.
                    if tombes.contains(&(dim.clone(), folder, nom.clone()))
                        || !concerne(&dim, folder, &nom)
                    {
                        continue;
                    }
                    let b = self.overlay.read_external(&dim, folder, &nom)?;
                    sink.write_external(&dim, folder, &nom, &b)?;
                    rapport.externes_ecrites += 1;
                }
            }
        }

        // 4 bis. Les fichiers du monde — le document des composants. Le jeu
        // n'y touche pas, donc ils n'ont pas de conflit à craindre ; un
        // document VIDE se retire de la save plutôt que de s'y écrire vide.
        for nom in self.fichiers_en_attente()? {
            let octets = self.overlay.read_meta(&nom)?;
            if octets.is_empty() {
                sink.remove_meta(&nom)?;
            } else {
                sink.write_meta(&nom, &octets)?;
            }
            rapport.fichiers_ecrits += 1;
        }

        // 5. Une région que la save porte désormais telle quelle QUITTE la
        // copie : garder une région sans travail, c'est fabriquer un conflit
        // le jour où le joueur y remet les pieds. Ce que la save porte se
        // RELIT plutôt que de se supposer : si le puits n'est pas la save,
        // rien n'a changé pour elle, et la région écrite reste en attente —
        // avec la même base, puisque la save n'a pas bougé.
        let mut rendre = Vec::new();
        for (cle, e, copie) in &etats {
            let ecrite = *e == EtatRegion::EnAttente
                && signature(&self.source, &cle.0, cle.1, cle.2)? == *copie;
            if ecrite || *e == EtatRegion::AJour {
                rendre.push(cle.clone());
            }
        }
        self.rafraichir_regions(&rendre)?;
        // Même règle pour les fichiers, RELUE elle aussi : si le puits n'était
        // pas la save, ils attendent toujours.
        self.alleger_fichiers()?;
        Ok(rapport)
    }
}

/// Les dimensions d'une couche, plus les trois vanilla d'office.
///
/// Une source sur disque ne découvre une dimension que par son dossier
/// `region/` : une couche qui n'aurait écrit que des entités serait
/// invisible, et sa reprise la perdrait.
fn dimensions_de<T: RegionSource + ?Sized>(src: &T) -> Result<Vec<Dimension>> {
    let mut v = src.dimensions()?;
    v.extend(Dimension::VANILLA);
    v.sort();
    v.dedup();
    Ok(v)
}

// ── empreintes ──────────────────────────────────────────────────────────────

/// **L'empreinte d'une région telle qu'une source la montre** — le `.mca` et
/// les charges déportées (`c.X.Z.mcc`) de ses chunks. `None` si rien de tout
/// ça n'existe.
///
/// Les `.mcc` en font partie : l'écriture les écrit aussi, et un chunk
/// déporté que le joueur a modifié en jeu ne change pas le `.mca` — seulement
/// son `.mcc`.
pub fn signature<R: RegionSource + ?Sized>(
    src: &R,
    dim: &Dimension,
    folder: Folder,
    pos: RegionPos,
) -> Result<Option<u64>> {
    let mut h: u64 = 0x5EED_0F71_F04E_57A1;
    let mut rien = true;
    match src.read_region(dim, folder, pos) {
        Ok(b) => {
            h = empreinte_rapide(h, &b);
            rien = false;
        }
        Err(SourceError::NotFound) => h = empreinte_rapide(h, b"absent"),
        Err(e) => return Err(e),
    }
    let mut noms: Vec<String> = src
        .external_names(dim, folder)?
        .into_iter()
        .filter(|n| region_du_mcc(n) == Some(pos))
        .collect();
    noms.sort();
    for n in noms {
        match src.read_external(dim, folder, &n) {
            Ok(b) => {
                h = empreinte_rapide(h, n.as_bytes());
                h = empreinte_rapide(h, &b);
                rien = false;
            }
            Err(SourceError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    Ok((!rien).then_some(h))
}

/// **Les deux sources portent-elles le même CONTENU pour cette région ?**
/// Chunk par chunk, décompressé, charges déportées résolues ; l'horodatage
/// des chunks ne compte pas — le jeu le réécrit de toute façon.
///
/// Rapide quand c'est non : on s'arrête au premier chunk qui diffère, et un
/// chunk dont les octets compressés sont identiques ne se décompresse pas.
/// Une région illisible d'un côté n'est pas « la même » : dans le doute, la
/// copie garde son travail.
fn meme_contenu<A, B>(a: &A, b: &B, cle: &Cle) -> Result<bool>
where
    A: RegionSource + ?Sized,
    B: RegionSource + ?Sized,
{
    let (dim, folder, pos) = (&cle.0, cle.1, cle.2);
    let lire = |s: &dyn Fn() -> Result<Vec<u8>>| match s() {
        Ok(b) => Ok(Some(b)),
        Err(SourceError::NotFound) => Ok(None),
        Err(e) => Err(e),
    };
    let octets_a = lire(&|| a.read_region(dim, folder, pos))?;
    let octets_b = lire(&|| b.read_region(dim, folder, pos))?;
    let (Some(ra), Some(rb)) = (
        region_ou_vide(&octets_a, pos),
        region_ou_vide(&octets_b, pos),
    ) else {
        return Ok(false);
    };
    for i in 0..tf_anvil::CHUNKS {
        let (ca, cb) = match (&ra.slots[i], &rb.slots[i]) {
            (None, None) => continue,
            (Some(ca), Some(cb)) => (ca, cb),
            _ => return Ok(false),
        };
        if !ca.external
            && !cb.external
            && ca.compression == cb.compression
            && ca.payload == cb.payload
        {
            continue;
        }
        let (cx, cz) = ra.chunk_coords(ca);
        let nom = tf_anvil::external_file_name(cx, cz);
        let contenu = |c: &tf_anvil::RawChunk<'_>, s: &dyn Fn() -> Result<Vec<u8>>| {
            let charge = if c.needs_external() {
                s()?
            } else {
                c.payload.to_vec()
            };
            Ok::<_, SourceError>(tf_anvil::inflate(&charge, c.compression).ok())
        };
        let ia = contenu(ca, &|| a.read_external(dim, folder, &nom))?;
        let ib = contenu(cb, &|| b.read_external(dim, folder, &nom))?;
        match (ia, ib) {
            (Some(x), Some(y)) if x == y => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Une région absente se lit comme une région sans chunk ; illisible, `None`.
fn region_ou_vide(octets: &Option<Vec<u8>>, pos: RegionPos) -> Option<tf_anvil::Region<'_>> {
    match octets {
        Some(b) => tf_anvil::read(b, pos.x, pos.z).ok(),
        None => Some(tf_anvil::Region::vide(pos.x, pos.z)),
    }
}

/// La région d'une charge déportée, d'après son nom `c.X.Z.mcc`.
fn region_du_mcc(nom: &str) -> Option<RegionPos> {
    let reste = nom.strip_prefix("c.")?.strip_suffix(".mcc")?;
    let (x, z) = reste.split_once('.')?;
    let (x, z): (i32, i32) = (x.parse().ok()?, z.parse().ok()?);
    Some(RegionPos::new(x.div_euclid(32), z.div_euclid(32)))
}

/// Une empreinte par MOTS de huit octets. Il ne s'agit pas de résister à un
/// adversaire mais de voir qu'un fichier de plusieurs mégaoctets a changé :
/// FNV octet par octet y coûterait une vingtaine de millisecondes par région,
/// payées à la première écriture de chacune et à chaque validation.
fn empreinte_rapide(mut h: u64, b: &[u8]) -> u64 {
    const K1: u64 = 0x9E37_79B1_85EB_CA87;
    const K2: u64 = 0xC2B2_AE3D_27D4_EB4F;
    h ^= (b.len() as u64).wrapping_mul(K1);
    let mut mots = b.chunks_exact(8);
    for m in &mut mots {
        let w = u64::from_le_bytes(m.try_into().unwrap());
        h = (h ^ w.wrapping_mul(K2)).rotate_left(31).wrapping_mul(K1);
    }
    for &x in mots.remainder() {
        h = (h ^ x as u64).wrapping_mul(K2).rotate_left(11);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(K2);
    h ^ (h >> 29)
}

// ── les métadonnées de la couche, sur disque ────────────────────────────────

/// Un format binaire se reconnaît à ses OCTETS.
const MAGIE_COUCHE: &[u8; 4] = b"TFC1";

type EtatCouche = (BTreeMap<Cle, Option<u64>>, BTreeSet<CleExterne>);

fn encoder_etat(bases: &BTreeMap<Cle, Option<u64>>, tombes: &BTreeSet<CleExterne>) -> Vec<u8> {
    let mut w = W(MAGIE_COUCHE.to_vec());
    w.u32(bases.len() as u32);
    for ((dim, folder, pos), b) in bases {
        ecrire_dimension(&mut w, dim);
        w.u8(folder.code()).i32(pos.x).i32(pos.z);
        match b {
            Some(h) => {
                w.u8(1).u64(*h);
            }
            None => {
                w.u8(0);
            }
        }
    }
    w.u32(tombes.len() as u32);
    for (dim, folder, nom) in tombes {
        ecrire_dimension(&mut w, dim);
        w.u8(folder.code()).texte(nom);
    }
    w.0
}

/// `None` au moindre doute : une table à moitié lue donnerait des bases
/// FAUSSES, pire que pas de base du tout — qui fait refuser par prudence.
fn decoder_etat(b: &[u8]) -> Option<EtatCouche> {
    let corps = b.strip_prefix(MAGIE_COUCHE.as_slice())?;
    let mut r = R::new(corps);
    let mut bases = BTreeMap::new();
    for _ in 0..r.u32().ok()? {
        let dim = lire_dimension(&mut r).ok()?;
        let folder = Folder::depuis_code(r.u8().ok()?)?;
        let pos = RegionPos::new(r.i32().ok()?, r.i32().ok()?);
        let base = match r.u8().ok()? {
            0 => None,
            1 => Some(r.u64().ok()?),
            _ => return None,
        };
        bases.insert((dim, folder, pos), base);
    }
    let mut tombes = BTreeSet::new();
    for _ in 0..r.u32().ok()? {
        let dim = lire_dimension(&mut r).ok()?;
        let folder = Folder::depuis_code(r.u8().ok()?)?;
        tombes.insert((dim, folder, r.texte().ok()?));
    }
    (r.p == r.b.len()).then_some((bases, tombes))
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

    /// Le monde tel que la copie le montre : ses fichiers d'abord, ceux de la
    /// save sinon — comme ses régions.
    fn read_meta(&self, nom: &str) -> Result<Vec<u8>> {
        self.lire_fichier(nom)
    }

    fn meta_names(&self) -> Result<Vec<String>> {
        let mut v = self.source.meta_names()?;
        v.extend(self.overlay.meta_names()?);
        v.retain(|n| n != META_COUCHE);
        v.sort();
        v.dedup();
        Ok(v)
    }
}
