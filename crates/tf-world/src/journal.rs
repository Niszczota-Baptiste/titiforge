//! Le journal d'annulation — persistant, typé, à points de reprise nommés.
//!
//! **Ce n'est pas un journal de blocs.** Un PNJ posé un jour par un greffon
//! devra s'annuler avec le même Ctrl+Z qu'un `//set` ; une pile d'instantanés
//! de sections ne saurait pas le faire, et la refonte coûterait toute la pile
//! d'annulation et tout ce qui s'en sert. Une entrée est donc une **suite de
//! corrections typées**, dont le correctif de chunk n'est qu'un genre.
//!
//! Un correctif de chunk ne copie pas le chunk : il porte les OCTETS des plages
//! réécrites, dans les deux sens. Mesuré sur une région pleine (100 663 296
//! blocs), un `//replace` de palette pèse 175 ko dans un sens — contre 30,8 Mo
//! si l'on réécrivait les blocs de champs entiers, et contre 26 Mo si l'on
//! copiait la région. C'est ce qui rend l'annulation proportionnelle à ce
//! qu'une opération a ÉCRIT, et non à ce qu'elle a survolé.
//!
//! ## Ce que le format garantit
//!
//! Le fichier est **ajout seul**. Annuler, refaire, tronquer une branche morte
//! n'écrivent que quelques octets à la fin — jamais une réécriture du journal,
//! qui ferait perdre tout l'historique à la première coupure de courant. Un
//! enregistrement tronqué par un arrêt brutal est détecté par sa longueur et
//! son empreinte, et la lecture s'arrête là : le journal se répare tout seul,
//! au prix de la dernière action.
//!
//! Chaque correctif porte l'empreinte du chunk **avant** et **après**. Un
//! chunk qui a changé sous le journal — édité ailleurs, restauré depuis une
//! sauvegarde — fait échouer l'annulation au lieu de la réussir à côté. Une
//! annulation qui écrit au mauvais endroit est pire qu'une annulation refusée.

use std::collections::BTreeSet;

use tf_anvil::{inverse_edits, splice, Edit, SpliceError};
use tf_nbt::Span;

use crate::coords::{BBox, RegionPos};
use crate::source::{Dimension, Folder};

/// Magie du format. Un format binaire se reconnaît à ses OCTETS, jamais à son
/// nom de fichier — c'est ce qui permet de relire l'ancien.
pub const MAGIE: &[u8; 4] = b"TFJ1";

/// Au-delà, on refuse de lire : un journal forgé annoncerait 4 Go et ferait
/// mourir le processus sur une allocation.
pub const MAX_CORPS: usize = 256 * 1024 * 1024;

// ── le chunk visé ───────────────────────────────────────────────────────────

/// Adresse d'un chunk dans le monde.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cible {
    pub dim: Dimension,
    pub folder: Folder,
    pub region: RegionPos,
    /// Index du chunk dans sa région, 0..1024 — l'index de l'en-tête Anvil,
    /// donc `(z & 31) * 32 + (x & 31)`.
    pub chunk: u16,
}

// ── les corrections ─────────────────────────────────────────────────────────

/// Un correctif sur un chunk : les octets des plages réécrites, dans les deux
/// sens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkPatch {
    pub cible: Cible,
    /// Empreinte du chunk inflaté AVANT l'opération.
    pub avant_hash: u64,
    /// Empreinte du chunk inflaté APRÈS.
    pub apres_hash: u64,
    /// Éditions qui REFONT : s'appliquent à l'état d'avant.
    pub refaire: Vec<Edit>,
    /// Éditions qui ANNULENT : s'appliquent à l'état d'après.
    ///
    /// Elles ne se déduisent PAS des précédentes une fois l'annulation faite :
    /// reconstruire le sens « refaire » demanderait les octets d'après, qui ont
    /// justement disparu. Les deux sens sont donc stockés — et c'est bon marché
    /// parce que les éditions sont resserrées sur ce qui change vraiment.
    pub annuler: Vec<Edit>,
}

