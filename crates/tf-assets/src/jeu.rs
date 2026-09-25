//! Trouver l'installation de Minecraft d'un utilisateur, et ce qu'elle porte.
//!
//! **On ne redistribue PAS les assets de Mojang.** Les embarquer dans un
//! installeur exposerait celui qui le diffuse — l'EULA l'interdit. On lit
//! l'installation, comme WorldPainter, Amulet et Litematica. Et le résultat est
//! MEILLEUR qu'un pack embarqué : le dossier d'un launcher contient aussi les
//! **packs du serveur**, donc les blocs `minefield:*` arrivent avec leurs
//! textures sans rien demander.
//!
//! Deux règles payées cher dans `ExeWorldEdit`, et qui ne sont pas celles
//! qu'on croit :
//!
//! 1. **Chercher « .minecraft » ne trouve pas l'installation.** Un serveur a
//!    son propre launcher : celui de l'utilisateur de référence est
//!    `%APPDATA%\.minefield_1_18`. Le critère est la présence d'un dossier
//!    `versions/`, jamais le nom.
//! 2. **Trier des versions en TEXTE choisit la mauvaise.** « 1.9 » passe après
//!    « 1.18 » en lexicographique, et on ouvrirait les textures d'une version
//!    de 2016. La comparaison est numérique composant par composant — et une
//!    publication passe avant une capture instantanée, `24w14a` triant plus
//!    haut que `1.21` sur le seul texte.

use std::path::{Path, PathBuf};

use crate::source::{Source, SourceError};
use crate::Archive;

/// Une version installée : son nom et son `.jar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub nom: String,
    pub jar: PathBuf,
}

/// Ce qu'une installation offre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub racine: PathBuf,
    /// Les versions qui portent un `.jar`, **de la plus récente à la plus
    /// ancienne**.
    pub versions: Vec<Version>,
    /// Les packs de ressources déposés par l'utilisateur ou par le serveur.
    pub packs: Vec<PathBuf>,
}

/// Vraie si ce dossier est une installation.
///
/// Le critère est `versions/`, jamais le nom : un serveur a son propre
/// launcher, et le chercher par « .minecraft » ne le trouve pas.
pub fn est_une_installation(racine: impl AsRef<Path>) -> bool {
    racine.as_ref().join("versions").is_dir()
}

