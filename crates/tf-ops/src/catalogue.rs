//! **Ce que les opérations DISENT d'elles-mêmes.**
//!
//! Une opération n'est pas seulement du code qui écrit des blocs : c'est aussi
//! un nom, des paramètres, des bornes, et une phrase qui dit ce qu'elle fait.
//! Tant que ce savoir-là n'existe qu'en commentaire, chaque hôte le réécrit —
//! la ligne de commande a son `usage()`, la coque aurait ses formulaires, un
//! greffon aurait les siens, et les trois divergeraient. Ce dépôt a payé
//! **quatre fois** le piège des tables qui divergent ; c'en serait la
//! cinquième, et la plus chère, parce qu'elle grandirait à chaque opération
//! ajoutée.
//!
//! D'où ce module : le **catalogue**. Il est de la DONNÉE — c'est la règle
//! « le greffon décrit, le cœur exécute », appliquée au cœur lui-même. Il ne
//! connaît ni écran, ni ligne de commande, ni widget : un paramètre déclare ce
//! qu'il ATTEND (un bloc, un compte, une direction), jamais comment on le
//! saisit.
//!
//! ## Le descripteur est SUR LE CHEMIN, il n'est pas à côté
//!
//! `ExeWorldEdit` a payé la version faible : un normaliseur écrit, testé, et
//! qu'aucun hôte n'appelait — « Naturaliser → Personnalisé » plantait sur
//! `s.includes is not a function`, parce que l'inspecteur envoyait un objet là
//! où l'opération attendait une chaîne. Ici `construire` commence par chercher
//! le descripteur et normaliser : **il n'existe aucune façon de fabriquer un
//! travail sans passer par sa description.** Ce qui exige que la
//! normalisation soit IDEMPOTENTE, et un test l'exige pour chaque opération.
//!
//! ## Ce qu'il ne fait pas
//!
//! Il ne remplace pas `Plan` : il le construit. Les trois étages, les masques,
//! les motifs n'ont pas bougé d'une ligne — le catalogue est la couche qui les
//! rend NOMMABLES depuis l'extérieur.

use crate::masque::Masque;
use crate::motif::Motif;
use crate::plan::Plan;
use tf_blocks::Transfo;
use tf_world::selection::{Direction, DIRECTIONS};

/// Ce qu'un paramètre attend. **Ce qu'il VEUT DIRE, jamais comment on le
/// saisit** : un hôte en fait un champ de texte, une liste ou un sélecteur de
/// blocs, et le cœur n'en sait rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saisie {
    /// Un état de bloc : `minecraft:stone`, `minefield:chaise[facing=north]`.
    Bloc,
    /// Un nom de biome.
    Biome,
    /// Un mélange pondéré : `3:minecraft:stone, 1:minecraft:dirt`.
    Melange,
    /// Un compte, BORNÉ. Les bornes vivent ici et nulle part ailleurs :
    /// l'interface génère son champ depuis elles, et le normaliseur serre
    /// dessus. Deux sources pour la même borne divergeraient.
    Entier { min: i64, max: i64 },
    /// Trois entiers : un décalage en blocs.
    Vecteur,
    /// Une des six directions du repère Minecraft.
    Direction,
    /// Une rotation ou un miroir, ou rien.
    Transformation,
}

impl Saisie {
    /// Toutes les saisies, une par variante.
    ///
    /// **Elle sert à EXIGER qu'un hôte sache les saisir toutes.**
    /// `ExeWorldEdit` a livré trois types de paramètres déclarés sans champ
    /// pour les saisir : `blocklist`, `pattern` et `mask` retombaient sur la
    /// case de texte par défaut, et la chaîne partait telle quelle vers une
    /// opération qui attend un tableau. « Remplacer » et « Mélange » ne
    /// pouvaient pas fonctionner, sans la moindre erreur à l'écran.
    ///
    /// Les bornes des entiers sont ici celles du type : cette table sert à
    /// énumérer des GENRES, pas à décrire un paramètre.
    pub const TOUTES: [Saisie; 7] = [
        Saisie::Bloc,
        Saisie::Biome,
        Saisie::Melange,
        Saisie::Entier {
            min: i64::MIN,
            max: i64::MAX,
        },
        Saisie::Vecteur,
        Saisie::Direction,
        Saisie::Transformation,
    ];

