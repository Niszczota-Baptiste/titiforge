//! **Le dessin de l'interface. Il LIT l'état, il ne décide rien.**
//!
//! **Aucun formulaire d'opération ne s'écrit ici.** Ils se GÉNÈRENT depuis les
//! descripteurs de `tf-ops` : ce fichier sait dessiner une saisie — un bloc,
//! un entier borné, une direction — et ne sait RIEN des opérations qui
//! existent. Ajouter `//deform` au moteur demandera zéro ligne de renderer.
//!
//! C'est la leçon d'`ExeWorldEdit`, reprise telle quelle, et sa contrepartie
//! aussi : *un type de paramètre déclaré sans champ pour le saisir* y a livré
//! « Remplacer » et « Mélange » inutilisables, sans la moindre erreur à
//! l'écran. D'où `champ`, qui rend `false` quand il ne sait pas dessiner — et
//! un test qui l'exige vrai pour CHAQUE variante de `Saisie`.

use egui::{Color32, RichText, Ui};
use tf_ops::catalogue::{self, Param, Saisie, Valeur};
use tf_render::controles::Mode;
use tf_world::selection::DIRECTIONS;

use crate::etat::{Etat, Note};
use crate::nuancier::Nuancier;

/// **Ce qu'un champ de bloc propose** : le nuancier, et le bloc visé — la
/// première proposition, puisqu'on regarde en général ce qu'on veut prendre.
#[derive(Clone, Copy)]
pub struct Aide<'a> {
    pub nuancier: &'a Nuancier,
    pub vise: Option<&'a str>,
}

impl<'a> Aide<'a> {
    /// Celle de l'état : ce que la coque a nourri.
    pub fn de(e: &'a Etat) -> Aide<'a> {
        Aide {
            nuancier: &e.nuancier,
            vise: e.bloc_vise.as_deref(),
        }
    }
}

/// Les couleurs du quadrillage, partagées avec ce qui le dessine en 3D —
/// une seule table, sinon la légende finit par mentir sur ce qu'on voit.
pub const BLEU: Color32 = Color32::from_rgb(90, 170, 255);
pub const ORANGE: Color32 = Color32::from_rgb(255, 190, 90);
pub const ROUGE: Color32 = Color32::from_rgb(255, 90, 90);
pub const VERT: Color32 = Color32::from_rgb(120, 220, 140);
pub const GRIS: Color32 = Color32::from_rgb(150, 160, 170);

/// Dessine toute l'interface par-dessus le viewport.
pub fn dessiner(ctx: &egui::Context, e: &mut Etat) {
    egui::TopBottomPanel::top("barre").show(ctx, |ui| barre(ui, e));
    egui::SidePanel::right("inspecteur")
        .default_width(310.0)
        .show(ctx, |ui| inspecteur(ui, e));
    if e.accueil.ouvert {
        accueil(ctx, &mut e.accueil);
    } else {
        // Le réticule est au premier plan : il se dessinerait PAR-DESSUS la
        // fenêtre d'accueil, en plein milieu de la liste des saves.
        reticule(ctx);
    }
}

fn barre(ui: &mut Ui, e: &mut Etat) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("titiforge").strong());
        if ui
            .button("Ouvrir un monde…")
            .on_hover_text("Les saves de vos installations, les mondes récents, ou un chemin.")
            .clicked()
        {
            e.accueil.ouvert = true;
        }
        ui.separator();

        // **Le bouton de mode.** Les deux modes sont nommés d'après ce qu'ils
        // FONT, pas d'après les logiciels dont ils héritent.
        ui.label("mode :");
        ui.selectable_value(&mut e.mode, Mode::Edition, "Édition")
            .on_hover_text(
                "Transformer ce qui existe. Gauche et droit posent les deux coins d'un volume.",
            );
        ui.selectable_value(&mut e.mode, Mode::Conception, "Conception")
            .on_hover_text(
                "Concevoir. Gauche et droit désignent des faces, des arêtes, des composants.",
            );

        ui.separator();
        // La répartition est FIXE, et elle est écrite là où on la lit sans
        // chercher : la caméra a un bouton à elle, qu'aucun outil ne prend.
        ui.label(
            RichText::new("molette enfoncée : tourner · +Maj : panoramique · rouler : avancer")
                .color(GRIS),
        );
        ui.separator();
        // La légende vient de l'OUTIL, pas d'une phrase écrite à côté : deux
        // constantes indépendantes finissent par diverger, et une barre qui
        // annonce le mauvais bouton est pire qu'une barre muette.
        ui.label(
            RichText::new(match e.mode {
                Mode::Edition => "gauche : coin 1 · droit : coin 2",
                Mode::Conception => e.outil.legende(),
            })
            .color(if e.mode == Mode::Edition {
                VERT
            } else {
                ORANGE
            }),
        );
    });
}

