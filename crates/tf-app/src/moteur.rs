//! **Le moteur dans un fil à part. L'interface ne bloque jamais.**
//!
//! C'est une règle d'architecture, pas une optimisation, et elle ne se
//! retrofite pas : une opération de trois secondes sur la boucle d'images fige
//! la fenêtre, et une fenêtre figée est indistinguable d'une fenêtre plantée.
//! L'utilisateur clique ailleurs, Windows la grise, et on ne peut même plus lui
//! dire que ça travaille.
//!
//! ## Ce qui traverse la frontière
//!
//! Des COMMANDES et des RÉPONSES, jamais un monde. Le fil est propriétaire du
//! chantier — la copie de travail, le journal, la table d'états — et rien
//! d'autre ne le touche. Ce n'est pas de la prudence : un `StateId` n'a de
//! sens que relativement à SON interner, et deux fils qui en tiendraient
//! chacun un compareraient deux systèmes de coordonnées.
//!
//! ## Et si le fil meurt ?
//!
//! Une opération qui panique tuerait le moteur en silence : l'interface
//! resterait vivante, les boutons répondraient, et plus rien n'arriverait. Le
//! fil attrape donc ce qu'il peut (`catch_unwind` autour de l'exécution) et
//! rend un `Echec` — puis continue. Un moteur mort se dit ; il ne se devine
//! pas.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use tf_anvil::Interner;
use tf_blocks::Transfo;
use tf_formats::{Format, Meta};
use tf_ops::catalogue::{construire, Params};
use tf_ops::composant::{self, Projet};
use tf_ops::edition::{coller, copier, rejouer, Pas, Sens};
use tf_ops::executer::{executer, Options};
use tf_ops::{Forme, Presse};
use tf_world::coords::{BBox, BlockPos};
use tf_world::journal::{Journal, Record};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::staging::{CommitError, RegionStore, Staging};
use tf_world::{Fermeture, Seance};

/// **Ce qui garde la trace du chantier sur le disque** — la séance.
///
/// Un trait et pas `Seance` directement : le moteur se teste sur une copie de
/// travail en mémoire, qui n'a pas de disque derrière elle.
pub trait Carnet: Send {
    /// Une action commence. Si le programme s'arrête pendant, la reprise
    /// saura laquelle.
    fn commencer(&mut self, label: &str);
    /// Elle est finie, réussie ou non.
    fn terminer(&mut self);
    /// Range des enregistrements du journal. Le journal en mémoire est
    /// prêté : au-delà de son plafond, le carnet l'élague.
    fn noter(&mut self, journal: &mut Journal, records: &[Record]) -> Result<(), String>;
    /// Le chantier s'arrête. Rend ce qu'il faut en dire.
    fn fermer(self: Box<Self>) -> String;
}

impl Carnet for Seance {
    fn commencer(&mut self, label: &str) {
        // Une marque qui ne s'écrit pas n'empêche pas d'éditer : elle ne
        // servirait qu'à DIRE une interruption qui n'a pas eu lieu.
        let _ = Seance::commencer(self, label);
    }

    fn terminer(&mut self) {
        let _ = Seance::terminer(self);
    }

    fn noter(&mut self, journal: &mut Journal, records: &[Record]) -> Result<(), String> {
        Seance::noter(self, journal, records).map_err(|e| e.to_string())
    }

    fn fermer(self: Box<Self>) -> String {
        match Seance::fermer(*self) {
            Ok(Fermeture::Effacee) => "séance close : la save porte tout".into(),
            Ok(Fermeture::Gardee { regions, fichiers }) => format!(
                "séance gardée : {regions} région(s) modifiée(s){} pas encore écrites dans \
                 la save — elles seront là à la prochaine ouverture de ce monde",
                if fichiers > 0 {
                    " et le document des composants"
                } else {
                    ""
                }
            ),
            Err(e) => format!("{e} — la séance est gardée telle quelle"),
        }
    }
}

/// Ce qu'on demande au moteur.
#[derive(Debug, Clone)]
pub enum Commande {
    Appliquer {
        /// L'identifiant CATALOGUE. Le fil appellera `construire`, donc la
        /// normalisation est faite là-bas — l'interface ne peut pas la sauter.
        op: &'static str,
        params: Params,
        sel: BBox,
        forme: Forme,
        compter: bool,
        seed: u64,
    },
    Annuler,
    Refaire,
    /// **Écrit la copie de travail dans la save.**
    ///
    /// L'ordre est celui que le projet s'impose et que `Staging::commit` fait
    /// respecter : refuser si Minecraft tient le monde, sauvegarder en copie
    /// horodatée, PUIS écrire. Une sauvegarde prise après la première écriture
    /// ne sauvegarde plus rien.
    ///
    /// `confirme_sans_verrou` : hors Windows le verrou est consultatif et une
    /// ouverture réussie ne prouve rien. On ne réduit PAS ça à un booléen dans
    /// le moteur — c'est l'utilisateur qui confirme que le jeu est fermé, et
    /// l'interface qui le lui demande.
    Ecrire {
        confirme_sans_verrou: bool,
    },
    /// Une action sur les COMPOSANTS du monde. Une entrée de journal, comme
    /// une opération : un Ctrl+Z défait la mise à jour d'une définition ET ses
    /// réestampages.
    Composant(ActionComposant),
    /// **Copie la sélection dans un fichier d'échange.** Ni le monde ni le
    /// journal ne sont touchés. Le fichier n'est JAMAIS écrasé : un nom déjà
    /// pris devient « nom (2) » — l'export d'hier est peut-être celui qu'on
    /// voulait garder.
    Exporter {
        sel: BBox,
        format: Format,
        chemin: PathBuf,
        meta: Meta,
    },
    /// Lit un fichier d'échange dans le PRESSE-PAPIERS du chantier.
    Importer {
        chemin: PathBuf,
    },
    /// La sélection devient le presse-papiers — `//copy`.
    Copier {
        sel: BBox,
    },
    /// **Colle le presse-papiers**, son coin de plus petites coordonnées en
    /// `coin`, dans l'orientation donnée. Une entrée de journal.
    Coller {
        coin: BlockPos,
        transfo: Option<Transfo>,
        avec_air: bool,
    },
    /// Range le chantier et termine le fil.
    Arreter,
}

impl Commande {
    /// Écrit-elle dans la copie de travail ? Seules celles-là marquent la
    /// séance « action en cours » : un export interrompu n'a rien laissé à
    /// réparer, et la reprise ne doit pas prétendre le contraire.
    fn ecrit(&self) -> bool {
        !matches!(
            self,
            Commande::Exporter { .. }
                | Commande::Importer { .. }
                | Commande::Copier { .. }
                | Commande::Arreter
        )
    }
}

