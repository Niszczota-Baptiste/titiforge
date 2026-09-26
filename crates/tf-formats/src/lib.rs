//! Les formats d'échange — `.schem` (Sponge v1 à v3), `.litematic`, et le
//! `.nbt` des blocs de structure — entre un fichier et le presse-papiers du
//! moteur (`tf_ops::Presse`).
//!
//! C'est ce qui manque pour échanger un build avec quelqu'un qui n'a pas
//! titiforge : le coller avec WorldEdit sur un serveur, le suivre avec
//! Litematica pour le bâtir à la main en survie, le charger dans un bloc de
//! structure ou un datapack — et, dans l'autre sens, poser ici un build
//! téléchargé.
//!
//! ## Trois règles
//!
//! 1. **Un format se reconnaît à ses OCTETS**, jamais à son extension :
//!    `r.0.0 (16).mca` a déjà appris au dépôt qu'un nom de fichier ment.
//! 2. **Rien de ce qui n'est pas compris n'est ré-encodé.** Le contenu d'un
//!    coffre, l'inventaire d'un porte-armure voyagent par leurs octets ; on ne
//!    réécrit que les champs qui les SITUENT. Un `minefield:*` reste un
//!    `minefield:*` (invariant n° 3) : ici, aucun nom de bloc n'est remappé.
//! 3. **Ce qui ne passe pas est NOMMÉ** (`Remarque`), jamais tu : des régions
//!    fusionnées, des biomes laissés, une entité de mod qu'on ne sait pas
//!    raccrocher.
//!
//! ## Le repère
//!
//! Le presse-papiers est en coordonnées LOCALES — son coin de plus petites
//! coordonnées à l'origine — et son `ancre` est le point qu'un collage remet
//! sous le joueur. Chaque format a son idée de l'origine ; la lecture les y
//! ramène toutes, et l'écriture choisit le repère où les deux lecteurs d'un
//! même format tombent d'accord (voir `sponge.rs`).

#![forbid(unsafe_code)]

mod commun;
mod entites;
mod litematic;
mod sponge;
mod structure;

use std::collections::BTreeSet;
use std::fmt;

use tf_anvil::{Interner, StateId};
use tf_ops::edition::{MAX_OCTETS_MATERIALISES, OCTETS_COPIE};
use tf_ops::Presse;

use commun::Compound;
pub use commun::MAX_OCTETS_NBT;

/// Un format qu'on sait ÉCRIRE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// `.schem` version 2 : WorldEdit 7.2 (celui de Minecraft 1.18) — et le
    /// seul `.schem` que Litematica sache ouvrir.
    SpongeV2,
    /// `.schem` version 3 : WorldEdit 7.3 et suivants.
    SpongeV3,
    /// `.litematic` : Litematica, pour bâtir en survie devant un fantôme.
    Litematic,
    /// `.nbt` : un bloc de structure, `/place template`, un datapack.
    Structure,
}

impl Format {
    pub const TOUS: [Format; 4] = [
        Format::Litematic,
        Format::SpongeV2,
        Format::SpongeV3,
        Format::Structure,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            Format::SpongeV2 | Format::SpongeV3 => "schem",
            Format::Litematic => "litematic",
            Format::Structure => "nbt",
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Format::SpongeV2 => ".schem (Sponge v2)",
            Format::SpongeV3 => ".schem (Sponge v3)",
            Format::Litematic => ".litematic",
            Format::Structure => ".nbt de structure",
        }
    }

    /// Qui le lit — ce qu'il faut savoir pour choisir.
    pub fn pour(self) -> &'static str {
        match self {
            Format::SpongeV2 => "WorldEdit 7.2 (Minecraft 1.18) et Litematica",
            Format::SpongeV3 => "WorldEdit 7.3 et suivants — pas Litematica 1.18",
            Format::Litematic => "Litematica : bâtir en survie devant le fantôme du build",
            Format::Structure => "un bloc de structure, /place template, un datapack",
        }
    }
}

/// Ce qu'on a LU, exactement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lecture {
    Sponge { version: u8 },
    Litematic { version: i32, regions: usize },
    Structure,
}