fn inspecteur(ui: &mut Ui, e: &mut Etat) {
    ui.add_space(4.0);
    ui.heading("Inspecteur");

    // ── ce que le réticule désigne
    ui.add_space(6.0);
    ui.label(RichText::new("SOUS LE RÉTICULE").strong().color(GRIS));
    match (e.reticule.case, e.reticule.pose) {
        (Some(c), pose) => {
            ui.label(format!("casser : {}, {}, {}", c.x, c.y, c.z));
            match pose {
                // **Les deux, parce que poser et casser ne visent pas la même
                // case.** Un rayon touche une face, donc un plan ENTRE deux
                // cases.
                Some(p) => ui.label(format!("poser  : {}, {}, {}", p.x, p.y, p.z)),
                None => ui.label(RichText::new("poser  : rien (on est dedans)").color(GRIS)),
            };
        }
        _ => {
            ui.label(RichText::new("rien — le rayon ne touche pas").color(GRIS));
        }
    }

    // ── l'accrochage, et POURQUOI
    if let Some(a) = &e.reticule.accroche {
        ui.add_space(4.0);
        let n = a.axes_accroches();
        let quoi = match n {
            0 => "libre".to_string(),
            1 => "accroché à un PLAN".to_string(),
            2 => "accroché à une DROITE".to_string(),
            _ => "accroché à un POINT".to_string(),
        };
        ui.label(
            RichText::new(format!("inférence : {quoi}")).color(if n > 0 { VERT } else { GRIS }),
        );
        // Une inférence qui accroche en SILENCE est une inférence qu'on
        // combat : on dit à quoi, axe par axe.
        for (k, r) in a.raisons.iter().enumerate() {
            if let Some(r) = r {
                let axe = ["x", "y", "z"][k];
                ui.label(
                    RichText::new(format!(
                        "  {axe} → {} ({}, {}, {})",
                        r.genre.nom(),
                        r.reference.x,
                        r.reference.y,
                        r.reference.z
                    ))
                    .color(VERT)
                    .small(),
                );
            }
        }
    }

    // ── la sélection
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("SÉLECTION").strong().color(GRIS));
    match e.resume_selection() {
        None => {
            ui.label(RichText::new("aucune — gauche pose le coin 1, droit le coin 2").color(GRIS));
        }
        Some(r) => {
            let (sx, sy, sz) = r.taille;
            ui.label(format!("{sx} × {sy} × {sz} = {} blocs", r.volume));
            ui.label(
                RichText::new(format!(
                    "{}, {}, {} → {}, {}, {}",
                    r.min.x, r.min.y, r.min.z, r.max.x, r.max.y, r.max.z
                ))
                .small()
                .color(GRIS),
            );
            // **Le coût n'est PAS ici.** Il dépend de l'opération : une
            // opération à portée `Colonne` ne verra jamais l'étage palette,
            // quelle que soit l'alignement de la sélection. Le dire deux fois
            // ferait dire deux choses différentes le jour où elles divergent.
            ui.label(
                RichText::new(if r.alignee_chunk {
                    "alignée sur les chunks"
                } else {
                    "non alignée sur les chunks"
                })
                .small()
                .color(if r.alignee_chunk { VERT } else { GRIS }),
            );
        }
    }

    // ── exporter, importer
    echanges(ui, e);

    // ── le quadrillage
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("DÉCOUPAGE").strong().color(GRIS));
    let mut chunks = e.quadrillage.chunks.is_some();
    if ui.checkbox(&mut chunks, "chunks (16)").changed() {
        e.quadrillage.chunks = chunks.then_some(2);
    }
    if let Some(r) = &mut e.quadrillage.chunks {
        ui.add(egui::Slider::new(r, 0..=8).text("rayon"));
    }
    let mut mca = e.quadrillage.mca.is_some();
    if ui.checkbox(&mut mca, "fichiers .mca (512)").changed() {
        e.quadrillage.mca = mca.then_some(1);
    }
    ui.label(
        RichText::new("les chunks se teignent par la parité de LEUR .mca")
            .small()
            .color(GRIS),
    );
    ui.horizontal(|ui| {
        ui.colored_label(BLEU, "■");
        ui.label(RichText::new("pair").small());
        ui.colored_label(ORANGE, "■");
        ui.label(RichText::new("impair").small());
        ui.colored_label(ROUGE, "■");
        ui.label(RichText::new(".mca").small());
    });

    // ── l'accrochage
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("ACCROCHAGE").strong().color(GRIS));
    ui.add(egui::Slider::new(&mut e.tolerance, 0..=8).text("tolérance (blocs)"));
    ui.label(
        RichText::new("0 éteint. La tolérance est en BLOCS, pas en pixels.")
            .small()
            .color(GRIS),
    );

    // Le pousser-tirer n'a de sens qu'en Conception : l'afficher en Édition
    // proposerait un geste que les boutons n'y font pas.
    if e.mode == Mode::Conception {
        tirage(ui, e);
    }

    // ── l'atelier : l'opération, engendrée depuis son descripteur
    ui.add_space(10.0);
    ui.separator();
    operations(ui, e);

    if !e.message.is_empty() {
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(&e.message).color(ORANGE));
    }
}

