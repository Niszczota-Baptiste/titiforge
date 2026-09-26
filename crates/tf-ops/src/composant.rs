//! **Les composants : une définition, N instances, et la mise à jour partout.**
//!
//! Le troisième héritage — SketchUp — et le seul qui touche à l'architecture :
//! une fenêtre posée vingt fois doit se mettre à jour partout quand on modifie
//! sa définition, et UN Ctrl+Z doit défaire la modification, pas les vingt
//! poses.
//!
//! **Le monde reste souverain.** Un composant est une COUCHE qui s'estampe
//! dans le monde, pas un modèle dont le monde serait la sortie : on peut
//! toujours poser un bloc à la main n'importe où, y compris dans une instance.
//!
//! ## Ce qu'une instance occupe
//!
//! Une définition est un extrait en coordonnées LOCALES. Ses cases d'AIR ne
//! font pas partie du composant : l'instance y est transparente, et ce que le
//! monde porte là y reste — le mur autour d'une fenêtre.
//!
//! ## Ce qu'une mise à jour écrit
//!
//! **Ce qui CHANGE entre l'ancienne définition et la nouvelle, et rien
//! d'autre** : un bloc, ou le contenu d'un coffre. Ce qui était matière et ne
//! l'est plus devient de l'air. Le reste n'est pas touché, et c'est voulu
//! deux fois : un coffre qu'un joueur a rempli dans l'une des vingt maisons
//! garde son contenu tant que la définition ne change pas CE coffre — la
//! réestamper en entier l'aurait vidé ; et une mise à jour qui change trois
//! blocs ne relit que trois blocs sous chaque instance, pas la maison
//! entière. La contrepartie est celle de SketchUp, en plus étroit : une
//! retouche faite dans une instance est perdue là où la définition change.
//!
//! Mettre à jour une définition DEPUIS une instance prend tout ce que sa boîte
//! contient : posée dans un mur, ses cases transparentes portent le mur, qui
//! entrerait dans le composant. Rien ne distingue « ajouté exprès » de « déjà
//! là » ; l'action COMPTE donc les cases qui entrent dans le composant et
//! celles qui en sortent, pour que ça se voie — et un Ctrl+Z le défait.
//!
//! ## Où vit le document
//!
//! Dans le MONDE : un fichier de la copie de travail ([`FICHIER`]), écrit dans
//! la save avec les régions, après la même sauvegarde. Il voyage avec la save
//! qu'on s'échange, survit à la fermeture de la séance, et s'annule par le
//! même journal que les blocs — [`faire`] fait UNE entrée des correctifs des
//! chunks et de celui du document.
//!
//! ## Tout ou rien
//!
//! Une mise à jour écrit sous vingt instances, l'une après l'autre. Qu'elle
//! échoue à la douzième, et les onze premières sont DÉFAITES avant que
//! l'erreur ne remonte (`defaire_rapport`) : restées là sans entrée de
//! journal, aucun Ctrl+Z ne les aurait atteintes. Le terrain manquant, lui,
//! se vérifie sous TOUTES les instances avant la première écriture.
//!
//! **L'identité est stable** : un identifiant n'est jamais réutilisé, même
//! après un détachement. L'instance n° 37 reste la 37, et un 37 neuf ne peut
//! pas prendre sa place.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use tf_anvil::entites::Entite;
use tf_anvil::{Interner, StateId};
use tf_blocks::Transfo;
use tf_world::coords::{BBox, BlockPos, ChunkPos};
use tf_world::journal::{Correction, FichierPatch, Genre, Journal, Record};
use tf_world::source::{Dimension, Folder, RegionSource, SourceError};
use tf_world::staging::{RegionStore, Staging};

use crate::edition::{chunks_absents, coller, copier, defaire_rapport, Erreur, Pas, RapportRegion};
use crate::executer::Regle;
use crate::presse::{Collage, Presse, TransfoBoite};

/// Le fichier du monde qui porte le document : `titiforge-projet`, à la
/// racine de la save.
pub const FICHIER: &str = "projet";

/// Magie du format. Un format se reconnaît à ses OCTETS.
const MAGIE: &[u8; 4] = b"TFP1";
/// La version écrite. Un document d'une version PLUS RÉCENTE est refusé : le
/// réécrire le tronquerait de ce qu'on n'en comprend pas.
const VERSION: u16 = 1;
/// Au-delà, une définition est refusée à la lecture : un document forgé
/// annoncerait des milliards de cases, et `vec![]` n'échoue pas gentiment.
pub const MAX_CASES: usize = 16 * 1024 * 1024;

/// Ce que le collage d'une instance SAUTE : les cases qui ne font pas partie
/// du composant. Un état qu'aucun monde ne porte — le collage ne l'écrit
/// jamais, puisqu'il le reçoit comme son « air ».
const TRANSPARENT: &str = "titiforge:transparent";

// ── le document ─────────────────────────────────────────────────────────────

/// Le document des composants d'un monde.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Projet {
    /// Le prochain identifiant. Jamais réutilisé.
    pub prochain: u64,
    /// Triées par identifiant.
    pub definitions: Vec<Definition>,
    /// Triées par identifiant.
    pub instances: Vec<Instance>,
}

/// Une définition : ce que toutes ses instances portent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub id: u64,
    pub nom: String,
    pub contenu: Contenu,
}

