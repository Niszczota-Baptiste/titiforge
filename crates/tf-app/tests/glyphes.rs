//! **Chaque caractère que l'interface écrit existe dans sa police.**
//!
//! egui dessine un carré « □ » à la place d'un glyphe qu'il n'a pas — sans
//! erreur, sans avertissement. Les flèches « → » de l'inspecteur (« x → milieu
//! (…) », les deux coins d'une sélection) se sont affichées ainsi depuis leur
//! arrivée, sur toutes les captures, et personne ne l'avait relevé : un carré
//! se lit comme une puce.
//!
//! Et pas seulement elles : le « ▲ » qui précède chaque avertissement de
//! l'atelier, et le « ✎ » qui signale sur l'accueil un monde au travail pas
//! encore écrit, avaient le même sort.
//!
//! Le test lit les SOURCES de ce qui parle à l'utilisateur — ce qu'il peut
//! voir s'y écrit en littéraux — et demande chaque caractère non ASCII à la
//! police que la coque utilise, celle d'egui par défaut.

use std::collections::BTreeMap;
use std::path::Path;

/// Ce qui écrit au terminal, pas à l'écran de la coque.
const TERMINAL: [&str; 3] = ["println!", "eprintln!", "phase_texte("];

/// Les caractères non ASCII écrits ENTRE GUILLEMETS, avec un endroit où les
/// trouver. Les commentaires ne s'affichent pas ; un littéral de caractère
/// (`'"'`) n'ouvre pas de chaîne.
fn caracteres_des_litteraux(fichier: &Path, v: &mut BTreeMap<char, String>) {
    let source = std::fs::read_to_string(fichier).unwrap();
    let mut dedans = false;
    for (n, ligne) in source.lines().enumerate() {
        // Ce qui part au TERMINAL ne passe pas par egui : un terminal a ses
        // polices. Une ligne à la fois — assez pour les sorties d'une ligne,
        // qui sont les seules à porter des flèches aujourd'hui.
        if !dedans && TERMINAL.iter().any(|m| ligne.contains(m)) {
            continue;
        }
        let c: Vec<char> = ligne.chars().collect();
        let mut i = 0;
        while i < c.len() {
            if dedans {
                match c[i] {
                    '\\' => i += 1,
                    '"' => dedans = false,
                    x if !x.is_ascii() => {
                        v.entry(x)
                            .or_insert_with(|| format!("{}:{}", fichier.display(), n + 1));
                    }
                    _ => {}
                }
            } else if c[i] == '/' && c.get(i + 1) == Some(&'/') {
                break;
            } else if c[i] == '\'' {
                // Un littéral de caractère : `'x'` ou `'\x'`. Une durée de vie
                // (`'a`) n'est suivie d'aucune apostrophe proche.
                if c.get(i + 2) == Some(&'\'') {
                    i += 2;
                } else if c.get(i + 1) == Some(&'\\') && c.get(i + 3) == Some(&'\'') {
                    i += 3;
                }
            } else if c[i] == '"' {
                dedans = true;
            }
            i += 1;
        }
    }
}

/// Tout le dossier, sous-dossiers compris : un module rangé plus bas ne doit
/// pas échapper au test sans que rien ne le dise.
fn lire_le_dossier(dossier: &Path, vus: &mut BTreeMap<char, String>) {
    for f in std::fs::read_dir(dossier).unwrap() {
        let f = f.unwrap().path();
        if f.is_dir() {
            lire_le_dossier(&f, vus);
        } else if f.extension().is_some_and(|e| e == "rs") {
            caracteres_des_litteraux(&f, vus);
        }
    }
}

#[test]
fn chaque_caractere_affiche_a_son_glyphe() {
    let racine = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut vus = BTreeMap::new();
    // La coque, et ce qu'elle affiche des autres : les noms et paramètres des
    // opérations, des formes, des directions, et les erreurs du moteur.
    for dossier in ["tf-app/src", "tf-ops/src", "tf-world/src", "tf-formats/src"] {
        lire_le_dossier(&racine.join(dossier), &mut vus);
    }
    assert!(
        vus.contains_key(&'é'),
        "la prémisse : les sources sont lues"
    );

    let ctx = egui::Context::default();
    // Les polices ne se chargent qu'à la première image.
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let manquants: Vec<String> = ctx.fonts(|f| {
        vus.iter()
            .filter(|(c, _)| !f.has_glyph(&egui::FontId::proportional(14.0), **c))
            .map(|(c, ou)| format!("« {c} » (U+{:04X}) — {ou}", *c as u32))
            .collect()
    });
    assert!(
        manquants.is_empty(),
        "la police de l'interface n'a pas ces caractères — ils s'afficheraient en carré :\n{}",
        manquants.join("\n")
    );
}
