//! **L'état de la coque, et rien qui dessine.**
//!
//! Tout ce que l'interface montre et modifie vit ici, en types purs : le mode
//! de travail, la sélection, le pilotage de la caméra, les réglages du
//! quadrillage. `interface.rs` le LIT et le modifie ; il ne décide rien.
//!
//! La séparation n'est pas de la coquetterie. Elle permet de vérifier ce que
//! la coque FAIT sans ouvrir une fenêtre — un morceau d'interface qui n'existe
//! que derrière un serveur graphique ne se teste pas, et ce dépôt a déjà
//! tranché la question pour le rendu.

use tf_ops::catalogue::{self, Descripteur, Params, Saisie, Valeur};
use tf_render::controles::{Mode, Vue};
use tf_world::coords::BlockPos;
use tf_world::decoupe::Niveau;
use tf_world::inference::{accrocher, Accroche, Raison, TOLERANCE};
use tf_world::selection::{glissement, tranche, Direction, Selection};

/// Ce que le quadrillage montre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quadrillage {
    /// Rayon en CELLULES autour de la caméra. `None` = éteint.
    pub chunks: Option<u32>,
    pub mca: Option<u32>,
}

impl Default for Quadrillage {
    fn default() -> Self {
        // Les chunks allumés, les `.mca` éteints : le premier sert à chaque
        // geste de construction, le second à décider d'un export. Allumer les
        // deux d'office donnerait un écran illisible au premier lancement.
        Quadrillage {
            chunks: Some(2),
            mca: None,
        }
    }
}

/// Ce que le CURSEUR désigne, tel que la dernière image l'a trouvé.
///
/// **Sous la souris, pas au centre de l'écran.** La souris est libre — la
/// caméra tourne à la molette enfoncée —, c'est donc elle qui montre. Un
/// réticule central obligeait à tourner toute la vue pour viser un bloc :
/// l'habitude du jeu, où la souris EST la caméra, transportée là où elle ne
/// l'est plus. C'est le premier retour de l'essai sous Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SousLeCurseur {
    /// La case qui arrête le rayon — celle qu'on casserait.
    pub case: Option<BlockPos>,
    /// Celle d'avant — celle où l'on poserait. Les deux, parce que poser et
    /// casser ne visent pas la même.
    pub pose: Option<BlockPos>,
    /// Ce que l'accrochage a fait de `pose`, et pourquoi.
    pub accroche: Option<Accroche>,
}

/// **L'atelier : l'opération choisie et ses paramètres.**
///
/// Rien ici ne connaît egui. L'interface LIT le descripteur et engendre ses
/// champs ; elle ne sait pas quelles opérations existent, et c'est tout
/// l'intérêt — ajouter une opération au moteur ne demande pas une ligne de
/// renderer. `ExeWorldEdit` l'a prouvé dans l'autre sens : chaque formulaire
/// écrit à la main est un formulaire à réécrire.
#[derive(Debug, Clone)]
pub struct Atelier {
    /// L'identifiant de l'opération choisie. Toujours valide : il vient du
    /// catalogue et `choisir` refuse ce qu'il ne connaît pas.
    op: &'static str,
    /// Ses paramètres, tenus à jour par l'interface.
    pub params: Params,
    /// Ce qui filtre la palette. On tape « //walls » ou « mur ».
    pub recherche: String,
}

impl Default for Atelier {
    fn default() -> Self {
        // La première du catalogue, jamais une constante écrite à part.
        // **Une opération qui n'appartient pas à l'outil affiché** est un
        // piège déjà payé : `tool: 'select'` et `operation: 'set'` étaient
        // deux constantes indépendantes, et la liste a fini par afficher
        // « Copier » pendant que le bouton disait « Remplir ».
        let d = &catalogue::OPS[0];
        Atelier {
            op: d.id,
            params: d.defauts(),
            recherche: String::new(),
        }
    }
}

impl Atelier {
    pub fn descripteur(&self) -> &'static Descripteur {
        catalogue::descripteur(self.op).expect("l'identifiant vient du catalogue")
    }

    pub fn op(&self) -> &'static str {
        self.op
    }

    /// Change d'opération, et REPART de ses défauts.
    ///
    /// Garder les paramètres de la précédente paraissait aimable ; c'est
    /// faux : deux opérations qui partagent un nom de paramètre ne lui
    /// donnent pas forcément le même sens, et `normaliser` refuserait de
    /// toute façon ce qui ne lui appartient pas. Un formulaire qui garde une
    /// valeur d'une autre opération est un formulaire qui ment.
    pub fn choisir(&mut self, id: &str) -> bool {
        let Some(d) = catalogue::descripteur(id) else {
            return false;
        };
        self.op = d.id;
        self.params = d.defauts();
        true
    }

    /// La valeur d'un paramètre, telle que l'interface doit l'afficher.
    /// Jamais absente : le descripteur en donne le défaut, et ce qui n'en a
    /// pas reçoit une valeur VIDE — un champ sans valeur ne se dessine pas.
    pub fn valeur(&self, nom: &str) -> Valeur {
        if let Some(v) = self.params.get(nom) {
            return v.clone();
        }
        match self.descripteur().param(nom).map(|p| p.saisie) {
            Some(Saisie::Bloc) | Some(Saisie::Biome) => Valeur::Texte(String::new()),
            Some(Saisie::Melange) => Valeur::Melange(Vec::new()),
            Some(Saisie::Entier { min, .. }) => Valeur::Entier(min),
            Some(Saisie::Vecteur) => Valeur::Vecteur([0; 3]),
            Some(Saisie::Direction) => Valeur::Direction(Direction::PlusX),
            Some(Saisie::Transformation) => Valeur::Transformation(None),
            None => Valeur::Texte(String::new()),
        }
    }

    /// Ce que l'opération ferait à cette sélection — ou ce qui l'en empêche.
    ///
    /// **Rendu même quand tout va bien** : un panneau qui ne parle que pour
    /// refuser laisse croire qu'il ne sait rien dire.
    pub fn verdict(&self, sel: Option<&ResumeSelection>) -> Vec<Note> {
        let d = self.descripteur();
        let mut out = Vec::new();

        // Ce qui manque pour pouvoir lancer, nommé.
        match catalogue::normaliser(d, &self.params) {
            Ok(_) => {}
            Err(e) => out.push(Note::Bloquant(e.to_string())),
        }

        let Some(s) = sel else {
            out.push(Note::Bloquant("aucune sélection".into()));
            return out;
        };

        // **Ce qui matérialise toute la sélection doit l'ANNONCER avant de
        // commencer.** `vec![]` n'échoue pas gentiment : une allocation
        // refusée ABANDONNE le processus, et l'éditeur disparaîtrait avec le
        // travail en cours.
        if d.cout.materialise {
            let boite = tf_world::coords::BBox::new(s.min, s.max);
            match tf_ops::edition::verifier_materialisable(&boite, tf_ops::edition::OCTETS_CREUSAGE)
            {
                Ok(cases) => out.push(Note::Attention(format!(
                    "matérialise {} cases — {}",
                    cases,
                    octets(cases * tf_ops::edition::OCTETS_CREUSAGE)
                ))),
                Err(e) => out.push(Note::Bloquant(e.to_string())),
            }
        }

        if d.cout.colonne {
            out.push(Note::Info(
                "lit la COLONNE entière : ni étage section ni étage palette".into(),
            ));
        } else {
            out.push(Note::Info(s.verdict()));
        }
        out
    }
}