/// Un extrait en CLÉS d'état — jamais en `StateId`, qui n'a de sens que
/// relativement à SON interner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contenu {
    /// X, Y, Z.
    pub taille: [u32; 3],
    /// Les états, triés — deux contenus égaux s'encodent pareil.
    pub palette: Vec<String>,
    /// Un indice de palette par case, en YZX.
    pub cases: Vec<u32>,
    /// Les block entities, en coordonnées LOCALES : un coffre d'un composant
    /// se retrouve dans chacune de ses instances.
    pub entites: Vec<Entite>,
}

/// Une instance : une définition posée quelque part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub id: u64,
    pub definition: u64,
    pub dim: Dimension,
    /// Le coin de plus petites coordonnées de sa boîte, en MONDE.
    pub coin: BlockPos,
    /// Comment la définition y est tournée ou réfléchie.
    pub transfo: Option<Transfo>,
}

/// L'air et ses variantes ne sont pas de la matière : une instance y est
/// transparente.
fn est_air(cle: &str) -> bool {
    matches!(
        cle,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

impl Contenu {
    /// Le contenu d'un extrait, sous une forme CANONIQUE : deux copies du
    /// même composant prises à deux endroits du monde doivent être égales,
    /// sinon chaque mise à jour se croirait un changement.
    ///
    /// - Les variantes de l'air se ramènent à `minecraft:air` : elles disent
    ///   toutes « pas de matière ».
    /// - Une block entity porte dans ses octets les coordonnées MONDE d'où on
    ///   l'a prise : elles sont réécrites à sa case LOCALE. Un coffre copié
    ///   en (2, −60, 2) et le même en (130, −60, 22) ne diffèrent sinon que
    ///   par ces douze octets — de quoi réécrire les vingt instances d'une
    ///   maison qui n'a pas changé.
    /// - Elles sont triées par case : l'ordre du ramassage dépend du
    ///   découpage en chunks, donc de l'endroit.
    pub fn depuis_presse(p: &Presse, interner: &Interner) -> Contenu {
        let cle = |id: StateId| {
            let c = interner.resolve(id).unwrap_or("minecraft:air");
            if est_air(c) {
                "minecraft:air"
            } else {
                c
            }
        };
        let mut palette: Vec<String> = p
            .palette()
            .into_iter()
            .map(|id| cle(id).to_string())
            .collect();
        palette.sort_unstable();
        palette.dedup();
        let rang: HashMap<&str, u32> = palette
            .iter()
            .enumerate()
            .map(|(i, c)| (c.as_str(), i as u32))
            .collect();
        let mut memo: HashMap<StateId, u32> = HashMap::new();
        let cases = p
            .blocs
            .iter()
            .map(|id| *memo.entry(*id).or_insert_with(|| rang[cle(*id)]))
            .collect();
        let mut entites: Vec<Entite> = p
            .entites
            .iter()
            .map(|e| Entite {
                case: e.case,
                nbt: e.octets(),
                champs: e.champs,
            })
            .collect();
        entites.sort_by(|a, b| {
            (a.case[1], a.case[2], a.case[0], &a.nbt)
                .cmp(&(b.case[1], b.case[2], b.case[0], &b.nbt))
        });
        Contenu {
            taille: p.taille,
            palette,
            cases,
            entites,
        }
    }

    /// Combien de cases de MATIÈRE — tout sauf l'air.
    pub fn matiere(&self) -> usize {
        let air: Vec<bool> = self.palette.iter().map(|c| est_air(c)).collect();
        self.cases.iter().filter(|&&i| !air[i as usize]).count()
    }

    /// L'extrait À ESTAMPER : l'air y devient `transparent`, que le collage
    /// saute.
    fn a_estamper(&self, interner: &mut Interner, transparent: StateId) -> Presse {
        let ids: Vec<StateId> = self
            .palette
            .iter()
            .map(|c| {
                if est_air(c) {
                    transparent
                } else {
                    interner.intern(c)
                }
            })
            .collect();
        Presse {
            taille: self.taille,
            blocs: self.cases.iter().map(|&i| ids[i as usize]).collect(),
            ancre: [0, 0, 0],
            entites: self.entites.clone(),
            mobiles: Vec::new(),
        }
    }
}

impl Instance {
    /// La boîte MONDE qu'elle occupe, selon SA définition.
    pub fn boite(&self, def: &Definition) -> BBox {
        let [sx, sy, sz] = match self.transfo {
            Some(t) => t.taille_apres(def.contenu.taille),
            None => def.contenu.taille,
        };
        BBox::new(
            self.coin,
            BlockPos::new(
                self.coin.x + sx as i32 - 1,
                self.coin.y + sy as i32 - 1,
                self.coin.z + sz as i32 - 1,
            ),
        )
    }
}

impl Projet {
    pub fn definition(&self, id: u64) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.id == id)
    }

    pub fn instance(&self, id: u64) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    /// Les instances d'une définition.
    pub fn instances_de(&self, definition: u64) -> impl Iterator<Item = &Instance> {
        self.instances
            .iter()
            .filter(move |i| i.definition == definition)
    }

    /// La boîte d'une instance.
    pub fn boite(&self, i: &Instance) -> Option<BBox> {
        self.definition(i.definition).map(|d| i.boite(d))
    }

    /// **L'instance qui occupe cette case** — la plus RÉCENTE si plusieurs
    /// se chevauchent, pour un choix qui ne dépende pas de l'ordre du
    /// document.
    pub fn instance_en(&self, dim: &Dimension, p: BlockPos) -> Option<&Instance> {
        self.instances
            .iter()
            .filter(|i| i.dim == *dim)
            .filter(|i| self.boite(i).is_some_and(|b| b.contains(p)))
            .max_by_key(|i| i.id)
    }

    /// Un identifiant neuf. Jamais réutilisé : c'est ce qui rend l'identité
    /// STABLE.
    fn nouvel_id(&mut self) -> u64 {
        self.prochain = self.prochain.max(1);
        let id = self.prochain;
        self.prochain += 1;
        id
    }

    /// **Lit le document du monde** tel que la copie de travail le montre.
    /// Rend aussi ses octets : c'est l'état d'AVANT que le correctif de
    /// journal gardera. Absent vaut vide.
    pub fn lire<S: RegionSource, O: RegionStore>(
        staging: &Staging<S, O>,
    ) -> Result<(Vec<u8>, Projet), ErreurComposant> {
        let octets = match staging.lire_fichier(FICHIER) {
            Ok(b) => b,
            Err(SourceError::NotFound) => Vec::new(),
            Err(e) => return Err(ErreurComposant::Edition(e.into())),
        };
        let p = Projet::decoder(&octets).map_err(ErreurComposant::Document)?;
        Ok((octets, p))
    }

    // ── le format ───────────────────────────────────────────────────────────

    /// **Encode le document.** Définitions et instances ENVELOPPÉES — chacune
    /// un blob — pour qu'une version future y ajoute des champs à la fin sans
    /// rendre les documents d'aujourd'hui illisibles.
    pub fn encoder(&self) -> Vec<u8> {
        if self.definitions.is_empty() && self.instances.is_empty() && self.prochain == 0 {
            return Vec::new();
        }
        let mut w = W(Vec::new());
        w.octets(MAGIE).u16(VERSION).u64(self.prochain);
        w.u32(self.definitions.len() as u32);
        for d in &self.definitions {
            let mut c = W(Vec::new());
            c.u64(d.id).texte(&d.nom);
            let t = &d.contenu;
            c.u32(t.taille[0]).u32(t.taille[1]).u32(t.taille[2]);
            c.u32(t.palette.len() as u32);
            for e in &t.palette {
                c.texte(e);
            }
            // Deux octets par case tant que la palette le permet : c'est le
            // cas de tout composant raisonnable, et ça divise le document par
            // deux.
            let large = t.palette.len() > u16::MAX as usize;
            c.u8(if large { 4 } else { 2 });
            let mut cases = Vec::with_capacity(t.cases.len() * if large { 4 } else { 2 });
            for &i in &t.cases {
                if large {
                    cases.extend_from_slice(&i.to_le_bytes());
                } else {
                    cases.extend_from_slice(&(i as u16).to_le_bytes());
                }
            }
            c.blob(&cases);
            c.u32(t.entites.len() as u32);
            for e in &t.entites {
                c.i32(e.case[0]).i32(e.case[1]).i32(e.case[2]);
                for &k in &e.champs {
                    c.u64(k as u64);
                }
                c.blob(&e.nbt);
            }
            w.blob(&c.0);
        }
        w.u32(self.instances.len() as u32);
        for i in &self.instances {
            let mut c = W(Vec::new());
            c.u64(i.id)
                .u64(i.definition)
                .texte(&i.dim.id())
                .i32(i.coin.x)
                .i32(i.coin.y)
                .i32(i.coin.z)
                .u8(code_transfo(i.transfo));
            w.blob(&c.0);
        }
        w.0
    }

    /// **Décode un document.** Vide : aucun composant. Sinon, refuse AU
    /// MOINDRE DOUTE — et surtout ne rend jamais un document vide à la place
    /// d'un document illisible : le premier geste suivant l'écraserait, et
    /// les définitions de l'utilisateur partiraient avec.
    pub fn decoder(octets: &[u8]) -> Result<Projet, String> {
        if octets.is_empty() {
            return Ok(Projet::default());
        }
        let tronque = || "document des composants tronqué ou abîmé".to_string();
        let mut r = R { b: octets, p: 0 };
        if r.octets(4) != Some(MAGIE.as_slice()) {
            return Err("ce fichier n'est pas un document de composants titiforge".into());
        }
        let version = r.u16().ok_or_else(tronque)?;
        if version > VERSION {
            return Err(format!(
                "document des composants d'une version plus récente ({version}) : \
                 il n'est ni lu, ni réécrit"
            ));
        }
        let prochain = r.u64().ok_or_else(tronque)?;
        let n = r.u32().ok_or_else(tronque)? as usize;
        let mut definitions = Vec::new();
        for _ in 0..n {
            let corps = r.blob().ok_or_else(tronque)?;
            definitions.push(lire_definition(corps).ok_or_else(tronque)?);
        }
        let n = r.u32().ok_or_else(tronque)? as usize;
        let mut instances = Vec::new();
        for _ in 0..n {
            let corps = r.blob().ok_or_else(tronque)?;
            instances.push(lire_instance(corps).ok_or_else(tronque)?);
        }
        if r.p != octets.len() {
            return Err(tronque());
        }
        let p = Projet {
            prochain,
            definitions,
            instances,
        };
        // Un identifiant déjà donné ne doit pas pouvoir revenir, ni figurer
        // deux fois : définitions et instances puisent au même compteur, et
        // « la 37 » doit désigner une seule chose.
        let mut ids: Vec<u64> = p
            .definitions
            .iter()
            .map(|d| d.id)
            .chain(p.instances.iter().map(|i| i.id))
            .collect();
        ids.sort_unstable();
        let max = ids.last().copied().unwrap_or(0);
        if (max >= p.prochain && max > 0) || ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(tronque());
        }
        Ok(p)
    }
}