/// **Le presse-papiers du chantier, tel que le fil l'a publié** — sans sa
/// grille : la coque n'en a besoin que pour montrer où il ira, et le dire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PressePapiers {
    /// Sa boîte ; `None` quand il est vide.
    pub taille: Option<[u32; 3]>,
    pub ancre: [i32; 3],
    /// D'où il vient : un fichier, ou la sélection.
    pub source: String,
    /// Le `DataVersion` du fichier, quand il en porte un : plus récent que le
    /// monde, ses blocs peuvent ne pas y exister, et c'est la coque — qui
    /// connaît le monde — qui le dit.
    pub data_version: Option<i32>,
    /// Ce que la lecture n'a pas su porter, une phrase chacun.
    pub remarques: Vec<String>,
    /// Change à chaque publication.
    pub version: u64,
}

/// Ce qu'on fait aux composants. Les identifiants sont ceux du document que
/// le fil PUBLIE (`Moteur::composants`) : l'interface ne les invente pas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionComposant {
    /// La sélection devient un composant — et sa première instance, sur
    /// place. Le monde n'est pas touché.
    Creer {
        sel: BBox,
        nom: String,
    },
    /// Une instance de plus, son coin de plus petites coordonnées en `coin`.
    Poser {
        definition: u64,
        coin: BlockPos,
        transfo: Option<Transfo>,
    },
    /// La définition prend ce que CETTE instance porte, et toutes les autres
    /// suivent — dans toutes les dimensions.
    MettreAJour {
        instance: u64,
    },
    /// L'instance garde ses blocs et ne suit plus sa définition.
    Detacher {
        instance: u64,
    },
    Renommer {
        definition: u64,
        nom: String,
    },
}

/// **Le document des composants, tel que le fil l'a lu** — publié après chaque
/// action qui a pu le changer, et au lancement.
///
/// Décodé par le FIL : la coque ne décode rien, elle prend un `Arc` sous un
/// verrou tenu le temps d'un clone. Le document entier plutôt qu'un résumé :
/// un résumé aurait sa propre règle pour « l'instance sous le réticule », et
/// deux règles pour la même chose finissent par diverger.
#[derive(Debug, Clone, Default)]
pub struct Composants {
    pub projet: std::sync::Arc<Projet>,
    /// Le document ne se lit pas : ce qu'il faut en dire. Les actions sur
    /// les composants sont alors refusées par le moteur — rien n'est écrit
    /// par-dessus.
    pub erreur: Option<String>,
    /// Change à chaque publication : la coque ne recopie que ce qui a changé.
    pub version: u64,
}

/// Ce qu'il répond. **Une réponse par commande, toujours** : sans ça, le
/// compteur d'en-vol dériverait et l'interface resterait grise pour toujours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reponse {
    /// L'opération a écrit. `bornes` est ce qu'elle a VRAIMENT écrit, d'un
    /// bloc ; `zones` le même, chunk par chunk — c'est d'elles que part le
    /// remaillage incrémental (voir [`zones_de`]).
    Fait {
        op: String,
        resume: String,
        bornes: Option<BBox>,
        zones: Vec<BBox>,
    },
    Defait {
        label: String,
        bornes: Option<BBox>,
        zones: Vec<BBox>,
    },
    Refait {
        label: String,
        bornes: Option<BBox>,
        zones: Vec<BBox>,
    },
    /// Elle s'est exécutée et n'a rien changé. Ce n'est pas un échec, et le
    /// taire ferait croire à un bouton qui ne marche pas.
    Rien(String),
    /// La save a été écrite. `sauvegarde` est le dossier horodaté qu'on a
    /// posé avant — le DIRE compte autant que le faire, parce que c'est là
    /// qu'on va chercher quand on regrette.
    Ecrit {
        regions: usize,
        sauvegarde: String,
    },
    /// Ce qui s'est fait sans toucher au monde : un fichier écrit, un
    /// presse-papiers rempli.
    Note(String),
    Echec(String),
}

impl Reponse {
    /// Les bornes de ce qui a bougé, quand il y en a. Ce que le remaillage
    /// incrémental regarde.
    pub fn bornes(&self) -> Option<BBox> {
        match self {
            Reponse::Fait { bornes, .. }
            | Reponse::Defait { bornes, .. }
            | Reponse::Refait { bornes, .. } => *bornes,
            _ => None,
        }
    }

    /// Ce qui a bougé, chunk par chunk, dans la dimension regardée. C'est ce
    /// qu'on REMAILLE — jamais l'union, qui contient tout ce qui est entre
    /// les deux bouts d'un déplacement.
    pub fn zones(&self) -> &[BBox] {
        match self {
            Reponse::Fait { zones, .. }
            | Reponse::Defait { zones, .. }
            | Reponse::Refait { zones, .. } => zones,
            _ => &[],
        }
    }

    pub fn texte(&self) -> String {
        match self {
            Reponse::Fait { op, resume, .. } => format!("{op} : {resume}"),
            Reponse::Defait { label, .. } => format!("annulé : {label}"),
            Reponse::Refait { label, .. } => format!("refait : {label}"),
            Reponse::Rien(s) => format!("{s} — rien n'a changé"),
            Reponse::Ecrit {
                regions,
                sauvegarde,
            } => format!("écrit : {regions} région(s) · sauvegarde dans {sauvegarde}"),
            Reponse::Note(s) | Reponse::Echec(s) => s.clone(),
        }
    }

    pub fn echoue(&self) -> bool {
        matches!(self, Reponse::Echec(_))
    }
}

/// Le côté INTERFACE du moteur. Il ne contient qu'un tuyau.
pub struct Moteur {
    vers: Sender<Commande>,
    depuis: Receiver<Reponse>,
    /// Commandes envoyées dont la réponse n'est pas revenue. Il n'y a rien à
    /// verrouiller : c'est un simple compte, et c'est ce qui grise le bouton.
    en_vol: usize,
    vivant: bool,
    fil: Option<std::thread::JoinHandle<()>>,
    /// Les règles de rotation des états, partagées avec le fil. Une case et
    /// non un paramètre de lancement : un monde d'une autre installation
    /// apporte ses propres assets, donc ses propres règles, sans relancer le
    /// moteur.
    regles: std::sync::Arc<std::sync::Mutex<crate::regles::Regles>>,
    /// Le document des composants, publié par le fil.
    composants: std::sync::Arc<std::sync::Mutex<Composants>>,
    /// Le presse-papiers, publié par le fil.
    presse: std::sync::Arc<std::sync::Mutex<PressePapiers>>,
}