/// Ce que le pousser-tirer fait pendant qu'on le fait.
///
/// **Un geste qui ne DIT pas ce qu'il vaut est un geste qu'on refait trois
/// fois.** SketchUp affiche le nombre pendant le glissement, et c'est la
/// moitié de sa précision.
fn tirage(ui: &mut Ui, e: &mut Etat) {
    use crate::etat::Outil;
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("OUTIL").strong().color(GRIS));
    ui.horizontal_wrapped(|ui| {
        for o in Outil::TOUS {
            ui.selectable_value(&mut e.outil, o, o.nom());
        }
    });
    if e.outil == Outil::Composant {
        composants(ui, e);
        return;
    }
    if e.outil == Outil::Coller {
        coller(ui, e);
        return;
    }
    if e.outil != Outil::Tirer {
        ui.label(
            RichText::new(
                "Poser et casser ne visent pas la même case : un rayon touche \
                 une FACE, donc un plan ENTRE deux cases. Le panneau du \
                 réticule montre les deux.",
            )
            .small()
            .color(GRIS),
        );
        bloc_en_main(ui, e);
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new("POUSSER-TIRER").strong().color(GRIS));
    match &e.tirage {
        None => {
            ui.label(
                RichText::new(
                    "Clic gauche sur une FACE de la sélection, puis glisser. \
                     Droit ou Échap abandonne.",
                )
                .small()
                .color(GRIS),
            );
        }
        Some(t) => {
            let n = t.blocs;
            ui.label(
                RichText::new(format!(
                    "{} {} de {} bloc(s)",
                    if n >= 0 { "tiré" } else { "poussé" },
                    t.face.nom(),
                    n.abs()
                ))
                .color(if n >= 0 { VERT } else { ORANGE }),
            );
            ui.label(
                RichText::new(if n >= 0 {
                    "tirer POSE la matière"
                } else {
                    "pousser pose de l'AIR"
                })
                .small()
                .color(GRIS),
            );
            // **Une inférence qui accroche en SILENCE est une inférence qu'on
            // combat.** Ce qui rend celle de SketchUp utilisable n'est pas sa
            // précision, c'est qu'elle DIT ce qu'elle a attrapé.
            if let Some(r) = t.raison {
                ui.label(
                    RichText::new(format!(
                        "accroché : {} ({}, {}, {})",
                        r.genre.nom(),
                        r.reference.x,
                        r.reference.y,
                        r.reference.z
                    ))
                    .small()
                    .color(VERT),
                );
            }
        }
    }
    bloc_en_main(ui, e);
}

/// **La fiche de l'outil Composant** : les définitions du monde, la création
/// depuis la sélection, l'orientation de la pose, et l'instance visée.
///
/// Rien n'est décidé ici : chaque bouton envoie ce que l'état construit
/// (`Etat::demande_*`), et les identifiants viennent du document que le fil
/// a publié.
fn composants(ui: &mut Ui, e: &mut Etat) {
    use crate::etat::{nom_orientation, ORIENTATIONS};
    ui.add_space(6.0);
    ui.label(RichText::new("COMPOSANTS").strong().color(GRIS));
    if let Some(err) = &e.composants.erreur {
        ui.colored_label(ROUGE, format!("{err} — rien n'y sera écrit"));
    }
    let actif = e.editable && !e.occupe && e.composants.erreur.is_none();

    // ── créer, renommer
    ui.horizontal(|ui| {
        ui.label("nom");
        ui.add(
            egui::TextEdit::singleline(&mut e.nom_composant)
                .desired_width(150.0)
                .hint_text("fenêtre, porte…"),
        );
    });
    let creer = e.demande_creer_composant();
    let renommer = e.demande_renommer();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                actif && creer.is_some(),
                egui::Button::new("Créer depuis la sélection"),
            )
            .on_hover_text(
                "La sélection devient un composant, et sa première instance sur \
                 place. L'air n'en fait pas partie : une instance y est \
                 transparente, et le terrain autour reste.",
            )
            .clicked()
        {
            e.demande = creer;
        }
        if ui
            .add_enabled(
                actif && renommer.is_some(),
                egui::Button::new("Renommer le choisi"),
            )
            .clicked()
        {
            e.demande = renommer;
        }
    });
    if e.selection.boite().is_none() {
        ui.label(
            RichText::new("créer demande une sélection — en Édition, gauche et droit")
                .small()
                .color(GRIS),
        );
    }

    // ── les définitions du monde
    ui.add_space(4.0);
    let projet = std::sync::Arc::clone(&e.composants.projet);
    if projet.definitions.is_empty() {
        ui.label(RichText::new("aucun composant dans ce monde").color(GRIS));
    }
    for d in &projet.definitions {
        let n = projet.instances_de(d.id).count();
        let [x, y, z] = d.contenu.taille;
        let texte = format!("{} — {x} × {y} × {z} · {n} instance(s)", d.nom);
        if ui
            .selectable_label(e.composant_choisi == Some(d.id), texte)
            .clicked()
        {
            e.composant_choisi = Some(d.id);
        }
    }

    // ── la pose
    ui.add_space(4.0);
    ui.label(
        RichText::new("la pose met le coin de plus petites coordonnées sur la case visée")
            .small()
            .color(GRIS),
    );
    ui.horizontal_wrapped(|ui| {
        for t in ORIENTATIONS {
            ui.selectable_value(&mut e.orientation, t, nom_orientation(t));
        }
    });

    // ── l'instance sous le réticule
    ui.add_space(6.0);
    match e.instance_visee().cloned() {
        None => {
            ui.label(RichText::new("sous le réticule : aucune instance").color(GRIS));
        }
        Some(i) => {
            let nom = projet
                .definition(i.definition)
                .map_or("?", |d| d.nom.as_str());
            ui.label(format!(
                "sous le réticule : instance n° {} de « {nom} » ({})",
                i.id,
                nom_orientation(i.transfo)
            ));
            let maj = e.demande_mettre_a_jour();
            let detacher = e.demande_detacher();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(actif, egui::Button::new(format!("Mettre à jour « {nom} »")))
                    .on_hover_text(
                        "La définition prend TOUT ce que la boîte de cette instance \
                         contient — un mur autour y entre, et le compte rendu le \
                         dit. Toutes les autres instances suivent ; un Ctrl+Z défait \
                         le tout.",
                    )
                    .clicked()
                {
                    e.demande = maj;
                }
                if ui
                    .add_enabled(actif, egui::Button::new("Détacher"))
                    .on_hover_text("Elle garde ses blocs et ne suit plus sa définition.")
                    .clicked()
                {
                    e.demande = detacher;
                }
            });
        }
    }
    ui.label(
        RichText::new(
            "une retouche faite dans une instance survit, sauf là où la définition change",
        )
        .small()
        .color(GRIS),
    );
}