fn lire_definition(corps: &[u8]) -> Option<Definition> {
    let mut r = R { b: corps, p: 0 };
    let id = r.u64()?;
    let nom = r.texte()?;
    let taille = [r.u32()?, r.u32()?, r.u32()?];
    let volume = (taille[0] as usize)
        .checked_mul(taille[1] as usize)?
        .checked_mul(taille[2] as usize)?;
    if volume == 0 || volume > MAX_CASES {
        return None;
    }
    let np = r.u32()? as usize;
    if np == 0 || np > volume {
        return None;
    }
    // La longueur annoncée vient du FICHIER : la réserver telle quelle ferait
    // tenter une allocation démesurée sur un document forgé. La boucle butera
    // de toute façon sur la fin des octets.
    let mut palette = Vec::with_capacity(np.min(1024));
    for _ in 0..np {
        palette.push(r.texte()?);
    }
    let largeur = r.u8()? as usize;
    if largeur != 2 && largeur != 4 {
        return None;
    }
    let brut = r.blob()?;
    if brut.len() != volume * largeur {
        return None;
    }
    let cases: Vec<u32> = brut
        .chunks_exact(largeur)
        .map(|c| {
            if largeur == 2 {
                u16::from_le_bytes([c[0], c[1]]) as u32
            } else {
                u32::from_le_bytes([c[0], c[1], c[2], c[3]])
            }
        })
        .collect();
    if cases.iter().any(|&i| i as usize >= np) {
        return None;
    }
    let ne = r.u32()? as usize;
    let mut entites = Vec::new();
    for _ in 0..ne {
        let case = [r.i32()?, r.i32()?, r.i32()?];
        let champs = [
            usize::try_from(r.u64()?).unwrap_or(usize::MAX),
            usize::try_from(r.u64()?).unwrap_or(usize::MAX),
            usize::try_from(r.u64()?).unwrap_or(usize::MAX),
        ];
        let nbt = r.blob()?.to_vec();
        entites.push(Entite { case, nbt, champs });
    }
    Some(Definition {
        id,
        nom,
        contenu: Contenu {
            taille,
            palette,
            cases,
            entites,
        },
    })
}

