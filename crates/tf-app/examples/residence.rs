//! **Ce qu'une cellule coûte à rendre résidente** — avant d'écrire le
//! streaming, pas après.
//!
//! La phase 5 promet « monde de 800 régions, vol continu, aucune pause > 8 ms
//! sur le fil principal ». Ce chiffre ne se tient pas en espérant : il se
//! budgète. Et ce dépôt a déjà payé trois fois le piège n° 1 — *découper une
//! chaîne AVANT de choisir quoi accélérer n'est pas une précaution*. La
//! dernière fois, le remaillage incrémental gagnait × 1,1 parce que toute la
//! dépense était dans la relecture.
//!
//! On mesure donc, sur de vraies régions écrites sur disque :
//!
//! 1. ce que coûte une région entière, phase par phase ;
//! 2. ce que coûte UN chunk pris seul — c'est-à-dire ce qu'un chargeur qui
//!    streame au chunk paierait en frais fixes à chaque appel ;
//! 3. ce qu'une région laisse RÉSIDENT, en octets, puisque la fenêtre de
//!    résidence est plafonnée en octets et pas en cellules.
//!
//! **Les DEUX fixtures, et c'est le point.** `Terrain` (du sous-sol) mesure
//! Anvil, `Build` (un bâtiment) mesure le rendu — les confondre fausse tout,
//! et c'est écrit dans `docs/fixtures.md`. Sur du sous-sol, presque toutes les
//! sections sont homogènes : elles se maillent en six quads et le maillage
//! résident tombe à un quarantième de la grille. Publier ce chiffre comme
//! « ce que coûte le maillage » serait exactement l'erreur que ce dépôt
//! s'interdit. On mesure donc les deux, et on les NOMME.
//!
//! Et on maille avec `mailler_parallele`, parce que c'est ce que la coque
//! appelle. Mesurer un chemin que l'application ne prend pas donne un budget
//! qui ne la concerne pas.
//!
//! ```text
//! cargo run --release -p tf-app --example residence
//! cargo run --release -p tf-app --example residence -- 16     # côté en chunks
//! ```

use std::collections::HashMap;
use std::time::Instant;

use tf_anvil::{Interner, StateId};
use tf_bench::catalogue::{Forme, BLOCS};
use tf_mesh::{Cuboide, Grille, TableFormes};
use tf_world::coords::BlockPos;
use tf_world::RegionSource;

/// La table de formes du catalogue, sans pack : `tf-bench` la porte, et elle
/// est ENGENDRÉE par le même code que l'application (le piège des deux
/// implémentations d'une même règle a déjà coûté 16 désaccords sur 220 blocs).
fn table_de(interner: &Interner) -> TableFormes {
    let formes: HashMap<&str, (Forme, u8)> = BLOCS.iter().map(|(n, f, c)| (*n, (*f, *c))).collect();
    let mut t = TableFormes::new();
    for id in 0..interner.len() as StateId {
        let cle = interner.resolve(id).unwrap_or("minecraft:air");
        let nu = cle.split('|').next().unwrap_or(cle);
        match nu {
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" => {
                t.pousser(true, false, Vec::new())
            }
            _ => match formes.get(nu) {
                Some((Forme::Modele, n)) => t.pousser(false, false, cuboides(*n)),
                // Inconnu du catalogue : un cube plein. C'est le cas le plus
                // FRÉQUENT sur du terrain, et le plus cher à mailler.
                _ => t.pousser(false, true, Vec::new()),
            },
        };
    }
    t
}

/// `n` cuboïdes empilés, comme la table du bench de maillage.
fn cuboides(n: u8) -> Vec<Cuboide> {
    (0..n)
        .map(|k| {
            let t = n.max(1) as i32;
            let bas = (k as i32 * 16 / t) as f32;
            let haut = (((k as i32 + 1) * 16 / t).max(bas as i32 + 1).min(16)) as f32;
            Cuboide {
                min: [0.0, bas, 0.0],
                max: [16.0, haut, 16.0],
                faces: 0x3F,
                cull: 0x3F,
            }
        })
        .collect()
}