    /// Un rang par variante. L'exhaustivité est portée par le `match` : une
    /// saisie ajoutée sans être mise dans `TOUTES` ne compile pas, et un test
    /// exige que les rangs couvrent exactement `0..TOUTES.len()`.
    pub const fn rang(self) -> usize {
        match self {
            Saisie::Bloc => 0,
            Saisie::Biome => 1,
            Saisie::Melange => 2,
            Saisie::Entier { .. } => 3,
            Saisie::Vecteur => 4,
            Saisie::Direction => 5,
            Saisie::Transformation => 6,
        }
    }
}

/// La valeur d'un paramètre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Valeur {
    Texte(String),
    Entier(i64),
    Vecteur([i32; 3]),
    Melange(Vec<(u32, String)>),
    Direction(Direction),
    /// `None` = aucune transformation.
    Transformation(Option<Transfo>),
}

impl Valeur {
    pub fn texte(s: &str) -> Valeur {
        Valeur::Texte(s.to_string())
    }

    /// La saisie à laquelle cette valeur répond. Sert au contrôle de type —
    /// une valeur du mauvais genre est REFUSÉE, jamais convertie en silence :
    /// une conversion muette est exactement ce qui a fait planter
    /// « Naturaliser → Personnalisé ».
    pub fn genre(&self) -> &'static str {
        match self {
            Valeur::Texte(_) => "texte",
            Valeur::Entier(_) => "entier",
            Valeur::Vecteur(_) => "vecteur",
            Valeur::Melange(_) => "mélange",
            Valeur::Direction(_) => "direction",
            Valeur::Transformation(_) => "transformation",
        }
    }

    fn accepte(&self, s: Saisie) -> bool {
        matches!(
            (self, s),
            (Valeur::Texte(_), Saisie::Bloc)
                | (Valeur::Texte(_), Saisie::Biome)
                | (Valeur::Melange(_), Saisie::Melange)
                | (Valeur::Entier(_), Saisie::Entier { .. })
                | (Valeur::Vecteur(_), Saisie::Vecteur)
                | (Valeur::Direction(_), Saisie::Direction)
                | (Valeur::Transformation(_), Saisie::Transformation)
        )
    }
}

/// Un paramètre d'opération.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    /// La clé. Stable : elle finira sérialisée dans un journal rejouable.
    pub nom: &'static str,
    /// Ce que l'écran affiche, en français.
    pub label: &'static str,
    pub saisie: Saisie,
    /// Ce que vaut le paramètre quand l'hôte ne le donne pas. `None` = il est
    /// obligatoire, et son absence est une erreur plutôt qu'une supposition.
    pub defaut: Option<Defaut>,
}

/// Le défaut d'un paramètre. Un `const` ne peut pas porter de `String` : on
/// garde la forme statique et `Valeur` naît à la lecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defaut {
    Texte(&'static str),
    Entier(i64),
    Vecteur([i32; 3]),
    Direction(Direction),
    /// Aucune transformation.
    SansTransformation,
    /// Un mélange vide — il n'y a pas de mélange par défaut qui ait un sens.
    MelangeVide,
}

impl Defaut {
    pub fn valeur(self) -> Valeur {
        match self {
            Defaut::Texte(s) => Valeur::texte(s),
            Defaut::Entier(n) => Valeur::Entier(n),
            Defaut::Vecteur(v) => Valeur::Vecteur(v),
            Defaut::Direction(d) => Valeur::Direction(d),
            Defaut::SansTransformation => Valeur::Transformation(None),
            Defaut::MelangeVide => Valeur::Melange(Vec::new()),
        }
    }
}

/// Ce qu'une opération lit autour d'elle, et ce qu'elle coûte à dire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cout {
    /// Accepte-t-elle une forme (`//sphere`, `//cyl`, `//walls`…) ?
    pub forme: bool,
    /// **Matérialise-t-elle toute la sélection en mémoire ?** `//hollow` le
    /// fait — c'est assumé, borné, et ça doit s'ANNONCER avant de commencer,
    /// pas se découvrir quand l'éditeur disparaît.
    pub materialise: bool,
    /// Lit-elle hors de sa section ? Une opération qui lit une colonne ne peut
    /// pas atteindre l'étage palette.
    pub colonne: bool,
}