fn lire_instance(corps: &[u8]) -> Option<Instance> {
    let mut r = R { b: corps, p: 0 };
    Some(Instance {
        id: r.u64()?,
        definition: r.u64()?,
        dim: dimension_de(&r.texte()?),
        coin: BlockPos::new(r.i32()?, r.i32()?, r.i32()?),
        transfo: transfo_de(r.u8()?)?,
    })
}

/// La correspondance écrite à la main : le discriminant d'un `enum` n'est pas
/// un format de fichier.
fn code_transfo(t: Option<Transfo>) -> u8 {
    match t {
        None => 0,
        Some(Transfo::Rot90) => 1,
        Some(Transfo::Rot180) => 2,
        Some(Transfo::Rot270) => 3,
        Some(Transfo::MiroirX) => 4,
        Some(Transfo::MiroirZ) => 5,
    }
}

fn transfo_de(c: u8) -> Option<Option<Transfo>> {
    Some(match c {
        0 => None,
        1 => Some(Transfo::Rot90),
        2 => Some(Transfo::Rot180),
        3 => Some(Transfo::Rot270),
        4 => Some(Transfo::MiroirX),
        5 => Some(Transfo::MiroirZ),
        _ => return None,
    })
}

fn dimension_de(id: &str) -> Dimension {
    match id {
        "minecraft:overworld" => Dimension::Overworld,
        "minecraft:the_nether" => Dimension::Nether,
        "minecraft:the_end" => Dimension::End,
        autre => {
            let (ns, chemin) = autre.split_once(':').unwrap_or(("minecraft", autre));
            Dimension::Custom {
                namespace: ns.to_string(),
                path: chemin.to_string(),
            }
        }
    }
}

// ── les actions ─────────────────────────────────────────────────────────────

/// Pourquoi une action sur les composants n'a pas eu lieu.
#[derive(Debug)]
pub enum ErreurComposant {
    Edition(Erreur),
    /// Le document ne se lit pas. Rien n'est écrit par-dessus.
    Document(String),
    DefinitionInconnue(u64),
    InstanceInconnue(u64),
    /// La sélection ne porte aucun bloc : un composant d'air n'estamperait
    /// rien.
    Vide,
    /// Au-delà de [`MAX_CASES`], le document ne se RELIRAIT pas : refusé à la
    /// création, plutôt que d'écrire un document qui verrouillerait tous les
    /// composants du monde à la lecture suivante.
    TropGrand {
        cases: u64,
        max: usize,
    },
    /// Une écriture tomberait, au moins en partie, dans du terrain jamais
    /// généré — un collage n'y écrit rien, et le document mentirait.
    /// `instance` : celle qu'une mise à jour réestamperait ; `None` pour une
    /// pose.
    HorsTerrain {
        absents: usize,
        exemple: ChunkPos,
        instance: Option<u64>,
    },
    /// L'action a échoué en route, ET ce qu'elle avait déjà écrit n'a pas pu
    /// être défait. Le seul cas où la copie de travail garde une action à
    /// moitié faite — et il se DIT.
    AMoitie {
        cause: Box<ErreurComposant>,
        defaire: Erreur,
    },
}

impl From<Erreur> for ErreurComposant {
    fn from(e: Erreur) -> Self {
        ErreurComposant::Edition(e)
    }
}