impl fmt::Display for Lecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lecture::Sponge { version } => write!(f, ".schem (Sponge v{version})"),
            Lecture::Litematic { version, regions } => {
                write!(f, ".litematic (v{version}, {regions} région(s))")
            }
            Lecture::Structure => write!(f, ".nbt de structure"),
        }
    }
}

/// Ce qu'une écriture porte en plus des blocs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// Le `DataVersion` du monde d'où vient l'extrait : la FORME des octets
    /// de ses block entities et de ses entités. Minecraft 1.18.2 : 2975.
    pub data_version: i32,
    pub nom: String,
    pub auteur: String,
    pub description: String,
    /// Millisecondes depuis 1970 — passées, jamais lues à l'horloge : la même
    /// écriture rend les mêmes octets.
    pub date_ms: i64,
}

/// Un fichier lu.
#[derive(Debug, Clone, PartialEq)]
pub struct Lu {
    pub presse: Presse,
    pub lecture: Lecture,
    /// Le `DataVersion` du fichier, s'il en porte un. Plus récent que le
    /// monde, ses blocs et ses objets peuvent ne pas y exister : c'est à
    /// l'hôte, qui connaît le monde, de le dire.
    pub data_version: Option<i32>,
    pub nom: Option<String>,
    pub auteur: Option<String>,
    pub remarques: Vec<Remarque>,
}

/// Un fichier écrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ecrit {
    pub octets: Vec<u8>,
    pub remarques: Vec<Remarque>,
}

/// Ce qu'une lecture ou une écriture n'a pas su porter exactement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remarque {
    /// Plusieurs régions Litematica, fusionnées en une seule boîte.
    RegionsFusionnees { regions: usize, recouvertes: u64 },
    /// Des cases qu'une structure ne décrit pas (`structure_void`) : de l'air,
    /// qu'un collage sans l'air laisse tel quel.
    CasesVides(u64),
    /// Une structure à VARIANTES (une épave) : seule la première est lue.
    VariantesIgnorees(usize),
    /// Les biomes du fichier : le presse-papiers n'en porte pas.
    BiomesIgnores,
    /// Des ticks programmés (Litematica) : le presse-papiers n'en porte pas.
    TicksIgnores(usize),
    /// Des block entities sans position, hors de la boîte, ou illisibles.
    BlockEntitiesIgnorees(usize),
    /// Plusieurs block entities sur une même case : la dernière est gardée.
    BlockEntitiesEnDouble(usize),
    /// Des block entities sans `id`, que ce format ne sait pas ranger.
    BlockEntitiesSansId(usize),
    /// Des entités illisibles, ou sans `id`.
    EntitesIgnorees(usize),
    /// Des cases dont des entités se souviennent (un lit, une ruche) qu'on ne
    /// sait pas situer : laissées telles quelles, comme dans une copie dont le
    /// lit est resté dehors.
    SouvenirsLaisses(usize),
    /// Des entités accrochées qu'on ne sait pas raccrocher — une entité de
    /// mod : sa case reste celle du fichier.
    AccrochesInconnues(Vec<String>),
    /// Des états de palette illisibles, remplacés par de l'air.
    EtatsIllisibles(Vec<String>),
    /// Ce format n'a pas d'ancre : un collage partira du coin de l'extrait.
    AncrePerdue,
}