/// **Les échanges** : exporter la sélection dans un fichier que WorldEdit,
/// Litematica ou un bloc de structure liront ; importer un fichier dans le
/// presse-papiers ; copier la sélection.
///
/// Rien n'est décidé ici : les boutons envoient ce que l'état construit, et
/// le dossier proposé vient de `dossier_par_defaut`.
fn echanges(ui: &mut Ui, e: &mut Etat) {
    use tf_formats::Format;
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("ÉCHANGES").strong().color(GRIS));
    let actif = e.editable && !e.occupe;

    // ── exporter
    ui.horizontal_wrapped(|ui| {
        for f in Format::TOUS {
            if ui
                .selectable_label(e.echanges.format == f, f.nom())
                .on_hover_text(f.pour())
                .clicked()
            {
                e.choisir_format(f);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("nom");
        ui.add(
            egui::TextEdit::singleline(&mut e.echanges.nom)
                .desired_width(170.0)
                .hint_text("porte nord"),
        );
    });
    ui.horizontal(|ui| {
        ui.label("dans");
        ui.add(
            egui::TextEdit::singleline(&mut e.echanges.dossier)
                .desired_width(230.0)
                .hint_text("un dossier"),
        );
    });
    match e.chemin_d_export() {
        Some(c) => {
            ui.label(RichText::new(c.display().to_string()).small().color(GRIS));
        }
        None => {
            ui.label(
                RichText::new("aucun dossier : ouvrir un monde d'une installation, ou en taper un")
                    .small()
                    .color(GRIS),
            );
        }
    }
    let maintenant = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let exporter = e.demande_exporter(maintenant);
    let copier = e.demande_copier();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                actif && exporter.is_some(),
                egui::Button::new("Exporter la sélection"),
            )
            .on_hover_text(
                "Ce que la copie de travail porte, retouches comprises. Un fichier \
                 du même nom n'est jamais écrasé : le nouveau prend « (2) ».",
            )
            .clicked()
        {
            e.demande = exporter;
        }
        if ui
            .add_enabled(actif && copier.is_some(), egui::Button::new("Copier"))
            .on_hover_text("La sélection devient le presse-papiers, que l'outil Coller pose.")
            .clicked()
        {
            e.demande = copier;
        }
    });
    if e.selection.boite().is_none() {
        ui.label(
            RichText::new("exporter et copier demandent une sélection")
                .small()
                .color(GRIS),
        );
    }

    // ── importer
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("importer").color(GRIS));
        if ui
            .small_button("relire")
            .on_hover_text("Relit les dossiers de Litematica, de WorldEdit et du monde.")
            .clicked()
        {
            e.chercher_fichiers();
        }
    });
    let mut choisi = None;
    egui::ScrollArea::vertical()
        .id_salt("fichiers-d-echange")
        .max_height(120.0)
        .show(ui, |ui| {
            if e.echanges.trouves.is_empty() {
                ui.label(
                    RichText::new("aucun fichier dans les dossiers habituels")
                        .small()
                        .color(GRIS),
                );
            }
            for t in &e.echanges.trouves {
                if ui
                    .add_enabled(
                        actif,
                        egui::Button::new(format!("{} — {} Ko", t.nom, t.octets.div_ceil(1024)))
                            .frame(false),
                    )
                    .on_hover_text(t.chemin.display().to_string())
                    .clicked()
                {
                    choisi = Some(t.chemin.clone());
                }
            }
        });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut e.echanges.a_importer)
                .desired_width(200.0)
                .hint_text("chemin d'un .litematic, .schem, .nbt"),
        );
        let tape = e.echanges.a_importer.trim().to_string();
        if ui
            .add_enabled(actif && !tape.is_empty(), egui::Button::new("Importer"))
            .clicked()
        {
            choisi = Some(tape.into());
        }
    });
    ui.label(
        RichText::new("ou glisser le fichier sur la fenêtre")
            .small()
            .color(GRIS),
    );
    if let Some(c) = choisi {
        e.demande = Some(e.demande_importer(&c));
    }
}

/// **La fiche de l'outil Coller** : ce que porte le presse-papiers, ce que sa
/// lecture n'a pas su porter, et comment il se pose.
fn coller(ui: &mut Ui, e: &mut Etat) {
    use crate::etat::{nom_orientation, ORIENTATIONS};
    ui.add_space(6.0);
    ui.label(RichText::new("PRESSE-PAPIERS").strong().color(GRIS));
    match e.presse.taille {
        None => {
            ui.label(
                RichText::new(
                    "vide — importer un fichier ou copier la sélection (ÉCHANGES, plus haut)",
                )
                .color(GRIS),
            );
            return;
        }
        Some([x, y, z]) => {
            ui.label(format!("« {} » — {x} × {y} × {z}", e.presse.source));
        }
    }
    if let Some((f, m)) = e.presse_plus_recente() {
        ui.colored_label(
            ROUGE,
            format!(
                "fichier d'un Minecraft plus récent que ce monde (DataVersion {f} > {m}) : \
                 des blocs peuvent ne pas y exister — le jeu les remplacerait par de l'air"
            ),
        );
    }
    for r in &e.presse.remarques {
        ui.label(RichText::new(r).small().color(ORANGE));
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "la pose met le coin de plus petites coordonnées sur la case visée — le \
             contour ambre montre où",
        )
        .small()
        .color(GRIS),
    );
    ui.horizontal_wrapped(|ui| {
        for t in ORIENTATIONS {
            ui.selectable_value(&mut e.orientation, t, nom_orientation(t));
        }
    });
    ui.checkbox(&mut e.echanges.avec_air, "coller aussi l'air")
        .on_hover_text(
            "Non par défaut, comme WorldEdit : on colle un bâtiment sur un terrain. \
             Oui pour reposer un extrait à l'identique, trous compris.",
        );
}