impl ChunkPatch {
    /// Enregistre un correctif depuis ce que l'opération a produit.
    ///
    /// `edits` sont les éditions appliquées à `avant` pour obtenir `apres`.
    pub fn record(
        cible: Cible,
        avant: &[u8],
        apres: &[u8],
        edits: &[Edit],
    ) -> Result<Self, SpliceError> {
        Ok(ChunkPatch {
            cible,
            avant_hash: empreinte(avant),
            apres_hash: empreinte(apres),
            annuler: inverse_edits(avant, edits)?,
            refaire: edits.to_vec(),
        })
    }

    /// Poids en octets de ce que ce correctif fait porter au journal.
    pub fn poids(&self) -> usize {
        let f = |v: &Vec<Edit>| v.iter().map(|e| e.bytes.len() + 8).sum::<usize>();
        f(&self.annuler) + f(&self.refaire)
    }

    /// Applique le sens ANNULER à un chunk inflaté.
    pub fn undo(&self, courant: &[u8]) -> Result<Vec<u8>, JournalError> {
        self.appliquer(courant, self.apres_hash, &self.annuler)
    }

    /// Applique le sens REFAIRE.
    pub fn redo(&self, courant: &[u8]) -> Result<Vec<u8>, JournalError> {
        self.appliquer(courant, self.avant_hash, &self.refaire)
    }

    fn appliquer(
        &self,
        courant: &[u8],
        attendu: u64,
        edits: &[Edit],
    ) -> Result<Vec<u8>, JournalError> {
        let vu = empreinte(courant);
        if vu != attendu {
            // Refuser plutôt qu'écrire à côté : le chunk a changé sous le
            // journal, et appliquer des plages calculées sur d'autres octets
            // corromprait la save sans rien signaler.
            return Err(JournalError::Divergence {
                cible: self.cible.clone(),
                attendu,
                vu,
            });
        }
        let mut e = edits.to_vec();
        splice(courant, &mut e).map_err(JournalError::Splice)
    }
}

/// Ce qu'une entrée sait défaire.
///
/// L'`enum` est la couture de l'extensibilité : un greffon qui poserait un PNJ
/// ajoutera son genre ici, et l'annulation continuera de marcher pour tout le
/// reste. `Inconnu` existe pour qu'un journal écrit par une version qui en sait
/// plus reste LISIBLE : on ne comprend pas l'entrée, on refuse de l'annuler, et
/// on ne perd pas l'historique qu'il y a autour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correction {
    Chunk(ChunkPatch),
    Inconnu { genre: u8, octets: Vec<u8> },
}

impl Correction {
    pub fn poids(&self) -> usize {
        match self {
            Correction::Chunk(p) => p.poids(),
            Correction::Inconnu { octets, .. } => octets.len(),
        }
    }
}

// ── les entrées ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Genre {
    /// Une opération réversible.
    Operation {
        /// Nom court de l'opération (`replace`, `set`…). Opaque au journal :
        /// c'est ce qui permet à un greffon d'en inventer.
        op: String,
        /// Ce que l'opération a VRAIMENT écrit. Sert au remaillage incrémental
        /// et à recadrer la vue sur une annulation.
        bounds: Option<BBox>,
        corrections: Vec<Correction>,
    },
    /// Un point de reprise nommé. Ne défait rien : c'est un repère.
    Reprise,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entree {
    pub id: u64,
    /// Ce que l'utilisateur lit dans la liste : « Remplacer pierre → terre ».
    pub label: String,
    /// Millisecondes depuis l'époque Unix. Zéro si l'hôte n'a pas d'horloge.
    pub horodatage: i64,
    pub genre: Genre,
}

impl Entree {
    pub fn est_reprise(&self) -> bool {
        matches!(self.genre, Genre::Reprise)
    }

    pub fn poids(&self) -> usize {
        match &self.genre {
            Genre::Operation { corrections, .. } => {
                corrections.iter().map(Correction::poids).sum::<usize>()
            }
            Genre::Reprise => 0,
        }
    }

    /// Les correctifs dans l'ordre où il faut les appliquer pour **REFAIRE**
    /// — celui où ils ont été enregistrés.
    ///
    /// Une opération peut toucher DEUX FOIS le même chunk : `//move` efface
    /// sa source puis repose l'extrait, et les deux passes s'enregistrent dans
    /// la MÊME entrée, parce qu'un seul `Ctrl+Z` doit défaire le déplacement
    /// entier.
    pub fn a_refaire(&self) -> impl Iterator<Item = &Correction> {
        self.corrections().iter()
    }