impl fmt::Display for Remarque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remarque::RegionsFusionnees {
                regions,
                recouvertes,
            } => {
                write!(f, "{regions} régions fusionnées en une boîte")?;
                if *recouvertes > 0 {
                    write!(
                        f,
                        " — {recouvertes} case(s) en recouvrement, la dernière région l'emporte"
                    )?;
                }
                Ok(())
            }
            Remarque::CasesVides(n) => write!(
                f,
                "{n} case(s) que la structure ne décrit pas, lues comme de l'air"
            ),
            Remarque::VariantesIgnorees(n) => write!(
                f,
                "structure à {n} variantes : seule la première est lue"
            ),
            Remarque::BiomesIgnores => write!(f, "les biomes du fichier ne sont pas repris"),
            Remarque::TicksIgnores(n) => {
                write!(f, "{n} tick(s) programmé(s) non repris")
            }
            Remarque::BlockEntitiesIgnorees(n) => write!(
                f,
                "{n} block entit(é/ies) ignorée(s) : sans position, hors de la boîte ou illisible(s)"
            ),
            Remarque::BlockEntitiesEnDouble(n) => write!(
                f,
                "{n} block entit(é/ies) en double sur une case : la dernière est gardée"
            ),
            Remarque::BlockEntitiesSansId(n) => write!(
                f,
                "{n} block entit(é/ies) sans identifiant, que ce format ne sait pas ranger"
            ),
            Remarque::EntitesIgnorees(n) => {
                write!(f, "{n} entité(s) ignorée(s) : illisible(s) ou sans identifiant")
            }
            Remarque::SouvenirsLaisses(n) => write!(
                f,
                "{n} case(s) retenue(s) par des entités (lit, ruche…) laissée(s) telle(s) quelle(s)"
            ),
            Remarque::AccrochesInconnues(ids) => write!(
                f,
                "entités accrochées qu'on ne sait pas raccrocher : {}",
                ids.join(", ")
            ),
            Remarque::EtatsIllisibles(cles) => write!(
                f,
                "états illisibles, remplacés par de l'air : {}",
                cles.join(", ")
            ),
            Remarque::AncrePerdue => write!(
                f,
                "le format n'a pas d'ancre : un collage partira du coin de l'extrait"
            ),
        }
    }
}

/// Pourquoi un fichier ne se lit pas, ou un extrait ne s'écrit pas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erreur {
    /// Ni gzip ni NBT, ou un NBT tronqué.
    Illisible,
    /// Un NBT lisible, mais ni un `.schem`, ni un `.litematic`, ni un `.nbt`
    /// de structure.
    Inconnu,
    /// Un format d'AVANT 1.13, qui numérote ses blocs : il faudrait la table
    /// d'aplatissement du jeu pour le lire.
    Ancien(&'static str),
    Version {
        format: &'static str,
        version: i64,
    },
    /// Un champ obligatoire absent, ou du mauvais type.
    Manque(&'static str),
    /// Des données qui contredisent la taille, ou un indice hors palette.
    Incoherent(String),
    TropGros {
        octets: u64,
        plafond: u64,
    },
    /// Un extrait plus grand que ce que le format sait décrire.
    TropGrandPourLeFormat {
        format: &'static str,
        taille: [u32; 3],
        max: u32,
    },
    /// Un extrait de plus de cases que ce que le format sait raisonnablement
    /// décrire.
    TropDeCases {
        format: &'static str,
        cases: u64,
        max: u64,
    },
    /// Un état du presse-papiers que l'interner ne résout pas.
    EtatInconnu(StateId),
}

impl fmt::Display for Erreur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Erreur::Illisible => write!(f, "fichier illisible : ni gzip ni NBT, ou tronqué"),
            Erreur::Inconnu => write!(
                f,
                "ce NBT n'est ni un .schem, ni un .litematic, ni un .nbt de structure"
            ),
            Erreur::Ancien(quoi) => write!(
                f,
                "{quoi} : un format d'avant Minecraft 1.13, qui numérote ses blocs — \
                 réenregistre-le avec une version récente de l'outil qui l'a produit"
            ),
            Erreur::Version { format, version } => {
                write!(f, "{format} en version {version} : version non gérée")
            }
            Erreur::Manque(champ) => write!(f, "champ obligatoire absent ou mal formé : {champ}"),
            Erreur::Incoherent(quoi) => write!(f, "fichier incohérent : {quoi}"),
            Erreur::TropGros { octets, plafond } => write!(
                f,
                "extrait trop gros : {} Mo, au-delà du plafond de {} Mo",
                octets / 1_000_000,
                plafond / 1_000_000
            ),
            Erreur::TropGrandPourLeFormat {
                format,
                taille,
                max,
            } => write!(
                f,
                "{} × {} × {} : {format} ne décrit pas plus de {max} blocs par côté",
                taille[0], taille[1], taille[2]
            ),
            Erreur::TropDeCases { format, cases, max } => write!(
                f,
                "{cases} cases : un {format} n'en décrit pas plus de {max} — \
                 préfère .schem ou .litematic pour un build entier"
            ),
            Erreur::EtatInconnu(id) => write!(f, "état n° {id} inconnu de l'interner"),
        }
    }
}

