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
}

impl Outil {
    pub const TOUS: [Outil; 3] = [Outil::Tirer, Outil::Poser, Outil::Casser];

    pub const fn nom(self) -> &'static str {
        match self {
            Outil::Tirer => "Tirer une face",
            Outil::Poser => "Poser",
            Outil::Casser => "Casser",
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
        }
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
    pub reticule: SousLeReticule,
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
            reticule: SousLeReticule::default(),
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
        }
    }

    /// **Un autre monde vient de s'ouvrir** : la caméra se recadre sur lui, et
    /// tout ce qui désignait des cases de l'ancien s'efface — sélection,
    /// tirage, réticule. Le reste reste : le mode, l'outil, l'opération et
    /// ses paramètres sont des choix de l'utilisateur, pas des propriétés du
    /// monde.
    ///
    /// « Minecraft est fermé » se redemande : c'était vrai d'une AUTRE save.
    pub fn recadrer(&mut self, min: [f32; 3], max: [f32; 3], aspect: f32) {
        self.vue = Vue::cadrer(min, max, aspect);
        self.selection = Selection::nouvelle();
        self.tirage = None;
        self.reticule = SousLeReticule::default();
        self.demande = None;
        self.jeu_ferme = false;
        self.message.clear();
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
    /// Rend faux quand le réticule ne désigne rien : un clic dans le ciel ne
    /// doit pas déplacer une sélection existante.
    pub fn poser_coin(&mut self, premier: bool) -> bool {
        let Some(c) = self.reticule.case else {
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
        match &self.reticule.accroche {
            Some(a) => Some(a.position),
            None => self.reticule.pose,
        }
    }

    /// **Attrape une face de la sélection.** Le début du pousser-tirer.
    ///
    /// Le rayon est celui du réticule, comme tout le reste : on tire la face
    /// qu'on REGARDE. Rend faux quand le rayon ne touche aucune face — un clic
    /// à côté ne doit pas démarrer un geste fantôme qui déplacera la sélection
    /// au premier mouvement de souris.
    pub fn attraper(&mut self, camera: &tf_render::Camera, aspect: f32) -> bool {
        let d = tf_render::viser::rayon_ecran(camera, [0.0, 0.0], aspect);
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
        let c = self.reticule.case?;
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