/// Ce que l'atelier a à dire, par gravité. Trois niveaux et pas un de plus :
/// ce qui empêche, ce qui coûte, ce qui informe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Bloquant(String),
    Attention(String),
    Info(String),
}

impl Note {
    pub fn texte(&self) -> &str {
        match self {
            Note::Bloquant(s) | Note::Attention(s) | Note::Info(s) => s,
        }
    }

    pub fn bloque(&self) -> bool {
        matches!(self, Note::Bloquant(_))
    }
}

/// Des octets, dits comme un humain les lit.
pub fn octets(n: u64) -> String {
    const UNITES: [&str; 4] = ["o", "ko", "Mo", "Go"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < UNITES.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} o")
    } else {
        format!("{v:.1} {}", UNITES[i])
    }
}

/// **Ce que le clic gauche fait, en Conception.**
///
/// Le mode dit ce qu'on manipule ; l'outil dit avec quoi. Deux constantes
/// indépendantes finiraient par diverger — `ExeWorldEdit` a livré une liste
/// affichant « Copier » pendant que le bouton disait « Remplir » — donc
/// l'outil est porté par l'état, la barre le LIT, et le clic l'interroge.
///
/// Trois outils, et pas un de plus pour l'instant : c'est le minimum qui
/// rende l'inférence ATTEIGNABLE. Elle était écrite, testée, affichée — et
/// aucun geste ne s'en servait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outil {
    /// Attraper une face de la sélection et la tirer.
    #[default]
    Tirer,
    /// Poser un bloc devant ce qu'on vise, à l'endroit que l'inférence
    /// désigne.
    Poser,
    /// Casser le bloc visé.
    Casser,
    /// Poser le COMPOSANT choisi, et tenir ses instances : les mettre à jour,
    /// les détacher.
    Composant,
    /// Coller le PRESSE-PAPIERS — un fichier importé, ou la sélection copiée.
    Coller,
}

impl Outil {
    pub const TOUS: [Outil; 5] = [
        Outil::Tirer,
        Outil::Poser,
        Outil::Casser,
        Outil::Composant,
        Outil::Coller,
    ];

    pub const fn nom(self) -> &'static str {
        match self {
            Outil::Tirer => "Tirer une face",
            Outil::Poser => "Poser",
            Outil::Casser => "Casser",
            Outil::Composant => "Composant",
            Outil::Coller => "Coller",
        }
    }

    /// Ce que la barre annonce. **Le clic droit garde toujours le même sens
    /// dans un outil donné** — un bouton qui change de rôle selon l'outil est
    /// un bouton qu'on n'ose plus cliquer.
    pub const fn legende(self) -> &'static str {
        match self {
            Outil::Tirer => "gauche : tirer une FACE · droit : abandonner",
            Outil::Poser => "gauche : poser · droit : casser",
            Outil::Casser => "gauche : casser · droit : poser",
            Outil::Composant => {
                "gauche : poser le composant choisi · droit : le tourner d'un quart de tour"
            }
            Outil::Coller => {
                "gauche : coller le presse-papiers · droit : le tourner d'un quart de tour"
            }
        }
    }
}

// ── les échanges ────────────────────────────────────────────────────────────

/// Un fichier d'échange trouvé sur la machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trouve {
    pub chemin: std::path::PathBuf,
    pub nom: String,
    pub octets: u64,
}

/// **Ce que le panneau des échanges retient** — exporter la sélection,
/// importer un fichier.
#[derive(Debug, Clone, PartialEq)]
pub struct Echanges {
    pub format: tf_formats::Format,
    /// Le nom du fichier, sans extension — il deviendra un nom de FICHIER
    /// (`nom_de_fichier`).
    pub nom: String,
    /// Où exporter. Proposé selon le format — là où l'outil qui le lit le
    /// cherche — et modifiable.
    pub dossier: String,
    /// Un chemin tapé ou collé, pour importer.
    pub a_importer: String,
    /// Les fichiers d'échange trouvés, du plus récent au plus ancien.
    pub trouves: Vec<Trouve>,
    /// Coller écrase-t-il avec l'AIR de l'extrait ? Non par défaut, comme
    /// WorldEdit : on colle un bâtiment sur un terrain, pas un cube d'air.
    pub avec_air: bool,
    /// Le `DataVersion` du monde ouvert (`level.dat`) : ce qu'un export
    /// emporte, et ce à quoi un import se compare.
    pub version_monde: Option<i32>,
    /// L'installation du monde ouvert, et le monde lui-même : les dossiers
    /// d'échange par défaut en dépendent.
    pub installation: Option<std::path::PathBuf>,
    pub monde: Option<std::path::PathBuf>,
}

impl Default for Echanges {
    fn default() -> Self {
        Echanges {
            // Litematica d'abord : pour bâtir en survie sur un serveur, c'est
            // lui qui montre le build à reproduire.
            format: tf_formats::Format::Litematic,
            nom: String::new(),
            dossier: String::new(),
            a_importer: String::new(),
            trouves: Vec::new(),
            avec_air: false,
            version_monde: None,
            installation: None,
            monde: None,
        }
    }
}

/// Minecraft 1.18.2 : le `DataVersion` qu'on suppose quand le monde ne dit
/// pas le sien — la version du serveur que ce projet sert d'abord.
pub const DV_PAR_DEFAUT: i32 = 2975;

/// **Où un format se range**, par défaut : là où l'outil qui le lit le
/// cherche. Litematica lit `schematics/` à la racine de l'installation,
/// WorldEdit en solo `config/worldedit/schematics/`, et un bloc de structure
/// charge `minecraft:<nom>` depuis `generated/minecraft/structures/` DU
/// MONDE.
pub fn dossier_par_defaut(
    format: tf_formats::Format,
    installation: Option<&std::path::Path>,
    monde: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    use tf_formats::Format;
    match format {
        Format::Litematic => installation.map(|i| i.join("schematics")),
        Format::SpongeV2 | Format::SpongeV3 => {
            installation.map(|i| i.join("config").join("worldedit").join("schematics"))
        }
        Format::Structure => {
            monde.map(|m| m.join("generated").join("minecraft").join("structures"))
        }
    }
}