    /// Les correctifs dans l'ordre où il faut les appliquer pour **ANNULER** :
    /// l'inverse du précédent.
    ///
    /// **Ce n'est pas un détail de présentation.** Chaque correctif est gardé
    /// par l'empreinte de l'état qu'il attend ; deux correctifs sur le même
    /// chunk s'enchaînent, et les rejouer dans l'ordre d'enregistrement fait
    /// échouer le second sur `Divergence`. Le sens correct est donné ici, et
    /// une seule fois : un appelant qui écrirait `corrections.iter()` à la
    /// main aurait raison tant qu'aucune opération ne repasse sur un chunk,
    /// puis tort sans prévenir — c'est exactement la forme des pièges que ce
    /// dépôt paie le plus cher.
    pub fn a_annuler(&self) -> impl Iterator<Item = &Correction> {
        self.corrections().iter().rev()
    }

    fn corrections(&self) -> &[Correction] {
        match &self.genre {
            Genre::Operation { corrections, .. } => corrections,
            Genre::Reprise => &[],
        }
    }

    /// Régions touchées, pour savoir quoi relire avant d'annuler.
    pub fn regions(&self) -> BTreeSet<(Dimension, Folder, RegionPos)> {
        let mut out = BTreeSet::new();
        if let Genre::Operation { corrections, .. } = &self.genre {
            for c in corrections {
                if let Correction::Chunk(p) = c {
                    out.insert((p.cible.dim.clone(), p.cible.folder, p.cible.region));
                }
            }
        }
        out
    }
}

// ── le journal ──────────────────────────────────────────────────────────────

/// Historique linéaire. La 1.0 est solo : une branche suffit, et une branche
/// suffit aussi à des points de reprise nommés.
#[derive(Debug, Clone, Default)]
pub struct Journal {
    entrees: Vec<Entree>,
    /// Nombre d'entrées APPLIQUÉES. Celles au-delà sont annulées et attendent
    /// d'être refaites.
    curseur: usize,
    prochain_id: u64,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entrees(&self) -> &[Entree] {
        &self.entrees
    }

    pub fn curseur(&self) -> usize {
        self.curseur
    }

    pub fn peut_annuler(&self) -> bool {
        self.curseur > 0
    }

    pub fn peut_refaire(&self) -> bool {
        self.curseur < self.entrees.len()
    }

    /// Poids total de ce que le journal porte, en octets d'éditions.
    pub fn poids(&self) -> usize {
        self.entrees.iter().map(Entree::poids).sum()
    }

    /// Ajoute une entrée. Les entrées annulées qui la suivaient DISPARAISSENT —
    /// c'est ce qu'attend tout éditeur : agir après une annulation abandonne la
    /// branche défaite.
    ///
    /// Rend les enregistrements à écrire pour que le fichier reflète l'état.
    pub fn pousser(&mut self, label: &str, horodatage: i64, genre: Genre) -> Vec<Record> {
        let mut sortie = Vec::new();
        if self.curseur < self.entrees.len() {
            let depuis = self.entrees[self.curseur].id;
            self.entrees.truncate(self.curseur);
            sortie.push(Record::Troncature(depuis));
        }
        let e = Entree {
            id: self.prochain_id,
            label: label.to_string(),
            horodatage,
            genre,
        };
        self.prochain_id += 1;
        self.entrees.push(e.clone());
        self.curseur = self.entrees.len();
        sortie.push(Record::Entree(e));
        sortie
    }

    /// Pose un point de reprise nommé.
    pub fn reprise(&mut self, label: &str, horodatage: i64) -> Vec<Record> {
        self.pousser(label, horodatage, Genre::Reprise)
    }