/// L'identifiant du champ « bloc en main » : un seul dans l'interface, et
/// la capture sait ainsi lui donner le focus.
pub const ID_BLOC_EN_MAIN: &str = "bloc-en-main";

/// **Le bloc EN MAIN** — celui que posent « Poser » et le pousser-tirer — et
/// la pipette qui le prend sous le réticule.
fn bloc_en_main(ui: &mut Ui, e: &mut Etat) {
    ui.horizontal(|ui| {
        ui.label("bloc");
        if ui
            .small_button("pipette")
            .on_hover_text(
                "Prend le bloc sous le réticule, sous l'état exact que le jeu a \
                 écrit. Aussi : Alt + clic gauche.",
            )
            .clicked()
        {
            e.pipette();
        }
    });
    let aide = Aide {
        nuancier: &e.nuancier,
        vise: e.bloc_vise.as_deref(),
    };
    champ_de_bloc(
        ui,
        egui::Id::new(ID_BLOC_EN_MAIN),
        &mut e.bloc_tirage,
        aide,
        260.0,
    );
}

/// La palette d'opérations et le formulaire de celle qui est choisie.
///
/// Rien de ce qui suit ne nomme une opération. Tout vient du catalogue.
fn operations(ui: &mut Ui, e: &mut Etat) {
    ui.label(RichText::new("OPÉRATION").strong().color(GRIS));

    // La palette. On tape ce qu'on connaît — « //walls » aussi bien que
    // « mur » — et on VOIT les candidats : `//set` en nomme deux, et en
    // choisir un à la place de l'utilisateur serait décider pour lui.
    ui.horizontal(|ui| {
        ui.label("chercher");
        ui.add(
            egui::TextEdit::singleline(&mut e.atelier.recherche)
                .desired_width(150.0)
                .hint_text("//walls, mur…"),
        );
    });
    let trouves: Vec<&'static catalogue::Descripteur> =
        catalogue::chercher(&e.atelier.recherche).collect();
    if trouves.is_empty() {
        ui.colored_label(ORANGE, "aucune opération ne répond");
    }
    let courant = e.atelier.op();
    let mut choix: Option<&'static str> = None;
    ui.horizontal_wrapped(|ui| {
        for d in &trouves {
            if ui
                .selectable_label(d.id == courant, d.label)
                .on_hover_text(format!("{}\n{}", d.we.join("  "), d.resume))
                .clicked()
            {
                choix = Some(d.id);
            }
        }
    });
    if let Some(id) = choix {
        e.atelier.choisir(id);
    }

    let d = e.atelier.descripteur();
    ui.add_space(4.0);
    ui.label(RichText::new(d.resume).small().color(GRIS));

    // ── les champs, un par paramètre DÉCLARÉ
    ui.add_space(6.0);
    for p in d.params {
        let mut v = e.atelier.valeur(p.nom);
        ui.horizontal(|ui| {
            ui.label(p.label);
        });
        let aide = Aide {
            nuancier: &e.nuancier,
            vise: e.bloc_vise.as_deref(),
        };
        if champ(ui, p, &mut v, aide) {
            e.atelier.params.poser(p.nom, v);
        } else {
            // Il ne peut pas se produire tant que le test de couverture
            // passe ; s'il se produit quand même, il se VOIT.
            ui.colored_label(ROUGE, format!("saisie non dessinable : {}", p.nom));
        }
    }

    if d.cout.forme {
        volume(ui, e);
    }

    // ── les réglages qui ne sont pas des paramètres d'opération
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.checkbox(&mut e.compter, "compter les blocs")
            .on_hover_text(
                "Coûte × 31 à l'étage palette : c'est le parcours qu'on vient \
                 d'éviter. Un choix, jamais un service rendu d'office.",
            );
        ui.label("graine");
        ui.add(egui::DragValue::new(&mut e.seed).speed(1.0));
    });

    // ── ce que ça coûterait, avant de le faire
    ui.add_space(6.0);
    let resume = e.resume_selection();
    let notes = e.atelier.verdict(resume.as_ref());
    for n in &notes {
        let (c, prefixe) = match n {
            Note::Bloquant(_) => (ROUGE, "✖ "),
            Note::Attention(_) => (ORANGE, "▲ "),
            Note::Info(_) => (GRIS, ""),
        };
        ui.label(
            RichText::new(format!("{prefixe}{}", n.texte()))
                .small()
                .color(c),
        );
    }

    ui.add_space(4.0);
    let pret = !notes.iter().any(|n| n.bloque()) && e.editable && !e.occupe;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(pret, |ui| {
            if ui.button(RichText::new("Appliquer").strong()).clicked() {
                // L'interface DÉCRIT ce qu'elle veut ; c'est la boucle qui
                // envoie, et c'est l'ÉTAT qui construit la commande. La bâtir
                // ici est ce qui avait fait perdre la forme, la graine et le
                // comptage — trois réglages câblés en dur dans un bouton que
                // personne ne teste.
                e.demande = e.demande_operation();
            }
        });
        ui.add_enabled_ui(e.editable && !e.occupe, |ui| {
            if ui.button("Annuler").on_hover_text("Ctrl+Z").clicked() {
                e.demande = Some(crate::moteur::Commande::Annuler);
            }
            if ui.button("Refaire").on_hover_text("Ctrl+Y").clicked() {
                e.demande = Some(crate::moteur::Commande::Refaire);
            }
        });
    });
    // ── écrire dans la save
    if e.editable {
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new("SAVE").strong().color(GRIS));
        ui.checkbox(&mut e.jeu_ferme, "Minecraft est fermé");
        ui.label(
            RichText::new(
                "Hors Windows, le verrou de la save est consultatif : une \
                 ouverture réussie ne prouve rien. C'est donc à vous de le dire.",
            )
            .small()
            .color(GRIS),
        );
        ui.add_enabled_ui(e.jeu_ferme && !e.occupe, |ui| {
            if ui
                .button(RichText::new("Écrire dans la save").strong())
                .on_hover_text(
                    "Refuse si le jeu tient le monde, SAUVEGARDE en copie \
                     horodatée, puis écrit. Dans cet ordre.",
                )
                .clicked()
            {
                e.demande = Some(crate::moteur::Commande::Ecrire {
                    confirme_sans_verrou: e.jeu_ferme,
                });
            }
        });
    }

    if e.occupe {
        ui.label(RichText::new("le moteur travaille…").small().color(ORANGE));
    } else if !e.editable {
        ui.label(
            RichText::new(
                "La fixture n'a pas de save derrière elle : rien à éditer. \
                 « Ouvrir un monde… », en haut à gauche.",
            )
            .small()
            .color(ORANGE),
        );
    } else {
        ui.label(
            RichText::new("Tout se fait sur une COPIE de travail : la save n'est pas touchée.")
                .small()
                .color(GRIS),
        );
    }
}