impl std::fmt::Display for ErreurComposant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErreurComposant::Edition(e) => write!(f, "{e}"),
            ErreurComposant::Document(e) => write!(f, "{e} — rien n'a été écrit"),
            ErreurComposant::DefinitionInconnue(id) => {
                write!(f, "aucun composant n° {id} dans ce monde")
            }
            ErreurComposant::InstanceInconnue(id) => {
                write!(f, "aucune instance n° {id} dans ce monde")
            }
            ErreurComposant::Vide => write!(
                f,
                "la sélection ne porte aucun bloc : un composant d'air n'estamperait rien"
            ),
            ErreurComposant::TropGrand { cases, max } => write!(
                f,
                "sélection de {cases} cases : un composant en compte au plus {max}. \
                 Rien n'a été écrit — en faire plusieurs plus petits"
            ),
            ErreurComposant::HorsTerrain {
                absents,
                exemple,
                instance: None,
            } => write!(
                f,
                "l'instance tomberait dans {absents} chunk(s) jamais générés (dont le \
                 chunk {}, {}) : un collage n'y écrit rien. Rien n'a été écrit — générer \
                 le terrain en jeu, ou poser ailleurs",
                exemple.x, exemple.z
            ),
            ErreurComposant::HorsTerrain {
                absents,
                exemple,
                instance: Some(id),
            } => write!(
                f,
                "sous l'instance n° {id}, la mise à jour écrirait dans {absents} chunk(s) \
                 jamais générés (dont le chunk {}, {}) : un collage n'y écrit rien. Rien \
                 n'a été écrit — générer le terrain en jeu, ou détacher cette instance",
                exemple.x, exemple.z
            ),
            ErreurComposant::AMoitie { cause, defaire } => write!(
                f,
                "{cause} — et ce qui avait déjà été écrit n'a pas pu être défait \
                 ({defaire}) : la copie de travail garde une partie de l'action, sans \
                 entrée d'annulation — la vérifier avant d'écrire dans la save"
            ),
        }
    }
}

impl std::error::Error for ErreurComposant {}

/// Ce qu'une action a fait, en DONNÉES : le moteur ne parle pas à
/// l'utilisateur.
#[derive(Debug, Default)]
pub struct Action {
    /// Les correctifs des chunks.
    pub rapport: RapportRegion,
    /// Le document d'après.
    pub projet: Projet,
    pub definition: u64,
    pub instance: Option<u64>,
    /// Les instances réestampées par une mise à jour.
    pub reestampees: usize,
    /// Parmi elles, celles d'une AUTRE dimension que l'instance modifiée :
    /// réestampées aussi, mais invisibles d'ici — le compte rendu le dit.
    pub ailleurs: usize,
    /// Les cases qui entrent dans le composant, et celles qui en sortent —
    /// voir l'en-tête : un mur autour d'une instance y entre sans le dire.
    pub gagnees: usize,
    pub perdues: usize,
    /// Les entités (cadres, porte-armures…) laissées hors du composant.
    pub entites_laissees: usize,
    /// Les états DISTINCTS qu'une rotation n'a pas su réécrire, laissés tels
    /// quels.
    pub intacts: usize,
}

/// L'extrait d'une définition, orienté comme une instance le porte.
fn estampe(
    contenu: &Contenu,
    transfo: Option<Transfo>,
    interner: &mut Interner,
    transparent: StateId,
    regle: Option<Regle>,
) -> (Presse, Vec<StateId>) {
    let p = contenu.a_estamper(interner, transparent);
    match transfo {
        None => (p, Vec::new()),
        Some(t) => {
            let r = p.transformer(t, interner, &|cle, t| regle.and_then(|f| f(cle, t)));
            // La case transparente n'est pas un état du monde : elle ne se
            // tourne pas, et ne se signale pas.
            let intacts = r
                .intacts
                .into_iter()
                .filter(|id| *id != transparent)
                .collect();
            (r.presse, intacts)
        }
    }
}

/// **Crée un composant depuis une sélection**, et en fait la première
/// instance, sur place. Le monde n'est pas touché : c'est le document qui
/// change.
pub fn creer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    sel: &BBox,
    nom: &str,
    projet: &Projet,
    interner: &mut Interner,
) -> Result<Action, ErreurComposant> {
    // Avant de copier : une sélection démesurée coûterait sa copie pour rien.
    let (sx, sy, sz) = sel.size();
    let cases = sx as u64 * sy as u64 * sz as u64;
    if cases > MAX_CASES as u64 {
        return Err(ErreurComposant::TropGrand {
            cases,
            max: MAX_CASES,
        });
    }
    let p = copier(staging, dim, Folder::Region, sel, interner)?;
    // Les entités ne font pas partie d'un composant : un cadre ou une bête
    // posés vingt fois seraient vingt entités, et ce n'est pas ce qu'on
    // demande à une fenêtre. Elles sont COMPTÉES, pour que ça se dise.
    let entites_laissees = p.mobiles.len();
    let contenu = Contenu::depuis_presse(&p, interner);
    if contenu.matiere() == 0 {
        return Err(ErreurComposant::Vide);
    }
    let mut projet = projet.clone();
    let definition = projet.nouvel_id();
    let instance = projet.nouvel_id();
    projet.definitions.push(Definition {
        id: definition,
        nom: nom.trim().to_string(),
        contenu,
    });
    projet.instances.push(Instance {
        id: instance,
        definition,
        dim: dim.clone(),
        coin: sel.min,
        transfo: None,
    });
    Ok(Action {
        projet,
        definition,
        instance: Some(instance),
        entites_laissees,
        ..Default::default()
    })
}

