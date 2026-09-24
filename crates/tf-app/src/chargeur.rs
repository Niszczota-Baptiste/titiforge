//! **Le chargeur dans un fil à part.** Une cellule à la fois, la plus urgente
//! d'abord.
//!
//! Même règle d'architecture que le fil moteur, et elle ne se retrofite pas
//! davantage : le budget est mesuré (`tf-app --example residence`) et il est
//! sans appel. Une région bâtie met **867 ms** à venir — lecture, décodage,
//! maillage — soit **108 images à 8 ms**. Un vol qui traverse une région en
//! moins de deux secondes distance donc le chargeur, quoi qu'on optimise. Il
//! n'y a pas de version « assez rapide pour le fil principal » : il y a un
//! fil, ou il y a des à-coups.
//!
//! ## Ce qui traverse la frontière
//!
//! Des sections et une TABLE D'ÉTATS locale, jamais un monde. Un `StateId`
//! n'a de sens que relativement à SON interner : le fil interne dans le sien,
//! l'hôte fusionne (`Interner::merge_from`) et remappe les palettes. C'est la
//! pièce que le décodage parallèle réclamait déjà, mesurée à 1,06 ms pour une
//! région pleine — 3,9 % du décodage — et elle sert ici telle quelle.
//!
//! ## Une réponse par CELLULE, une lecture par RÉGION
//!
//! Les deux unités sont différentes et c'est la mesure qui l'a tranché : un
//! chunk demandé seul coûte 4,74 ms contre 0,47 ms amorti sur sa région —
//! **× 10**, parce que le `.mca` est relu à chaque appel. Le fil lit donc le
//! fichier une fois par lot et répond cellule par cellule, pour que l'hôte
//! puisse en intégrer une par image sans dépasser son budget.
//!
//! ## Et si le fil meurt ?
//!
//! Comme le moteur : un `.mca` abîmé ne doit pas faire disparaître le
//! chargement en silence. Le fil attrape ce qu'il peut et rend un `Echec`,
//! puis continue.

use std::sync::mpsc::{channel, Receiver, Sender, SyncSender, TryRecvError};

use tf_anvil::Interner;
use tf_world::coords::{BBox, BlockPos};
use tf_world::demande::Lot;
use tf_world::lecture::SectionLue;
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::{Cellule, Niveau};

/// Ce qu'on demande au chargeur.
#[derive(Debug, Clone)]
pub enum Commande {
    /// **La file COMPLÈTE, la plus urgente d'abord.** Elle remplace la
    /// précédente au lieu de s'y ajouter : dès que la caméra bouge, ce qui
    /// n'a pas encore été lu n'est plus ce qu'il faut lire. Une file qui
    /// s'accumule ferait charger le passé pendant qu'on vole vers l'avenir.
    Charger(Vec<Lot>),
    Arreter,
}

/// Ce que le chargeur rend.
///
/// Pas de `Debug` : elle porte un `Interner`, et une table de plusieurs
/// centaines d'états déversée dans un message d'erreur n'aide personne.
pub enum Reponse {
    /// Une cellule prête, avec la table d'états dans laquelle ses
    /// identifiants ont un sens.
    ///
    /// `sections` peut être vide : une cellule sans contenu est une réponse,
    /// pas un silence. Sans elle, l'hôte redemanderait indéfiniment une
    /// cellule qui n'a simplement rien à montrer.
    Prete {
        cellule: Cellule,
        sections: Vec<SectionLue>,
        interner: Interner,
    },
    /// Un `.mca` qu'on n'a pas su lire. Nommé, jamais tu.
    Echec(String),
}

impl Reponse {
    /// La cellule concernée, quand il y en a une.
    pub fn cellule(&self) -> Option<&Cellule> {
        match self {
            Reponse::Prete { cellule, .. } => Some(cellule),
            Reponse::Echec(_) => None,
        }
    }
}

