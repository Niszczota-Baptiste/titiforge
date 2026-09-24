//! **Ce qu'une image de VOL coûte**, sur du bâti et sur du terrain.
//!
//! La phase 5 promet « aucune pause > 8 ms sur le fil principal ». Ce chiffre
//! se mesure, il ne s'affirme pas — et ce dépôt a payé trois fois le piège
//! n° 1, *découper une chaîne avant de savoir où part le temps*. La dernière
//! fois, le remaillage incrémental gagnait × 1,1 parce que toute la dépense
//! était ailleurs.
//!
//! On fait donc voler une caméra sur un monde écrit sur disque, par le MÊME
//! chemin que la coque (`tf_app::pilote`), et on relève la distribution des
//! temps d'image. La médiane ne suffit pas : ce qui gâche une application,
//! c'est la QUEUE de distribution — une image sur vingt à 40 ms se voit,
//! une médiane à 2 ms ne se voit pas.
//!
//! **Les deux fixtures, et c'est le point.** `Terrain` mesure Anvil, `Build`
//! mesure le rendu. Une image de vol traverse les deux : on lit et on décode
//! (Anvil), puis on maille et on recopie les arènes (rendu). Ne mesurer que
//! l'une donnerait un budget qui ne concerne pas l'application.
//!
//! ```text
//! cargo run --release -p tf-app --example vol
//! cargo run --release -p tf-app --example vol -- --rayon 8
//! cargo run --release -p tf-app --example vol -- --pack <assets>
//! ```
//!
//! Sans `--pack`, le codex MINIMAL des tests est écrit à la volée
//! (`tests/commun`) : il couvre les 221 blocs `minefield:*` du catalogue avec
//! leurs vraies formes, donc la mesure tourne partout — y compris sur une
//! machine qui n'a pas le serveur sous la main. `--pack` la refait sur un vrai
//! pack, un codex ou une installation de launcher.

use std::time::Instant;

#[path = "../tests/commun/mod.rs"]
mod commun;

use tf_app::pilote::{Pilote, CELLULES_PAR_IMAGE};
use tf_app::scene::Ouvert;
use tf_world::coords::BlockPos;
use tf_world::{Dimension, Niveau};

const HAUTEUR: (i32, i32) = (-64, 319);
const EST: [f32; 3] = [1.0, 0.0, 0.0];
/// Côté d'une région écrite, en chunks. Deux régions en x : le vol franchit
/// une frontière de `.mca`, ce qui est le cas où le chargeur travaille.
const COTE: u32 = 32;
/// **La cadence d'une vraie fenêtre**, 60 images par seconde.
///
/// Sans elle la mesure ne mesure rien, et c'est arrivé au premier essai :
/// quatre cents images enchaînées sans attendre durent 4 ms en tout, le fil
/// n'a pas le temps de lire une seule région, et la mesure annonce 0,01 ms
/// par image pour ZÉRO cellule posée. Une vraie fenêtre attend la
/// synchronisation verticale entre deux images, et c'est pendant ce temps-là
/// que le fil travaille.
const IMAGE: std::time::Duration = std::time::Duration::from_micros(16_667);

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// La distribution d'une série de temps d'image.
struct Profil {
    n: usize,
    mediane: f64,
    p95: f64,
    pire: f64,
    /// Images au-dessus du budget.
    au_dela: usize,
}

fn profil(mut v: Vec<f64>, budget: f64) -> Profil {
    v.sort_by(f64::total_cmp);
    let n = v.len().max(1);
    Profil {
        n: v.len(),
        mediane: v[n / 2],
        p95: v[(n * 95 / 100).min(n - 1)],
        pire: *v.last().unwrap_or(&0.0),
        au_dela: v.iter().filter(|t| **t > budget).count(),
    }
}

/// Écrit `2 × 1` régions de la fixture voulue, et rend le dossier.
fn semer(dir: &std::path::Path, bati: bool) -> usize {
    let region = dir.join("region");
    std::fs::create_dir_all(&region).expect("dossier créable");
    let mut octets = 0;
    for rx in 0..2 {
        let brut = if bati {
            tf_bench::build::region_en(
                &tf_bench::Build {
                    side: COTE,
                    ..Default::default()
                },
                rx,
                0,
            )
        } else {
            tf_bench::region_en(
                &tf_bench::Terrain {
                    side: COTE,
                    biomes: true,
                    ..Default::default()
                },
                rx,
                0,
            )
        };
        octets += brut.len();
        std::fs::write(
            region.join(tf_anvil::region::region_file_name(rx, 0)),
            &brut,
        )
        .expect("écriture");
    }
    std::fs::write(dir.join("level.dat"), []).expect("level.dat");
    octets
}