/// Une opération, telle qu'un hôte peut la nommer et la remplir.
#[derive(Debug, Clone, Copy)]
pub struct Descripteur {
    /// L'identifiant stable. Il part dans le journal ; il ne change jamais.
    pub id: &'static str,
    /// Les noms WorldEdit, pour qu'on la trouve en tapant ce qu'on connaît.
    pub we: &'static [&'static str],
    pub label: &'static str,
    pub resume: &'static str,
    pub params: &'static [Param],
    pub cout: Cout,
}

impl Descripteur {
    pub fn param(&self, nom: &str) -> Option<&'static Param> {
        self.params.iter().find(|p| p.nom == nom)
    }

    /// Les paramètres au complet, aux valeurs par défaut.
    pub fn defauts(&self) -> Params {
        let mut p = Params::new();
        for d in self.params {
            if let Some(v) = d.defaut {
                p.poser(d.nom, v.valeur());
            }
        }
        p
    }
}

const PAS_DE_FORME: Cout = Cout {
    forme: false,
    materialise: false,
    colonne: false,
};
const AVEC_FORME: Cout = Cout {
    forme: true,
    materialise: false,
    colonne: false,
};

/// **Le catalogue.** Une opération qui n'y est pas est inatteignable ; une
/// opération qui y est doit se construire, et un test l'exige pour chacune.
pub const OPS: &[Descripteur] = &[
    Descripteur {
        id: "poser",
        we: &["//set"],
        label: "Remplir",
        resume: "Pose le même bloc partout dans la sélection.",
        params: &[Param {
            nom: "bloc",
            label: "Bloc",
            saisie: Saisie::Bloc,
            defaut: Some(Defaut::Texte("minecraft:stone")),
        }],
        cout: AVEC_FORME,
    },
    Descripteur {
        id: "remplacer",
        we: &["//replace"],
        label: "Remplacer",
        resume: "Ne touche que les cases qui portent déjà l'état visé.",
        params: &[
            Param {
                nom: "de",
                label: "Remplacer",
                saisie: Saisie::Bloc,
                defaut: None,
            },
            Param {
                nom: "vers",
                label: "Par",
                saisie: Saisie::Bloc,
                defaut: None,
            },
        ],
        cout: AVEC_FORME,
    },
    Descripteur {
        id: "melanger",
        we: &["//set", "//pattern"],
        label: "Mélange",
        resume: "Un tirage pondéré, haché sur la POSITION : rejouable, et \
                 indépendant de l'ordre de parcours.",
        params: &[Param {
            nom: "melange",
            label: "Mélange",
            saisie: Saisie::Melange,
            defaut: Some(Defaut::MelangeVide),
        }],
        cout: AVEC_FORME,
    },
    Descripteur {
        id: "copier-vers",
        we: &["//copy", "//paste"],
        label: "Copier vers",
        resume: "Copie la sélection, la transforme, la repose décalée. Les \
                 états de blocs sont réécrits ; les coffres suivent.",
        params: &[
            Param {
                nom: "decalage",
                label: "Décalage",
                saisie: Saisie::Vecteur,
                defaut: Some(Defaut::Vecteur([0, 0, 0])),
            },
            Param {
                nom: "transformation",
                label: "Transformation",
                saisie: Saisie::Transformation,
                defaut: Some(Defaut::SansTransformation),
            },
        ],
        cout: PAS_DE_FORME,
    },
    Descripteur {
        id: "deplacer",
        we: &["//move"],
        label: "Déplacer",
        resume: "Déplace le contenu ; la source est remplie. Une seule entrée \
                 de journal, donc un seul Ctrl+Z.",
        params: &[
            Param {
                nom: "decalage",
                label: "Décalage",
                saisie: Saisie::Vecteur,
                defaut: Some(Defaut::Vecteur([0, 0, 0])),
            },
            Param {
                nom: "remplir",
                label: "Laisser derrière",
                saisie: Saisie::Bloc,
                defaut: Some(Defaut::Texte("minecraft:air")),
            },
        ],
        cout: PAS_DE_FORME,
    },
    Descripteur {
        id: "empiler",
        we: &["//stack"],
        label: "Empiler",
        resume: "Répète la sélection, d'un pas égal à sa propre taille.",
        params: &[
            Param {
                nom: "fois",
                label: "Répétitions",
                saisie: Saisie::Entier { min: 1, max: 4096 },
                defaut: Some(Defaut::Entier(1)),
            },
            Param {
                nom: "direction",
                label: "Direction",
                saisie: Saisie::Direction,
                defaut: Some(Defaut::Direction(Direction::PlusX)),
            },
        ],
        cout: PAS_DE_FORME,
    },
    Descripteur {
        id: "naturaliser",
        we: &["//naturalize"],
        label: "Naturaliser",
        resume: "Refait la stratigraphie colonne par colonne : une surface, \
                 un sous-sol, de la roche.",
        params: &[
            Param {
                nom: "surface",
                label: "Surface",
                saisie: Saisie::Bloc,
                defaut: Some(Defaut::Texte("minecraft:grass_block")),
            },
            Param {
                nom: "sous-sol",
                label: "Sous-sol",
                saisie: Saisie::Bloc,
                defaut: Some(Defaut::Texte("minecraft:dirt")),
            },
            Param {
                nom: "roche",
                label: "Roche",
                saisie: Saisie::Bloc,
                defaut: Some(Defaut::Texte("minecraft:stone")),
            },
            Param {
                nom: "profondeur",
                label: "Épaisseur du sous-sol",
                saisie: Saisie::Entier { min: 0, max: 64 },
                defaut: Some(Defaut::Entier(3)),
            },
        ],
        cout: Cout {
            forme: false,
            materialise: false,
            colonne: true,
        },
    },
    Descripteur {
        id: "biome",
        we: &["//setbiome"],
        label: "Biome",
        resume: "Pose un biome. ATTENTION : la grille d'un biome est de \
                 4 × 4 × 4 blocs — une sélection qui ne tombe pas sur un \
                 multiple de 4 déborde d'autant.",
        params: &[Param {
            nom: "biome",
            label: "Biome",
            saisie: Saisie::Biome,
            defaut: Some(Defaut::Texte("minecraft:plains")),
        }],
        cout: PAS_DE_FORME,
    },
    Descripteur {
        id: "lisser",
        we: &["//smooth"],
        label: "Lisser",
        resume: "Moyenne la hauteur du terrain avec celle de ses voisines. La \
                 LECTURE déborde de la sélection du rayon, sinon le bord se \
                 lisserait contre le vide.",
        params: &[
            Param {
                nom: "rayon",
                label: "Rayon",
                saisie: Saisie::Entier { min: 1, max: 64 },
                defaut: Some(Defaut::Entier(2)),
            },
            Param {
                nom: "passes",
                label: "Passes",
                saisie: Saisie::Entier { min: 1, max: 16 },
                defaut: Some(Defaut::Entier(1)),
            },
        ],
        cout: Cout {
            forme: false,
            materialise: false,
            colonne: true,
        },
    },
    Descripteur {
        id: "creuser",
        we: &["//hollow"],
        label: "Creuser",
        resume: "Vide ce qu'aucun chemin de VIDE ne relie au dehors. \
                 TOPOLOGIQUE : une salle déjà ouverte par une porte ne se \
                 remplit pas, une sphère pleine se vide. Seule opération qui \
                 matérialise toute la sélection.",
        params: &[Param {
            nom: "epaisseur",
            label: "Épaisseur de paroi",
            saisie: Saisie::Entier { min: 1, max: 64 },
            defaut: Some(Defaut::Entier(1)),
        }],
        cout: Cout {
            forme: false,
            materialise: true,
            colonne: false,
        },
    },
];