/// **Un nom qui devient un nom de FICHIER**, sous Windows comme ailleurs.
///
/// On ne retire que ce qu'un système de fichiers refuse VRAIMENT : les
/// caractères interdits de Windows et les contrôles, les noms de périphériques
/// réservés (`CON`, `LPT1`…), un point ou une espace en fin de nom. Jamais
/// `\w` : « Vallée » deviendrait `Vall_e`, et deux builds coréens sortiraient
/// sous le même nom — le piège qu'`ExeWorldEdit` a payé.
///
/// Windows juge un nom de périphérique sur ce qui précède le PREMIER point,
/// espaces de fin retirées : `CON.txt` et `CON .txt` sont la console. Et les
/// ports vont de 0 à 9, plus `¹`, `²` et `³`.
pub fn nom_de_fichier(nom: &str) -> String {
    let propre: String = nom
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let propre = propre.trim().trim_end_matches(['.', ' ']).to_string();
    let base = propre
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let port = base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|n| {
            let mut c = n.chars();
            matches!(
                (c.next(), c.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
        });
    let reserve = port
        || matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        );
    if propre.is_empty() {
        "export".into()
    } else if reserve {
        format!("_{propre}")
    } else {
        propre
    }
}

/// **Les fichiers d'échange de ces dossiers**, du plus récent au plus ancien —
/// reconnus à leur extension pour la LISTE seulement : c'est la lecture qui
/// décidera, par leurs octets, de ce qu'ils sont.
///
/// Les extensions viennent de `Format::extension` — une table de plus
/// divergerait au premier format ajouté —, plus le `.schematic` d'avant 1.13 :
/// la lecture le refuse en le NOMMANT, ce qui vaut mieux qu'un fichier que la
/// liste tairait.
pub fn fichiers_d_echange(dossiers: &[std::path::PathBuf]) -> Vec<Trouve> {
    let mut v: Vec<(std::time::SystemTime, Trouve)> = Vec::new();
    for d in dossiers {
        let Ok(lecture) = std::fs::read_dir(d) else {
            continue;
        };
        for e in lecture.flatten() {
            let chemin = e.path();
            let Some(ext) = chemin
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
            else {
                continue;
            };
            if ext != "schematic"
                && !tf_formats::Format::TOUS
                    .iter()
                    .any(|f| f.extension() == ext)
            {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            if !m.is_file() {
                continue;
            }
            v.push((
                m.modified().unwrap_or(std::time::UNIX_EPOCH),
                Trouve {
                    nom: e.file_name().to_string_lossy().into_owned(),
                    chemin,
                    octets: m.len(),
                },
            ));
        }
    }
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.nom.cmp(&b.1.nom)));
    v.into_iter().map(|(_, t)| t).take(40).collect()
}

/// La dimension que la scène montre. La coque ne dessine que la surface
/// aujourd'hui (`scene.rs`) ; une instance d'une autre dimension n'est donc
/// jamais « sous le curseur ».
pub const DIMENSION_VUE: tf_world::source::Dimension = tf_world::source::Dimension::Overworld;

/// Les orientations d'une pose, dans l'ordre où l'inspecteur les propose.
pub const ORIENTATIONS: [Option<tf_blocks::Transfo>; 6] = [
    None,
    Some(tf_blocks::Transfo::Rot90),
    Some(tf_blocks::Transfo::Rot180),
    Some(tf_blocks::Transfo::Rot270),
    Some(tf_blocks::Transfo::MiroirX),
    Some(tf_blocks::Transfo::MiroirZ),
];

/// Ce qu'une orientation veut dire, en mots.
pub const fn nom_orientation(t: Option<tf_blocks::Transfo>) -> &'static str {
    use tf_blocks::Transfo::*;
    match t {
        None => "telle quelle",
        Some(Rot90) => "quart de tour",
        Some(Rot180) => "demi-tour",
        Some(Rot270) => "trois quarts de tour",
        Some(MiroirX) => "miroir est-ouest",
        Some(MiroirZ) => "miroir nord-sud",
    }
}

/// **Un pousser-tirer en cours.**
///
/// Le premier tiers de SketchUp, et c'est un GESTE, pas une structure de
/// données : la sélection existe déjà, le remplissage aussi. Ce qui manquait
/// est la poignée.
#[derive(Debug, Clone)]
pub struct Tirage {
    /// La face attrapée.
    pub face: Direction,
    /// Le point du monde où le geste a commencé — l'origine de la mesure.
    pub ancre: [f32; 3],
    /// La sélection AVANT le geste.
    ///
    /// **On repart d'elle à chaque image**, jamais de la sélection courante :
    /// cumuler les tirages ferait accélérer la face à mesure qu'on la tire,
    /// ce qui se lit « la poignée s'emballe ».
    pub depart: Selection,
    /// Ce qui est tiré, en blocs. Négatif = poussé.
    pub blocs: i32,
    /// À quoi le tirage s'est accroché, s'il l'a fait. C'est le trait
    /// pointillé de SketchUp : sans lui, la face saute et on ne sait pas si
    /// l'on a mal visé ou si l'outil a décidé.
    pub raison: Option<Raison>,
}

