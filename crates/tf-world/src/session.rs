//! **Une séance de travail qui survit à la fermeture.**
//!
//! Fermer titiforge sans avoir écrit dans la save perdait tout : la copie de
//! travail vivait dans un dossier temporaire effacé en partant, et la pile
//! d'annulation en mémoire. Une séance range les deux à un endroit STABLE,
//! dérivé du chemin de la save : rouvrir le même monde reprend la copie et
//! l'annulation là où on les avait laissées.
//!
//! ```text
//! <racine>/<nom>-<empreinte du chemin>/
//!     verrou        tenu tant que la séance est ouverte
//!     monde         le chemin de la save, en clair — pour qui ouvre le dossier
//!     couche/       la copie de travail, et ses bases (voir `staging`)
//!     journal.tfj   l'annulation, en ajout seul
//!     en-cours      le nom d'une action commencée et pas finie
//! ```
//!
//! ## Ce que la reprise vérifie
//!
//! Entre deux séances, le joueur a pu jouer. Chaque région de la copie se
//! classe ([`crate::staging::classer`]) :
//!
//! - **périmée** — la save a changé, la copie n'y ajoutait rien : elle se
//!   relit depuis la save, sans rien demander ;
//! - **en conflit** — la save a changé ET la copie y portait du travail : la
//!   séance ENTIÈRE est mise de côté, intacte, dans un dossier voisin, et on
//!   repart de la save. Rien n'est effacé, rien n'est écrit par-dessus le
//!   jeu. On ne garde pas une moitié de séance : une entrée du journal touche
//!   plusieurs régions, et la moitié d'une annulation n'en est pas une.
//!
//! ## Deux fenêtres, un monde
//!
//! Deux séances sur la même copie de travail s'écriraient l'une par-dessus
//! l'autre. Le fichier `verrou` est tenu EXCLUSIVEMENT tant que la séance vit,
//! et la seconde fenêtre est refusée avec une phrase qui le dit. C'est le
//! verrou du système : un processus tué le relâche, il n'y a rien à nettoyer.
//!
//! ## Ce qui n'est pas garanti
//!
//! Rien n'est forcé sur le disque (`fsync`) : un arrêt du PROGRAMME ne perd
//! rien, une coupure de courant peut perdre les dernières actions. Et une
//! action interrompue au milieu peut laisser la copie à moitié écrite — le
//! journal ne la connaît pas encore. La reprise le DIT (`en-cours`) ; elle ne
//! sait pas le réparer.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fs_source::FsSource;
use crate::journal::{decoder, encoder, entete, Journal, Record};
use crate::source::SourceError;
use crate::staging::{nom_de_region, Cle, EtatRegion, Staging};

/// **Le plafond de l'annulation**, en octets d'éditions.
///
/// Une annulation persistante sans plafond finit par remplir le disque — et la
/// mémoire, puisque le journal y vit aussi. Au-delà, les plus VIEILLES actions
/// s'oublient (`Journal::elaguer`), jusqu'aux trois quarts : élaguer au ras du
/// plafond réécrirait le fichier entier à chaque action suivante.
pub const BUDGET_JOURNAL: usize = 256 * 1024 * 1024;

const VERROU: &str = "verrou";
const MONDE: &str = "monde";
const COUCHE: &str = "couche";
const JOURNAL: &str = "journal.tfj";
const EN_COURS: &str = "en-cours";

/// La copie de travail d'une séance : la save en lecture seule, la couche sur
/// disque.
pub type CopieDeTravail = Staging<FsSource, FsSource>;

pub struct Seance {
    dossier: PathBuf,
    staging: Arc<CopieDeTravail>,
    /// Le journal, ouvert en AJOUT. `None` le temps d'un compactage.
    journal: Option<File>,
    /// Tenu verrouillé tant que la séance vit ; le lâcher, c'est la fermer.
    _verrou: Option<File>,
    budget: usize,
}