/// **Combien de cellules le fil peut avoir d'AVANCE sur l'hôte.**
///
/// Le canal des réponses est BORNÉ, et c'est la correction d'un défaut que
/// seul le pilote pouvait montrer : le fil lit une région d'un coup et émet
/// ses mille cellules, quand l'hôte n'en intègre que deux par image. Mesuré
/// sur un vol de soixante chunks — 240 images, une demande tous les quatre —
/// l'hôte recevait encore des cellules **300 images après s'être arrêté**, et
/// les sections décodées s'empilaient dans le canal, hors de tout budget :
/// la fenêtre de résidence ne compte que ce qui est POSÉ.
///
/// Borné, le fil attend quand l'hôte est en retard. C'est exactement ce qu'on
/// veut : il n'a rien de mieux à faire, et ce qu'il aurait décodé en avance
/// serait de toute façon périmé au premier mouvement de caméra.
///
/// 64 cellules de bâti font une douzaine de mégaoctets en attente — le même
/// ordre qu'une image de travail, et trente images d'avance à deux cellules
/// par image.
const AVANCE_MAX: usize = 64;

/// La poignée côté hôte.
pub struct Chargeur {
    vers: Sender<Commande>,
    /// **Une `Option` pour pouvoir la LÂCHER.** Le canal étant borné, le fil
    /// peut être bloqué dans un `send` ; le joindre sans rien faire serait
    /// alors un interblocage. Lâcher le récepteur fait échouer son `send`,
    /// donc sortir sa boucle — déterministe, sans sondage ni délai.
    depuis: Option<Receiver<Reponse>>,
    /// Cellules demandées dont la réponse n'est pas revenue.
    en_vol: usize,
    vivant: bool,
    fil: Option<std::thread::JoinHandle<()>>,
}

impl Chargeur {
    /// Lance le fil sur une source — la copie de travail, jamais ce qu'elle
    /// recouvre.
    ///
    /// **Le staging et non la save** : après une édition, ce qu'il faut
    /// dessiner est ce que le staging porte. Relire la source rendrait le
    /// monde d'AVANT, ce qui se lit « le bouton ne fait rien ».
    pub fn lancer<S>(source: std::sync::Arc<S>, dim: Dimension) -> Chargeur
    where
        S: RegionSource + Send + Sync + 'static,
    {
        let (vers, commandes) = channel::<Commande>();
        // BORNÉ : voir `AVANCE_MAX`. Un canal sans borne laisse le fil
        // décoder des gigaoctets que l'hôte ne prendra jamais.
        let (reponses, depuis) = std::sync::mpsc::sync_channel::<Reponse>(AVANCE_MAX);
        let fil = std::thread::Builder::new()
            .name("chargeur".into())
            .spawn(move || travailler(&*source, dim, commandes, reponses))
            .expect("le fil du chargeur doit démarrer");
        Chargeur {
            vers,
            depuis: Some(depuis),
            en_vol: 0,
            vivant: true,
            fil: Some(fil),
        }
    }

    /// Demande une file. **Ne bloque jamais** : le canal n'a pas de borne.
    ///
    /// Rend le nombre de cellules attendues, ou `None` si le fil est mort.
    ///
    /// **Le compte REMPLACE, il ne s'ajoute pas**, parce que le fil remplace
    /// sa file : ce qui n'avait pas encore été lu ne le sera jamais. L'ajouter
    /// ferait grossir `en_vol` à chaque mouvement de caméra et le témoin de
    /// chargement resterait allumé pour toujours.
    ///
    /// C'est une APPROXIMATION assumée : le fil peut encore rendre des
    /// cellules de l'ancienne file — il finit le lot qu'il tenait — et elles
    /// décrémentent le compte de la nouvelle. `occupe` est un témoin
    /// d'interface, pas une comptabilité ; ce qui est vraiment résident se
    /// lit dans les RÉPONSES, qui nomment chacune leur cellule.
    pub fn demander(&mut self, lots: Vec<Lot>) -> Option<usize> {
        if !self.vivant {
            return None;
        }
        let n: usize = lots.iter().map(|l| l.cellules.len()).sum();
        match self.vers.send(Commande::Charger(lots)) {
            Ok(()) => {
                self.en_vol = n;
                Some(n)
            }
            Err(_) => {
                self.vivant = false;
                None
            }
        }
    }