/// Le VOLUME visé dans la sélection — et il n'apparaît que pour les
/// opérations qui en acceptent un.
///
/// **Le catalogue le DIT** (`cout.forme`). Proposer une sphère à `//move`
/// donnerait un réglage sans effet, ce qui est pire qu'un réglage absent :
/// l'utilisateur croit avoir demandé quelque chose.
fn volume(ui: &mut Ui, e: &mut Etat) {
    use tf_ops::Volume;
    ui.add_space(6.0);
    ui.label(RichText::new("FORME").strong().color(GRIS));
    let courant = e.volume.rang();
    let mut choix = None;
    ui.horizontal_wrapped(|ui| {
        for v in Volume::TOUS {
            if ui.selectable_label(v.rang() == courant, v.nom()).clicked() {
                choix = Some(v);
            }
        }
    });
    if let Some(v) = choix {
        // On repart des valeurs par défaut de la variante : garder un rayon
        // en changeant de forme donnerait une pyramide de « rayon 8 », qui ne
        // veut rien dire.
        e.volume = v;
    }
    // Les champs de la forme choisie, engendrés depuis sa variante.
    match &mut e.volume {
        Volume::Aucun => {}
        Volume::Sphere { rayon } => {
            ui.add(egui::Slider::new(rayon, 1.0..=256.0).text("rayon"));
        }
        Volume::Cylindre { rayon, hauteur } => {
            ui.add(egui::Slider::new(rayon, 1.0..=256.0).text("rayon"));
            ui.add(egui::Slider::new(hauteur, 1.0..=256.0).text("demi-hauteur"));
        }
        Volume::Pyramide {
            demi_base,
            hauteur,
            renversee,
        } => {
            ui.add(egui::Slider::new(demi_base, 1.0..=256.0).text("demi-base"));
            ui.add(egui::Slider::new(hauteur, 1.0..=256.0).text("hauteur"));
            ui.checkbox(renversee, "pointe en bas");
        }
        Volume::Murs { epaisseur } | Volume::Faces { epaisseur } => {
            ui.add(egui::Slider::new(epaisseur, 1.0..=32.0).text("épaisseur"));
        }
    }
    if !matches!(e.volume, Volume::Aucun) {
        let mut creux = e.creux.is_some();
        ui.horizontal(|ui| {
            if ui.checkbox(&mut creux, "creuse").changed() {
                e.creux = creux.then_some(1.0);
            }
            if let Some(ep) = &mut e.creux {
                ui.add(egui::DragValue::new(ep).speed(0.5).range(0.5..=32.0));
            }
        });
        ui.label(
            RichText::new(
                "« creuse » évide la FORME (//hsphere). Géométrique — à ne pas \
                 confondre avec « Creuser », qui INSPECTE ce qui touche le dehors.",
            )
            .small()
            .color(GRIS),
        );
        ui.label(
            RichText::new(if e.volume.enveloppe() {
                "prise sur la sélection"
            } else {
                "centrée sur la sélection"
            })
            .small()
            .color(GRIS),
        );
    }
}

