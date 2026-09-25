//! **Ouvrir un monde sans ligne de commande.**
//!
//! Ce qui se DÉCIDE est ici, en types purs, et se teste sans fenêtre : quelles
//! saves proposer, lesquelles portent du travail pas encore écrit, ce qu'un
//! chemin collé désigne vraiment. L'interface ne fait que le dessiner, et la
//! coque que l'exécuter (`Accueil::demande`).
//!
//! Trois portes, parce qu'il y a trois façons de désigner un monde : la
//! liste des saves des installations trouvées sur la machine, les mondes
//! ouverts récemment, et un chemin — collé, ou un dossier glissé sur la
//! fenêtre.

use std::path::{Path, PathBuf};

use tf_world::niveau::{saves_de, SaveTrouvee};

/// Combien de mondes récents on retient.
pub const MAX_RECENTS: usize = 10;

/// Une save telle que l'accueil la montre.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveVue {
    pub save: SaveTrouvee,
    /// **Une séance existe pour ce monde** : du travail n'y est pas encore
    /// écrit. Le dire dans la liste, c'est éviter qu'on croie l'avoir perdu —
    /// ou qu'on joue dans le monde en croyant qu'il porte déjà les
    /// modifications.
    pub seance_en_cours: bool,
}

/// Une installation et ses saves.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallationVue {
    pub racine: PathBuf,
    pub nom: String,
    pub saves: Vec<SaveVue>,
}

/// L'écran d'accueil.
#[derive(Debug, Clone, Default)]
pub struct Accueil {
    /// Est-il affiché ?
    pub ouvert: bool,
    pub installations: Vec<InstallationVue>,
    /// Les mondes ouverts récemment, le plus récent d'abord — seulement ceux
    /// qui sont encore des saves.
    pub recents: Vec<SaveVue>,
    /// Un chemin tapé ou collé.
    pub chemin: String,
    /// Le monde choisi, que la coque prendra.
    pub demande: Option<PathBuf>,
    /// Ce qui n'a pas marché, à dire.
    pub erreur: Option<String>,
}

impl Accueil {
    /// **Explore la machine** : les saves de ces installations, et les
    /// récents qui existent encore. `seances` est la racine des séances,
    /// pour signaler le travail pas encore écrit.
    pub fn explorer(
        installations: &[PathBuf],
        seances: Option<&Path>,
        recents: &[PathBuf],
    ) -> Self {
        let installations = installations
            .iter()
            .map(|r| InstallationVue {
                racine: r.clone(),
                nom: r
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| r.display().to_string()),
                saves: saves_de(r).into_iter().map(|s| vue(s, seances)).collect(),
            })
            .collect();
        let recents = recents
            .iter()
            .filter(|p| tf_world::FsSource::looks_like_world(p))
            .map(|p| {
                let nom = tf_world::niveau::lire_fichier(p)
                    .and_then(|n| n.nom)
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                vue(
                    SaveTrouvee {
                        chemin: p.clone(),
                        nom,
                        derniere_partie: None,
                    },
                    seances,
                )
            })
            .collect();
        Accueil {
            installations,
            recents,
            ..Default::default()
        }
    }

    /// Combien de saves en tout — de quoi dire « aucune » plutôt qu'une liste
    /// vide.
    pub fn nombre_de_saves(&self) -> usize {
        self.installations.iter().map(|i| i.saves.len()).sum()
    }

    /// **Choisit un monde.** Refuse ce qui n'est pas une save, en le disant :
    /// ouvrir un dossier quelconque montrerait un monde vide, et on croirait
    /// l'outil cassé.
    ///
    /// Le `level.dat` lui-même désigne son dossier : c'est souvent lui qu'on
    /// glisse ou qu'on copie.
    pub fn choisir(&mut self, chemin: PathBuf) {
        let chemin = if chemin.file_name().is_some_and(|n| n == "level.dat") {
            chemin.parent().map(Path::to_path_buf).unwrap_or(chemin)
        } else {
            chemin
        };
        if tf_world::FsSource::looks_like_world(&chemin) {
            self.erreur = None;
            self.demande = Some(chemin);
        } else {
            self.demande = None;
            self.erreur = Some(format!(
                "{} n'est pas une save : il n'y a pas de level.dat dedans. Une save \
                 est le dossier d'un monde, dans `saves/` de l'installation",
                chemin.display()
            ));
        }
    }

    /// Choisit le chemin TAPÉ. Les guillemets autour sont retirés : c'est ce
    /// que met « Copier en tant que chemin d'accès » de l'explorateur de
    /// Windows, et les laisser ferait chercher un dossier dont le nom commence
    /// par un guillemet.
    pub fn choisir_texte(&mut self) {
        let t = self.chemin.trim().trim_matches('"').trim();
        if t.is_empty() {
            self.erreur = Some("aucun chemin".into());
            return;
        }
        self.choisir(PathBuf::from(t));
    }
}