/// L'état complet de la coque.
#[derive(Debug, Clone)]
pub struct Etat {
    pub mode: Mode,
    pub vue: Vue,
    pub selection: Selection,
    pub quadrillage: Quadrillage,
    /// Tolérance d'accrochage, en blocs. Zéro l'éteint.
    pub tolerance: i32,
    /// Le VOLUME visé dans la sélection — sphère, cylindre, murs…
    ///
    /// Gardé ici et non dans les paramètres de l'opération : il vient des
    /// gestes, pas du descripteur, et il doit survivre au changement
    /// d'opération. Le mettre dans le formulaire le ferait réapparaître comme
    /// un champ de chaque opération, ce qu'il n'est pas.
    pub volume: tf_ops::Volume,
    /// Évide la FORME — `//hsphere`. Géométrique, pas topologique.
    pub creux: Option<f64>,
    /// Compter les blocs modifiés exactement.
    ///
    /// **× 31 à l'étage palette** : c'est le parcours qu'on vient d'éviter.
    /// Un choix, jamais un service rendu d'office — je l'avais câblé à `true`
    /// dans le bouton, ce qui est très exactement ce que ce dépôt s'interdit.
    pub compter: bool,
    /// La graine des tirages. Zéro en dur rendait tous les mélanges
    /// identiques d'un projet à l'autre, et elle n'était réglable nulle part.
    pub seed: u64,
    pub vise: SousLeCurseur,
    /// L'opération choisie et ses paramètres.
    pub atelier: Atelier,
    /// L'outil de Conception. Sans effet en Édition, où gauche et droit
    /// posent les deux coins.
    pub outil: Outil,
    /// Le pousser-tirer en cours, s'il y en a un.
    pub tirage: Option<Tirage>,
    /// Le bloc que le pousser-tirer pose. Celui de l'opération choisie quand
    /// elle en nomme un, sinon de la pierre — jamais rien, sinon le geste
    /// n'écrirait pas.
    pub bloc_tirage: String,
    /// Ce que l'interface veut envoyer au moteur, posé pendant le dessin et
    /// ramassé juste après.
    ///
    /// **L'interface ne parle pas au moteur elle-même.** Elle DÉCRIT ce
    /// qu'elle veut ; la boucle envoie. Sans cette séparation, dessiner un
    /// bouton demanderait de tenir un canal, et l'interface ne se testerait
    /// plus sans fil.
    pub demande: Option<crate::moteur::Commande>,
    /// Le moteur travaille-t-il ? C'est ce qui grise le bouton.
    pub occupe: bool,
    /// Le monde ouvert est-il éditable ? La fixture ne l'est pas, et il faut
    /// le DIRE plutôt que de griser sans raison.
    pub editable: bool,
    /// L'utilisateur confirme-t-il que Minecraft est fermé ?
    ///
    /// **Ce n'est pas un réglage de confort.** Hors Windows, le verrou
    /// `session.lock` est consultatif et une ouverture réussie ne prouve rien :
    /// `probe_lock` rend `{ locked, reliable }`, et réduire ça à un booléen
    /// serait affirmer qu'un monde est libre sans le savoir. C'est donc
    /// l'utilisateur qui tranche, et il doit le faire EXPRÈS.
    pub jeu_ferme: bool,
    /// Ce que l'interface a à dire, en une ligne. Vide = rien à signaler.
    pub message: String,
    /// L'écran d'ouverture d'un monde.
    pub accueil: crate::accueil::Accueil,
    /// Ce que les champs de bloc proposent. La coque le nourrit — le pack à
    /// l'ouverture, les états de la scène à mesure qu'ils arrivent.
    pub nuancier: crate::nuancier::Nuancier,
    /// L'état de la case visée, tel que la scène le tient : ce que la pipette
    /// prend, et la première proposition de chaque champ de bloc.
    pub bloc_vise: Option<String>,
    /// **La pipette ARMÉE par son bouton** : le prochain clic gauche sur la
    /// scène prend le bloc au lieu d'agir. Le bouton ne peut pas la déclencher
    /// lui-même — pour l'atteindre, la souris a quitté ce qu'elle désignait.
    pub pipette_armee: bool,
    /// Le document des composants, tel que le fil l'a PUBLIÉ. L'interface ne
    /// le lit jamais du disque.
    pub composants: crate::moteur::Composants,
    /// La définition que l'outil « Composant » pose.
    pub composant_choisi: Option<u64>,
    /// Le nom tapé pour créer un composant, ou renommer le choisi.
    pub nom_composant: String,
    /// L'orientation de la prochaine pose — d'un composant comme d'un
    /// collage. Un choix de l'utilisateur : elle survit au changement de
    /// monde.
    pub orientation: Option<tf_blocks::Transfo>,
    /// Le presse-papiers, tel que le fil l'a PUBLIÉ.
    pub presse: crate::moteur::PressePapiers,
    /// Le panneau des échanges.
    pub echanges: Echanges,
}

/// La face que `viser` rend, dite dans le vocabulaire de la sélection.
///
/// **Par le SENS, jamais par le rang.** Les deux tables vivent dans des crates
/// qui ne peuvent pas se voir (`tf-mesh` pour le mailleur, `tf-world` pour
/// l'éditeur) et ce dépôt a déjà payé QUATRE fois le piège des tables qui
/// divergent. Un `FACES.iter().position(...)` marcherait aujourd'hui et
/// deviendrait faux le jour où l'une des deux listes est réordonnée — sans
/// erreur, en tirant la paroi OPPOSÉE à celle qu'on a visée. L'axe et le signe
/// sont ce que les deux veulent dire ; le rang n'est qu'une coïncidence
/// d'écriture.
pub fn direction(f: tf_mesh::forme::Face) -> tf_world::selection::Direction {
    tf_world::selection::Direction::depuis(f.axe(), f.positif())
}

impl Etat {
    /// L'état d'ouverture, cadré sur ce qu'on vient de charger.
    pub fn cadre(min: [f32; 3], max: [f32; 3], aspect: f32) -> Etat {
        Etat {
            mode: Mode::Edition,
            vue: Vue::cadrer(min, max, aspect),
            selection: Selection::nouvelle(),
            quadrillage: Quadrillage::default(),
            tolerance: TOLERANCE,
            volume: tf_ops::Volume::Aucun,
            creux: None,
            compter: true,
            seed: 0,
            vise: SousLeCurseur::default(),
            atelier: Atelier::default(),
            outil: Outil::default(),
            tirage: None,
            bloc_tirage: "minecraft:stone".into(),
            demande: None,
            occupe: false,
            editable: false,
            jeu_ferme: false,
            message: String::new(),
            accueil: crate::accueil::Accueil::default(),
            nuancier: crate::nuancier::Nuancier::default(),
            bloc_vise: None,
            pipette_armee: false,
            composants: crate::moteur::Composants::default(),
            composant_choisi: None,
            nom_composant: String::new(),
            orientation: None,
            presse: crate::moteur::PressePapiers::default(),
            echanges: Echanges::default(),
        }
    }

    /// **Un autre monde vient de s'ouvrir** : la caméra se recadre sur lui, et
    /// tout ce qui désignait des cases de l'ancien s'efface — sélection,
    /// tirage, visée. Le reste reste : le mode, l'outil, l'opération et
    /// ses paramètres sont des choix de l'utilisateur, pas des propriétés du
    /// monde.
    ///
    /// « Minecraft est fermé » se redemande : c'était vrai d'une AUTRE save.
    pub fn recadrer(&mut self, min: [f32; 3], max: [f32; 3], aspect: f32) {
        self.vue = Vue::cadrer(min, max, aspect);
        self.selection = Selection::nouvelle();
        self.tirage = None;
        self.vise = SousLeCurseur::default();
        self.bloc_vise = None;
        self.demande = None;
        self.jeu_ferme = false;
        self.message.clear();
        // Les identifiants de composants sont ceux d'un DOCUMENT : ceux de
        // l'ancien monde désigneraient, dans le nouveau, autre chose ou rien.
        self.composants = crate::moteur::Composants::default();
        self.composant_choisi = None;
        // Le presse-papiers vivait dans le moteur de l'ANCIEN monde : ses
        // états sont ceux de son interner, et il est parti avec lui.
        self.presse = crate::moteur::PressePapiers::default();
    }

    /// **Un monde vient de s'ouvrir** : les dossiers d'échange par défaut
    /// sont ceux de SON installation et de lui-même, et le nom proposé est le
    /// sien.
    pub fn situer_echanges(
        &mut self,
        installation: Option<std::path::PathBuf>,
        monde: Option<std::path::PathBuf>,
        version_monde: Option<i32>,
        nom: Option<&str>,
    ) {
        let e = &mut self.echanges;
        e.installation = installation;
        e.monde = monde;
        e.version_monde = version_monde;
        if let Some(n) = nom {
            e.nom = nom_de_fichier(n);
        }
        self.choisir_format(self.echanges.format);
        self.chercher_fichiers();
    }