/// **Dessine le champ d'un paramètre, et rien d'autre.**
///
/// Rend `false` quand ce genre de saisie n'a pas de champ — c'est ce qu'un
/// test exige faux pour chaque variante de `Saisie`. Dans `ExeWorldEdit`, trois
/// types déclarés sans champ retombaient sur la case de texte par défaut, et
/// la chaîne partait telle quelle vers une opération qui attend un tableau :
/// deux opérations inutilisables, sans une erreur à l'écran. Un repli muet est
/// pire qu'un refus visible.
pub fn champ(ui: &mut Ui, p: &Param, v: &mut Valeur, aide: Aide) -> bool {
    match (p.saisie, v) {
        (Saisie::Bloc, Valeur::Texte(s)) => {
            let id = ui.make_persistent_id(("bloc", p.nom));
            champ_de_bloc(ui, id, s, aide, ui.available_width());
            true
        }
        (Saisie::Biome, Valeur::Texte(s)) => {
            ui.add(
                egui::TextEdit::singleline(s)
                    .desired_width(f32::INFINITY)
                    .hint_text("minecraft:plains"),
            );
            true
        }
        // **Les bornes viennent du descripteur.** L'interface ne connaît pas
        // le maximum d'un rayon de lissage, et ne doit pas : deux sources
        // pour la même borne divergeraient, et c'est le serrage qui décide.
        (Saisie::Entier { min, max }, Valeur::Entier(n)) => {
            let (bas, haut) = (min.max(i32::MIN as i64), max.min(i32::MAX as i64));
            ui.add(egui::Slider::new(n, bas..=haut));
            true
        }
        (Saisie::Vecteur, Valeur::Vecteur(d)) => {
            ui.horizontal(|ui| {
                for (k, axe) in ["x", "y", "z"].iter().enumerate() {
                    ui.label(*axe);
                    ui.add(egui::DragValue::new(&mut d[k]).speed(1.0));
                }
            });
            true
        }
        (Saisie::Direction, Valeur::Direction(d)) => {
            egui::ComboBox::from_id_salt(p.nom)
                .selected_text(d.nom())
                .show_ui(ui, |ui| {
                    for cand in DIRECTIONS {
                        ui.selectable_value(d, cand, cand.nom());
                    }
                });
            true
        }
        (Saisie::Transformation, Valeur::Transformation(t)) => {
            egui::ComboBox::from_id_salt(p.nom)
                .selected_text(nom_transfo(*t))
                .show_ui(ui, |ui| {
                    ui.selectable_value(t, None, nom_transfo(None));
                    for cand in tf_blocks::transfo::TOUTES {
                        ui.selectable_value(t, Some(cand), nom_transfo(Some(cand)));
                    }
                });
            true
        }
        // Un mélange est une LISTE : une case de texte y perdrait les poids,
        // ce qui est très exactement la faute d'`ExeWorldEdit`.
        (Saisie::Melange, Valeur::Melange(entrees)) => {
            let mut retirer = None;
            for (i, (poids, bloc)) in entrees.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(poids).speed(1.0).range(0..=1000));
                    if ui.small_button("×").clicked() {
                        retirer = Some(i);
                    }
                });
                let id = ui.make_persistent_id(("melange", p.nom, i));
                champ_de_bloc(ui, id, bloc, aide, ui.available_width());
            }
            if let Some(i) = retirer {
                entrees.remove(i);
            }
            if ui.small_button("+ un bloc").clicked() {
                entrees.push((1, String::new()));
            }
            // Les proportions, dites : un poids seul ne se lit pas.
            let total: u32 = entrees.iter().map(|(n, _)| *n).sum();
            if total > 0 {
                let part: Vec<String> = entrees
                    .iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, b)| {
                        let court = b.rsplit(':').next().unwrap_or(b);
                        format!("{court} {:.0} %", *n as f32 * 100.0 / total as f32)
                    })
                    .collect();
                ui.label(RichText::new(part.join(" · ")).small().color(GRIS));
            }
            true
        }
        // Valeur et saisie ne s'accordent pas : on ne réécrit RIEN. Convertir
        // en silence est ce qui a fait planter « Naturaliser → Personnalisé ».
        _ => false,
    }
}

/// **Un champ de bloc : on tape, il propose.**
///
/// Sous le texte, les propositions du nuancier — le visé, les récents, ce que
/// le monde porte, ce que le pack déclare. Un clic en prend une ; Entrée
/// prend la première quand ce qui est tapé n'est pas déjà un bloc connu — un
/// identifiant exact tapé à la main n'est pas remplacé par un voisin.
///
/// **Ce qui ne va pas se dit TOUT DE SUITE**, sous le champ : un texte qui ne
/// se lit pas comme un bloc, ou un bloc que ni le pack ni le monde ne
/// connaissent — très probablement une faute de frappe, et le jeu remplace
/// par de l'air ce qu'il ne connaît pas. Attendre « Appliquer » pour le dire,
/// c'est le dire après qu'on a regardé ailleurs.
pub fn champ_de_bloc(ui: &mut Ui, id: egui::Id, s: &mut String, aide: Aide, largeur: f32) {
    let r = ui.add(
        egui::TextEdit::singleline(s)
            .id(id)
            .desired_width(largeur)
            .hint_text("stone, minecraft:oak_stairs[facing=east]…"),
    );
    let popup = id.with("propositions");
    // Ouverte tant que le champ a le focus — pas seulement quand il le
    // GAGNE : un focus donné avant l'image (la capture, un raccourci) ne
    // passe jamais par `gained_focus`, et la liste ne s'ouvrait qu'à la
    // première lettre tapée.
    if r.has_focus() {
        ui.memory_mut(|m| m.open_popup(popup));
    }
    let lisible = catalogue::cle_de_bloc(s);
    let connu = lisible.as_ref().is_ok_and(|c| aide.nuancier.connu(c));
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if !connu {
            if let Some(c) = aide.nuancier.chercher(s, aide.vise, 1).into_iter().next() {
                *s = c.affiche;
            }
        }
        ui.memory_mut(|m| m.close_popup());
    }
    let mut pris = None;
    egui::popup_below_widget(
        ui,
        popup,
        &r,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(largeur.max(260.0));
            let props = aide.nuancier.chercher(s, aide.vise, 10);
            if props.is_empty() {
                ui.label(
                    RichText::new("aucun bloc connu ne répond")
                        .small()
                        .color(GRIS),
                );
            }
            for c in props {
                ui.horizontal(|ui| {
                    if ui.selectable_label(false, &c.affiche).clicked() {
                        pris = Some(c.affiche.clone());
                    }
                    ui.label(RichText::new(c.origine.nom()).small().color(GRIS));
                });
            }
        },
    );
    if let Some(a) = pris {
        *s = a;
        ui.memory_mut(|m| m.close_popup());
    }
    // Pendant la frappe, c'est la LISTE qui répond : « oak st » est une
    // recherche, pas un identifiant raté, et le dire en rouge à chaque lettre
    // serait crier avant la fin de la phrase.
    if r.has_focus() || s.trim().is_empty() {
        return;
    }
    match catalogue::cle_de_bloc(s) {
        Err(e) => {
            ui.label(RichText::new(e).small().color(ROUGE));
        }
        Ok(c) if !aide.nuancier.connu(&c) => {
            ui.label(
                RichText::new(
                    "inconnu du pack et du monde — le jeu remplace ce qu'il ne \
                     connaît pas par de l'air",
                )
                .small()
                .color(ORANGE),
            );
        }
        Ok(_) => {}
    }
}