/// **Pose une instance** : la définition estampée à `coin`, tournée ou
/// réfléchie. Refusée AVANT toute écriture si elle tombe dans du terrain
/// jamais généré — sinon le document dirait une instance que le monde ne
/// porte qu'à moitié.
#[allow(clippy::too_many_arguments)]
pub fn poser<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    projet: &Projet,
    definition: u64,
    coin: BlockPos,
    transfo: Option<Transfo>,
    interner: &mut Interner,
    regle: Option<Regle>,
) -> Result<Action, ErreurComposant> {
    let def = projet
        .definition(definition)
        .ok_or(ErreurComposant::DefinitionInconnue(definition))?;
    let transparent = interner.intern(TRANSPARENT);
    let (presse, intacts) = estampe(&def.contenu, transfo, interner, transparent, regle);
    garder_du_terrain(staging, dim, &presse, coin, transparent, None)?;
    let rapport = coller(
        staging,
        dim,
        Folder::Region,
        &presse,
        coin,
        Pas {
            d: [0, 0, 0],
            avec_air: false,
            air: transparent,
            compter: false,
        },
        interner,
    )?;
    let mut projet = projet.clone();
    let instance = projet.nouvel_id();
    projet.instances.push(Instance {
        id: instance,
        definition,
        dim: dim.clone(),
        coin,
        transfo,
    });
    Ok(Action {
        rapport,
        projet,
        definition,
        instance: Some(instance),
        intacts: intacts.len(),
        ..Default::default()
    })
}

fn garder_du_terrain<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    presse: &Presse,
    coin: BlockPos,
    transparent: StateId,
    instance: Option<u64>,
) -> Result<(), ErreurComposant> {
    let c = Collage {
        presse,
        coin,
        avec_air: false,
        air: transparent,
        compter: false,
    };
    let absents = chunks_absents(staging, dim, Folder::Region, &c.chunks_ecrits())?;
    match absents.first() {
        Some(&exemple) => Err(ErreurComposant::HorsTerrain {
            absents: absents.len(),
            exemple,
            instance,
        }),
        None => Ok(()),
    }
}

/// **Met à jour une définition depuis une de ses instances**, et réestampe
/// TOUTES les autres — dans toutes les dimensions, en une seule action, donc
/// un seul Ctrl+Z.
///
/// La nouvelle définition est ce que la boîte de l'instance contient, ramené
/// dans le repère de la définition (la transformation inverse). Chaque autre
/// instance ne reçoit que ce qui CHANGE entre l'ancienne et la nouvelle —
/// voir l'en-tête.
pub fn mettre_a_jour<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    projet: &Projet,
    depuis: u64,
    interner: &mut Interner,
    regle: Option<Regle>,
) -> Result<Action, ErreurComposant> {
    let inst = projet
        .instance(depuis)
        .ok_or(ErreurComposant::InstanceInconnue(depuis))?
        .clone();
    let def = projet
        .definition(inst.definition)
        .ok_or(ErreurComposant::DefinitionInconnue(inst.definition))?
        .clone();

    // 1. Ce que l'instance porte AUJOURD'HUI, ramené dans le repère de la
    //    définition.
    let mut vu = copier(
        staging,
        &inst.dim,
        Folder::Region,
        &inst.boite(&def),
        interner,
    )?;
    let entites_laissees = vu.mobiles.len();
    vu.mobiles.clear();
    // Les états qu'une rotation n'a pas su réécrire, DISTINCTS : le même
    // escalier inconnu, retrouvé dans trois orientations et au retour dans le
    // repère de la définition, est UN état laissé tel quel, pas quatre.
    let mut intacts: HashSet<StateId> = HashSet::new();
    let canon = match inst.transfo {
        None => vu,
        Some(t) => {
            let r = vu.transformer(t.inverse(), interner, &|cle, t| {
                regle.and_then(|f| f(cle, t))
            });
            intacts.extend(r.intacts);
            r.presse
        }
    };
    let nouveau = Contenu::depuis_presse(&canon, interner);
    let mut action = Action {
        projet: projet.clone(),
        definition: def.id,
        instance: Some(inst.id),
        entites_laissees,
        ..Default::default()
    };
    if nouveau == def.contenu {
        return Ok(action);
    }
    let matiere = |c: &Contenu| -> Vec<bool> {
        let air: Vec<bool> = c.palette.iter().map(|k| est_air(k)).collect();
        c.cases.iter().map(|&i| !air[i as usize]).collect()
    };
    let (avant, apres) = (matiere(&def.contenu), matiere(&nouveau));
    action.gagnees = avant
        .iter()
        .zip(&apres)
        .filter(|(a, b)| !**a && **b)
        .count();
    action.perdues = avant
        .iter()
        .zip(&apres)
        .filter(|(a, b)| **a && !**b)
        .count();

    // 2. Ce que chaque AUTRE instance recevra : la différence, dans SON
    //    orientation — calculée une fois par orientation, pas par instance —
    //    rognée à ce qui s'écrit.
    let transparent = interner.intern(TRANSPARENT);
    let air = interner.intern("minecraft:air");
    let mut orientees: HashMap<Option<Transfo>, Option<(Presse, [i32; 3])>> = HashMap::new();
    let mut a_ecrire: Vec<(&Instance, BlockPos)> = Vec::new();
    for j in projet.instances_de(def.id).filter(|j| j.id != inst.id) {
        let d = match orientees.entry(j.transfo) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                // L'ancienne passe par la MÊME règle que la nouvelle : comparée
                // non tournée, chaque escalier paraîtrait changé.
                let (ancien, _) = estampe(&def.contenu, j.transfo, interner, transparent, regle);
                let (neuf, n) = estampe(&nouveau, j.transfo, interner, transparent, regle);
                intacts.extend(n);
                v.insert(rogner(
                    difference(&ancien, neuf, transparent, air),
                    transparent,
                ))
            }
        };
        if let Some((mix, decalage)) = d {
            let coin = BlockPos::new(
                j.coin.x + decalage[0],
                j.coin.y + decalage[1],
                j.coin.z + decalage[2],
            );
            // Le terrain, sous TOUTES les instances, avant la première
            // écriture.
            garder_du_terrain(staging, &j.dim, mix, coin, transparent, Some(j.id))?;
            a_ecrire.push((j, coin));
        }
    }

    // 3. Les écritures. Une qui échoue défait les précédentes.
    for (j, coin) in a_ecrire {
        let Some((mix, _)) = &orientees[&j.transfo] else {
            continue;
        };
        let pas = Pas {
            d: [0, 0, 0],
            avec_air: false,
            air: transparent,
            compter: false,
        };
        match coller(staging, &j.dim, Folder::Region, mix, coin, pas, interner) {
            Ok(r) => action.rapport.absorber(r),
            Err(e) => return Err(abandonner(staging, &action.rapport, e.into())),
        }
        action.reestampees += 1;
        if j.dim != inst.dim {
            action.ailleurs += 1;
        }
    }

    // 4. Le document : la définition prend son nouveau contenu.
    if let Some(d) = action
        .projet
        .definitions
        .iter_mut()
        .find(|d| d.id == def.id)
    {
        d.contenu = nouveau;
    }
    action.intacts = intacts.len();
    Ok(action)
}