fn vue(save: SaveTrouvee, seances: Option<&Path>) -> SaveVue {
    let seance_en_cours = seances.is_some_and(|r| {
        tf_world::session::dossier_de(r, &save.chemin)
            .join("couche")
            .is_dir()
    });
    SaveVue {
        save,
        seance_en_cours,
    }
}

/// Ce que la coque sait de la machine en démarrant : d'où viennent les
/// assets, où vivent les séances, ce qu'on peut ouvrir.
pub struct Depart {
    /// La racine des assets en service.
    pub assets: String,
    /// L'utilisateur les a-t-il DÉSIGNÉS ? Alors ils servent pour tout monde.
    /// Sinon, un monde d'une autre installation prend les siens — ce sont ses
    /// packs de serveur qui portent ses textures.
    pub assets_designes: bool,
    pub installations: Vec<std::path::PathBuf>,
    pub seances: Option<std::path::PathBuf>,
    /// Ce qu'il faut dire à la première image.
    pub message: Option<String>,
    /// Montrer l'accueil dès l'ouverture : aucun monde n'a été demandé.
    pub accueil: bool,
}

// ── les mondes récents, sur disque ──────────────────────────────────────────

/// Le fichier des récents, à côté des séances : `<données>/titiforge/recents.txt`.
pub fn fichier_recents(seances: &Path) -> Option<PathBuf> {
    Some(seances.parent()?.join("recents.txt"))
}

/// Un chemin par ligne. Absent ou illisible : aucun récent — ce n'est pas une
/// raison de refuser d'ouvrir.
pub fn lire_recents(fichier: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(fichier)
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .take(MAX_RECENTS)
                .collect()
        })
        .unwrap_or_default()
}

/// Met ce monde en tête des récents — une seule fois, et pas plus de
/// [`MAX_RECENTS`].
pub fn noter_recent(fichier: &Path, monde: &Path) -> std::io::Result<()> {
    let mut v = lire_recents(fichier);
    v.retain(|p| p != monde);
    v.insert(0, monde.to_path_buf());
    v.truncate(MAX_RECENTS);
    if let Some(d) = fichier.parent() {
        std::fs::create_dir_all(d)?;
    }
    let texte: Vec<String> = v.iter().map(|p| p.display().to_string()).collect();
    std::fs::write(fichier, texte.join("\n") + "\n")
}

// ── où ouvrir, et avec quels assets ─────────────────────────────────────────

/// **La zone chargée à l'ouverture**, en chunks : les 3 × 3 autour de là où
/// l'on joue (`Niveau::ou_regarder`), ou l'origine quand `level.dat` ne dit
/// rien. Le reste arrive en volant : la caméra pilote le chargement.
pub fn zone_d_ouverture(niveau: Option<&tf_world::niveau::Niveau>) -> [i32; 4] {
    match niveau.and_then(|n| n.ou_regarder()) {
        Some([x, _, z]) => {
            let (cx, cz) = (tf_world::floor_div(x, 16), tf_world::floor_div(z, 16));
            [cx - 1, cz - 1, cx + 1, cz + 1]
        }
        None => [0, 0, 1, 1],
    }
}

/// L'installation qui porte cette save — `<installation>/saves/<monde>` — ou
/// `None` pour un monde rangé ailleurs. C'est d'elle que viennent les bonnes
/// textures : les packs du serveur y sont.
pub fn installation_de(monde: &Path) -> Option<PathBuf> {
    let saves = monde.parent()?;
    if saves.file_name()? != "saves" {
        return None;
    }
    let i = saves.parent()?;
    tf_assets::est_une_installation(i).then(|| i.to_path_buf())
}

/// **Les assets à prendre au démarrage**, quand l'utilisateur n'en a désigné
/// aucun : l'installation du monde demandé s'il vient d'une, sinon celle dont
/// une save a été jouée le plus récemment — c'est celle dont on se sert.
///
/// Une installation dont aucune version n'est téléchargée n'a pas de textures
/// à donner : elle est écartée — `versions/` dit qu'un launcher est passé par
/// là, pas que le jeu y a été installé.
pub fn assets_par_defaut(installations: &[PathBuf], monde: Option<&Path>) -> Option<PathBuf> {
    let utilisable = |i: &PathBuf| {
        tf_assets::inspecter(i)
            .map(|x| !x.versions.is_empty())
            .unwrap_or(false)
    };
    if let Some(i) = monde.and_then(installation_de).filter(utilisable) {
        return Some(i);
    }
    installations
        .iter()
        .filter(|i| utilisable(i))
        .max_by_key(|i| {
            saves_de(i)
                .first()
                .and_then(|s| s.derniere_partie)
                .unwrap_or(std::time::UNIX_EPOCH)
        })
        .cloned()
}