    /// L'entrée à défaire, et le curseur reculé. `None` si tout est déjà défait.
    ///
    /// Un point de reprise se traverse sans rien faire : il ne défait rien, et
    /// s'arrêter dessus obligerait l'utilisateur à appuyer deux fois.
    pub fn annuler(&mut self) -> Option<(&Entree, Record)> {
        while self.curseur > 0 && self.entrees[self.curseur - 1].est_reprise() {
            self.curseur -= 1;
        }
        if self.curseur == 0 {
            return None;
        }
        self.curseur -= 1;
        Some((&self.entrees[self.curseur], Record::Curseur(self.curseur)))
    }

    /// L'entrée à refaire, et le curseur avancé.
    pub fn refaire(&mut self) -> Option<(&Entree, Record)> {
        while self.curseur < self.entrees.len() && self.entrees[self.curseur].est_reprise() {
            self.curseur += 1;
        }
        if self.curseur >= self.entrees.len() {
            return None;
        }
        let i = self.curseur;
        self.curseur += 1;
        Some((&self.entrees[i], Record::Curseur(self.curseur)))
    }

    /// Les points de reprise, du plus récent au plus ancien.
    pub fn reprises(&self) -> Vec<&Entree> {
        self.entrees
            .iter()
            .rev()
            .filter(|e| e.est_reprise())
            .collect()
    }

    /// Ce qu'il faut faire pour se retrouver JUSTE APRÈS l'entrée `id`.
    ///
    /// Rend les entrées dans l'ordre d'application. Chercher par identifiant et
    /// non par position : les positions bougent à chaque troncature, et un
    /// signet posé sur une position désignerait un jour une autre action.
    pub fn chemin_vers(&self, id: u64) -> Option<Chemin> {
        let i = self.entrees.iter().position(|e| e.id == id)?;
        let vise = i + 1;
        Some(if vise < self.curseur {
            Chemin::Annuler((vise..self.curseur).rev().collect())
        } else if vise > self.curseur {
            Chemin::Refaire((self.curseur..vise).collect())
        } else {
            Chemin::Rien
        })
    }

    /// Déplace le curseur après avoir joué un chemin.
    pub fn poser_curseur(&mut self, position: usize) -> Record {
        self.curseur = position.min(self.entrees.len());
        Record::Curseur(self.curseur)
    }

    pub fn entree(&self, index: usize) -> Option<&Entree> {
        self.entrees.get(index)
    }

    /// Oublie les plus VIEILLES entrées jusqu'à tenir dans un budget d'octets.
    /// Rend le nombre d'entrées oubliées.
    ///
    /// Une annulation persistante sans plafond finit par remplir le disque :
    /// un `//set` sur une grosse sélection doit garder les 4 096 indices de
    /// chaque section touchée pour pouvoir les rendre, et rien ne limite le
    /// nombre d'opérations qu'un utilisateur enchaîne.
    ///
    /// On élague par le VIEUX bout, jamais par le récent, et jamais au-delà du
    /// curseur : ce qui est annulé attend d'être refait, l'oublier perdrait du
    /// travail. Les points de reprise ne pèsent rien et restent : un repère
    /// reste atteignable tant que ce qui le SUIT est intact.
    ///
    /// L'élagage ne s'écrit pas en ajoutant : c'est le moment de réécrire le
    /// fichier avec `reecrire()`.
    pub fn elaguer(&mut self, budget: usize) -> usize {
        let mut poids = self.poids();
        let mut jeter = vec![false; self.entrees.len()];
        for (e, j) in self.entrees.iter().zip(jeter.iter_mut()).take(self.curseur) {
            if poids <= budget {
                break;
            }
            // Un repère ne pèse rien : le jeter ne rapprocherait pas du budget
            // et retirerait un point que l'utilisateur a NOMMÉ pour y revenir.
            if e.est_reprise() {
                continue;
            }
            poids -= e.poids();
            *j = true;
        }
        let jetes = jeter.iter().filter(|b| **b).count();
        if jetes == 0 {
            return 0;
        }
        self.curseur -= jeter[..self.curseur].iter().filter(|b| **b).count();
        let mut i = 0;
        self.entrees.retain(|_| {
            i += 1;
            !jeter[i - 1]
        });
        jetes
    }