impl std::error::Error for Erreur {}

/// Lit un fichier d'échange, quel qu'il soit — le format se reconnaît à ses
/// octets.
pub fn lire(octets: &[u8], interner: &mut Interner) -> Result<Lu, Erreur> {
    let nbt = commun::decompresser(octets)?;
    let (_, racine) = Compound::racine(&nbt)?;
    if racine.champ("Regions").is_some() && racine.entier("Version").is_some() {
        return litematic::lire(&racine, interner);
    }
    // Sponge v3 : une racine anonyme qui porte un compound `Schematic`.
    if let Some(s) = racine.compound("Schematic")? {
        return sponge::lire(&s, interner);
    }
    if racine.entier("Version").is_some()
        && (racine.champ("Palette").is_some() || racine.champ("Blocks").is_some())
        && racine.champ("Width").is_some()
    {
        return sponge::lire(&racine, interner);
    }
    if racine.chaine("Materials").is_some()
        || (racine.octets("Blocks").is_some() && racine.octets("Data").is_some())
    {
        return Err(Erreur::Ancien("un .schematic de MCEdit ou de Schematica"));
    }
    if racine.champ("size").is_some() && racine.champ("blocks").is_some() {
        return structure::lire(&racine, interner);
    }
    Err(Erreur::Inconnu)
}

/// Écrit un extrait dans un format.
pub fn ecrire(
    presse: &Presse,
    format: Format,
    interner: &Interner,
    meta: &Meta,
) -> Result<Ecrit, Erreur> {
    if presse.blocs.len() != Presse::volume(presse.taille) || presse.blocs.is_empty() {
        return Err(Erreur::Incoherent(format!(
            "{} cases pour une boîte de {:?}",
            presse.blocs.len(),
            presse.taille
        )));
    }
    let mut bilan = Bilan::default();
    let nbt = match format {
        Format::SpongeV2 => sponge::ecrire(presse, 2, interner, meta, &mut bilan)?,
        Format::SpongeV3 => sponge::ecrire(presse, 3, interner, meta, &mut bilan)?,
        Format::Litematic => litematic::ecrire(presse, interner, meta, &mut bilan)?,
        Format::Structure => structure::ecrire(presse, interner, meta, &mut bilan)?,
    };
    Ok(Ecrit {
        octets: commun::compresser(&nbt),
        remarques: bilan.fin(),
    })
}

// ── partagé par les formats ─────────────────────────────────────────────────

/// Refuse une taille que le moteur ne saurait pas porter — AVANT d'allouer
/// quoi que ce soit.
///
/// C'est le plafond du presse-papiers, pas un autre : un fichier que la
/// lecture accepterait et que `//copy` refuserait serait un extrait que
/// l'application tient sans pouvoir le produire.
pub(crate) fn verifier_taille(taille: [i64; 3]) -> Result<[u32; 3], Erreur> {
    if taille.iter().any(|&t| t <= 0) {
        return Err(Erreur::Incoherent(format!("taille {taille:?}")));
    }
    let cases = taille.iter().fold(1u64, |a, &t| a.saturating_mul(t as u64));
    let octets = cases.saturating_mul(OCTETS_COPIE);
    if octets > MAX_OCTETS_MATERIALISES {
        return Err(Erreur::TropGros {
            octets,
            plafond: MAX_OCTETS_MATERIALISES,
        });
    }
    // Sous le plafond, chaque côté tient forcément dans un `u32`.
    Ok(taille.map(|t| t as u32))
}