/// Ce que l'ouverture a trouvé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reprise {
    /// Aucune séance précédente : on part de la save.
    Neuve,
    /// La séance précédente reprend là où elle s'était arrêtée.
    Reprise {
        /// Entrées du journal, annulables ou refaisables.
        actions: usize,
        /// Régions dont la copie porte du travail que la save n'a pas.
        regions: usize,
        /// Régions périmées, relues depuis la save.
        rafraichies: usize,
        /// Une action commencée et jamais finie : la copie peut la porter à
        /// moitié.
        interrompue: Option<String>,
    },
    /// La save a changé là où la copie portait du travail : la séance est
    /// mise de côté, intacte, dans `vers`, et on repart de la save.
    MiseDeCote { vers: PathBuf, conflits: Vec<Cle> },
}

impl Reprise {
    /// Ce qu'on en dit à l'utilisateur. `None` : rien à dire.
    pub fn texte(&self) -> Option<String> {
        match self {
            Reprise::Neuve => None,
            Reprise::Reprise {
                actions,
                regions,
                rafraichies,
                interrompue,
            } => {
                let mut t = format!(
                    "séance reprise : {regions} région(s) modifiée(s) pas encore écrites \
                     dans la save, {actions} action(s) dans l'historique"
                );
                if *rafraichies > 0 {
                    t.push_str(&format!(
                        " · {rafraichies} région(s) relue(s) depuis la save, que le jeu a \
                         changées depuis"
                    ));
                }
                if let Some(l) = interrompue {
                    t.push_str(&format!(
                        " · « {l} » a été interrompue : la copie de travail peut la porter \
                         à moitié"
                    ));
                }
                Some(t)
            }
            Reprise::MiseDeCote { vers, conflits } => {
                let noms: Vec<String> = conflits.iter().take(3).map(nom_de_region).collect();
                Some(format!(
                    "la save a changé depuis la dernière séance là où la copie de travail \
                     portait des modifications ({}{}) : la séance a été mise de côté, \
                     intacte, dans {} — on repart de la save",
                    noms.join(", "),
                    if conflits.len() > noms.len() {
                        ", …"
                    } else {
                        ""
                    },
                    vers.display()
                ))
            }
        }
    }
}

/// Ce que la fermeture a fait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fermeture {
    /// La save porte tout : la séance est effacée.
    Effacee,
    /// Du travail attend d'être écrit : la séance reste, et se reprendra.
    Gardee { regions: usize },
}

#[derive(Debug)]
pub enum ErreurSeance {
    /// Une autre fenêtre tient cette séance.
    DejaOuverte(PathBuf),
    /// La save elle-même.
    Save(SourceError),
    Io(String),
}

impl std::fmt::Display for ErreurSeance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErreurSeance::DejaOuverte(d) => write!(
                f,
                "ce monde est déjà ouvert dans une autre fenêtre de titiforge ({}) — \
                 deux copies de travail s'écriraient l'une par-dessus l'autre",
                d.display()
            ),
            ErreurSeance::Save(SourceError::NotFound) => write!(f, "save introuvable"),
            ErreurSeance::Save(e) => write!(f, "save : {e}"),
            ErreurSeance::Io(e) => write!(f, "séance : {e}"),
        }
    }
}

impl std::error::Error for ErreurSeance {}

impl From<io::Error> for ErreurSeance {
    fn from(e: io::Error) -> Self {
        ErreurSeance::Io(e.to_string())
    }
}

impl From<SourceError> for ErreurSeance {
    fn from(e: SourceError) -> Self {
        ErreurSeance::Save(e)
    }
}