    /// Les enregistrements d'un fichier NEUF portant l'état courant.
    ///
    /// Le journal est en ajout seul, donc il grossit même quand l'historique
    /// rétrécit. C'est le compactage : on l'écrit ailleurs, puis on remplace.
    pub fn reecrire(&self) -> Vec<Record> {
        let mut out: Vec<Record> = self.entrees.iter().cloned().map(Record::Entree).collect();
        out.push(Record::Curseur(self.curseur));
        out
    }

    /// Rejoue un enregistrement lu sur le disque.
    pub fn appliquer_record(&mut self, r: Record) {
        match r {
            Record::Entree(e) => {
                self.prochain_id = self.prochain_id.max(e.id + 1);
                self.entrees.push(e);
                self.curseur = self.entrees.len();
            }
            Record::Curseur(c) => self.curseur = c.min(self.entrees.len()),
            Record::Troncature(depuis) => {
                self.entrees.retain(|e| e.id < depuis);
                self.curseur = self.curseur.min(self.entrees.len());
            }
        }
    }
}

/// Ce qu'il faut jouer pour atteindre un point de l'historique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chemin {
    Rien,
    /// Indices des entrées à ANNULER, dans l'ordre (de la plus récente à la
    /// plus ancienne).
    Annuler(Vec<usize>),
    /// Indices des entrées à REFAIRE, dans l'ordre.
    Refaire(Vec<usize>),
}

impl Chemin {
    pub fn est_vide(&self) -> bool {
        matches!(self, Chemin::Rien)
    }

    pub fn indices(&self) -> &[usize] {
        match self {
            Chemin::Rien => &[],
            Chemin::Annuler(v) | Chemin::Refaire(v) => v,
        }
    }
}

/// Un enregistrement du fichier. Le journal est en AJOUT SEUL : annuler,
/// refaire et tronquer n'écrivent que quelques octets à la fin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Entree(Entree),
    Curseur(usize),
    /// Toutes les entrées d'identifiant ≥ celui-ci ont été abandonnées.
    Troncature(u64),
}

// ── erreurs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// Le chunk a changé sous le journal. On refuse : appliquer des plages
    /// calculées sur d'autres octets corromprait la save en silence.
    Divergence {
        cible: Cible,
        attendu: u64,
        vu: u64,
    },
    Splice(SpliceError),
    /// Le fichier ne commence pas par la magie du format.
    PasUnJournal,
    /// Un enregistrement est tronqué ou son empreinte ne colle pas. La lecture
    /// s'arrête là, elle n'échoue pas : c'est l'arrêt brutal ordinaire.
    Tronque,
    /// Genre d'enregistrement qu'on ne sait pas lire.
    Illisible(u8),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalError::Divergence { cible, .. } => write!(
                f,
                "le chunk {} de la région {},{} a changé depuis cette action : \
                 annulation refusée",
                cible.chunk, cible.region.x, cible.region.z
            ),
            JournalError::Splice(e) => write!(f, "{e}"),
            JournalError::PasUnJournal => write!(f, "ce fichier n'est pas un journal titiforge"),
            JournalError::Tronque => write!(f, "journal tronqué"),
            JournalError::Illisible(g) => write!(f, "enregistrement de genre inconnu ({g})"),
        }
    }
}

impl std::error::Error for JournalError {}

// ── empreinte ───────────────────────────────────────────────────────────────

