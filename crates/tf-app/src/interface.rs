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
    reticule(ctx);
}

fn barre(ui: &mut Ui, e: &mut Etat) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("titiforge").strong());
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
        ui.label(
            RichText::new(match e.mode {
                Mode::Edition => "gauche : coin 1 · droit : coin 2",
                Mode::Conception => "gauche : tirer une FACE · droit : abandonner",
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
    ui.add_space(10.0);
    ui.separator();
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
    ui.horizontal(|ui| {
        ui.label("bloc");
        ui.add(
            egui::TextEdit::singleline(&mut e.bloc_tirage)
                .desired_width(180.0)
                .hint_text("minecraft:stone"),
        );
    });
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
        if champ(ui, p, &mut v) {
            e.atelier.params.poser(p.nom, v);
        } else {
            // Il ne peut pas se produire tant que le test de couverture
            // passe ; s'il se produit quand même, il se VOIT.
            ui.colored_label(ROUGE, format!("saisie non dessinable : {}", p.nom));
        }
    }

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
                // envoie. Tenir un canal ici rendrait l'interface intestable
                // sans fil.
                if let Some(sel) = e.selection.boite() {
                    e.demande = Some(crate::moteur::Commande::Appliquer {
                        op: e.atelier.op(),
                        params: e.atelier.params.clone(),
                        sel,
                        forme: tf_ops::Forme::Boite,
                        compter: true,
                        seed: 0,
                    });
                }
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
                 Ouvrir un monde avec --monde.",
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

/// **Dessine le champ d'un paramètre, et rien d'autre.**
///
/// Rend `false` quand ce genre de saisie n'a pas de champ — c'est ce qu'un
/// test exige faux pour chaque variante de `Saisie`. Dans `ExeWorldEdit`, trois
/// types déclarés sans champ retombaient sur la case de texte par défaut, et
/// la chaîne partait telle quelle vers une opération qui attend un tableau :
/// deux opérations inutilisables, sans une erreur à l'écran. Un repli muet est
/// pire qu'un refus visible.
pub fn champ(ui: &mut Ui, p: &Param, v: &mut Valeur) -> bool {
    match (p.saisie, v) {
        (Saisie::Bloc, Valeur::Texte(s)) | (Saisie::Biome, Valeur::Texte(s)) => {
            ui.add(
                egui::TextEdit::singleline(s)
                    .desired_width(f32::INFINITY)
                    .hint_text(if p.saisie == Saisie::Bloc {
                        "minecraft:stone"
                    } else {
                        "minecraft:plains"
                    }),
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
                    ui.add(
                        egui::TextEdit::singleline(bloc)
                            .desired_width(150.0)
                            .hint_text("minecraft:stone"),
                    );
                    if ui.small_button("×").clicked() {
                        retirer = Some(i);
                    }
                });
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

/// « Aucune » n'est pas une transformation, donc ce n'est pas à `tf-blocks`
/// de la nommer ; tout le reste vient de là.
fn nom_transfo(t: Option<tf_blocks::Transfo>) -> &'static str {
    match t {
        None => "aucune",
        Some(x) => x.nom(),
    }
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