impl Seance {
    /// **Ouvre la séance d'une save**, en reprenant la précédente s'il y en a
    /// une. Rend aussi le journal relu — c'est au moteur de le tenir.
    pub fn ouvrir(racine: &Path, monde: &Path) -> Result<(Seance, Journal, Reprise), ErreurSeance> {
        // La save d'abord : une save illisible ne doit pas laisser de séance.
        let source = FsSource::open(monde)?.read_only();
        let dossier = dossier_de(racine, monde);
        let couche = dossier.join(COUCHE);
        let deja = couche.is_dir();
        fs::create_dir_all(&couche)?;
        let verrou = verrouiller(&dossier)?;

        let interrompue = fs::read_to_string(dossier.join(EN_COURS)).ok();
        let journal = lire_journal(&dossier)?;
        let staging = Staging::reopen(source, FsSource::open(&couche)?)?;
        let etats = staging.etats()?;

        let conflits: Vec<Cle> = etats
            .iter()
            .filter(|(_, e)| *e == EtatRegion::EnConflit)
            .map(|(c, _)| c.clone())
            .collect();
        if !conflits.is_empty() {
            drop(staging);
            let vers = mettre_de_cote(&dossier)?;
            fs::create_dir_all(&couche)?;
            fs::write(dossier.join(JOURNAL), entete())?;
            fs::write(dossier.join(MONDE), texte_du_chemin(monde))?;
            let staging =
                Staging::new(FsSource::open(monde)?.read_only(), FsSource::open(&couche)?);
            let s = Seance::nouvelle(dossier, staging, verrou);
            return Ok((s, Journal::new(), Reprise::MiseDeCote { vers, conflits }));
        }

        // Une région périmée se relit depuis la save ; une région à jour n'a
        // rien à faire dans la copie. Ni l'une ni l'autre ne porte de travail.
        let rendre: Vec<Cle> = etats
            .iter()
            .filter(|(_, e)| !e.porte_du_travail())
            .map(|(c, _)| c.clone())
            .collect();
        staging.rafraichir_regions(&rendre)?;
        let rafraichies = etats
            .iter()
            .filter(|(_, e)| *e == EtatRegion::Perimee)
            .count();
        let regions = etats.iter().filter(|(_, e)| e.porte_du_travail()).count();
        // Dit une fois : la marque ne doit pas revenir à chaque ouverture.
        let _ = fs::remove_file(dossier.join(EN_COURS));
        fs::write(dossier.join(MONDE), texte_du_chemin(monde))?;

        let rien = regions == 0
            && journal.entrees().is_empty()
            && rafraichies == 0
            && interrompue.is_none();
        let reprise = if !deja || rien {
            Reprise::Neuve
        } else {
            Reprise::Reprise {
                actions: journal.entrees().len(),
                regions,
                rafraichies,
                interrompue,
            }
        };
        Ok((Seance::nouvelle(dossier, staging, verrou), journal, reprise))
    }

    fn nouvelle(dossier: PathBuf, staging: CopieDeTravail, verrou: Option<File>) -> Seance {
        Seance {
            dossier,
            staging: Arc::new(staging),
            journal: None,
            _verrou: verrou,
            budget: BUDGET_JOURNAL,
        }
    }

    /// La copie de travail, à partager avec qui la lit.
    pub fn staging(&self) -> Arc<CopieDeTravail> {
        self.staging.clone()
    }

    pub fn dossier(&self) -> &Path {
        &self.dossier
    }

    /// Change le plafond de l'annulation. Un test doit pouvoir le serrer assez
    /// pour que l'élagage ARRIVE.
    pub fn budget_journal(&mut self, octets: usize) {
        self.budget = octets;
    }

    fn fichier(&mut self) -> io::Result<&mut File> {
        if self.journal.is_none() {
            let f = OpenOptions::new()
                .append(true)
                .open(self.dossier.join(JOURNAL))?;
            self.journal = Some(f);
        }
        Ok(self.journal.as_mut().expect("ouvert juste au-dessus"))
    }