/// FNV-1a 64 bits.
///
/// Il ne s'agit pas de résister à un adversaire : il s'agit de repérer un chunk
/// qui a changé sous le journal. Une empreinte cryptographique coûterait dix
/// fois plus pour la même réponse.
pub fn empreinte(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ── sérialisation ───────────────────────────────────────────────────────────

mod octets {
    use super::*;

    pub struct W(pub Vec<u8>);

    impl W {
        pub fn u8(&mut self, v: u8) -> &mut Self {
            self.0.push(v);
            self
        }
        pub fn u16(&mut self, v: u16) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn u32(&mut self, v: u32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn u64(&mut self, v: u64) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn i32(&mut self, v: i32) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn i64(&mut self, v: i64) -> &mut Self {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn texte(&mut self, s: &str) -> &mut Self {
            self.u32(s.len() as u32);
            self.0.extend_from_slice(s.as_bytes());
            self
        }
        pub fn blob(&mut self, b: &[u8]) -> &mut Self {
            self.u32(b.len() as u32);
            self.0.extend_from_slice(b);
            self
        }
    }

    pub struct R<'a> {
        pub b: &'a [u8],
        pub p: usize,
    }

    type Res<T> = Result<T, JournalError>;

    impl<'a> R<'a> {
        pub fn new(b: &'a [u8]) -> Self {
            R { b, p: 0 }
        }
        fn prendre(&mut self, n: usize) -> Res<&'a [u8]> {
            let fin = self.p.checked_add(n).ok_or(JournalError::Tronque)?;
            if fin > self.b.len() {
                return Err(JournalError::Tronque);
            }
            let s = &self.b[self.p..fin];
            self.p = fin;
            Ok(s)
        }
        pub fn u8(&mut self) -> Res<u8> {
            Ok(self.prendre(1)?[0])
        }
        pub fn u16(&mut self) -> Res<u16> {
            Ok(u16::from_le_bytes(self.prendre(2)?.try_into().unwrap()))
        }
        pub fn u32(&mut self) -> Res<u32> {
            Ok(u32::from_le_bytes(self.prendre(4)?.try_into().unwrap()))
        }
        pub fn u64(&mut self) -> Res<u64> {
            Ok(u64::from_le_bytes(self.prendre(8)?.try_into().unwrap()))
        }
        pub fn i32(&mut self) -> Res<i32> {
            Ok(i32::from_le_bytes(self.prendre(4)?.try_into().unwrap()))
        }
        pub fn i64(&mut self) -> Res<i64> {
            Ok(i64::from_le_bytes(self.prendre(8)?.try_into().unwrap()))
        }
        /// Une longueur venue du fichier se vérifie contre la place RESTANTE
        /// avant de servir à réserver : `u32::MAX` annoncé dans un fichier de
        /// quatre octets tuerait le processus sur l'allocation.
        pub fn longueur(&mut self) -> Res<usize> {
            let n = self.u32()? as usize;
            if n > self.b.len() - self.p {
                return Err(JournalError::Tronque);
            }
            Ok(n)
        }
        pub fn texte(&mut self) -> Res<String> {
            let n = self.longueur()?;
            let s = self.prendre(n)?;
            String::from_utf8(s.to_vec()).map_err(|_| JournalError::Tronque)
        }
        pub fn blob(&mut self) -> Res<Vec<u8>> {
            let n = self.longueur()?;
            Ok(self.prendre(n)?.to_vec())
        }
    }
}

use octets::{R, W};

const G_ENTREE: u8 = 0;
const G_CURSEUR: u8 = 1;
const G_TRONCATURE: u8 = 2;

const C_OPERATION: u8 = 0;
const C_REPRISE: u8 = 1;

const K_CHUNK: u8 = 0;

const D_OVERWORLD: u8 = 0;
const D_NETHER: u8 = 1;
const D_END: u8 = 2;
const D_CUSTOM: u8 = 3;

fn ecrire_dimension(w: &mut W, d: &Dimension) {
    match d {
        Dimension::Overworld => {
            w.u8(D_OVERWORLD);
        }
        Dimension::Nether => {
            w.u8(D_NETHER);
        }
        Dimension::End => {
            w.u8(D_END);
        }
        Dimension::Custom { namespace, path } => {
            w.u8(D_CUSTOM).texte(namespace).texte(path);
        }
    }
}

fn lire_dimension(r: &mut R) -> Result<Dimension, JournalError> {
    Ok(match r.u8()? {
        D_OVERWORLD => Dimension::Overworld,
        D_NETHER => Dimension::Nether,
        D_END => Dimension::End,
        D_CUSTOM => Dimension::Custom {
            namespace: r.texte()?,
            path: r.texte()?,
        },
        autre => return Err(JournalError::Illisible(autre)),
    })
}

fn ecrire_edits(w: &mut W, edits: &[Edit]) {
    w.u32(edits.len() as u32);
    for e in edits {
        w.u32(e.span.start as u32)
            .u32(e.span.end as u32)
            .blob(&e.bytes);
    }
}

fn lire_edits(r: &mut R) -> Result<Vec<Edit>, JournalError> {
    let n = r.u32()? as usize;
    // Une édition pèse au moins 12 octets sur le disque : au-delà de ce que le
    // reste du corps peut porter, la longueur est fausse.
    if n > (r.b.len() - r.p) / 12 + 1 {
        return Err(JournalError::Tronque);
    }
    let mut out = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let start = r.u32()? as usize;
        let end = r.u32()? as usize;
        if start > end {
            return Err(JournalError::Tronque);
        }
        out.push(Edit {
            span: Span { start, end },
            bytes: r.blob()?,
        });
    }
    Ok(out)
}