/// Le descripteur d'un identifiant, ou à défaut d'un nom WorldEdit.
///
/// **L'identifiant d'abord, TOUJOURS.** Un nom WorldEdit n'est pas une clé :
/// `//set` désigne aussi bien « remplir » que « mélange », parce que dans le
/// jeu les deux sont la même commande à un motif près. Chercher les deux dans
/// la même passe ferait dépendre la réponse de l'ordre du tableau — et un
/// identifiant finirait un jour par tomber sur le nom WorldEdit d'une autre
/// opération, ce qu'un test refuse.
pub fn descripteur(nom: &str) -> Option<&'static Descripteur> {
    OPS.iter()
        .find(|d| d.id == nom)
        .or_else(|| OPS.iter().find(|d| d.we.contains(&nom)))
}

/// Tout ce qui répond à un bout de texte — pour une palette où l'on tape
/// « //walls » ou « mur » et où l'on veut VOIR les candidats, pas en recevoir
/// un choisi à sa place.
pub fn chercher(texte: &str) -> impl Iterator<Item = &'static Descripteur> + '_ {
    let t = texte.trim().to_lowercase();
    OPS.iter().filter(move |d| {
        t.is_empty()
            || d.id.contains(&t)
            || d.label.to_lowercase().contains(&t)
            || d.we.iter().any(|w| w.contains(&t))
    })
}