    /// Change le format de l'export — et le dossier, qui en dépend.
    pub fn choisir_format(&mut self, f: tf_formats::Format) {
        let e = &mut self.echanges;
        e.format = f;
        e.dossier = dossier_par_defaut(f, e.installation.as_deref(), e.monde.as_deref())
            .map(|d| d.display().to_string())
            .unwrap_or_default();
    }

    /// Relit les fichiers d'échange des dossiers par défaut — et du dossier
    /// d'export tapé, sinon ce qu'on vient d'y exporter n'apparaîtrait pas
    /// dans la liste d'à côté.
    pub fn chercher_fichiers(&mut self) {
        let e = &self.echanges;
        let tape = Some(e.dossier.trim())
            .filter(|d| !d.is_empty())
            .map(std::path::PathBuf::from);
        let mut dossiers = Vec::new();
        for d in tf_formats::Format::TOUS
            .iter()
            .filter_map(|&f| dossier_par_defaut(f, e.installation.as_deref(), e.monde.as_deref()))
            .chain(tape)
        {
            if !dossiers.contains(&d) {
                dossiers.push(d);
            }
        }
        self.echanges.trouves = fichiers_d_echange(&dossiers);
    }

    /// Le fichier où l'export ira — avant qu'un nom pris ne le numérote.
    pub fn chemin_d_export(&self) -> Option<std::path::PathBuf> {
        let e = &self.echanges;
        let dossier = e.dossier.trim();
        if dossier.is_empty() {
            return None;
        }
        Some(std::path::Path::new(dossier).join(format!(
            "{}.{}",
            nom_de_fichier(&e.nom),
            e.format.extension()
        )))
    }

    /// **Exporte la sélection.** `date_ms` est passée, pas lue : l'état ne
    /// touche pas à l'horloge, et un test compare ce qu'il construit.
    pub fn demande_exporter(&self, date_ms: i64) -> Option<crate::moteur::Commande> {
        let sel = self.selection.boite()?;
        let chemin = self.chemin_d_export()?;
        let e = &self.echanges;
        Some(crate::moteur::Commande::Exporter {
            sel,
            format: e.format,
            chemin,
            meta: tf_formats::Meta {
                data_version: e.version_monde.unwrap_or(DV_PAR_DEFAUT),
                nom: e.nom.trim().to_string(),
                auteur: "titiforge".into(),
                description: String::new(),
                date_ms,
            },
        })
    }

    /// Importe un fichier dans le presse-papiers.
    pub fn demande_importer(&self, chemin: &std::path::Path) -> crate::moteur::Commande {
        crate::moteur::Commande::Importer {
            chemin: chemin.to_path_buf(),
        }
    }

    /// La sélection devient le presse-papiers.
    pub fn demande_copier(&self) -> Option<crate::moteur::Commande> {
        Some(crate::moteur::Commande::Copier {
            sel: self.selection.boite()?,
        })
    }

    /// **Colle le presse-papiers** — son coin de plus petites coordonnées sur
    /// la case de pose, dans l'orientation choisie.
    pub fn coller_ici(&mut self) -> Option<crate::moteur::Commande> {
        if self.presse.taille.is_none() {
            self.message = "le presse-papiers est vide — importer un fichier, ou copier la \
                            sélection"
                .into();
            return None;
        }
        let coin = self.point_de_pose()?;
        Some(crate::moteur::Commande::Coller {
            coin,
            transfo: self.orientation,
            avec_air: self.echanges.avec_air,
        })
    }

    /// **Le fil a publié le presse-papiers.** Un presse-papiers NEUF met
    /// l'outil « Coller » en main : on vient d'importer pour coller.
    pub fn suivre_presse(&mut self, p: crate::moteur::PressePapiers) {
        if p.version == self.presse.version {
            return;
        }
        if p.taille.is_some() {
            self.mode = Mode::Conception;
            self.outil = Outil::Coller;
        }
        self.presse = p;
    }

    /// Le fichier importé est-il d'un Minecraft plus RÉCENT que le monde ?
    /// Ses blocs peuvent n'y pas exister — le jeu les remplacerait par de
    /// l'air au chargement.
    pub fn presse_plus_recente(&self) -> Option<(i32, i32)> {
        match (self.presse.data_version, self.echanges.version_monde) {
            (Some(f), Some(m)) if f > m => Some((f, m)),
            _ => None,
        }
    }

    /// **Où la pose irait**, si l'on cliquait maintenant : la boîte du
    /// presse-papiers — ou du composant choisi — posée sur la case visée,
    /// dans l'orientation choisie. C'est l'aperçu.
    pub fn contour_d_arrivee(&self) -> Option<tf_world::coords::BBox> {
        let taille = match self.outil {
            Outil::Coller => self.presse.taille?,
            Outil::Composant => {
                self.composants
                    .projet
                    .definition(self.composant_choisi?)?
                    .contenu
                    .taille
            }
            _ => return None,
        };
        let [sx, sy, sz] = match self.orientation {
            Some(tf_blocks::Transfo::Rot90 | tf_blocks::Transfo::Rot270) => {
                [taille[2], taille[1], taille[0]]
            }
            _ => taille,
        };
        let c = self.point_de_pose()?;
        Some(tf_world::coords::BBox::new(
            c,
            BlockPos::new(
                c.x + sx as i32 - 1,
                c.y + sy as i32 - 1,
                c.z + sz as i32 - 1,
            ),
        ))
    }

    /// **Le fil a publié le document des composants.** Une définition choisie
    /// qui n'y est plus est oubliée : on ne pose pas un fantôme. Rien n'est
    /// recopié quand la version n'a pas bougé.
    pub fn suivre_composants(&mut self, c: crate::moteur::Composants) {
        if c.version == self.composants.version {
            return;
        }
        if let Some(d) = self.composant_choisi {
            if c.projet.definition(d).is_none() {
                self.composant_choisi = None;
            }
        }
        self.composants = c;
    }

    /// L'instance sous le curseur — la plus récente si plusieurs s'y
    /// chevauchent, selon la règle du document lui-même.
    pub fn instance_visee(&self) -> Option<&tf_ops::composant::Instance> {
        let c = self.vise.case?;
        self.composants.projet.instance_en(&DIMENSION_VUE, c)
    }

    /// La sélection devient un composant.
    pub fn demande_creer_composant(&self) -> Option<crate::moteur::Commande> {
        let sel = self.selection.boite()?;
        let nom = self.nom_composant.trim();
        Some(crate::moteur::Commande::Composant(
            crate::moteur::ActionComposant::Creer {
                sel,
                nom: if nom.is_empty() { "composant" } else { nom }.into(),
            },
        ))
    }

    /// **Pose le composant choisi** là où l'inférence le désigne — son coin de
    /// plus petites coordonnées sur la case de pose, dans l'orientation
    /// choisie.
    pub fn poser_un_composant(&mut self) -> Option<crate::moteur::Commande> {
        let Some(definition) = self.composant_choisi else {
            self.message = "aucun composant choisi — l'inspecteur les liste".into();
            return None;
        };
        let coin = self.point_de_pose()?;
        Some(crate::moteur::Commande::Composant(
            crate::moteur::ActionComposant::Poser {
                definition,
                coin,
                transfo: self.orientation,
            },
        ))
    }