    /// **Ajoute des enregistrements au journal sur disque** — ceux que
    /// `Journal::pousser`, `annuler` ou `refaire` viennent de rendre.
    ///
    /// D'un seul `write` : un enregistrement est entier ou tronqué, et un
    /// enregistrement tronqué se détecte à la relecture. Puis, si le journal
    /// dépasse son plafond, les plus vieilles actions s'oublient et le fichier
    /// se réécrit — c'est pour ça que le journal en mémoire est demandé.
    pub fn noter(&mut self, journal: &mut Journal, records: &[Record]) -> io::Result<()> {
        let mut b = Vec::new();
        for r in records {
            b.extend(encoder(r));
        }
        self.fichier()?.write_all(&b)?;
        if journal.poids() > self.budget && journal.elaguer(self.budget / 4 * 3) > 0 {
            self.compacter(journal)?;
        }
        Ok(())
    }

    /// Réécrit le journal avec son seul état courant. Le fichier est en ajout
    /// seul, donc il ne rétrécit jamais tout seul — ni quand on élague, ni
    /// quand une branche abandonnée meurt.
    pub fn compacter(&mut self, journal: &Journal) -> io::Result<()> {
        let mut b = entete();
        for r in journal.reecrire() {
            b.extend(encoder(&r));
        }
        let tmp = self.dossier.join(format!("{JOURNAL}.tmp"));
        fs::write(&tmp, &b)?;
        // Le fichier ouvert se ferme AVANT le renommage : Windows refuse de
        // remplacer un fichier qu'on tient.
        self.journal = None;
        fs::rename(&tmp, self.dossier.join(JOURNAL))
    }

    /// Une action commence. Si le programme s'arrête pendant, la reprise
    /// saura laquelle.
    pub fn commencer(&mut self, label: &str) -> io::Result<()> {
        fs::write(self.dossier.join(EN_COURS), label)
    }