    /// Ramasse ce qui est prêt. **Ne bloque jamais** — c'est le point de
    /// toute la pièce.
    ///
    /// `budget` borne ce qu'on prend dans une image : intégrer une cellule
    /// coûte à l'hôte (fusion d'interner, maillage, remplacement de tranche),
    /// et tout avaler d'un coup rendrait le fil inutile. Zéro veut dire
    /// « tout ce qui est là ».
    pub fn recevoir(&mut self, budget: usize) -> Vec<Reponse> {
        let mut out = Vec::new();
        let Some(depuis) = self.depuis.as_ref() else {
            return out;
        };
        loop {
            if budget > 0 && out.len() >= budget {
                break;
            }
            match depuis.try_recv() {
                Ok(r) => {
                    // Seule une CELLULE rendue sort du vol : un message
                    // d'échec l'accompagne, il ne la remplace pas. Le
                    // décompter ferait croire le fil libre une cellule trop
                    // tôt, et la demande repartirait chercher celle qui est
                    // encore en route.
                    if matches!(r, Reponse::Prete { .. }) {
                        self.en_vol = self.en_vol.saturating_sub(1);
                    }
                    out.push(r);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.en_vol > 0 {
                        out.push(Reponse::Echec(
                            "le chargeur s'est arrêté — les cellules en cours sont perdues".into(),
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

    /// Reste-t-il des cellules en vol ? C'est ce qui allume le témoin de
    /// chargement.
    pub fn occupe(&self) -> bool {
        self.en_vol > 0
    }

    pub fn vivant(&self) -> bool {
        self.vivant
    }

    /// Arrête le fil et l'attend. Appelé à la fermeture.
    pub fn arreter(&mut self) {
        let _ = self.vers.send(Commande::Arreter);
        self.vivant = false;
        // **On LÂCHE le récepteur avant de joindre.** Le canal des réponses
        // est borné (`AVANCE_MAX`) : le fil peut être bloqué dans un `send`
        // que plus personne ne viendra lire, et le joindre en l'état serait un
        // interblocage — l'application ne se fermerait plus. Sans récepteur,
        // son `send` échoue, `servir` rend faux, et la boucle sort.
        self.depuis = None;
        if let Some(f) = self.fil.take() {
            let _ = f.join();
        }
    }
}

impl Drop for Chargeur {
    fn drop(&mut self) {
        self.arreter();
    }
}

/// La boucle du fil.
fn travailler<S: RegionSource + ?Sized>(
    source: &S,
    dim: Dimension,
    commandes: Receiver<Commande>,
    reponses: SyncSender<Reponse>,
) {
    let mut file: Vec<Lot> = Vec::new();
    loop {
        // **On vide la boîte aux lettres AVANT de travailler**, et une
        // nouvelle file remplace l'ancienne. Sans ça, une caméra qui bouge
        // deux fois ferait charger deux horizons périmés avant le bon.
        let mut fini = false;
        loop {
            match commandes.try_recv() {
                Ok(Commande::Charger(l)) => file = l,
                Ok(Commande::Arreter) => fini = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => fini = true,
            }
        }
        if fini {
            return;
        }
        if file.is_empty() {
            // Rien à faire : on ATTEND une commande plutôt que de tourner à
            // vide. Un fil qui boucle sans travail mange un cœur et chauffe
            // la machine de quelqu'un qui ne fait que regarder son build.
            match commandes.recv() {
                Ok(Commande::Charger(l)) => file = l,
                _ => return,
            }
            continue;
        }
        // Le lot le plus urgent d'abord : `par_region` les a déjà triés.
        let lot = file.remove(0);
        if !servir(source, &dim, &lot, &reponses) {
            return;
        }
    }
}

/// Lit une région UNE fois et répond cellule par cellule.
///
/// Rend `false` quand l'hôte a raccroché.
fn servir<S: RegionSource + ?Sized>(
    source: &S,
    dim: &Dimension,
    lot: &Lot,
    reponses: &SyncSender<Reponse>,
) -> bool {
    // L'emprise qui couvre toutes les cellules du lot : une seule lecture du
    // `.mca`, et c'est tout l'intérêt du groupement.
    let Some(emprise) = emprise(lot) else {
        return true;
    };
    // Une cellule par clé, pour ranger les sections à mesure qu'elles
    // arrivent. L'ordre des cellules est celui du lot, donc celui de
    // l'urgence : c'est dans cet ordre qu'on répondra.
    let mut paquets: Vec<(Cellule, Vec<SectionLue>)> = lot
        .cellules
        .iter()
        .map(|v| (v.cellule.clone(), Vec::new()))
        .collect();
    let niveau = lot.cellules[0].cellule.niveau;
    let mut interner = Interner::new();

    let lu = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tf_world::sections_de(source, dim, Folder::Region, &emprise, &mut interner, |s| {
            // À quelle cellule appartient cette section ? Par le CONTENU du
            // chunk, jamais par le rang dans la boucle.
            let (cx, cz) = cellule_de(niveau, s.chunk.x, s.chunk.z);
            if let Some((_, v)) = paquets.iter_mut().find(|(c, _)| c.x == cx && c.z == cz) {
                v.push(s);
            }
            // Une section hors des cellules demandées est JETÉE : l'emprise
            // est un rectangle, la demande un disque, donc elle en couvre un
            // peu plus. Ce qu'elle coûte se mesure (`--example chargement`).
        })
    }));
    if lu.is_err() {
        let _ = reponses.send(Reponse::Echec(format!(
            "la région {} a fait paniquer le décodage — elle est sautée",
            lot.region.file_name()
        )));
        // On répond quand même pour chaque cellule, sinon l'hôte les
        // attendrait pour toujours.
        for (cellule, _) in paquets {
            if reponses
                .send(Reponse::Prete {
                    cellule,
                    sections: Vec::new(),
                    interner: Interner::new(),
                })
                .is_err()
            {
                return false;
            }
        }
        return true;
    }

    // **Ce qui ne s'est pas lu se DIT.** Une région ou un chunk illisible
    // rend ses cellules vides — elles sont inscrites, donc jamais relues en
    // boucle — mais sans ce message l'utilisateur verrait du vide sans savoir
    // pourquoi. Une fois par lecture : la région n'est pas redemandée.
    if let Ok(bilan) = &lu {
        if bilan.illisibles > 0 {
            let _ = reponses.send(Reponse::Echec(format!(
                "la région {} porte {} chunk(s) illisible(s) : ce qu'ils \
                 contenaient n'est pas affiché",
                lot.region.file_name(),
                bilan.illisibles
            )));
        }
    }

    // **Un interner par CELLULE**, taillé sur ses seules sections. Envoyer
    // celui de la région entière ferait fusionner à l'hôte des états qu'il ne
    // verra peut-être jamais — et surtout, il serait le même objet pour
    // plusieurs réponses, donc à cloner autant de fois.
    for (cellule, sections) in paquets {
        let (sections, local) = resserrer(sections, &interner);
        if reponses
            .send(Reponse::Prete {
                cellule,
                sections,
                interner: local,
            })
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Renumérote les sections d'une cellule dans une table qui ne porte QUE ses
/// états.
///
/// Sans ça, chaque cellule d'une région partirait avec la table de la région
/// entière : l'hôte fusionnerait les mêmes centaines d'états autant de fois
/// qu'il y a de cellules, pour le même résultat.
fn resserrer(mut sections: Vec<SectionLue>, source: &Interner) -> (Vec<SectionLue>, Interner) {
    let mut local = Interner::new();
    // Une table de correspondance par SECTION serait refaite pour rien : la
    // palette d'une section est courte, mais on en traite des milliers.
    for s in &mut sections {
        for id in s.section.palette.iter_mut() {
            let nom = source.resolve(*id).unwrap_or("minecraft:air");
            *id = local.intern(nom);
        }
        if let Some(b) = s.biomes.as_mut() {
            for id in b.iter_mut() {
                let nom = source.resolve(*id).unwrap_or("minecraft:plains");
                *id = local.intern(nom);
            }
        }
    }
    (sections, local)
}

/// L'emprise qui couvre toutes les cellules d'un lot.
fn emprise(lot: &Lot) -> Option<BBox> {
    let mut it = lot.cellules.iter().map(|v| v.cellule.boite);
    let premiere = it.next()?;
    Some(it.fold(premiere, |a, b| {
        BBox::new(
            BlockPos::new(
                a.min.x.min(b.min.x),
                a.min.y.min(b.min.y),
                a.min.z.min(b.min.z),
            ),
            BlockPos::new(
                a.max.x.max(b.max.x),
                a.max.y.max(b.max.y),
                a.max.z.max(b.max.z),
            ),
        )
    }))
}

/// La cellule d'un chunk, à ce niveau.
fn cellule_de(niveau: Niveau, cx: i32, cz: i32) -> (i32, i32) {
    match niveau {
        Niveau::Chunk => (cx, cz),
        // Division PLANCHER : le chunk −1 est dans la région −1, pas la 0.
        Niveau::Region => (cx.div_euclid(32), cz.div_euclid(32)),
    }
}