/// Les paramètres d'une opération, remplis.
///
/// Une liste et non une table de hachage : il y en a cinq au plus, l'ordre est
/// celui du descripteur — donc celui de l'écran — et il est DÉTERMINISTE, ce
/// qui compte le jour où ça part dans un journal rejouable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, Valeur)>);

impl Params {
    pub fn new() -> Params {
        Params(Vec::new())
    }

    pub fn poser(&mut self, nom: &str, v: Valeur) -> &mut Params {
        match self.0.iter_mut().find(|(n, _)| n == nom) {
            Some(e) => e.1 = v,
            None => self.0.push((nom.to_string(), v)),
        }
        self
    }

    pub fn get(&self, nom: &str) -> Option<&Valeur> {
        self.0.iter().find(|(n, _)| n == nom).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Valeur)> {
        self.0.iter().map(|(n, v)| (n.as_str(), v))
    }

    fn entier(&self, nom: &str) -> i64 {
        match self.get(nom) {
            Some(Valeur::Entier(n)) => *n,
            _ => 0,
        }
    }

    fn texte(&self, nom: &str) -> &str {
        match self.get(nom) {
            Some(Valeur::Texte(s)) => s.as_str(),
            _ => "",
        }
    }

    fn vecteur(&self, nom: &str) -> [i32; 3] {
        match self.get(nom) {
            Some(Valeur::Vecteur(v)) => *v,
            _ => [0; 3],
        }
    }
}

/// Ce qui peut mal se passer entre un hôte et une opération.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erreur {
    /// Aucune opération ne porte ce nom.
    Inconnue(String),
    /// Un paramètre que le descripteur ne déclare pas. **Refusé, jamais
    /// ignoré** : un paramètre qu'on jette en silence, c'est la graine qui
    /// disparaît entre le descripteur et l'opération.
    ParamInconnu { op: String, nom: String },
    /// Un paramètre obligatoire absent.
    ParamManquant { op: String, nom: String },
    /// Une valeur du mauvais genre. Refusée plutôt que convertie.
    MauvaisGenre {
        op: String,
        nom: String,
        attendu: &'static str,
        recu: &'static str,
    },
}

impl std::fmt::Display for Erreur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Erreur::Inconnue(n) => write!(f, "opération inconnue : « {n} »"),
            Erreur::ParamInconnu { op, nom } => {
                write!(f, "« {op} » ne prend pas de paramètre « {nom} »")
            }
            Erreur::ParamManquant { op, nom } => {
                write!(f, "« {op} » exige le paramètre « {nom} »")
            }
            Erreur::MauvaisGenre {
                op,
                nom,
                attendu,
                recu,
            } => write!(
                f,
                "« {op} », paramètre « {nom} » : {attendu} attendu, {recu} reçu"
            ),
        }
    }
}

impl std::error::Error for Erreur {}