    /// Elle est finie, réussie ou non.
    pub fn terminer(&mut self) -> io::Result<()> {
        match fs::remove_file(self.dossier.join(EN_COURS)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// **Ferme la séance.** Si la save porte tout ce que porte la copie, il
    /// n'y a rien à reprendre : elle est effacée. Sinon elle reste, et la
    /// prochaine ouverture du même monde la reprendra.
    pub fn fermer(mut self) -> Result<Fermeture, ErreurSeance> {
        let etats = self.staging.etats()?;
        let regions = etats.iter().filter(|(_, e)| e.porte_du_travail()).count();
        if regions > 0 {
            // Ce qui reste ne garde que le travail : une région sans travail
            // gardée jusqu'à la prochaine séance y deviendrait un conflit si
            // le joueur y jouait entre-temps.
            let rendre: Vec<Cle> = etats
                .into_iter()
                .filter(|(_, e)| !e.porte_du_travail())
                .map(|(c, _)| c)
                .collect();
            self.staging.rafraichir_regions(&rendre)?;
            return Ok(Fermeture::Gardee { regions });
        }
        // Tout lâcher avant d'effacer : Windows n'efface pas un fichier tenu.
        self.journal = None;
        self._verrou = None;
        fs::remove_dir_all(&self.dossier)?;
        Ok(Fermeture::Effacee)
    }
}

/// **Où vivent les séances**, par défaut.
///
/// `TITIFORGE_SEANCES` l'emporte. Sinon le dossier de données LOCAL de
/// l'utilisateur — surtout pas la save : `sauvegarder` copie la save entière
/// avant chaque écriture, et y emporterait la copie de travail à chaque fois.
pub fn racine_par_defaut() -> Option<PathBuf> {
    racine_selon(&|n| std::env::var_os(n))
}

/// [`racine_par_defaut`], l'environnement étant DONNÉ — pour se tester sans
/// toucher aux variables du processus, que les tests partagent.
pub fn racine_selon(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    // Une variable VIDE vaut absente — c'est la règle XDG, et la seule sûre :
    // `XDG_DATA_HOME=` prise au mot rangerait les séances dans un chemin
    // RELATIF, c'est-à-dire là où l'application a été lancée.
    let var = |n: &str| env(n).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = var("TITIFORGE_SEANCES") {
        return Some(p);
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share")))
    }?;
    Some(base.join("titiforge").join("seances"))
}

/// **Le dossier de la séance d'une save** : son nom, lisible, et l'empreinte
/// de son chemin CANONIQUE — deux saves du même nom dans deux dossiers sont
/// deux séances, et le même monde ouvert par deux chemins en est une seule.
pub fn dossier_de(racine: &Path, monde: &Path) -> PathBuf {
    let canon = chemin_canonique(monde);
    let nom: String = canon
        .file_name()
        .map(|n| {
            n.to_string_lossy()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .take(40)
                .collect()
        })
        .unwrap_or_default();
    let h = crate::journal::empreinte(canon.to_string_lossy().as_bytes());
    racine.join(format!("{nom}-{h:016x}"))
}

fn chemin_canonique(p: &Path) -> PathBuf {
    fs::canonicalize(p)
        .or_else(|_| std::path::absolute(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

fn texte_du_chemin(monde: &Path) -> String {
    chemin_canonique(monde).display().to_string()
}

/// Prend le verrou de la séance, ou dit qu'une autre fenêtre le tient.
///
/// Un système de fichiers qui ne sait pas verrouiller (certains partages
/// réseau) ne permet pas de SAVOIR ; refuser y rendrait l'application
/// inutilisable. On continue sans verrou, comme avant qu'il existe.
fn verrouiller(dossier: &Path) -> Result<Option<File>, ErreurSeance> {
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dossier.join(VERROU))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(fs::TryLockError::WouldBlock) => Err(ErreurSeance::DejaOuverte(dossier.to_path_buf())),
        Err(fs::TryLockError::Error(_)) => Ok(None),
    }
}

/// Relit le journal de la séance, en coupant ce qu'un arrêt brutal a laissé
/// à moitié écrit.
///
/// Couper n'est pas du ménage : un enregistrement tronqué ARRÊTE la lecture,
/// donc tout ce qu'on ajouterait derrière lui ne se relirait jamais — la
/// séance suivante perdrait tout ce que celle-ci aurait fait.
fn lire_journal(dossier: &Path) -> Result<Journal, ErreurSeance> {
    let p = dossier.join(JOURNAL);
    let octets = match fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    if octets.is_empty() {
        fs::write(&p, entete())?;
        return Ok(Journal::new());
    }
    match decoder(&octets) {
        Ok((j, valides)) => {
            if valides < octets.len() {
                OpenOptions::new()
                    .write(true)
                    .open(&p)?
                    .set_len(valides as u64)?;
            }
            Ok(j)
        }
        Err(_) => {
            // Pas un journal : mis de côté plutôt qu'effacé, et l'annulation
            // repart de zéro. La copie de travail, elle, est intacte.
            fs::rename(
                &p,
                dossier.join(format!("{JOURNAL}.illisible-{}", horodatage())),
            )?;
            fs::write(&p, entete())?;
            Ok(Journal::new())
        }
    }
}

/// Déplace le contenu de la séance dans un dossier VOISIN, horodaté. Le
/// verrou reste : la séance continue, vide.
fn mettre_de_cote(dossier: &Path) -> io::Result<PathBuf> {
    let nom = dossier
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = format!("{nom}.mise-de-cote-{}", horodatage());
    let mut vers = dossier.with_file_name(&base);
    let mut n = 1;
    while vers.exists() {
        vers = dossier.with_file_name(format!("{base}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&vers)?;
    for nom in [COUCHE, JOURNAL, MONDE, EN_COURS] {
        let de = dossier.join(nom);
        if de.exists() {
            fs::rename(&de, vers.join(nom))?;
        }
    }
    Ok(vers)
}

fn horodatage() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
