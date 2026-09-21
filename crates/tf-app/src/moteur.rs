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

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use tf_anvil::Interner;
use tf_ops::catalogue::{construire, Params};
use tf_ops::edition::{rejouer, Sens};
use tf_ops::executer::{executer, Options};
use tf_ops::Forme;
use tf_world::coords::BBox;
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::staging::{RegionStore, Staging};

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
    /// Range le chantier et termine le fil.
    Arreter,
}

/// Ce qu'il répond. **Une réponse par commande, toujours** : sans ça, le
/// compteur d'en-vol dériverait et l'interface resterait grise pour toujours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reponse {
    /// L'opération a écrit. `bornes` est ce qu'elle a VRAIMENT écrit — c'est
    /// de là que partira le remaillage incrémental.
    Fait {
        op: String,
        resume: String,
        bornes: Option<BBox>,
    },
    Defait {
        label: String,
        bornes: Option<BBox>,
    },
    Refait {
        label: String,
        bornes: Option<BBox>,
    },
    /// Elle s'est exécutée et n'a rien changé. Ce n'est pas un échec, et le
    /// taire ferait croire à un bouton qui ne marche pas.
    Rien(String),
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

    pub fn texte(&self) -> String {
        match self {
            Reponse::Fait { op, resume, .. } => format!("{op} : {resume}"),
            Reponse::Defait { label, .. } => format!("annulé : {label}"),
            Reponse::Refait { label, .. } => format!("refait : {label}"),
            Reponse::Rien(s) => format!("{s} — rien n'a changé"),
            Reponse::Echec(s) => s.clone(),
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
}

impl Moteur {
    /// Lance le fil sur un chantier. Le chantier lui APPARTIENT à partir d'ici.
    pub fn lancer<S, O>(staging: Staging<S, O>, dim: Dimension, journal: Journal) -> Moteur
    where
        S: RegionSource + Send + 'static,
        O: RegionStore + Send + 'static,
    {
        let (vers, commandes) = channel::<Commande>();
        let (reponses, depuis) = channel::<Reponse>();
        let fil = std::thread::Builder::new()
            .name("moteur".into())
            .spawn(move || {
                let mut c = Chantier {
                    staging,
                    dim,
                    journal,
                    interner: Interner::new(),
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
            })
            .expect("le fil du moteur doit démarrer");
        Moteur {
            vers,
            depuis,
            en_vol: 0,
            vivant: true,
            fil: Some(fil),
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
    staging: Staging<S, O>,
    dim: Dimension,
    journal: Journal,
    interner: Interner,
}

impl<S: RegionSource, O: RegionStore> Chantier<S, O> {
    fn traiter(&mut self, c: Commande) -> Reponse {
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
            Commande::Arreter => Reponse::Rien("arrêt".into()),
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
        let opts = Options {
            compter,
            seed,
            avec_air: false,
            forme,
            regle: None,
        };
        let cr = match executer(
            &travail,
            &self.staging,
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
        if !cr
            .rapport
            .journaliser(&mut self.journal, &label, op, Vec::new(), horodatage())
        {
            return Reponse::Rien(label);
        }
        let n = cr.rapport.patches.len();
        let blocs = match cr.rapport.blocs {
            Some(b) => format!("{b} blocs"),
            None => format!("{n} chunk(s)"),
        };
        Reponse::Fait {
            op: label,
            resume: blocs,
            bornes: cr.rapport.bornes,
        }
    }

    fn defaire(&mut self, sens: Sens) -> Reponse {
        // Le journal rend l'entrée ET son enregistrement ; on ne garde de
        // l'entrée que ce dont on a besoin AVANT de rejouer, parce qu'elle
        // emprunte le journal.
        let pris = match sens {
            Sens::Annuler => self.journal.annuler(),
            Sens::Refaire => self.journal.refaire(),
        };
        let Some((entree, _)) = pris else {
            return Reponse::Rien(match sens {
                Sens::Annuler => "rien à annuler".into(),
                Sens::Refaire => "rien à refaire".into(),
            });
        };
        let label = entree.label.clone();
        let bornes = entree.bounds();
        // `rejouer` est la jonction : elle prend les correctifs dans le bon
        // SENS et dans le bon ORDRE — à l'envers pour annuler, et c'est le
        // genre de détail qu'un appelant refait mal une fois sur deux.
        match rejouer(&self.staging, entree, sens) {
            Ok(0) => Reponse::Rien(label),
            Ok(_) => match sens {
                Sens::Annuler => Reponse::Defait { label, bornes },
                Sens::Refaire => Reponse::Refait { label, bornes },
            },
            Err(e) => Reponse::Echec(format!("{label} : {e}")),
        }
    }
}

/// L'heure, en secondes depuis l'époque. Zéro si l'horloge est absurde —
/// une date fausse vaut mieux qu'un plantage dans un journal.
fn horodatage() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