    /// Le clic droit de l'outil « Composant » : un quart de tour de plus. Un
    /// miroir choisi dans l'inspecteur repart de « tel quel ».
    pub fn tourner_le_composant(&mut self) {
        use tf_blocks::Transfo::*;
        self.orientation = match self.orientation {
            None => Some(Rot90),
            Some(Rot90) => Some(Rot180),
            Some(Rot180) => Some(Rot270),
            _ => None,
        };
        self.message = format!(
            "orientation de la pose : {}",
            nom_orientation(self.orientation)
        );
    }

    /// La définition de l'instance visée prend ce que CETTE instance porte.
    pub fn demande_mettre_a_jour(&self) -> Option<crate::moteur::Commande> {
        let i = self.instance_visee()?;
        Some(crate::moteur::Commande::Composant(
            crate::moteur::ActionComposant::MettreAJour { instance: i.id },
        ))
    }

    /// L'instance visée ne suit plus sa définition.
    pub fn demande_detacher(&self) -> Option<crate::moteur::Commande> {
        let i = self.instance_visee()?;
        Some(crate::moteur::Commande::Composant(
            crate::moteur::ActionComposant::Detacher { instance: i.id },
        ))
    }

    /// Le composant choisi prend le nom tapé.
    pub fn demande_renommer(&self) -> Option<crate::moteur::Commande> {
        let definition = self.composant_choisi?;
        let nom = self.nom_composant.trim();
        if nom.is_empty() {
            return None;
        }
        Some(crate::moteur::Commande::Composant(
            crate::moteur::ActionComposant::Renommer {
                definition,
                nom: nom.into(),
            },
        ))
    }