fn ecrire_bbox(w: &mut W, b: &Option<BBox>) {
    match b {
        None => {
            w.u8(0);
        }
        Some(b) => {
            w.u8(1)
                .i32(b.min.x)
                .i32(b.min.y)
                .i32(b.min.z)
                .i32(b.max.x)
                .i32(b.max.y)
                .i32(b.max.z);
        }
    }
}

fn lire_bbox(r: &mut R) -> Result<Option<BBox>, JournalError> {
    Ok(match r.u8()? {
        0 => None,
        _ => {
            let (x0, y0, z0) = (r.i32()?, r.i32()?, r.i32()?);
            let (x1, y1, z1) = (r.i32()?, r.i32()?, r.i32()?);
            Some(BBox::new(
                crate::coords::BlockPos::new(x0, y0, z0),
                crate::coords::BlockPos::new(x1, y1, z1),
            ))
        }
    })
}

fn ecrire_correction(w: &mut W, c: &Correction) {
    match c {
        Correction::Chunk(p) => {
            w.u8(K_CHUNK);
            ecrire_dimension(w, &p.cible.dim);
            w.u8(p.cible.folder.code())
                .i32(p.cible.region.x)
                .i32(p.cible.region.z)
                .u16(p.cible.chunk)
                .u64(p.avant_hash)
                .u64(p.apres_hash);
            ecrire_edits(w, &p.refaire);
            ecrire_edits(w, &p.annuler);
        }
        Correction::Inconnu { genre, octets } => {
            w.u8(*genre).blob(octets);
        }
    }
}

fn lire_correction(r: &mut R) -> Result<Correction, JournalError> {
    let genre = r.u8()?;
    if genre != K_CHUNK {
        // On ne comprend pas, mais on garde les octets : réécrire le journal
        // sans eux perdrait l'entrée d'une version plus récente.
        return Ok(Correction::Inconnu {
            genre,
            octets: r.blob()?,
        });
    }
    let dim = lire_dimension(r)?;
    let folder = Folder::depuis_code(r.u8()?).ok_or(JournalError::Tronque)?;
    let region = RegionPos::new(r.i32()?, r.i32()?);
    let chunk = r.u16()?;
    let avant_hash = r.u64()?;
    let apres_hash = r.u64()?;
    let refaire = lire_edits(r)?;
    let annuler = lire_edits(r)?;
    Ok(Correction::Chunk(ChunkPatch {
        cible: Cible {
            dim,
            folder,
            region,
            chunk,
        },
        avant_hash,
        apres_hash,
        refaire,
        annuler,
    }))
}

/// Sérialise le CORPS d'un enregistrement.
fn corps(r: &Record) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(64));
    match r {
        Record::Curseur(c) => {
            w.u8(G_CURSEUR).u64(*c as u64);
        }
        Record::Troncature(id) => {
            w.u8(G_TRONCATURE).u64(*id);
        }
        Record::Entree(e) => {
            w.u8(G_ENTREE).u64(e.id).i64(e.horodatage).texte(&e.label);
            match &e.genre {
                Genre::Reprise => {
                    w.u8(C_REPRISE);
                }
                Genre::Operation {
                    op,
                    bounds,
                    corrections,
                } => {
                    w.u8(C_OPERATION).texte(op);
                    ecrire_bbox(&mut w, bounds);
                    w.u32(corrections.len() as u32);
                    for c in corrections {
                        ecrire_correction(&mut w, c);
                    }
                }
            }
        }
    }
    w.0
}

