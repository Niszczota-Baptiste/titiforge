//! **Le dessin de l'interface. Il LIT l'état, il ne décide rien.**
//!
//! Ce qui n'est pas ici : les formulaires d'opérations. `ExeWorldEdit` l'a
//! prouvé — aucun formulaire ne s'y écrit à la main, tous se GÉNÈRENT depuis
//! les descripteurs du moteur, et ajouter une opération n'y demande pas une
//! ligne d'interface. `tf-ops` n'a pas encore de descripteurs ; en écrire les
//! formulaires à la main maintenant, ce serait écrire ce qu'il faudra
//! supprimer. Le trou est donc LAISSÉ VISIBLE, et nommé dans le panneau.

use egui::{Color32, RichText, Ui};
use tf_render::controles::Mode;

use crate::etat::Etat;

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
        ui.label(
            RichText::new("molette enfoncée : tourner · +Maj : panoramique · rouler : avancer")
                .color(GRIS),
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
            // **Ce qui décide l'étage se COMPTE.** « Alignée sur les chunks »
            // est vrai et ne prouve rien : une sélection alignée en x et z
            // dont la hauteur tombe au milieu d'une tranche de seize ne couvre
            // aucune section entière.
            let (ent, tot) = r.sections;
            ui.label(RichText::new(r.verdict()).color(if ent == tot && tot > 0 {
                VERT
            } else {
                ORANGE
            }));
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

    // ── ce qui manque, dit en toutes lettres
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("OPÉRATIONS").strong().color(GRIS));
    ui.label(
        RichText::new(
            "Pas encore ici. Les formulaires se GÉNÈRENT depuis les descripteurs \
             du moteur — aucun ne s'écrit à la main. `tf-ops` n'en a pas encore : \
             les écrire maintenant, ce serait écrire ce qu'il faudra supprimer.",
        )
        .small()
        .color(ORANGE),
    );
    ui.label(
        RichText::new("En attendant : `cargo run -p tf-ops --example editer`")
            .small()
            .color(GRIS),
    );

    if !e.message.is_empty() {
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(&e.message).color(ORANGE));
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