impl Moteur {
    /// Lance le fil sur une copie de travail.
    ///
    /// **Un `Arc` et non une possession exclusive**, parce que la coque doit
    /// RELIRE ce que le fil vient d'écrire pour le remailler. Il n'y a rien à
    /// verrouiller : `Staging` prend `&self` partout, et tout ce qui écrit
    /// passe par ce fil-ci. Donner la possession obligerait à faire revenir
    /// les octets par le canal — c'est-à-dire à recopier une région entière à
    /// chaque coup de pinceau.
    pub fn lancer<S, O>(
        staging: std::sync::Arc<Staging<S, O>>,
        dim: Dimension,
        journal: Journal,
        monde: Option<std::path::PathBuf>,
    ) -> Moteur
    where
        S: RegionSource + Send + Sync + 'static,
        O: RegionStore + Send + Sync + 'static,
    {
        Self::demarrer(staging, dim, journal, monde, None)
    }

    /// Lance le fil sur la copie de travail d'une SÉANCE : chaque action est
    /// rangée dans son journal sur disque, et la séance se ferme quand le fil
    /// s'arrête — dans le fil qui la tient, après la dernière écriture.
    pub fn lancer_en_seance<S, O>(
        staging: std::sync::Arc<Staging<S, O>>,
        dim: Dimension,
        journal: Journal,
        monde: Option<std::path::PathBuf>,
        carnet: Box<dyn Carnet>,
    ) -> Moteur
    where
        S: RegionSource + Send + Sync + 'static,
        O: RegionStore + Send + Sync + 'static,
    {
        Self::demarrer(staging, dim, journal, monde, Some(carnet))
    }

    fn demarrer<S, O>(
        staging: std::sync::Arc<Staging<S, O>>,
        dim: Dimension,
        journal: Journal,
        monde: Option<std::path::PathBuf>,
        carnet: Option<Box<dyn Carnet>>,
    ) -> Moteur
    where
        S: RegionSource + Send + Sync + 'static,
        O: RegionStore + Send + Sync + 'static,
    {
        let (vers, commandes) = channel::<Commande>();
        let (reponses, depuis) = channel::<Reponse>();
        let regles = std::sync::Arc::new(std::sync::Mutex::new(crate::regles::Regles::absentes()));
        let regles_du_fil = regles.clone();
        // Le document des composants est publié AVANT que le fil ne démarre :
        // la première image le montre déjà, et rien n'a à l'attendre.
        let (doc_vu, lu) = lire_document(staging.as_ref());
        let composants = std::sync::Arc::new(std::sync::Mutex::new(Composants::depuis(lu, 1)));
        let composants_du_fil = composants.clone();
        let presse = std::sync::Arc::new(std::sync::Mutex::new(PressePapiers::default()));
        let presse_du_fil = presse.clone();
        let fil = std::thread::Builder::new()
            .name("moteur".into())
            .spawn(move || {
                let mut c = Chantier {
                    staging,
                    dim,
                    journal,
                    interner: Interner::new(),
                    monde,
                    carnet,
                    regles: regles_du_fil,
                    composants: composants_du_fil,
                    doc_vu,
                    presse: None,
                    presse_publiee: presse_du_fil,
                };
                while let Ok(cmd) = commandes.recv() {
                    if matches!(cmd, Commande::Arreter) {
                        break;
                    }
                    let r = c.traiter(cmd);
                    if reponses.send(r).is_err() {
                        break;
                    }
                }
                // Après la dernière action, jamais pendant : fermer une séance
                // dont une écriture est en vol la jugerait sur un état faux.
                if let Some(k) = c.carnet.take() {
                    println!("{}", k.fermer());
                }
            })
            .expect("le fil du moteur doit démarrer");
        Moteur {
            vers,
            depuis,
            en_vol: 0,
            vivant: true,
            fil: Some(fil),
            regles,
            composants,
            presse,
        }
    }