fn voler(pack: &str, monde: &str, rayon: u32, pas: i32, images: usize) -> (Profil, usize, usize) {
    let mut o = Ouvert::ouvrir(pack, Some(monde), [0, 0, 0, 0]).expect("monde ouvert");
    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, rayon, HAUTEUR);
    let mut temps = Vec::with_capacity(images);
    let mut arrivees = 0;
    for i in 0..images {
        let oeil = BlockPos::new(8 + i as i32 * pas, 64, 8 + COTE as i32 * 8);
        let t = Instant::now();
        let f = p
            .image(&mut o, oeil, EST, CELLULES_PAR_IMAGE)
            .expect("une image");
        let d = t.elapsed();
        temps.push(ms(d));
        arrivees += f.arrivees;
        // Le reste de l'image : ce que la fenêtre passerait à attendre la
        // synchronisation verticale.
        if let Some(reste) = IMAGE.checked_sub(d) {
            std::thread::sleep(reste);
        }
    }
    p.arreter();
    let resident = o.octets_residents();
    (profil(temps, 8.0), arrivees, resident)
}

fn main() {
    let mut a = std::env::args().skip(1);
    let mut rayon = 6u32;
    let mut pack: Option<String> = None;
    while let Some(o) = a.next() {
        match o.as_str() {
            "--rayon" => {
                if let Some(r) = a.next().and_then(|r| r.parse().ok()) {
                    rayon = r;
                }
            }
            "--pack" => pack = a.next(),
            autre => eprintln!("option inconnue ignorée : {autre}"),
        }
    }
    // Le codex écrit à la volée vit le temps de la mesure.
    let jetable = commun::Jetable::neuf("vol-codex");
    let pack = pack.unwrap_or_else(|| commun::codex(jetable.chemin(), &[]));

    let racine = std::env::temp_dir().join(format!("tf-vol-{}", std::process::id()));
    println!(
        "Vol de 400 images, rayon {rayon} chunks, {CELLULES_PAR_IMAGE} cellules \
         intégrées par image.\n\
         Deux régions en x : le vol franchit une frontière de `.mca`.\n"
    );
    println!(
        "  {:<9} {:>8} {:>9} {:>9} {:>9} {:>8} {:>10} {:>9}",
        "fixture", "disque", "médiane", "p95", "pire", "> 8 ms", "cellules", "résident"
    );

    for (nom, bati) in [("Terrain", false), ("Build", true)] {
        let dir = racine.join(nom);
        let _ = std::fs::remove_dir_all(&dir);
        let disque = semer(&dir, bati);
        let (p, cellules, resident) = voler(
            &pack,
            dir.to_str().expect("chemin lisible"),
            rayon,
            // Quatre blocs par image : 1 600 blocs, soit cent chunks, ce qui
            // traverse les deux régions.
            4,
            400,
        );
        // **La prémisse, vérifiée.** Une mesure sans cellule posée ne dit
        // rien du coût d'une image de vol — c'est ce qu'a rendu le premier
        // essai, et il avait l'air excellent.
        assert!(
            cellules > 100,
            "{nom} : seulement {cellules} cellules posées — la mesure ne mesure pas le vol"
        );
        println!(
            "  {:<9} {:>5.1} Mo {:>6.2} ms {:>6.2} ms {:>6.1} ms {:>4}/{:<3} {:>10} {:>6.1} Mo",
            nom,
            disque as f64 / 1e6,
            p.mediane,
            p.p95,
            p.pire,
            p.au_dela,
            p.n,
            cellules,
            resident as f64 / 1e6
        );
    }
    println!();
    println!(
        "La MÉDIANE ne dit pas ce qu'on voit : une image sur vingt à 40 ms se \n\
         remarque, une médiane à 2 ms ne se remarque pas. C'est la colonne \n\
         « > 8 ms » qui porte la promesse de la phase 5."
    );
    let _ = std::fs::remove_dir_all(&racine);
}