/// La palette d'un extrait, dans l'ordre de première apparition (YZX) — celui
/// où WorldEdit numérote la sienne.
pub(crate) struct Palette {
    pub cles: Vec<String>,
    /// L'indice de chaque `StateId`, par son numéro : un tableau et pas une
    /// table de hachage, parce qu'on le consulte une fois par CASE.
    indices: Vec<u32>,
}

impl Palette {
    pub fn de(presse: &Presse, interner: &Interner, air_en_tete: bool) -> Result<Palette, Erreur> {
        const LIBRE: u32 = u32::MAX;
        let mut indices = vec![LIBRE; interner.len()];
        let mut cles = Vec::new();
        if air_en_tete {
            cles.push("minecraft:air".to_string());
            if let Some(a) = interner.get("minecraft:air") {
                indices[a as usize] = 0;
            }
        }
        for &id in &presse.blocs {
            let place = indices
                .get_mut(id as usize)
                .ok_or(Erreur::EtatInconnu(id))?;
            if *place == LIBRE {
                let cle = interner.resolve(id).ok_or(Erreur::EtatInconnu(id))?;
                *place = cles.len() as u32;
                cles.push(cle.to_string());
            }
        }
        Ok(Palette { cles, indices })
    }

    #[inline]
    pub fn indice(&self, id: StateId) -> u32 {
        self.indices[id as usize]
    }
}

/// Est-ce de l'air, au sens du jeu (`isAir`) ?
pub(crate) fn est_air(cle: &str) -> bool {
    matches!(
        cle,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Ce qu'une lecture ou une écriture accumule en route, rendu en remarques à
/// la fin — une par sorte, pas une par entité.
#[derive(Default)]
pub(crate) struct Bilan {
    pub remarques: Vec<Remarque>,
    pub be_ignorees: usize,
    pub be_doubles: usize,
    pub be_sans_id: usize,
    pub entites_ignorees: usize,
    pub souvenirs: usize,
    pub accroches: BTreeSet<String>,
    pub etats: BTreeSet<String>,
}

impl Bilan {
    /// Range une entité importée, et compte ce qu'elle n'a pas su porter.
    pub fn prendre(&mut self, i: entites::Importee) -> tf_anvil::mobiles::Mobile {
        self.souvenirs += i.souvenirs_laisses;
        if let Some(id) = i.accroche_inconnue {
            self.accroches.insert(id);
        }
        i.mobile
    }

    pub fn fin(mut self) -> Vec<Remarque> {
        let mut out = std::mem::take(&mut self.remarques);
        for (n, r) in [
            (
                self.be_ignorees,
                Remarque::BlockEntitiesIgnorees as fn(usize) -> Remarque,
            ),
            (self.be_doubles, Remarque::BlockEntitiesEnDouble),
            (self.be_sans_id, Remarque::BlockEntitiesSansId),
            (self.entites_ignorees, Remarque::EntitesIgnorees),
            (self.souvenirs, Remarque::SouvenirsLaisses),
        ] {
            if n > 0 {
                out.push(r(n));
            }
        }
        if !self.accroches.is_empty() {
            out.push(Remarque::AccrochesInconnues(
                self.accroches.into_iter().collect(),
            ));
        }
        if !self.etats.is_empty() {
            out.push(Remarque::EtatsIllisibles(self.etats.into_iter().collect()));
        }
        out
    }
}

/// Range les block entities lues : dans la boîte, une par case — la
/// dernière gagne, comme dans la table du jeu — et dans l'ordre YZX de
/// `copier`, pour qu'un extrait lu se compare à un extrait copié.
pub(crate) fn ranger_block_entities(
    presse: &mut Presse,
    lues: Vec<tf_anvil::entites::Entite>,
    bilan: &mut Bilan,
) {
    let mut par_case = std::collections::BTreeMap::new();
    for e in lues {
        let dedans = (0..3).all(|a| e.case[a] >= 0 && (e.case[a] as u32) < presse.taille[a]);
        if !dedans {
            bilan.be_ignorees += 1;
            continue;
        }
        let cle = (e.case[1], e.case[2], e.case[0]);
        if par_case.insert(cle, e).is_some() {
            bilan.be_doubles += 1;
        }
    }
    presse.entites = par_case.into_values().collect();
}