/// « Aucune » n'est pas une transformation, donc ce n'est pas à `tf-blocks`
/// de la nommer ; tout le reste vient de là.
fn nom_transfo(t: Option<tf_blocks::Transfo>) -> &'static str {
    match t {
        None => "aucune",
        Some(x) => x.nom(),
    }
}

/// **L'écran d'ouverture d'un monde.** Il LIT l'accueil et y pose ce qu'on
/// choisit ; c'est la coque qui ouvre.
fn accueil(ctx: &egui::Context, a: &mut crate::accueil::Accueil) {
    let mut ouvert = a.ouvert;
    egui::Window::new("Ouvrir un monde")
        .open(&mut ouvert)
        // Pas de fondu : une capture ne dessine que deux images, et
        // l'accueil y sortait à moitié transparent — illisible.
        .fade_in(false)
        .collapsible(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    if !a.recents.is_empty() {
                        ui.label(RichText::new("RÉCENTS").strong().color(GRIS));
                        let mut choix = None;
                        for s in &a.recents {
                            if ligne_de_save(ui, s) {
                                choix = Some(s.save.chemin.clone());
                            }
                        }
                        if let Some(c) = choix {
                            a.choisir(c);
                        }
                        ui.add_space(8.0);
                    }
                    if a.nombre_de_saves() == 0 {
                        ui.label(
                            RichText::new(
                                "Aucune save trouvée dans une installation de Minecraft. \
                                 Coller le chemin d'un monde ci-dessous, ou glisser son \
                                 dossier sur la fenêtre.",
                            )
                            .color(ORANGE),
                        );
                    }
                    let mut choix = None;
                    for i in &a.installations {
                        if i.saves.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(&i.nom).strong().color(GRIS))
                            .on_hover_text(i.racine.display().to_string());
                        for s in &i.saves {
                            if ligne_de_save(ui, s) {
                                choix = Some(s.save.chemin.clone());
                            }
                        }
                        ui.add_space(6.0);
                    }
                    if let Some(c) = choix {
                        a.choisir(c);
                    }
                });
            ui.separator();
            ui.label("Autre monde — le dossier qui contient level.dat :");
            ui.horizontal(|ui| {
                let champ = ui.add(
                    egui::TextEdit::singleline(&mut a.chemin)
                        .desired_width(330.0)
                        .hint_text(r"C:\…\saves\Mon monde"),
                );
                let entree = champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Ouvrir").clicked() || entree {
                    a.choisir_texte();
                }
            });
            ui.label(
                RichText::new("On peut aussi glisser le dossier d'une save sur la fenêtre.")
                    .small()
                    .color(GRIS),
            );
            if let Some(err) = &a.erreur {
                ui.colored_label(ROUGE, err);
            }
        });
    a.ouvert = ouvert;
}

/// Une save dans la liste. Rend vrai si on l'a choisie.
fn ligne_de_save(ui: &mut Ui, s: &crate::accueil::SaveVue) -> bool {
    let mut texte = RichText::new(&s.save.nom);
    if s.seance_en_cours {
        texte = texte.color(ORANGE);
    }
    let r = ui.selectable_label(false, texte);
    let r = if s.seance_en_cours {
        r.on_hover_text(format!(
            "{}\nModifications pas encore écrites dans la save : elles seront \
             reprises à l'ouverture.",
            s.save.chemin.display()
        ))
    } else {
        r.on_hover_text(s.save.chemin.display().to_string())
    };
    if s.seance_en_cours {
        ui.label(
            RichText::new("  ✎ modifications pas encore écrites")
                .small()
                .color(ORANGE),
        );
    }
    r.clicked()
}

/// Le réticule, au centre exact de la zone de dessin.
///
/// Dessiné par-dessus tout, en deux traits croisés : un point unique
/// disparaît sur un fond clair, et on ne saurait plus où l'on vise.
fn reticule(ctx: &egui::Context) {
    let ecran = ctx.screen_rect();
    let c = ecran.center();
    let peintre = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("reticule"),
    ));
    let trait_ = egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
    let r = 7.0;
    peintre.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], trait_);
    peintre.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], trait_);
}
