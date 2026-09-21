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
use tf_world::inference::{accrocher, Accroche, TOLERANCE};
use tf_world::selection::{Direction, Selection};

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

/// Ce que le réticule désigne, tel que la dernière image l'a trouvé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SousLeReticule {
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

/// L'état complet de la coque.
#[derive(Debug, Clone)]
pub struct Etat {
    pub mode: Mode,
    pub vue: Vue,
    pub selection: Selection,
    pub quadrillage: Quadrillage,
    /// Tolérance d'accrochage, en blocs. Zéro l'éteint.
    pub tolerance: i32,
    pub reticule: SousLeReticule,
    /// L'opération choisie et ses paramètres.
    pub atelier: Atelier,
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
    /// Ce que l'interface a à dire, en une ligne. Vide = rien à signaler.
    pub message: String,
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
            reticule: SousLeReticule::default(),
            atelier: Atelier::default(),
            demande: None,
            occupe: false,
            editable: false,
            message: String::new(),
        }
    }

    /// Relève ce que le réticule désigne, et ce que l'accrochage en fait.
    ///
    /// `solide` est la couture vers le monde — la même que `viser`. La coque
    /// ne lit pas les chunks elle-même : elle demande.
    ///
    /// **L'axe de la face est VERROUILLÉ pour l'accrochage.** Sans ça, la
    /// paroi visée est à un bloc — donc dans la tolérance — et l'inférence
    /// ramène la pose DANS le mur qu'on vise.
    pub fn relever_reticule(
        &mut self,
        camera: &tf_render::Camera,
        aspect: f32,
        portee: f32,
        solide: &dyn Fn([i32; 3]) -> bool,
    ) {
        let d = tf_render::viser::rayon_ecran(camera, [0.0, 0.0], aspect);
        let Some(t) = tf_render::viser::viser(camera.oeil, d, portee, solide) else {
            self.reticule = SousLeReticule::default();
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
        self.reticule = SousLeReticule {
            case: Some(case),
            pose,
            accroche,
        };
    }

    /// Le point qu'un clic poserait : l'accroché s'il y en a un, sinon le brut.
    pub fn point_de_pose(&self) -> Option<BlockPos> {
        match &self.reticule.accroche {
            Some(a) => Some(a.position),
            None => self.reticule.pose,
        }
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