/// Les octets qu'une grille tient : palette et indices packés, par section.
///
/// C'est la même comptabilité que demandera `Weighed` quand la grille entrera
/// dans la fenêtre de résidence — approximative, mais STABLE.
fn octets_grille(g: &Grille) -> usize {
    g.adresses()
        .iter()
        .filter_map(|a| g.section(*a))
        .map(|s| {
            std::mem::size_of::<tf_anvil::Section>()
                + s.palette.len() * std::mem::size_of::<StateId>()
                + s.data.len() * 8
        })
        .sum()
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// Ce qu'une région a coûté et ce qu'elle laisse derrière.
struct Releve {
    nom: &'static str,
    disque: usize,
    chunks: usize,
    sections: usize,
    lire: std::time::Duration,
    decoder: std::time::Duration,
    sequentiel: std::time::Duration,
    parallele: std::time::Duration,
    grille: usize,
    maillage: usize,
    quads: usize,
    poses: usize,
}

impl Releve {
    /// Le coût qu'un chargeur paie vraiment : lecture, décodage, maillage
    /// PARALLÈLE — le chemin de la coque.
    fn total(&self) -> f64 {
        ms(self.lire) + ms(self.decoder) + ms(self.parallele)
    }
    fn resident(&self) -> usize {
        self.grille + self.maillage
    }
}

/// Écrit la région, la relit par le chemin de l'application, et chronomètre.
fn mesurer(nom: &'static str, octets: &[u8], dir: &std::path::Path, cote: u32) -> Releve {
    std::fs::write(
        dir.join("region")
            .join(tf_anvil::region::region_file_name(0, 0)),
        octets,
    )
    .expect("écriture");

    let src = tf_world::FsSource::open(dir).expect("source");
    let dim = tf_world::Dimension::Overworld;
    let sel = tf_world::BBox::new(
        BlockPos::new(0, -64, 0),
        BlockPos::new(cote as i32 * 16 - 1, 319, cote as i32 * 16 - 1),
    );

    let t0 = Instant::now();
    let brut = src
        .read_region(
            &dim,
            tf_world::Folder::Region,
            tf_world::coords::RegionPos { x: 0, z: 0 },
        )
        .expect("région lisible");
    let lire = t0.elapsed();
    let disque = brut.len();
    drop(brut);

    let mut interner = Interner::new();
    let mut grille = Grille::new();
    let t0 = Instant::now();
    let bilan = tf_world::sections_de(
        &src,
        &dim,
        tf_world::Folder::Region,
        &sel,
        &mut interner,
        |s| {
            if let Some(b) = s.biomes {
                grille.poser_biomes(s.chunk.x, s.chunk.z, s.section.y, b);
            }
            grille.poser(s.chunk.x, s.chunk.z, s.section);
        },
    );
    let decoder = t0.elapsed();

    let table = table_de(&interner);
    let t0 = Instant::now();
    let seq = grille.mailler(&table);
    let sequentiel = t0.elapsed();
    let t0 = Instant::now();
    let chantier = grille.mailler_parallele(&table);
    let parallele = t0.elapsed();
    assert_eq!(
        seq.quads(),
        chantier.quads(),
        "séquentiel et parallèle doivent rendre le MÊME maillage"
    );

    Releve {
        nom,
        disque,
        chunks: bilan.chunks,
        sections: bilan.sections,
        lire,
        decoder,
        sequentiel,
        parallele,
        grille: octets_grille(&grille),
        maillage: chantier.octets(),
        quads: chantier.quads(),
        poses: chantier.poses(),
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let cote: u32 = a.next().and_then(|s| s.parse().ok()).unwrap_or(32);

    let dir = std::env::temp_dir().join(format!("tf-residence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("region")).expect("dossier créable");

    let terrain = tf_bench::Terrain {
        side: cote,
        biomes: true,
        ..Default::default()
    };
    let batiment = tf_bench::Build {
        side: cote,
        ..Default::default()
    };
    println!(
        "Une région de {cote}×{cote} chunks × {} sections, les DEUX fixtures.\n\
         `Terrain` mesure Anvil, `Build` mesure le rendu — les confondre fausse tout.\n",
        terrain.sections
    );

    let releves = [
        mesurer("Terrain", &tf_bench::region(&terrain), &dir, cote),
        mesurer("Build", &tf_bench::build::region(&batiment), &dir, cote),
    ];

    // ---- 1. Phase par phase -------------------------------------------
    println!(
        "  {:<9} {:>7} {:>8} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "fixture", "disque", "chunks", "sections", "lire", "décoder", "mailler", "TOTAL"
    );
    for r in &releves {
        println!(
            "  {:<9} {:>5.1} Mo {:>8} {:>9} {:>6.1} ms {:>6.1} ms {:>6.1} ms {:>6.1} ms",
            r.nom,
            r.disque as f64 / 1e6,
            r.chunks,
            r.sections,
            ms(r.lire),
            ms(r.decoder),
            ms(r.parallele),
            r.total()
        );
    }
    println!();
    for r in &releves {
        println!(
            "  {:<9} maillage séquentiel {:>6.1} ms contre {:>6.1} ms en parallèle (× {:.1})",
            r.nom,
            ms(r.sequentiel),
            ms(r.parallele),
            ms(r.sequentiel) / ms(r.parallele).max(1e-6)
        );
    }
    println!();

    // ---- 2. Ce qui reste RÉSIDENT -------------------------------------
    println!("Résident par région — la fenêtre est plafonnée en OCTETS :");
    println!(
        "  {:<9} {:>9} {:>10} {:>9} {:>11} {:>9}",
        "fixture", "grille", "maillage", "TOTAL", "par chunk", "quads"
    );
    for r in &releves {
        println!(
            "  {:<9} {:>6.1} Mo {:>7.1} Mo {:>6.1} Mo {:>8.0} ko {:>9}",
            r.nom,
            r.grille as f64 / 1e6,
            r.maillage as f64 / 1e6,
            r.resident() as f64 / 1e6,
            r.resident() as f64 / 1e3 / r.chunks.max(1) as f64,
            r.quads + r.poses
        );
    }
    println!();

    // ---- 3. Un chunk pris SEUL : les frais fixes ----------------------
    // C'est la question qui décide l'unité de streaming. Charger au chunk
    // relit le .mca à chaque appel ; si ce coût domine, l'unité doit être la
    // RÉGION, et un chargeur au chunk serait une fausse bonne idée.
    let src = tf_world::FsSource::open(&dir).expect("source");
    let dim = tf_world::Dimension::Overworld;
    let mut seuls: Vec<f64> = Vec::new();
    for (cx, cz) in [(0, 0), (1, 0), (0, 1), (5, 5), (7, 3)] {
        if cx >= cote as i32 || cz >= cote as i32 {
            continue;
        }
        let un = tf_world::BBox::new(
            BlockPos::new(cx * 16, -64, cz * 16),
            BlockPos::new(cx * 16 + 15, 319, cz * 16 + 15),
        );
        let mut i2 = Interner::new();
        let mut g2 = Grille::new();
        let t0 = Instant::now();
        let b2 = tf_world::sections_de(&src, &dim, tf_world::Folder::Region, &un, &mut i2, |s| {
            g2.poser(s.chunk.x, s.chunk.z, s.section);
        });
        let d = t0.elapsed();
        assert_eq!(b2.chunks, 1, "la sélection d'un chunk doit en rendre UN");
        seuls.push(ms(d));
    }
    seuls.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let seul = seuls[seuls.len() / 2];
    // Le dernier relevé est celui dont les octets sont encore sur le disque.
    let dernier = releves.last().expect("un relevé");
    let amorti = ms(dernier.decoder) / dernier.chunks.max(1) as f64;
    println!(
        "Un chunk pris SEUL, sur {} (le .mca est relu à chaque appel) :",
        dernier.nom
    );
    println!(
        "  {:.2} ms médiane sur {} — contre {:.3} ms amorti sur la région, soit × {:.0}",
        seul,
        seuls.len(),
        amorti,
        seul / amorti.max(1e-6)
    );
    println!(
        "  streamer au chunk gaspillerait donc {:.0} ms par région en relectures",
        (seul - amorti) * dernier.chunks as f64
    );
    println!();

    // ---- 4. Le budget d'image -----------------------------------------
    println!("Budget d'une image à 8 ms :");
    for r in &releves {
        let par_chunk = r.total() / r.chunks.max(1) as f64;
        println!(
            "  {:<9} {:>5.1} chunks par image · une région = {:>4.0} images · \
             2 Go de résidence = {:>3.0} régions",
            r.nom,
            8.0 / par_chunk,
            r.total() / 8.0,
            2e9 / r.resident().max(1) as f64
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