fn lire_corps(b: &[u8]) -> Result<Record, JournalError> {
    let mut r = R::new(b);
    Ok(match r.u8()? {
        G_CURSEUR => Record::Curseur(r.u64()? as usize),
        G_TRONCATURE => Record::Troncature(r.u64()?),
        G_ENTREE => {
            let id = r.u64()?;
            let horodatage = r.i64()?;
            let label = r.texte()?;
            let genre = match r.u8()? {
                C_REPRISE => Genre::Reprise,
                C_OPERATION => {
                    let op = r.texte()?;
                    let bounds = lire_bbox(&mut r)?;
                    let n = r.u32()? as usize;
                    if n > r.b.len() - r.p {
                        return Err(JournalError::Tronque);
                    }
                    let mut corrections = Vec::with_capacity(n.min(4096));
                    for _ in 0..n {
                        corrections.push(lire_correction(&mut r)?);
                    }
                    Genre::Operation {
                        op,
                        bounds,
                        corrections,
                    }
                }
                autre => return Err(JournalError::Illisible(autre)),
            };
            Record::Entree(Entree {
                id,
                label,
                horodatage,
                genre,
            })
        }
        autre => return Err(JournalError::Illisible(autre)),
    })
}

/// Encode un enregistrement, prêt à être AJOUTÉ au fichier.
///
/// Le corps est compressé quand ça vaut le coup — au niveau 1, et seulement
/// si l'on gagne au moins un dixième. Un `//set` sur une région entière doit
/// garder les 4 096 indices de chaque section pour pouvoir les rendre : c'est
/// là que la compression paie, et les indices packés se compressent très bien.
/// Un correctif de palette, lui, fait quelques dizaines d'octets : le
/// compresser les ferait GROSSIR.
pub fn encoder(r: &Record) -> Vec<u8> {
    let brut = corps(r);
    let (codec, corps) = match tf_anvil::deflate_level(&brut, tf_anvil::Compression::Zlib, 1) {
        Ok(z) if z.len() * 10 < brut.len() * 9 => (1u8, z),
        _ => (0u8, brut),
    };
    let mut out = Vec::with_capacity(corps.len() + 13);
    out.extend_from_slice(&(corps.len() as u32).to_le_bytes());
    out.extend_from_slice(&empreinte(&corps).to_le_bytes());
    out.push(codec);
    out.extend_from_slice(&corps);
    out
}

/// En-tête d'un journal neuf.
pub fn entete() -> Vec<u8> {
    MAGIE.to_vec()
}

/// Relit un journal entier.
///
/// Un enregistrement tronqué ou dont l'empreinte ne colle pas ARRÊTE la
/// lecture sans la faire échouer : c'est l'arrêt brutal ordinaire, et perdre
/// la dernière action vaut mieux que perdre l'historique. Le second membre dit
/// combien d'octets étaient valides — c'est là qu'il faut tronquer le fichier
/// avant de recommencer à y ajouter.
pub fn decoder(fichier: &[u8]) -> Result<(Journal, usize), JournalError> {
    if fichier.len() < 4 || &fichier[..4] != MAGIE {
        return Err(JournalError::PasUnJournal);
    }
    let mut j = Journal::new();
    let mut p = 4usize;
    loop {
        if p + 13 > fichier.len() {
            break;
        }
        let n = u32::from_le_bytes(fichier[p..p + 4].try_into().unwrap()) as usize;
        let h = u64::from_le_bytes(fichier[p + 4..p + 12].try_into().unwrap());
        let codec = fichier[p + 12];
        let debut = p + 13;
        if n > MAX_CORPS || debut + n > fichier.len() {
            break;
        }
        let corps = &fichier[debut..debut + n];
        if empreinte(corps) != h {
            break;
        }
        let brut = match codec {
            0 => corps.to_vec(),
            1 => match tf_anvil::inflate(corps, tf_anvil::Compression::Zlib) {
                Ok(b) => b,
                Err(_) => break,
            },
            _ => break,
        };
        match lire_corps(&brut) {
            Ok(r) => j.appliquer_record(r),
            // Un enregistrement qu'on ne comprend pas n'invalide pas ce qui
            // précède, mais on ne peut pas deviner ce qui suit : on s'arrête.
            Err(_) => break,
        }
        p = debut + n;
    }
    Ok((j, p))
}