    /// **Nomme la case visée** : l'état que la scène y tient. La coque le
    /// demande à chaque image, après `relever_vise` — l'état ne lit pas le
    /// monde lui-même. Rien n'est alloué tant que la visée ne change pas de
    /// bloc.
    pub fn nommer_vise<'a>(&mut self, etat_en: impl FnOnce(BlockPos) -> &'a str) {
        match self.vise.case {
            None => self.bloc_vise = None,
            Some(c) => {
                let nom = etat_en(c);
                if self.bloc_vise.as_deref() != Some(nom) {
                    self.bloc_vise = Some(nom.to_string());
                }
            }
        }
    }

    /// **La pipette** : le bloc visé devient le bloc EN MAIN — celui que
    /// posent « Poser » et le pousser-tirer. C'est le « choisir le bloc » de
    /// Minecraft : on regarde un bloc, on le prend, sous l'état EXACT que le
    /// jeu a écrit. Rend `false` quand rien n'est visé.
    pub fn pipette(&mut self) -> bool {
        let Some(cle) = self.bloc_vise.clone() else {
            self.message = "pipette : rien sous le curseur".into();
            return false;
        };
        self.bloc_tirage = catalogue::bloc_affiche(&cle);
        self.nuancier.utiliser(&cle);
        self.message = format!("en main : {}", self.bloc_tirage);
        true
    }

    /// **Arme (ou désarme) la pipette** : c'est le bouton de l'inspecteur. Le
    /// clic qui suit sur la scène passe par [`Etat::clic_de_pipette`].
    pub fn armer_pipette(&mut self, armee: bool) {
        self.pipette_armee = armee;
        self.message = if armee {
            "pipette : cliquer le bloc à prendre (Échap pour renoncer)".into()
        } else {
            "pipette rangée".into()
        };
    }

    /// **Un clic gauche sur la scène est-il pour la pipette ?** Oui si Alt est
    /// tenu, ou si son bouton l'a armée — et alors il la prend, et la range.
    ///
    /// Rend faux quand le clic est pour l'outil. Une pipette armée qui ne
    /// trouve rien sous le curseur reste armée : un clic dans le ciel ne
    /// doit ni prendre de l'air, ni poser un coin à la place.
    pub fn clic_de_pipette(&mut self, alt: bool) -> bool {
        if !alt && !self.pipette_armee {
            return false;
        }
        if self.pipette() {
            self.pipette_armee = false;
        }
        true
    }

    /// Relève ce que le curseur désigne, et ce que l'accrochage en fait.
    ///
    /// `curseur` est la souris en coordonnées normalisées
    /// ([`tf_render::viser::ndc_du_pixel`]), ou `None` quand elle n'est pas
    /// SUR la scène — sur l'inspecteur, sur l'accueil, hors de la fenêtre.
    ///
    /// **Hors de la scène, la visée se FIGE, elle ne s'efface pas.** On
    /// quitte la scène pour aller lire l'inspecteur, ou taper un nom dans le
    /// sélecteur de blocs — qui propose justement le bloc visé en tête. Effacer
    /// la visée à ce moment effaçait ce qu'on venait lire. Rien n'agit pour
    /// autant sur la case figée : un clic sur l'interface est pris par
    /// l'interface, et la coque ne le transmet pas.
    ///
    /// `solide` est la couture vers le monde — la même que `viser`. La coque
    /// ne lit pas les chunks elle-même : elle demande.
    ///
    /// **L'axe de la face est VERROUILLÉ pour l'accrochage.** Sans ça, la
    /// paroi visée est à un bloc — donc dans la tolérance — et l'inférence
    /// ramène la pose DANS le mur qu'on vise.
    pub fn relever_vise(
        &mut self,
        camera: &tf_render::Camera,
        aspect: f32,
        curseur: Option<[f32; 2]>,
        portee: f32,
        solide: &dyn Fn([i32; 3]) -> bool,
    ) {
        let Some(ndc) = curseur else { return };
        let d = tf_render::viser::rayon_ecran(camera, ndc, aspect);
        let Some(t) = tf_render::viser::viser(camera.oeil, d, portee, solide) else {
            self.vise = SousLeCurseur::default();
            return;
        };
        let case = BlockPos::new(t.case[0], t.case[1], t.case[2]);
        let pose = t.avant.map(|p| BlockPos::new(p[0], p[1], p[2]));
        let accroche = match (pose, t.face) {
            (Some(p), Some(f)) if self.tolerance > 0 => {
                let dir = direction(f);
                // Les références viennent de la sélection : c'est ce qui est
                // déjà BÂTI par l'utilisateur, et c'est là-dessus qu'il veut
                // s'aligner. Un monde entier de références serait à la fois
                // trop cher et trop bruyant.
                let refs = self
                    .selection
                    .boite()
                    .map(|b| b.references())
                    .unwrap_or_default();
                Some(accrocher(p, &refs, self.tolerance, dir.verrou(p)))
            }
            _ => None,
        };
        self.vise = SousLeCurseur {
            case: Some(case),
            pose,
            accroche,
        };
    }

    /// **Le geste de sélection : poser un coin sur ce qu'on vise.**
    ///
    /// La convention est celle de WorldEdit, et elle est délibérée : gauche
    /// pose le coin 1, droit le coin 2, tous deux sur la case VISÉE — pas sur
    /// celle d'avant. On sélectionne le bloc qu'on regarde, on ne sélectionne
    /// pas l'air devant lui.
    ///
    /// **L'accrochage ne s'applique PAS ici.** Il sert à poser un bloc au nu
    /// d'un mur ; un coin de sélection qu'une inférence déplacerait
    /// sélectionnerait autre chose que ce qu'on a visé, et l'utilisateur ne
    /// pourrait plus attraper le bord d'une paroi — qui est justement ce
    /// qu'il vise le plus souvent.
    ///
    /// Rend faux quand le curseur ne désigne rien : un clic dans le ciel ne
    /// doit pas déplacer une sélection existante.
    pub fn poser_coin(&mut self, premier: bool) -> bool {
        let Some(c) = self.vise.case else {
            return false;
        };
        if premier {
            self.selection.poser_coin1(c);
        } else {
            self.selection.poser_coin2(c);
        }
        self.message = format!(
            "coin {} : {}, {}, {}",
            if premier { 1 } else { 2 },
            c.x,
            c.y,
            c.z
        );
        true
    }

    /// Le point qu'un clic poserait : l'accroché s'il y en a un, sinon le brut.
    pub fn point_de_pose(&self) -> Option<BlockPos> {
        match &self.vise.accroche {
            Some(a) => Some(a.position),
            None => self.vise.pose,
        }
    }

    /// **Attrape une face de la sélection.** Le début du pousser-tirer.
    ///
    /// Le rayon part du CURSEUR, comme tout le reste : on tire la face qu'on
    /// MONTRE. Et c'est le même rayon que celui de [`Etat::tirer`], ce qui
    /// n'était pas le cas quand on attrapait au centre de l'écran : l'ancre
    /// se prenait au centre, le glissement se lisait sous la souris, et la
    /// face SAUTAIT de tout l'écart entre les deux au premier mouvement.
    ///
    /// Rend faux quand le rayon ne touche aucune face — un clic à côté ne doit
    /// pas démarrer un geste fantôme qui déplacera la sélection au premier
    /// mouvement de souris.
    pub fn attraper(&mut self, camera: &tf_render::Camera, aspect: f32, curseur: [f32; 2]) -> bool {
        let d = tf_render::viser::rayon_ecran(camera, curseur, aspect);
        let Some((face, t)) = self.selection.face_visee(camera.oeil, d) else {
            return false;
        };
        let ancre = [
            camera.oeil[0] + d[0] * t,
            camera.oeil[1] + d[1] * t,
            camera.oeil[2] + d[2] * t,
        ];
        self.tirage = Some(Tirage {
            face,
            ancre,
            depart: self.selection,
            blocs: 0,
            raison: None,
        });
        self.message = format!("face {} attrapée", face.nom());
        true
    }

    /// Met le geste à jour depuis la position de la souris, en NDC.
    ///
    /// Rend faux quand le rayon est trop parallèle à l'axe : là, un pixel
    /// vaudrait des dizaines de blocs. On ne bouge pas plutôt que de bouger
    /// n'importe comment — et la sélection garde la dernière valeur VALIDE,
    /// pas zéro : la face ne doit pas revenir à sa place parce qu'on a regardé
    /// dans l'axe une image.
    ///
    /// **Ce refus est aujourd'hui indistinguable d'un tirage de zéro**, parce
    /// que `Selection::agrandir` rend faux sur un non-changement et nous fait
    /// sortir au même endroit. Mesuré par mutation : remplacer le refus par un
    /// `unwrap_or(0)` ne fait rougir aucun test. Il reste, et c'est
    /// délibéré — sans lui, le geste dépendrait de ce que `agrandir` décide
    /// d'un zéro, qui n'est pas une promesse faite ici.
    pub fn tirer(&mut self, camera: &tf_render::Camera, aspect: f32, ndc: [f32; 2]) -> bool {
        let Some(t) = &self.tirage else { return false };
        let d = tf_render::viser::rayon_ecran(camera, ndc, aspect);
        let Some(n) = glissement(t.ancre, t.face, camera.oeil, d) else {
            return false;
        };
        let (face, depart) = (t.face, t.depart);
        let (n, raison) = self.accrocher_le_tirage(depart, face, n);
        let mut s = depart;
        if !s.agrandir(face, n) {
            return false;
        }
        self.selection = s;
        if let Some(t) = &mut self.tirage {
            t.blocs = n;
            t.raison = raison;
        }
        self.message = format!(
            "{} {} de {} bloc(s)",
            if n >= 0 { "tiré" } else { "poussé" },
            face.nom(),
            n.abs()
        );
        true
    }

    /// **Accroche la face tirée à ce qui est déjà bâti.**
    ///
    /// C'est le mot qui manquait au geste : sans lui, on tire au jugé et on
    /// recommence trois fois pour faire un cube. L'inférence est la même que
    /// pour la pose — axe par axe — et seul l'axe de la FACE est relu : une
    /// face ne se déplace que le long de sa normale.
    ///
    /// **Une référence à la position de DÉPART de la face est écartée.** Elle
    /// est toujours dans la tolérance au premier bloc tiré, et la face
    /// reviendrait donc se coller à son point de départ : on ne pourrait plus
    /// jamais faire un petit déplacement. Ce n'est pas un alignement, c'est un
    /// non-mouvement — l'accroche est juste, le geste est juste, c'est leur
    /// COMPOSITION qui ne l'est pas, exactement comme pour l'axe de pose.
    fn accrocher_le_tirage(
        &self,
        depart: Selection,
        face: Direction,
        n: i32,
    ) -> (i32, Option<Raison>) {
        if self.tolerance <= 0 || n == 0 {
            return (n, None);
        }
        let Some(b) = depart.boite() else {
            return (n, None);
        };
        let k = face.axe();
        let coins = [[b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]];
        // Là où la face EST au départ, et là où le geste l'emmène.
        let depart_k = if face.positif() {
            coins[1][k]
        } else {
            coins[0][k]
        };
        let vise_k = depart_k + if face.positif() { n } else { -n };

        let refs: Vec<_> = b
            .references()
            .into_iter()
            .filter(|r| [r.point.x, r.point.y, r.point.z][k] != depart_k)
            .collect();
        if refs.is_empty() {
            return (n, None);
        }
        let mut p = [coins[0][0], coins[0][1], coins[0][2]];
        p[k] = vise_k;
        let brut = BlockPos::new(p[0], p[1], p[2]);
        // **Aucun verrou, et c'est mesuré.** J'avais verrouillé les deux
        // autres axes « parce qu'une face ne bouge que le long de sa
        // normale » — vrai, et sans effet : on ne relit que l'axe `k`, et
        // `accrocher` choisit sa référence axe par axe. La mutation qui
        // retirait ces verrous n'a fait rougir aucun test, et elle avait
        // raison. Du code défensif qui ne peut pas se tromper est du code qui
        // ne dit rien.
        let acc = accrocher(brut, &refs, self.tolerance, [None; 3]);
        let obtenu = [acc.position.x, acc.position.y, acc.position.z][k];
        let delta = obtenu - depart_k;
        let neuf = if face.positif() { delta } else { -delta };
        (neuf, acc.raisons[k])
    }

    /// Lâche le geste, et rend l'opération à envoyer.
    ///
    /// **Elle ne porte que la TRANCHE**, jamais la sélection entière : tirer
    /// une face de trois blocs sur un bâtiment de cent mille ne doit pas
    /// réécrire le bâtiment. Tirer POSE le bloc choisi, pousser pose de l'air
    /// — c'est le modèle mental de SketchUp, où la même poignée ajoute et
    /// retire de la matière.
    pub fn lacher(&mut self) -> Option<crate::moteur::Commande> {
        let t = self.tirage.take()?;
        let (avant, apres) = (t.depart.boite()?, self.selection.boite()?);
        let zone = tranche(avant, apres, t.face)?;
        let mut params = Params::new();
        params.poser(
            "bloc",
            Valeur::Texte(if t.blocs >= 0 {
                self.bloc_tirage.clone()
            } else {
                "minecraft:air".into()
            }),
        );
        Some(crate::moteur::Commande::Appliquer {
            op: "poser",
            params,
            sel: zone,
            // **La tranche n'est pas un volume.** Une sphère appliquée à ce
            // qu'un tirage ajoute n'aurait aucun sens : on tire une face,
            // donc on remplit une dalle.
            forme: tf_ops::Forme::Boite,
            compter: self.compter,
            seed: self.seed,
        })
    }

    /// Abandonne le geste et remet la sélection d'avant. Échap, ou un clic
    /// droit pendant le tirage : SketchUp fait les deux, et un geste qu'on ne
    /// peut pas annuler est un geste qu'on n'ose pas commencer.
    pub fn abandonner(&mut self) -> bool {
        if self.pipette_armee {
            self.armer_pipette(false);
            return true;
        }
        let Some(t) = self.tirage.take() else {
            return false;
        };
        self.selection = t.depart;
        self.message = "tirage abandonné".into();
        true
    }

    /// La commande qu'un « Appliquer » enverrait, ou rien si la sélection
    /// manque.
    ///
    /// **Ici et pas dans le dessin de l'interface** : une commande construite
    /// au milieu d'un bouton est une commande qu'aucun test ne voit passer, et
    /// c'est là que les réglages se perdent — la forme, la graine et le
    /// comptage y étaient câblés en dur.
    pub fn demande_operation(&self) -> Option<crate::moteur::Commande> {
        let sel = self.selection.boite()?;
        let d = self.atelier.descripteur();
        Some(crate::moteur::Commande::Appliquer {
            op: self.atelier.op(),
            params: self.atelier.params.clone(),
            sel,
            // Une opération qui n'accepte pas de forme n'en reçoit pas : le
            // catalogue le DIT (`cout.forme`), et lui en passer une quand
            // même ferait `//move` déplacer une sphère de son contenu.
            forme: if d.cout.forme {
                self.volume.forme(&sel, self.creux)
            } else {
                tf_ops::Forme::Boite
            },
            compter: self.compter,
            seed: self.seed,
        })
    }

    /// **Pose un bloc là où l'inférence le désigne.**
    ///
    /// C'est le geste qui rend l'accrochage ATTEIGNABLE : il était écrit,
    /// testé, affiché dans le panneau — et aucun outil ne s'en servait.
    /// « Déclaré, branché, testé — et inatteignable », dans sa forme la plus
    /// discrète : la pièce marche, personne ne l'appelle.
    ///
    /// La case est celle d'AVANT — on pose devant le mur, pas dedans — et
    /// l'accrochage l'a déjà corrigée axe par axe, l'axe de la face resté
    /// verrouillé.
    pub fn poser_un_bloc(&mut self) -> Option<crate::moteur::Commande> {
        let p = self.point_de_pose()?;
        self.commande_une_case(p, self.bloc_tirage.clone())
    }

    /// Casse le bloc VISÉ — celui qui a arrêté le rayon, pas celui d'avant.
    ///
    /// Poser et casser ne visent pas la même case : un rayon touche une FACE,
    /// donc un plan ENTRE deux cases. C'est le piège d'`ExeWorldEdit` que
    /// `viser` existe pour fermer, et il se refermerait ici si l'un des deux
    /// gestes prenait la case de l'autre.
    pub fn casser_un_bloc(&mut self) -> Option<crate::moteur::Commande> {
        let c = self.vise.case?;
        self.commande_une_case(c, "minecraft:air".to_string())
    }

    /// Une opération sur UNE case. Elle ne paie que sa portée — c'est
    /// l'invariant n° 8, à sa plus petite échelle.
    fn commande_une_case(&self, p: BlockPos, bloc: String) -> Option<crate::moteur::Commande> {
        let mut params = Params::new();
        params.poser("bloc", Valeur::Texte(bloc));
        Some(crate::moteur::Commande::Appliquer {
            op: "poser",
            params,
            sel: tf_world::coords::BBox::single(p),
            forme: tf_ops::Forme::Boite,
            // Un bloc : le compte ne coûte rien et il RENSEIGNE — zéro dit
            // « c'était déjà ça », ce qui n'est pas une panne.
            compter: true,
            seed: self.seed,
        })
    }

    /// Ce que le panneau de sélection affiche.
    ///
    /// **Le compte de sections se MESURE, il ne se déduit pas.** « Alignée sur
    /// les chunks » est vrai et ne prouve rien : une sélection alignée en x et
    /// z dont la hauteur tombe au milieu d'une tranche de seize ne couvre
    /// aucune section entière, et paie l'étage bloc.
    pub fn resume_selection(&self) -> Option<ResumeSelection> {
        let b = self.selection.boite()?;
        let (sx, sy, sz) = b.size();
        let (entieres, total) = b.sections_entieres();
        Some(ResumeSelection {
            taille: (sx, sy, sz),
            volume: b.volume(),
            min: b.min,
            max: b.max,
            sections: (entieres, total),
            alignee_chunk: b.est_alignee(Niveau::Chunk),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeSelection {
    pub taille: (u32, u32, u32),
    pub volume: u128,
    pub min: BlockPos,
    pub max: BlockPos,
    pub sections: (usize, usize),
    pub alignee_chunk: bool,
}

impl ResumeSelection {
    /// Ce que coûtera l'opération, en une phrase — et jamais une promesse que
    /// le compte ne soutient pas.
    pub fn verdict(&self) -> String {
        let (e, t) = self.sections;
        if t == 0 {
            return "aucune section touchée".into();
        }
        if e == t {
            format!("{e} / {t} sections entières — étage palette atteignable")
        } else {
            format!("{e} / {t} sections entières — le reste passera par l'étage BLOC")
        }
    }
}