/// Complète, contrôle et SERRE les paramètres d'une opération.
///
/// **Idempotent, et un test l'exige pour chaque opération du catalogue.** Il
/// est sur le chemin de toute construction, donc il sera appliqué plusieurs
/// fois à la même chose (une fois par l'hôte, une fois par `construire`, une
/// fois au rejeu) : un normaliseur qui ne l'est pas déplacerait la valeur un
/// peu plus à chaque passage.
///
/// Le serrage est un endroit où une unité fausse devient invisible — `//smooth`
/// avec un rayon de 10 000 ne rend pas une erreur, il rend un monde plat et
/// une attente de dix minutes. Les bornes viennent du descripteur, donc de
/// l'endroit où l'interface est allée chercher les siennes.
pub fn normaliser(d: &Descripteur, p: &Params) -> Result<Params, Erreur> {
    for (nom, _) in p.iter() {
        if d.param(nom).is_none() {
            return Err(Erreur::ParamInconnu {
                op: d.id.to_string(),
                nom: nom.to_string(),
            });
        }
    }
    let mut out = Params::new();
    for decl in d.params {
        let donne = p.get(decl.nom);
        let v = match (donne, decl.defaut) {
            (Some(v), _) => {
                if !v.accepte(decl.saisie) {
                    return Err(Erreur::MauvaisGenre {
                        op: d.id.to_string(),
                        nom: decl.nom.to_string(),
                        attendu: attendu(decl.saisie),
                        recu: v.genre(),
                    });
                }
                v.clone()
            }
            (None, Some(def)) => def.valeur(),
            (None, None) => {
                return Err(Erreur::ParamManquant {
                    op: d.id.to_string(),
                    nom: decl.nom.to_string(),
                })
            }
        };
        out.poser(decl.nom, serrer(decl.saisie, v));
    }
    Ok(out)
}

fn attendu(s: Saisie) -> &'static str {
    match s {
        Saisie::Bloc | Saisie::Biome => "texte",
        Saisie::Melange => "mélange",
        Saisie::Entier { .. } => "entier",
        Saisie::Vecteur => "vecteur",
        Saisie::Direction => "direction",
        Saisie::Transformation => "transformation",
    }
}

fn serrer(s: Saisie, v: Valeur) -> Valeur {
    match (s, v) {
        (Saisie::Entier { min, max }, Valeur::Entier(n)) => Valeur::Entier(n.clamp(min, max)),
        (_, v) => v,
    }
}

/// Ce qu'une opération demande de faire, une fois ses paramètres tenus pour
/// justes.
///
/// **La distinction n'est pas esthétique** : un `Plan` se répartit sur les
/// trois étages et ne connaît qu'une section ; les autres ont besoin de la
/// copie de travail pour lire ce qu'elles reposent. Les mélanger donnerait une
/// opération qui a l'air d'être à l'étage palette et qui décode tout.
#[derive(Debug)]
pub enum Travail {
    /// Un masque et un motif. Les trois étages s'appliquent tels quels.
    ///
    /// L'identifiant voyage AVEC le plan plutôt que de se déduire de sa forme.
    /// La déduction paraissait plus sûre — elle ne l'est pas : `Motif::melange`
    /// d'une liste vide rend `Garder`, et d'une seule entrée rend `Bloc`, donc
    /// un « mélange » se relirait « remplir ». Un identifiant qui change selon
    /// ce qu'on a tapé dans un formulaire n'est pas un identifiant.
    Direct { id: &'static str, plan: Plan },
    /// Ce qui demande la copie de travail.
    Composee(Composee),
}

/// Les opérations qui ne sont pas « un masque et un motif ».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Composee {
    CopierVers {
        decalage: [i32; 3],
        transfo: Option<Transfo>,
    },
    Deplacer {
        decalage: [i32; 3],
        remplir: String,
    },
    Empiler {
        fois: u32,
        direction: Direction,
    },
    Naturaliser {
        surface: String,
        sous_sol: String,
        roche: String,
        profondeur: u32,
    },
    Biome {
        nom: String,
    },
    Lisser {
        rayon: u32,
        passes: u32,
    },
    Creuser {
        epaisseur: u32,
    },
}