    /// **Le presse-papiers**, tel que le fil l'a publié en dernier.
    pub fn presse_papiers(&self) -> PressePapiers {
        self.presse.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// **Le document des composants**, tel que le fil l'a publié en dernier.
    /// Ne bloque pas : le verrou n'est tenu que le temps d'un clone d'`Arc`.
    pub fn composants(&self) -> Composants {
        self.composants
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// **Donne au fil les règles de rotation des états.** Sans elles, une
    /// opération qui tourne un extrait déplace les cases et laisse chaque
    /// orientation telle quelle — et la réponse le dit.
    ///
    /// Elles peuvent être encore EN COURS de dérivation : le fil ne les attend
    /// que le jour où une opération en demande une.
    pub fn poser_regles(&self, r: crate::regles::Regles) {
        if let Ok(mut g) = self.regles.lock() {
            *g = r;
        }
    }

    /// Envoie une commande. **Ne bloque jamais** : le canal n'a pas de borne.
    pub fn envoyer(&mut self, c: Commande) -> bool {
        if !self.vivant {
            return false;
        }
        match self.vers.send(c) {
            Ok(()) => {
                self.en_vol += 1;
                true
            }
            Err(_) => {
                self.vivant = false;
                false
            }
        }
    }

    /// Ramasse ce qui est revenu. **Ne bloque jamais** — c'est le point de
    /// toute cette machinerie, et un test le mesure.
    pub fn recevoir(&mut self) -> Vec<Reponse> {
        let mut out = Vec::new();
        loop {
            match self.depuis.try_recv() {
                Ok(r) => {
                    self.en_vol = self.en_vol.saturating_sub(1);
                    out.push(r);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Le fil est mort. Ce qui était en vol ne reviendra jamais :
                    // on le dit, une fois, plutôt que de laisser l'interface
                    // grise pour toujours.
                    if self.en_vol > 0 {
                        out.push(Reponse::Echec(
                            "le moteur s'est arrêté — l'opération en cours est perdue".into(),
                        ));
                        self.en_vol = 0;
                    }
                    self.vivant = false;
                    break;
                }
            }
        }
        out
    }

    /// Y a-t-il du travail en vol ? C'est ce qui grise le bouton.
    pub fn occupe(&self) -> bool {
        self.en_vol > 0
    }

    pub fn vivant(&self) -> bool {
        self.vivant
    }

    /// Arrête le fil et l'attend. Appelé à la fermeture ; le `Drop` le fait
    /// aussi, parce qu'un fil qui survit à sa fenêtre tient la copie de
    /// travail ouverte.
    pub fn arreter(&mut self) {
        if self.vivant {
            let _ = self.vers.send(Commande::Arreter);
            self.vivant = false;
        }
        if let Some(f) = self.fil.take() {
            let _ = f.join();
        }
    }
}

impl Drop for Moteur {
    fn drop(&mut self) {
        self.arreter();
    }
}

/// Le côté FIL : tout ce qui touche au monde.
struct Chantier<S: RegionSource, O: RegionStore> {
    staging: std::sync::Arc<Staging<S, O>>,
    dim: Dimension,
    journal: Journal,
    interner: Interner,
    /// Le dossier de la save, quand il y en a une. `None` = rien à écrire.
    monde: Option<std::path::PathBuf>,
    /// La séance, quand il y en a une. `None` : rien ne survit à la
    /// fermeture — une copie jetable, ou un test.
    carnet: Option<Box<dyn Carnet>>,
    regles: std::sync::Arc<std::sync::Mutex<crate::regles::Regles>>,
    /// Le document des composants publié pour la coque.
    composants: std::sync::Arc<std::sync::Mutex<Composants>>,
    /// Ses octets à la dernière publication : on ne décode que ce qui a
    /// changé. `None` : illisible la dernière fois, à relire.
    doc_vu: Option<Vec<u8>>,
    /// Le presse-papiers : un fichier importé, ou la sélection copiée — ses
    /// états dans l'interner de CE fil, et sa provenance.
    presse: Option<(Presse, String)>,
    presse_publiee: std::sync::Arc<std::sync::Mutex<PressePapiers>>,
}

impl Composants {
    fn depuis(lu: Result<Projet, String>, version: u64) -> Composants {
        match lu {
            Ok(p) => Composants {
                projet: std::sync::Arc::new(p),
                erreur: None,
                version,
            },
            Err(e) => Composants {
                projet: std::sync::Arc::default(),
                erreur: Some(e),
                version,
            },
        }
    }

    /// Le nom d'une définition, pour les messages.
    fn nom(&self, definition: u64) -> String {
        self.projet
            .definition(definition)
            .map(|d| d.nom.clone())
            .unwrap_or_else(|| format!("composant n° {definition}"))
    }
}

/// Le document des composants de la copie de travail : ses octets, et ce
/// qu'ils disent. Des octets `None` quand on n'a pas pu les lire du tout.
fn lire_document<S: RegionSource, O: RegionStore>(
    st: &Staging<S, O>,
) -> (Option<Vec<u8>>, Result<Projet, String>) {
    match st.lire_fichier(composant::FICHIER) {
        Ok(b) => {
            let lu = Projet::decoder(&b);
            (Some(b), lu)
        }
        Err(tf_world::source::SourceError::NotFound) => (Some(Vec::new()), Ok(Projet::default())),
        Err(e) => (
            None,
            Err(format!("document des composants illisible : {e}")),
        ),
    }
}

impl<S: RegionSource, O: RegionStore> Chantier<S, O> {
    /// **Publie le document des composants** s'il a changé depuis la dernière
    /// fois — après une action sur eux, une annulation, un rétablissement.
    fn publier(&mut self) {
        let (octets, lu) = lire_document(self.staging.as_ref());
        if octets.is_some() && octets == self.doc_vu {
            return;
        }
        self.doc_vu = octets;
        if let Ok(mut g) = self.composants.lock() {
            let version = g.version + 1;
            *g = Composants::depuis(lu, version);
        }
    }

    /// L'étiquette d'une action sur les composants — son entrée de journal
    /// et ce que la réponse dit.
    fn label_composant(&self, a: &ActionComposant) -> String {
        let doc = self
            .composants
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        match a {
            ActionComposant::Creer { nom, .. } => format!("Créer « {} »", nom.trim()),
            ActionComposant::Poser { definition, .. } => {
                format!("Poser « {} »", doc.nom(*definition))
            }
            ActionComposant::MettreAJour { instance } => match doc.projet.instance(*instance) {
                Some(i) => format!("Mettre à jour « {} »", doc.nom(i.definition)),
                None => format!("Mettre à jour depuis l'instance n° {instance}"),
            },
            ActionComposant::Detacher { instance } => format!("Détacher l'instance n° {instance}"),
            ActionComposant::Renommer { definition, nom } => {
                format!(
                    "Renommer « {} » en « {} »",
                    doc.nom(*definition),
                    nom.trim()
                )
            }
        }
    }

    fn traiter(&mut self, c: Commande) -> Reponse {
        let quoi = match &c {
            Commande::Appliquer { op, .. } => tf_ops::catalogue::descripteur(op)
                .map(|d| d.label.to_string())
                .unwrap_or_else(|| op.to_string()),
            Commande::Annuler => "Annuler".into(),
            Commande::Refaire => "Refaire".into(),
            Commande::Ecrire { .. } => "Écrire dans la save".into(),
            Commande::Composant(a) => self.label_composant(a),
            Commande::Exporter { format, .. } => format!("Exporter en {}", format.nom()),
            Commande::Importer { chemin } => format!("Importer « {} »", nom_de(chemin)),
            Commande::Copier { .. } => "Copier la sélection".into(),
            Commande::Coller { .. } => self.label_coller(),
            Commande::Arreter => "Arrêter".into(),
        };
        // Ce qui a pu changer le document des composants : une action sur
        // eux, et le journal qu'on rejoue — il porte leurs correctifs aussi.
        let doc = matches!(
            c,
            Commande::Composant(_) | Commande::Annuler | Commande::Refaire
        );
        let marque = c.ecrit();
        if marque {
            if let Some(k) = &mut self.carnet {
                k.commencer(&quoi);
            }
        }
        let r = self.executer(c);
        if doc {
            self.publier();
        }
        if marque {
            if let Some(k) = &mut self.carnet {
                k.terminer();
            }
        }
        r
    }

    fn label_coller(&self) -> String {
        match &self.presse {
            Some((_, source)) => format!("Coller « {source} »"),
            None => "Coller".into(),
        }
    }

    /// **Publie le presse-papiers** pour la coque.
    fn publier_presse(&mut self, data_version: Option<i32>, remarques: Vec<String>) {
        if let Ok(mut g) = self.presse_publiee.lock() {
            let version = g.version + 1;
            *g = match &self.presse {
                Some((p, source)) => PressePapiers {
                    taille: Some(p.taille),
                    ancre: p.ancre,
                    source: source.clone(),
                    data_version,
                    remarques,
                    version,
                },
                None => PressePapiers {
                    version,
                    ..Default::default()
                },
            };
        }
    }

    /// Range des enregistrements dans la séance. Rend de quoi compléter le
    /// compte rendu — vide si tout va bien : un historique qui ne survivra
    /// pas à la fermeture se DIT.
    fn noter(&mut self, records: &[Record]) -> String {
        let Some(k) = &mut self.carnet else {
            return String::new();
        };
        match k.noter(&mut self.journal, records) {
            Ok(()) => String::new(),
            Err(e) => format!(
                " · historique non enregistré sur le disque ({e}) : il ne survivra pas \
                 à la fermeture"
            ),
        }
    }

    fn executer(&mut self, c: Commande) -> Reponse {
        match c {
            Commande::Appliquer {
                op,
                params,
                sel,
                forme,
                compter,
                seed,
            } => self.appliquer(op, &params, sel, forme, compter, seed),
            Commande::Annuler => self.defaire(Sens::Annuler),
            Commande::Refaire => self.defaire(Sens::Refaire),
            Commande::Ecrire {
                confirme_sans_verrou,
            } => self.ecrire(confirme_sans_verrou),
            Commande::Composant(a) => self.composant(a),
            Commande::Exporter {
                sel,
                format,
                chemin,
                meta,
            } => self.exporter(sel, format, &chemin, &meta),
            Commande::Importer { chemin } => self.importer(&chemin),
            Commande::Copier { sel } => self.copier(sel),
            Commande::Coller {
                coin,
                transfo,
                avec_air,
            } => self.coller(coin, transfo, avec_air),
            Commande::Arreter => Reponse::Rien("arrêt".into()),
        }
    }

    /// Voir `Commande::Exporter`. La sélection est relue MAINTENANT — ce qui
    /// part dans le fichier est ce que la copie de travail porte, retouches
    /// comprises.
    fn exporter(&mut self, sel: BBox, format: Format, chemin: &Path, meta: &Meta) -> Reponse {
        let p = match copier(
            self.staging.as_ref(),
            &self.dim,
            Folder::Region,
            &sel,
            &mut self.interner,
        ) {
            Ok(p) => p,
            Err(e) => return Reponse::Echec(format!("exporter : {e}")),
        };
        let ecrit = match tf_formats::ecrire(&p, format, &self.interner, meta) {
            Ok(e) => e,
            Err(e) => return Reponse::Echec(format!("exporter : {e}")),
        };
        let chemin = match ecrire_sans_ecraser(chemin, &ecrit.octets) {
            Ok(c) => c,
            Err(e) => {
                return Reponse::Echec(format!("exporter : {} — {e}", chemin.display()));
            }
        };
        let mut s = format!(
            "exporté : {} · {} · {} Ko",
            chemin.display(),
            resume_presse(&p),
            ecrit.octets.len().div_ceil(1024)
        );
        for r in &ecrit.remarques {
            s.push_str(&format!(" · {r}"));
        }
        Reponse::Note(s)
    }

    /// Voir `Commande::Importer`.
    fn importer(&mut self, chemin: &Path) -> Reponse {
        let nom = nom_de(chemin);
        // Un fichier plus gros que ce que la lecture accepte décompressé ne
        // se lit pas — on ne le charge pas en mémoire pour le découvrir.
        match std::fs::metadata(chemin) {
            Ok(m) if m.len() > tf_formats::MAX_OCTETS_NBT => {
                return Reponse::Echec(format!("« {nom} » : {} Mo, trop gros", m.len() >> 20));
            }
            Ok(_) => {}
            Err(e) => return Reponse::Echec(format!("« {nom} » : {e}")),
        }
        let octets = match std::fs::read(chemin) {
            Ok(o) => o,
            Err(e) => return Reponse::Echec(format!("« {nom} » : {e}")),
        };
        let lu = match tf_formats::lire(&octets, &mut self.interner) {
            Ok(lu) => lu,
            Err(e) => return Reponse::Echec(format!("« {nom} » : {e}")),
        };
        let mut s = format!(
            "« {nom} » dans le presse-papiers : {} · {}",
            lu.lecture,
            resume_presse(&lu.presse)
        );
        let remarques: Vec<String> = lu.remarques.iter().map(|r| r.to_string()).collect();
        for r in &remarques {
            s.push_str(&format!(" · {r}"));
        }
        self.presse = Some((lu.presse, nom));
        self.publier_presse(lu.data_version, remarques);
        Reponse::Note(s)
    }

    /// Voir `Commande::Copier`.
    fn copier(&mut self, sel: BBox) -> Reponse {
        let p = match copier(
            self.staging.as_ref(),
            &self.dim,
            Folder::Region,
            &sel,
            &mut self.interner,
        ) {
            Ok(p) => p,
            Err(e) => return Reponse::Echec(format!("copier : {e}")),
        };
        let s = format!("sélection copiée : {}", resume_presse(&p));
        self.presse = Some((p, "la sélection".into()));
        self.publier_presse(None, Vec::new());
        Reponse::Note(s)
    }

    /// Voir `Commande::Coller`.
    fn coller(&mut self, coin: BlockPos, transfo: Option<Transfo>, avec_air: bool) -> Reponse {
        let label = self.label_coller();
        let regles = self.regles.lock().map(|r| r.clone()).unwrap_or_default();
        let demandee = std::cell::Cell::new(false);
        let regle = |cle: &str, t: Transfo| {
            demandee.set(true);
            regles.table().and_then(|table| table.transformer(cle, t))
        };
        let Some((p, _)) = &self.presse else {
            return Reponse::Echec(
                "le presse-papiers est vide — importer un fichier ou copier la sélection".into(),
            );
        };
        let (p, intacts, approches) = match transfo {
            Some(t) => {
                let tr = p.transformer(t, &mut self.interner, &regle);
                (
                    std::borrow::Cow::Owned(tr.presse),
                    tr.intacts.len(),
                    tr.approches.len(),
                )
            }
            None => (std::borrow::Cow::Borrowed(p), 0, 0),
        };
        let pas = Pas {
            d: [0; 3],
            avec_air,
            air: self.interner.intern("minecraft:air"),
            compter: true,
        };
        let rap = match coller(
            self.staging.as_ref(),
            &self.dim,
            Folder::Region,
            &p,
            coin,
            pas,
            &self.interner,
        ) {
            Ok(r) => r,
            Err(e) => return Reponse::Echec(format!("{label} : {e}")),
        };
        let Some(records) = rap.journaliser(
            &mut self.journal,
            &label,
            "coller",
            Vec::new(),
            horodatage(),
        ) else {
            // Un collage sur du terrain jamais généré n'écrit rien : il
            // n'ENGENDRE pas de chunk. Le dire, sinon c'est un bouton mort.
            return Reponse::Rien(format!("{label} (aucun terrain généré sous l'extrait ?)"));
        };
        let mut s = match rap.blocs {
            Some(b) => format!("{b} blocs"),
            None => format!("{} chunk(s)", rap.patches.len()),
        };
        if rap.entites_posees > 0 {
            s.push_str(&format!(" · {} coffre(s) et autres", rap.entites_posees));
        }
        if rap.mobiles_poses > 0 {
            s.push_str(&format!(" · {} entité(s)", rap.mobiles_poses));
        }
        let laissees = rap.mobiles_sans_terrain + rap.mobiles_autre_version;
        if laissees > 0 {
            s.push_str(&format!(
                " · {laissees} entité(s) laissée(s) (pas de terrain à l'arrivée, ou chunk \
                 d'une autre version)"
            ));
        }
        if approches > 0 {
            s.push_str(&format!(" · {approches} entité(s) approchée(s)"));
        }
        s.push_str(&non_generes(rap.chunks_absents));
        if demandee.get() && regles.table().is_none() {
            s.push_str(" · orientations NON réécrites : les règles de rotation du pack manquent");
        } else if intacts > 0 {
            s.push_str(&format!(
                " · {intacts} état(s) que le pack ne sait pas tourner, laissés tels quels"
            ));
        }
        s.push_str(&self.noter(&records));
        Reponse::Fait {
            op: label,
            resume: s,
            bornes: rap.bornes,
            zones: zones_de(rap.patches.iter().map(|p| &p.cible), rap.bornes, &self.dim),
        }
    }

    /// **Une action sur les composants**, par la seule porte qui en fait UNE
    /// entrée de journal (`composant::faire`).
    fn composant(&mut self, a: ActionComposant) -> Reponse {
        let label = self.label_composant(&a);
        // Un document illisible n'est pas réécrit : `faire` le relit, et le
        // refuse avant la moindre écriture.
        // La règle est demandée, pas supposée — comme pour une opération.
        let regles = self.regles.lock().map(|r| r.clone()).unwrap_or_default();
        let demandee = std::cell::Cell::new(false);
        let regle = |cle: &str, t: Transfo| {
            demandee.set(true);
            regles.table().and_then(|table| table.transformer(cle, t))
        };
        let staging = self.staging.as_ref();
        let dim = self.dim.clone();
        let interner = &mut self.interner;
        let fait = composant::faire(
            staging,
            &mut self.journal,
            &label,
            horodatage(),
            |p| match &a {
                ActionComposant::Creer { sel, nom } => {
                    composant::creer(staging, &dim, sel, nom, p, interner)
                }
                ActionComposant::Poser {
                    definition,
                    coin,
                    transfo,
                } => composant::poser(
                    staging,
                    &dim,
                    p,
                    *definition,
                    *coin,
                    *transfo,
                    interner,
                    Some(&regle),
                ),
                ActionComposant::MettreAJour { instance } => {
                    composant::mettre_a_jour(staging, p, *instance, interner, Some(&regle))
                }
                ActionComposant::Detacher { instance } => composant::detacher(p, *instance),
                ActionComposant::Renommer { definition, nom } => {
                    composant::renommer(p, *definition, nom)
                }
            },
        );
        let (action, records) = match fait {
            Ok(x) => x,
            Err(e) => return Reponse::Echec(format!("{label} : {e}")),
        };
        let Some(records) = records else {
            return Reponse::Rien(label);
        };
        let mut resume = resume_composant(&a, &action);
        if demandee.get() && regles.table().is_none() {
            resume.push_str(
                " · orientations NON réécrites : les règles de rotation du pack \
                 manquent — les escaliers, portes et échelles regardent toujours \
                 du même côté",
            );
        } else if action.intacts > 0 {
            resume.push_str(&format!(
                " · {} état(s) que le pack ne sait pas tourner, laissés tels quels",
                action.intacts
            ));
        }
        resume.push_str(&self.noter(&records));
        let r = &action.rapport;
        Reponse::Fait {
            op: label,
            resume,
            bornes: r.bornes,
            zones: zones_de(r.patches.iter().map(|p| &p.cible), r.bornes, &self.dim),
        }
    }

    fn appliquer(
        &mut self,
        op: &'static str,
        params: &Params,
        sel: BBox,
        forme: Forme,
        compter: bool,
        seed: u64,
    ) -> Reponse {
        let travail = match construire(op, params, &mut self.interner) {
            Ok(t) => t,
            Err(e) => return Reponse::Echec(e.to_string()),
        };
        // **La règle est demandée, pas supposée.** Elle n'attend la
        // dérivation qu'au premier état qu'une opération veut tourner : un
        // `//set` lancé pendant qu'elle tourne encore ne l'attend pas.
        let regles = self.regles.lock().map(|r| r.clone()).unwrap_or_default();
        let demandee = std::cell::Cell::new(false);
        let regle = |cle: &str, t: tf_blocks::Transfo| {
            demandee.set(true);
            regles.table().and_then(|table| table.transformer(cle, t))
        };
        let opts = Options {
            compter,
            seed,
            avec_air: false,
            forme,
            regle: Some(&regle),
        };
        let cr = match executer(
            &travail,
            self.staging.as_ref(),
            &self.dim,
            Folder::Region,
            &sel,
            &mut self.interner,
            &opts,
        ) {
            Ok(cr) => cr,
            Err(e) => return Reponse::Echec(e.to_string()),
        };

        let label = tf_ops::catalogue::descripteur(op)
            .map(|d| d.label.to_string())
            .unwrap_or_else(|| op.to_string());
        // **Une opération qui n'écrit rien ne remplit pas le journal.**
        // `journaliser` le refuse déjà ; on le DIT plutôt que de laisser
        // croire à un bouton mort.
        let Some(records) =
            cr.rapport
                .journaliser(&mut self.journal, &label, op, Vec::new(), horodatage())
        else {
            return Reponse::Rien(label);
        };
        let note = self.noter(&records);
        let n = cr.rapport.patches.len();
        let mut blocs = match cr.rapport.blocs {
            Some(b) => format!("{b} blocs"),
            None => format!("{n} chunk(s)"),
        };
        let r = &cr.rapport;
        if r.mobiles_poses > 0 {
            blocs.push_str(&format!(" · {} entité(s)", r.mobiles_poses));
        }
        // Une entité laissée derrière se DIT : elle flotte à l'ancienne place,
        // et rien d'autre à l'écran ne l'expliquerait.
        let laissees = r.mobiles_sans_terrain + r.mobiles_autre_version;
        if laissees > 0 {
            blocs.push_str(&format!(
                " · {laissees} entité(s) laissée(s) (pas de terrain à l'arrivée, \
                 ou chunk d'une autre version)"
            ));
        }
        if !cr.approches.is_empty() {
            blocs.push_str(&format!(" · {} entité(s) approchée(s)", cr.approches.len()));
        }
        blocs.push_str(&non_generes(r.chunks_absents));
        // **Un build à moitié tourné se DIT.** Sans règles, aucune
        // orientation n'a bougé ; avec, celles que le pack ne sait pas tourner
        // sont restées telles quelles. Les deux se lisent pareil à l'écran.
        if demandee.get() && regles.table().is_none() {
            blocs.push_str(
                " · orientations NON réécrites : les règles de rotation du pack \
                 manquent — les escaliers, portes et échelles regardent toujours \
                 du même côté",
            );
        } else if !cr.intacts.is_empty() {
            blocs.push_str(&format!(
                " · {} état(s) que le pack ne sait pas tourner, laissés tels quels",
                cr.intacts.len()
            ));
        }
        blocs.push_str(&note);
        Reponse::Fait {
            op: label,
            resume: blocs,
            bornes: cr.rapport.bornes,
            zones: zones_de(
                cr.rapport.patches.iter().map(|p| &p.cible),
                cr.rapport.bornes,
                &self.dim,
            ),
        }
    }

    /// Écrit la copie de travail dans la save. Voir `Commande::Ecrire`.
    fn ecrire(&mut self, confirme_sans_verrou: bool) -> Reponse {
        let Some(monde) = self.monde.clone() else {
            return Reponse::Echec("aucune save derrière ce monde".into());
        };
        let sink = match tf_world::FsSource::open(&monde) {
            Ok(s) => s,
            Err(e) => return Reponse::Echec(format!("save illisible : {e:?}")),
        };
        let verrou = sink.probe_lock();
        let mut ou = String::new();
        // La sauvegarde est faite PAR `commit`, entre le refus et l'écriture.
        // La faire ici, avant, écrirait une copie même quand le verrou refuse.
        let r = self
            .staging
            .commit(&sink, verrou, confirme_sans_verrou, &mut || {
                let vers = tf_world::sauvegarder(&monde)?;
                ou = vers.display().to_string();
                Ok(())
            });
        match r {
            Ok(rap) => Reponse::Ecrit {
                regions: rap.regions_ecrites,
                sauvegarde: ou,
            },
            Err(e) => Reponse::Echec(match e {
                CommitError::WorldLocked => {
                    "Minecraft tient ce monde — le fermer d'abord. Rien n'a été écrit.".into()
                }
                CommitError::LockUnknown => {
                    "impossible de savoir si Minecraft tient ce monde (le verrou \
                     n'est consultable que sous Windows). Confirmer que le jeu est \
                     fermé, puis recommencer."
                        .into()
                }
                e @ CommitError::SaveModifiee(_) => format!(
                    "{e}. Pour repartir de la save : fermer puis rouvrir ce monde — la \
                     copie de travail sera mise de côté, intacte."
                ),
                autre => format!("écriture refusée : {autre}"),
            }),
        }
    }

    fn defaire(&mut self, sens: Sens) -> Reponse {
        // Le journal rend l'entrée ET son enregistrement ; on ne garde de
        // l'entrée que ce dont on a besoin AVANT de rejouer, parce qu'elle
        // emprunte le journal.
        let avant = self.journal.curseur();
        let pris = match sens {
            Sens::Annuler => self.journal.annuler(),
            Sens::Refaire => self.journal.refaire(),
        };
        let Some((entree, curseur)) = pris else {
            return Reponse::Rien(match sens {
                Sens::Annuler => "rien à annuler".into(),
                Sens::Refaire => "rien à refaire".into(),
            });
        };
        let label = entree.label.clone();
        let bornes = entree.bounds();
        let zones = zones_de(
            entree.a_refaire().filter_map(|c| match c {
                tf_world::journal::Correction::Chunk(p) => Some(&p.cible),
                _ => None,
            }),
            bornes,
            &self.dim,
        );
        // `rejouer` est la jonction : elle prend les correctifs dans le bon
        // SENS et dans le bon ORDRE — à l'envers pour annuler, et c'est le
        // genre de détail qu'un appelant refait mal une fois sur deux.
        match rejouer(self.staging.as_ref(), entree, sens) {
            Ok(n) => {
                let note = self.noter(&[curseur]);
                let label = label + &note;
                match (n, sens) {
                    (0, _) => Reponse::Rien(label),
                    (_, Sens::Annuler) => Reponse::Defait {
                        label,
                        bornes,
                        zones,
                    },
                    (_, Sens::Refaire) => Reponse::Refait {
                        label,
                        bornes,
                        zones,
                    },
                }
            }
            Err(e) => {
                // Rien n'a été écrit — `rejouer` valide tout avant d'écrire —
                // donc le curseur revient où il était. Resté avancé, il
                // désignerait comme défaite une entrée toujours appliquée, qui
                // ne se laisserait plus ni annuler ni refaire.
                self.journal.poser_curseur(avant);
                Reponse::Echec(format!("{label} : {e}"))
            }
        }
    }
}

/// Le nom d'un fichier, pour les messages.
fn nom_de(chemin: &Path) -> String {
    chemin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| chemin.display().to_string())
}

/// Un extrait en une phrase : sa boîte, ses coffres, ses entités.
fn resume_presse(p: &Presse) -> String {
    let [x, y, z] = p.taille;
    let mut s = format!("{x} × {y} × {z}");
    if !p.entites.is_empty() {
        s.push_str(&format!(" · {} block entit(é/ies)", p.entites.len()));
    }
    if !p.mobiles.is_empty() {
        s.push_str(&format!(" · {} entité(s)", p.mobiles.len()));
    }
    s
}

/// **Écrit un fichier sans JAMAIS en écraser un** : un nom pris devient
/// « nom (2).ext », « nom (3).ext »… Rend le chemin écrit.
///
/// Le nom se RÉSERVE d'abord (`create_new`, atomique : deux exports
/// simultanés ne prennent pas le même), les octets s'écrivent dans un
/// temporaire, puis le temporaire remplace la réservation. Un arrêt en plein
/// milieu laisse au pire un fichier vide sous le nom voulu — jamais un
/// fichier tronqué qui se lirait à moitié.
pub fn ecrire_sans_ecraser(voulu: &Path, octets: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    if let Some(d) = voulu.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    let tige = voulu
        .file_stem()
        .map(|t| t.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = voulu
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for k in 1..10_000u32 {
        let c = if k == 1 {
            voulu.to_path_buf()
        } else {
            voulu.with_file_name(format!("{tige} ({k}){ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&c)
        {
            Ok(reservation) => {
                drop(reservation);
                let tmp = c.with_file_name(format!(".{tige} ({k}){ext}.titiforge-tmp"));
                let ecrit = (|| {
                    let mut f = std::fs::File::create(&tmp)?;
                    f.write_all(octets)?;
                    f.sync_all()?;
                    std::fs::rename(&tmp, &c)
                })();
                if let Err(e) = ecrit {
                    let _ = std::fs::remove_file(&tmp);
                    let _ = std::fs::remove_file(&c);
                    return Err(e);
                }
                return Ok(c);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(
        "dix mille fichiers de ce nom déjà là",
    ))
}

/// Ce qu'une action sur les composants a fait, en une phrase.
fn resume_composant(a: &ActionComposant, act: &composant::Action) -> String {
    let def = act.projet.definition(act.definition);
    let mut s = match a {
        ActionComposant::Creer { .. } => {
            let (t, m) = def.map_or(([0; 3], 0), |d| (d.contenu.taille, d.contenu.matiere()));
            format!(
                "composant n° {} · {} × {} × {} · {m} bloc(s)",
                act.definition, t[0], t[1], t[2]
            )
        }
        ActionComposant::Poser { .. } => format!(
            "instance n° {} · {} chunk(s)",
            act.instance.unwrap_or(0),
            act.rapport.patches.len()
        ),
        ActionComposant::MettreAJour { .. } => {
            let mut s = format!("{} instance(s) réestampée(s)", act.reestampees);
            if act.ailleurs > 0 {
                s.push_str(&format!(" dont {} dans une autre dimension", act.ailleurs));
            }
            // Ce qui entre dans le composant se DIT : une instance posée dans
            // un mur en prend le mur, et rien d'autre ne le montrerait.
            s.push_str(&format!(
                " · {} case(s) entrée(s) dans le composant, {} sortie(s)",
                act.gagnees, act.perdues
            ));
            s
        }
        ActionComposant::Detacher { instance } => {
            format!("l'instance n° {instance} garde ses blocs et ne suit plus sa définition")
        }
        ActionComposant::Renommer { .. } => "l'identité ne change pas".into(),
    };
    if act.entites_laissees > 0 {
        s.push_str(&format!(
            " · {} entité(s) laissée(s) hors du composant (cadres, bêtes…)",
            act.entites_laissees
        ));
    }
    s
}

/// **Les zones à remailler**, une par chunk de BLOCS écrit dans la dimension
/// `dim` : la colonne du chunk, bornée par ce que l'opération a écrit.
///
/// Tirées des CORRECTIFS, et pas d'un compte rendu : une entrée de journal
/// les porte aussi, donc annuler et refaire les ont sans rien stocker de
/// plus. Les points d'intérêt et les entités n'en donnent pas — ils ne se
/// dessinent pas —, ni une autre dimension, qu'on ne regarde pas.
///
/// L'union des bornes ne sert qu'à borner chaque colonne : remailler l'union
/// elle-même, c'était remailler tout ce qui est entre les deux bouts d'un
/// `//move` — 196 sections pour un build de 5 × 3 × 5 déplacé de deux cents
/// blocs, et le CARRÉ de la distance pour un déplacement plus long.
pub fn zones_de<'a>(
    cibles: impl IntoIterator<Item = &'a tf_world::journal::Cible>,
    bornes: Option<BBox>,
    dim: &Dimension,
) -> Vec<BBox> {
    use tf_world::coords::BlockPos;
    let chunks: std::collections::BTreeSet<(i32, i32)> = cibles
        .into_iter()
        .filter(|c| c.folder == Folder::Region && c.dim == *dim)
        .map(|c| {
            (
                c.region.x * 32 + (c.chunk % 32) as i32,
                c.region.z * 32 + (c.chunk / 32) as i32,
            )
        })
        .collect();
    // Sans bornes, la colonne entière — toutes les hauteurs qu'une section
    // peut porter.
    let (y0, y1) = bornes.map_or((-2048, 2047), |b| (b.min.y, b.max.y));
    chunks
        .into_iter()
        .filter_map(|(cx, cz)| {
            let (mut x0, mut x1) = (cx * 16, cx * 16 + 15);
            let (mut z0, mut z1) = (cz * 16, cz * 16 + 15);
            if let Some(b) = bornes {
                x0 = x0.max(b.min.x);
                x1 = x1.min(b.max.x);
                z0 = z0.max(b.min.z);
                z1 = z1.min(b.max.z);
            }
            (x0 <= x1 && z0 <= z1)
                .then(|| BBox::new(BlockPos::new(x0, y0, z0), BlockPos::new(x1, y1, z1)))
        })
        .collect()
}

/// L'heure, en secondes depuis l'époque. Zéro si l'horloge est absurde —
/// une date fausse vaut mieux qu'un plantage dans un journal.
/// **Ce qu'une opération a SAUTÉ faute de terrain**, à ajouter à son résumé.
///
/// Une opération n'écrit que dans les chunks que le jeu a générés ; au-delà du
/// bord de la carte, elle s'arrête net. Sans cette phrase, l'arrêt se lisait
/// comme une limite d'affichage.
pub fn non_generes(absents: u64) -> String {
    if absents == 0 {
        return String::new();
    }
    format!(
        " · {absents} chunk(s) de la zone jamais générés par le jeu : rien n'y \
         est écrit — y aller en jeu d'abord, ou prégénérer la carte"
    )
}

fn horodatage() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ce que le compte rendu d'une mise à jour DIT, et qu'aucun monde de test
    /// à une seule dimension ne ferait dire : les instances d'ailleurs, et les
    /// entités laissées hors du composant.
    #[test]
    fn le_compte_rendu_dit_l_ailleurs_et_les_entites_laissees() {
        let a = ActionComposant::MettreAJour { instance: 2 };
        let mut act = composant::Action {
            reestampees: 3,
            ..Default::default()
        };
        let texte = resume_composant(&a, &act);
        assert!(!texte.contains("autre dimension"), "{texte}");
        assert!(!texte.contains("entité"), "{texte}");
        act.ailleurs = 1;
        act.entites_laissees = 2;
        let texte = resume_composant(&a, &act);
        assert!(texte.contains("dont 1 dans une autre dimension"), "{texte}");
        assert!(texte.contains("2 entité(s) laissée(s)"), "{texte}");
    }
}
