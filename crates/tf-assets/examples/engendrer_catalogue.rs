//! Engendre `crates/tf-bench/src/catalogue.rs` depuis un pack RÉEL.
//!
//! La table était produite par un script Python qui rejouait les mêmes règles
//! de son côté. Deux implémentations d'une même règle finissent par diverger —
//! mesuré : **16 désaccords sur 220 blocs**, dont onze cubes que le pack dit
//! translucides. Ici c'est `tf_assets::catalogue::classer` qui décide, le même
//! code que l'application utilisera.
//!
//! ```text
//! cargo run --release -p tf-assets --example engendrer_catalogue -- \
//!     ../titisite/public/codex > crates/tf-bench/src/catalogue.rs
//! ```

use std::collections::BTreeMap;

use tf_assets::catalogue::{blocs_translucides, classer, textures_citees, Classement, Disposition};
use tf_assets::{Atlas, Catalogue, Dossier};

/// Générateur déterministe : la table ne doit pas changer d'une exécution à
/// l'autre sans qu'on l'ait voulu.
struct Rng(u64);
impl Rng {
    fn suivant(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn dessous(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.suivant() % n as u64) as usize
        }
    }
}

const CIBLE: usize = 220;

fn main() {
    let Some(racine) = std::env::args().nth(1) else {
        eprintln!("usage : engendrer_catalogue <dossier de codex>");
        std::process::exit(2);
    };
    let src = Dossier::ouvrir(&racine).expect("pack lisible");
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).expect("blockstates");
    cat.resoudre_modeles(&src);
    let atlas = Atlas::batir(&src, textures_citees(&cat), &|n| {
        Disposition::Codex.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &atlas);
    eprintln!(
        "{} blocs, {} modèles, {} introuvables, {} translucides",
        cat.nb_blocs(),
        cat.nb_modeles(),
        cat.introuvables.len(),
        translucides.len()
    );

    // ── recenser la cible
    let mut lignes: Vec<(String, &'static str, usize)> = Vec::new();
    for (nom, _) in cat.blocs() {
        if !nom.starts_with("minefield:") {
            continue;
        }
        let Some(m) = cat.modele_de(nom) else {
            continue;
        };
        let classe = match classer(m) {
            // Un cube que ses textures trouent ne bouche pas sa case : il passe
            // par la passe de modèles, comme le verre.
            Classement::Cube if !translucides.contains(nom) => "Cube",
            Classement::Vide => "Vide",
            _ => "Modele",
        };
        lignes.push((nom.clone(), classe, m.elements.len()));
    }
    lignes.sort();

    let total = lignes.len();
    let mut par_classe: BTreeMap<&str, Vec<&(String, &str, usize)>> = BTreeMap::new();
    for l in &lignes {
        par_classe.entry(l.1).or_default().push(l);
    }

    // ── échantillonner, STRATIFIÉ par nombre de cuboïdes
    let mut rng = Rng(1789);
    let mut choisis: Vec<&(String, &str, usize)> = Vec::new();
    for (_, pool) in par_classe.iter() {
        let quota = ((CIBLE * pool.len()) as f64 / total as f64).round() as usize;
        let quota = quota.max(1).min(pool.len());
        let mut par_n: BTreeMap<usize, Vec<&&(String, &str, usize)>> = BTreeMap::new();
        for l in pool {
            par_n.entry(l.2).or_default().push(l);
        }
        let strates: Vec<usize> = par_n.keys().copied().collect();
        let mut reste = quota;
        for (i, n) in strates.iter().enumerate() {
            let grp = &par_n[n];
            let mut part = ((quota * grp.len()) as f64 / pool.len() as f64).round() as usize;
            if i + 1 == strates.len() {
                part = part.max(1);
            }
            part = part.min(grp.len()).min(reste);
            let mut dispo: Vec<usize> = (0..grp.len()).collect();
            for _ in 0..part {
                let k = rng.dessous(dispo.len());
                choisis.push(grp[dispo.swap_remove(k)]);
                reste -= 1;
            }
        }
    }
    choisis.sort();
    choisis.dedup_by(|a, b| a.0 == b.0);

    // ── statistiques, pour l'en-tête
    let stat = |v: &[&(String, &str, usize)], c: &str| v.iter().filter(|l| l.1 == c).count();
    let refs: Vec<&(String, &str, usize)> = lignes.iter().collect();
    let cub_codex: Vec<usize> = lignes
        .iter()
        .filter(|l| l.1 == "Modele")
        .map(|l| l.2)
        .collect();
    let cub_ici: Vec<usize> = choisis
        .iter()
        .filter(|l| l.1 == "Modele")
        .map(|l| l.2)
        .collect();
    let moy = |v: &[usize]| v.iter().sum::<usize>() as f64 / v.len().max(1) as f64;
    let med = |v: &[usize]| {
        let mut s = v.to_vec();
        s.sort_unstable();
        s.get(s.len() / 2).copied().unwrap_or(0)
    };
    let pire_codex = cub_codex.iter().max().copied().unwrap_or(0);
    let pire_nom = lignes
        .iter()
        .filter(|l| l.1 == "Modele")
        .max_by_key(|l| l.2)
        .map(|l| l.0.clone())
        .unwrap_or_default();

    let pct = |n: usize, d: usize| 100.0 * n as f64 / d as f64;
    println!("//! Catalogue Minefield — table ENGENDRÉE, à ne pas éditer à la main.");
    println!("//!");
    println!("//! Produite par `cargo run -p tf-assets --example engendrer_catalogue`,");
    println!("//! qui lit le pack du serveur et le classe avec");
    println!("//! `tf_assets::catalogue::classer` — le MÊME code que l'application. Un");
    println!("//! script qui rejouerait la règle de son côté finirait par diverger, et");
    println!("//! c'est arrivé : 16 désaccords sur 220 blocs, dont onze cubes que le pack");
    println!("//! dit translucides.");
    println!("//!");
    println!("//! Ce ne sont que des NOMS et des formes — ni textures ni modèles ne sont");
    println!("//! recopiés ici. C'est ce qui permet de construire un build réaliste sans");
    println!("//! redistribuer quoi que ce soit.");
    println!("//!");
    println!("//! ## Pourquoi une table réelle plutôt que `bloc_0 … bloc_n`");
    println!("//!");
    println!("//! Les fixtures de la phase 0 (pierre et minerais, cubes pleins, sections");
    println!("//! homogènes) sont justes pour mesurer le chargement Anvil et **mentent**");
    println!("//! pour le maillage : elles donnent 100 % de cubes pleins là où la cible en");
    println!("//! a un tiers. Les longueurs de nom comptent aussi — une palette de trente");
    println!("//! entrées à 31 caractères ne pèse pas ce qu'une palette de");
    println!("//! `minecraft:stone` pèse.");
    println!("//!");
    println!("//! ## Ce que l'échantillon préserve");
    println!("//!");
    println!("//! | | pack | ici |");
    println!("//! |---|---:|---:|");
    for c in ["Modele", "Cube", "Vide"] {
        println!(
            "//! | `{c}` | {} / {total} ({:.1} %) | {} / {} ({:.1} %) |",
            stat(&refs, c),
            pct(stat(&refs, c), total),
            stat(&choisis, c),
            choisis.len(),
            pct(stat(&choisis, c), choisis.len())
        );
    }
    println!(
        "//! | cuboïdes par modèle, moyenne | {:.2} | {:.2} |",
        moy(&cub_codex),
        moy(&cub_ici)
    );
    println!(
        "//! | cuboïdes par modèle, médiane | {} | {} |",
        med(&cub_codex),
        med(&cub_ici)
    );
    println!(
        "//! | le pire | {pire_codex} | {} |",
        cub_ici.iter().max().copied().unwrap_or(0)
    );
    println!("//!");
    println!("//! Un `Cube` que ses textures TROUENT compte comme modèle : le verre remplit");
    println!("//! son bloc et ne masque personne. Seules les faces du cuboïde qui remplit");
    println!("//! la case entrent dans ce jugement — la couche d'herbe transparente de");
    println!("//! `grass_block` ne rend pas le terrain translucide.");
    println!();
    println!("/// Ce qu'un bloc oppose au mailleur.");
    println!("///");
    println!("/// C'est la SEULE chose que le maillage a besoin de savoir d'un bloc. Un cube");
    println!("/// plein opaque bouche sa case, donc masque les faces de ses voisins et se");
    println!("/// fond dans un quad glouton ; tout le reste doit être dessiné cuboïde par");
    println!("/// cuboïde et ne masque rien.");
    println!("#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]");
    println!("pub enum Forme {{");
    println!("    /// Un cuboïde remplit le bloc, et ses textures ne le trouent pas.");
    println!("    Cube,");
    println!("    /// Des cuboïdes partiels, ou un cube que ses textures trouent.");
    println!("    Modele,");
    println!("    /// Aucun élément : fumées, effets. Rien à mailler.");
    println!("    Vide,");
    println!("}}");
    println!();
    println!("impl Forme {{");
    println!("    /// Un bloc-modèle n'est JAMAIS opaque. Le marquer opaque supprimerait les");
    println!("    /// faces de ses voisins : un escalier creuserait un trou dans le mur qu'il");
    println!("    /// touche.");
    println!("    pub const fn opaque(self) -> bool {{");
    println!("        matches!(self, Forme::Cube)");
    println!("    }}");
    println!("}}");
    println!();
    println!("/// `(nom, forme, nombre de cuboïdes)`.");
    println!("pub const BLOCS: &[(&str, Forme, u8)] = &[");
    for (nom, classe, n) in &choisis {
        println!("    (\"{nom}\", Forme::{classe}, {}),", (*n).min(255));
    }
    println!("];");
    println!();
    println!(
        "/// Le bloc le plus lourd du serveur : **{pire_codex} cuboïdes** dans une seule case."
    );
    println!("///");
    println!(
        "/// Il n'est PAS dans `BLOCS`, et c'est délibéré. Un sur {}, l'y glisser",
        stat(&refs, "Modele")
    );
    println!("/// tirerait la moyenne et ferait porter le pire cas par tous les chiffres.");
    println!("/// Les benchs moyens utilisent `BLOCS` ; le bench du pire cas utilise");
    println!("/// celui-ci.");
    println!(
        "pub const PIRE_CAS: (&str, Forme, u8) = (\"{pire_nom}\", Forme::Modele, {pire_codex});"
    );
    println!();
    println!("/// Nombre moyen de cuboïdes d'un bloc-modèle DANS LE PACK, × 100.");
    println!("///");
    println!("/// Figé ici pour qu'un test puisse refuser un échantillon qui dérive. Une");
    println!("/// fixture dont la forme s'éloigne de la cible mesure autre chose que la");
    println!("/// cible, et ne le dit pas.");
    println!(
        "pub const CUBOIDES_MOYENS_CODEX: u32 = {};",
        (moy(&cub_codex) * 100.0).round() as u32
    );
}