impl Travail {
    /// L'identifiant de l'opération qui a produit ce travail.
    ///
    /// L'aller-retour `descripteur → travail → identifiant` doit être
    /// l'identité, et un test l'exige pour chaque entrée du catalogue : une
    /// ligne de `match` copiée-collée construirait la MAUVAISE opération, et
    /// rien d'autre ne le verrait. Pour les opérations directes, où
    /// l'identifiant est recopié et non déduit, c'est le masque et le motif
    /// qu'un second test regarde — c'est là que la faute serait.
    pub fn id(&self) -> &'static str {
        match self {
            Travail::Direct { id, .. } => id,
            Travail::Composee(c) => match c {
                Composee::CopierVers { .. } => "copier-vers",
                Composee::Deplacer { .. } => "deplacer",
                Composee::Empiler { .. } => "empiler",
                Composee::Naturaliser { .. } => "naturaliser",
                Composee::Biome { .. } => "biome",
                Composee::Lisser { .. } => "lisser",
                Composee::Creuser { .. } => "creuser",
            },
        }
    }
}

/// **La seule façon de fabriquer un travail depuis un nom.**
///
/// Elle commence par le descripteur et par `normaliser` : il n'existe donc pas
/// de chemin qui les saute. C'est le remède au piège d'`ExeWorldEdit` — un
/// maillon de la chaîne qu'aucun hôte n'appelle — dans sa forme forte : plutôt
/// qu'espérer que chacun pense à valider, on rend la validation inévitable.
///
/// `interner` est passé parce qu'un `StateId` n'a de sens que relativement au
/// sien. Le lui faire deviner serait rouvrir une heure déjà perdue.
pub fn construire(
    nom: &str,
    p: &Params,
    interner: &mut tf_anvil::Interner,
) -> Result<Travail, Erreur> {
    let d = descripteur(nom).ok_or_else(|| Erreur::Inconnue(nom.to_string()))?;
    let p = normaliser(d, p)?;
    Ok(match d.id {
        "poser" => Travail::Direct {
            id: "poser",
            plan: Plan::nouveau(Masque::Tout, Motif::Bloc(interner.intern(p.texte("bloc")))),
        },
        "remplacer" => Travail::Direct {
            id: "remplacer",
            plan: Plan::nouveau(
                Masque::Etat(interner.intern(p.texte("de"))),
                Motif::Bloc(interner.intern(p.texte("vers"))),
            ),
        },
        "melanger" => {
            let v = match p.get("melange") {
                Some(Valeur::Melange(v)) => v.clone(),
                _ => Vec::new(),
            };
            Travail::Direct {
                id: "melanger",
                plan: Plan::nouveau(
                    Masque::Tout,
                    Motif::melange(v.iter().map(|(n, b)| (*n, interner.intern(b))).collect()),
                ),
            }
        }
        "copier-vers" => Travail::Composee(Composee::CopierVers {
            decalage: p.vecteur("decalage"),
            transfo: match p.get("transformation") {
                Some(Valeur::Transformation(t)) => *t,
                _ => None,
            },
        }),
        "deplacer" => Travail::Composee(Composee::Deplacer {
            decalage: p.vecteur("decalage"),
            remplir: p.texte("remplir").to_string(),
        }),
        "empiler" => Travail::Composee(Composee::Empiler {
            fois: p.entier("fois").max(0) as u32,
            direction: match p.get("direction") {
                Some(Valeur::Direction(d)) => *d,
                _ => DIRECTIONS[0],
            },
        }),
        "naturaliser" => Travail::Composee(Composee::Naturaliser {
            surface: p.texte("surface").to_string(),
            sous_sol: p.texte("sous-sol").to_string(),
            roche: p.texte("roche").to_string(),
            profondeur: p.entier("profondeur").max(0) as u32,
        }),
        "biome" => Travail::Composee(Composee::Biome {
            nom: p.texte("biome").to_string(),
        }),
        "lisser" => Travail::Composee(Composee::Lisser {
            rayon: p.entier("rayon").max(0) as u32,
            passes: p.entier("passes").max(0) as u32,
        }),
        "creuser" => Travail::Composee(Composee::Creuser {
            epaisseur: p.entier("epaisseur").max(0) as u32,
        }),
        // Inatteignable tant que `OPS` et ce `match` s'accordent — et c'est
        // très exactement ce qu'un test vérifie, pour chaque descripteur.
        autre => return Err(Erreur::Inconnue(autre.to_string())),
    })
}