/// Ce qu'une mise à jour écrit dans une instance : ce qui CHANGE entre
/// l'ancienne définition et la nouvelle — un état, ou les octets d'une block
/// entity. Ce qui était matière et ne l'est plus devient de l'AIR ; le reste
/// est transparent, et le collage le saute.
fn difference(ancien: &Presse, neuf: Presse, transparent: StateId, air: StateId) -> Presse {
    debug_assert_eq!(ancien.taille, neuf.taille);
    let index = |c: [i32; 3]| -> Option<usize> {
        if c.iter().any(|v| *v < 0) {
            return None;
        }
        neuf.index(c[0] as u32, c[1] as u32, c[2] as u32)
    };
    // Les cases dont la block entity change — présente d'un côté seulement,
    // ou d'autres octets : un panneau dont seul le texte change est un
    // changement, au même titre qu'un bloc.
    let (ea, en) = (par_case(ancien), par_case(&neuf));
    let mut touchees = vec![false; neuf.blocs.len()];
    for c in ea.keys().chain(en.keys()) {
        if ea.get(c) != en.get(c) {
            if let Some(i) = index(*c) {
                touchees[i] = true;
            }
        }
    }
    let mut blocs = neuf.blocs.clone();
    for (i, (n, a)) in blocs.iter_mut().zip(&ancien.blocs).enumerate() {
        if *n == *a && !touchees[i] {
            *n = transparent;
        } else if *n == transparent && *a != transparent {
            *n = air;
        }
    }
    // Seules voyagent les block entities d'une case qui reçoit de la matière.
    let entites = neuf
        .entites
        .iter()
        .filter(|e| index(e.case).is_some_and(|i| blocs[i] != transparent && blocs[i] != air))
        .cloned()
        .collect();
    Presse {
        taille: neuf.taille,
        blocs,
        ancre: [0, 0, 0],
        entites,
        mobiles: Vec::new(),
    }
}

/// Les octets des block entities d'un extrait, par case.
fn par_case(p: &Presse) -> HashMap<[i32; 3], &[u8]> {
    p.entites
        .iter()
        .map(|e| (e.case, e.nbt.as_slice()))
        .collect()
}

/// **La plus petite boîte qui contient ce qui s'écrit**, et son décalage
/// dans l'extrait. Une mise à jour qui change trois blocs ne relit pas la
/// maison entière sous chacune des vingt instances. `None` : rien à écrire.
fn rogner(p: Presse, transparent: StateId) -> Option<(Presse, [i32; 3])> {
    let [sx, sy, sz] = p.taille;
    let mut min = [u32::MAX; 3];
    let mut max = [0u32; 3];
    for y in 0..sy {
        for z in 0..sz {
            for x in 0..sx {
                if p.get(x, y, z) != Some(transparent) {
                    for (k, v) in [x, y, z].into_iter().enumerate() {
                        min[k] = min[k].min(v);
                        max[k] = max[k].max(v);
                    }
                }
            }
        }
    }
    if min[0] == u32::MAX {
        return None;
    }
    let taille = [
        max[0] - min[0] + 1,
        max[1] - min[1] + 1,
        max[2] - min[2] + 1,
    ];
    let decalage = [min[0] as i32, min[1] as i32, min[2] as i32];
    let mut out = Presse::uniforme(taille, transparent);
    for y in 0..taille[1] {
        for z in 0..taille[2] {
            for x in 0..taille[0] {
                if let (Some(i), Some(id)) = (
                    out.index(x, y, z),
                    p.get(x + min[0], y + min[1], z + min[2]),
                ) {
                    out.blocs[i] = id;
                }
            }
        }
    }
    out.entites = p
        .entites
        .into_iter()
        .map(|e| Entite {
            case: [
                e.case[0] - decalage[0],
                e.case[1] - decalage[1],
                e.case[2] - decalage[2],
            ],
            ..e
        })
        .collect();
    Some((out, decalage))
}