/// **Les installations sous ces dossiers** — chacun d'eux, et ses enfants
/// directs. Triées, sans doublon.
///
/// Le critère reste `versions/`, jamais le nom : c'est ce qui trouve
/// `%APPDATA%\.minefield_1_18` à côté de `.minecraft`. Un dossier illisible
/// est sauté, pas une erreur — chercher ne doit pas empêcher d'ouvrir.
pub fn installations_sous(dossiers: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dossiers {
        if est_une_installation(d) {
            out.push(d.clone());
        }
        let Ok(entrees) = std::fs::read_dir(d) else {
            continue;
        };
        for e in entrees.flatten() {
            let p = e.path();
            if p.is_dir() && est_une_installation(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// **Où chercher les installations** sur cette machine : là où les
/// launchers les posent — `%APPDATA%` sous Windows, `Application Support`
/// sous macOS, le dossier personnel ailleurs (`~/.minecraft`).
pub fn dossiers_ou_chercher() -> Vec<PathBuf> {
    // Vide vaut absente : chercher dans « » listerait le dossier courant.
    let var = |n: &str| {
        std::env::var_os(n)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let mut v = Vec::new();
    if cfg!(windows) {
        v.extend(var("APPDATA"));
    } else if cfg!(target_os = "macos") {
        v.extend(var("HOME").map(|h| h.join("Library/Application Support")));
    } else {
        v.extend(var("HOME"));
    }
    v
}

/// Ce qu'une installation contient. Ne lit aucun `.jar` — seulement les noms.
pub fn inspecter(racine: impl AsRef<Path>) -> Result<Installation, SourceError> {
    let racine = racine.as_ref().to_path_buf();
    if !est_une_installation(&racine) {
        return Err(SourceError::Absent(format!(
            "{} : pas de dossier `versions/`, donc pas une installation",
            racine.display()
        )));
    }
    let mut versions: Vec<Version> = Vec::new();
    if let Ok(entrees) = std::fs::read_dir(racine.join("versions")) {
        for e in entrees.flatten() {
            let Some(nom) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            // Un dossier de version porte un `.jar` du MÊME nom. Sans ce
            // `.jar`, la version est déclarée mais pas téléchargée.
            let jar = e.path().join(format!("{nom}.jar"));
            if jar.is_file() {
                versions.push(Version { nom, jar });
            }
        }
    }
    versions.sort_by(|a, b| ordre(&b.nom, &a.nom));

    let mut packs: Vec<PathBuf> = Vec::new();
    for dossier in ["resourcepacks", "server-resource-packs"] {
        if let Ok(entrees) = std::fs::read_dir(racine.join(dossier)) {
            for e in entrees.flatten() {
                let p = e.path();
                // Un pack est un `.zip`, ou un dossier dépaqueté.
                if p.is_dir() || p.extension().map(|x| x == "zip").unwrap_or(false) {
                    packs.push(p);
                }
            }
        }
    }
    packs.sort();
    Ok(Installation {
        racine,
        versions,
        packs,
    })
}

/// Compare deux noms de version. Plus grand = plus récent.
///
/// Composant par composant et en NUMÉRIQUE : « 1.9 » est plus vieux que
/// « 1.18 », ce qu'un tri lexicographique dit exactement à l'envers. Et une
/// publication passe avant une capture instantanée — `24w14a` commence par un
/// nombre plus grand que `1.21` et gagnerait sans ça.
pub fn ordre(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (publication(a), publication(b)) {
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let mut ca = a
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty());
    let mut cb = b
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty());
    loop {
        match (ca.next(), cb.next()) {
            (None, None) => break,
            // « 1.18 » est plus récent que « 1.18.2 » ? Non : un composant en
            // plus veut dire plus tard.
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let (x, y) = (x.parse::<u64>().unwrap_or(0), y.parse::<u64>().unwrap_or(0));
                if x != y {
                    return x.cmp(&y);
                }
            }
        }
    }
    // À composants égaux, le texte départage — pour que l'ordre soit TOTAL et
    // qu'un tri soit reproductible.
    a.cmp(b)
}

/// Une capture instantanée se reconnaît à son `w` de semaine (`24w14a`), et
/// une préversion à son `pre` ou `rc`.
fn publication(v: &str) -> bool {
    let bas = v.to_ascii_lowercase();
    !(bas.contains('w') || bas.contains("pre") || bas.contains("rc"))
}

impl Installation {
    /// La pile de sources, **dans l'ordre où elle doit recouvrir**.
    ///
    /// Les packs de l'utilisateur et du serveur d'abord, le `.jar` du jeu en
    /// dernier : c'est ce qui fait qu'un bloc `minefield:*` prend sa texture du
    /// serveur, et `minecraft:stone` celle du jeu. L'ordre est EXPLICITE — une
    /// pile qui chercherait « au mieux » rendrait le résultat imprévisible
    /// selon ce qui traîne sur le disque.
    ///
    /// `version` choisit laquelle ouvrir ; `None` prend la plus récente.
    pub fn pile(&self, version: Option<&str>) -> Result<crate::Pile, SourceError> {
        let mut sources: Vec<Box<dyn Source>> = Vec::new();
        for p in &self.packs {
            // Un pack illisible ne doit pas empêcher d'ouvrir les autres : un
            // dossier `resourcepacks` contient ce que l'utilisateur y a mis.
            match ouvrir_pack(p) {
                Ok(s) => sources.push(s),
                Err(e) => eprintln!("pack ignoré — {e}"),
            }
        }
        let v = match version {
            Some(n) => self.versions.iter().find(|v| v.nom == n),
            None => self.versions.first(),
        };
        let Some(v) = v else {
            return Err(SourceError::Absent(format!(
                "{} : aucune version téléchargée",
                self.racine.display()
            )));
        };
        sources.push(Box::new(Archive::ouvrir(&v.jar)?));
        Ok(crate::Pile::new(sources))
    }
}

/// Un pack, qu'il soit dépaqueté ou en archive.
fn ouvrir_pack(p: &Path) -> Result<Box<dyn Source>, SourceError> {
    if p.is_dir() {
        Ok(Box::new(crate::Dossier::ouvrir(p)?))
    } else {
        Ok(Box::new(Archive::ouvrir(p)?))
    }
}

/// Ce qu'on peut désigner à titiforge comme source d'assets.
///
/// L'utilisateur donne un chemin ; c'est à nous de reconnaître ce que c'est.
/// Lui demander de choisir entre « codex », « pack » et « installation »
/// serait lui demander de connaître nos formats internes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Genre {
    /// Une installation de launcher : `versions/` + les packs à côté.
    Installation,
    /// Un pack Minecraft, en dossier ou en archive : `assets/<ns>/...`.
    Pack,
    /// Le codex extrait du site : un seul `blockstates.json`.
    Codex,
}

/// Reconnaît ce qu'un chemin désigne, et l'ouvre.
///
/// L'ordre des essais compte : une installation contient des packs, et un pack
/// dépaqueté contient `assets/`. On teste donc du plus englobant au plus
/// précis, sinon une installation serait prise pour un pack et on perdrait les
/// packs du serveur.
pub fn ouvrir(chemin: impl AsRef<Path>) -> Result<(crate::Pile, Genre), SourceError> {
    let chemin = chemin.as_ref();
    if est_une_installation(chemin) {
        let i = inspecter(chemin)?;
        return Ok((i.pile(None)?, Genre::Installation));
    }
    let source = ouvrir_pack(chemin)?;
    // Le genre se lit dans le CONTENU, pas dans l'extension : un codex livré
    // en `.zip` et un pack livré en dossier existent tous les deux.
    let genre = if source.lire("blockstates.json").is_ok() {
        Genre::Codex
    } else {
        Genre::Pack
    };
    Ok((crate::Pile::new(vec![source]), genre))
}

impl Genre {
    pub fn disposition(self) -> crate::catalogue::Disposition {
        match self {
            Genre::Codex => crate::catalogue::Disposition::Codex,
            _ => crate::catalogue::Disposition::Pack,
        }
    }
}

/// Charge un catalogue depuis ce qu'un chemin désigne — codex, pack ou
/// installation — et résout ses modèles.
///
/// C'est le point d'entrée que les outils appellent : ils n'ont plus à savoir
/// quelle disposition va avec quoi, et ils acceptent donc les trois sans une
/// ligne de plus.
pub fn catalogue(
    chemin: impl AsRef<Path>,
) -> Result<(crate::Catalogue, crate::Pile, Genre), String> {
    let (pile, genre) = ouvrir(chemin).map_err(|e| e.to_string())?;
    let mut cat = crate::Catalogue::new(genre.disposition());
    match genre {
        Genre::Codex => cat.charger_codex(&pile).map(|_| ())?,
        _ => cat.charger_pack(&pile).map(|_| ())?,
    }
    cat.resoudre_modeles(&pile);
    Ok((cat, pile, genre))
}