/// L'action a échoué : ce qu'elle a déjà écrit est DÉFAIT avant que l'erreur
/// ne remonte. Si même ça échoue, l'erreur le dit.
fn abandonner<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    rapport: &RapportRegion,
    cause: ErreurComposant,
) -> ErreurComposant {
    if rapport.patches.is_empty() {
        return cause;
    }
    match defaire_rapport(staging, rapport) {
        Ok(_) => cause,
        Err(defaire) => ErreurComposant::AMoitie {
            cause: Box::new(cause),
            defaire,
        },
    }
}

/// **Détache une instance** : ses blocs restent dans le monde, mais elle ne
/// suit plus sa définition. Son identifiant n'est jamais redonné.
pub fn detacher(projet: &Projet, instance: u64) -> Result<Action, ErreurComposant> {
    let i = projet
        .instance(instance)
        .ok_or(ErreurComposant::InstanceInconnue(instance))?;
    let mut p = projet.clone();
    let definition = i.definition;
    p.instances.retain(|x| x.id != instance);
    Ok(Action {
        projet: p,
        definition,
        instance: Some(instance),
        ..Default::default()
    })
}

/// **Renomme une définition.** Le nom est une étiquette : l'identité est le
/// numéro.
pub fn renommer(projet: &Projet, definition: u64, nom: &str) -> Result<Action, ErreurComposant> {
    let mut p = projet.clone();
    let d = p
        .definitions
        .iter_mut()
        .find(|d| d.id == definition)
        .ok_or(ErreurComposant::DefinitionInconnue(definition))?;
    d.nom = nom.trim().to_string();
    Ok(Action {
        projet: p,
        definition,
        ..Default::default()
    })
}

/// **La jonction : lire le document, faire l'action, l'enregistrer** — en
/// une fois, et UNE entrée de journal : les correctifs des chunks, puis celui
/// du document. C'est ce qui fait qu'un Ctrl+Z défait la mise à jour d'une
/// définition ET les vingt réestampages qu'elle a faits.
///
/// Trois étapes laissées à l'appelant seraient trois occasions d'oublier la
/// dernière : des blocs écrits sans entrée de journal, qu'aucun Ctrl+Z
/// n'atteint — ou un document d'AVANT relu au mauvais moment, et un
/// correctif qui ne s'applique plus.
///
/// Rend l'action et les enregistrements à ranger dans la séance ; `None`
/// quand rien n'a changé — une entrée qui ne défait rien serait un Ctrl+Z
/// pour rien.
pub fn faire<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    journal: &mut Journal,
    label: &str,
    horodatage: i64,
    action: impl FnOnce(&Projet) -> Result<Action, ErreurComposant>,
) -> Result<(Action, Option<Vec<Record>>), ErreurComposant> {
    let (avant, projet) = Projet::lire(staging)?;
    let a = action(&projet)?;
    let records = enregistrer(staging, &a, &avant, journal, label, horodatage)?;
    Ok((a, records))
}

/// Le document d'après est écrit dans la copie de travail ici — les chunks
/// l'ont déjà été par l'action. S'il ne s'écrit pas, les chunks sont DÉFAITS :
/// une instance posée que le document ne connaîtrait pas ne suivrait plus
/// jamais sa définition.
fn enregistrer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    action: &Action,
    avant: &[u8],
    journal: &mut Journal,
    label: &str,
    horodatage: i64,
) -> Result<Option<Vec<Record>>, ErreurComposant> {
    let apres = action.projet.encoder();
    let patch = FichierPatch::entre(FICHIER, avant, &apres);
    if patch.is_some() {
        if let Err(e) = staging.ecrire_fichier(FICHIER, &apres) {
            return Err(abandonner(
                staging,
                &action.rapport,
                ErreurComposant::Edition(e.into()),
            ));
        }
    }
    let mut genre = action.rapport.genre("composant", Vec::new());
    let vide = match &mut genre {
        Genre::Operation { corrections, .. } => {
            if let Some(p) = patch {
                corrections.push(Correction::Fichier(p));
            }
            corrections.is_empty()
        }
        Genre::Reprise => true,
    };
    if vide {
        return Ok(None);
    }
    Ok(Some(journal.pousser(label, horodatage, genre)))
}

// ── le codec ────────────────────────────────────────────────────────────────

struct W(Vec<u8>);

impl W {
    fn octets(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.octets(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.octets(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.octets(&v.to_le_bytes())
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.octets(&v.to_le_bytes())
    }
    fn texte(&mut self, s: &str) -> &mut Self {
        self.blob(s.as_bytes())
    }
    fn blob(&mut self, b: &[u8]) -> &mut Self {
        self.u32(b.len() as u32).octets(b)
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    /// Des octets, ou `None` s'il n'y en a pas assez — vérifié AVANT de
    /// prendre, jamais après.
    fn octets(&mut self, n: usize) -> Option<&'a [u8]> {
        let fin = self.p.checked_add(n)?;
        let s = self.b.get(self.p..fin)?;
        self.p = fin;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.octets(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.octets(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.octets(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.octets(8)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.octets(4)?.try_into().ok()?))
    }
    fn blob(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.octets(n)
    }
    fn texte(&mut self) -> Option<String> {
        String::from_utf8(self.blob()?.to_vec()).ok()
    }
}
