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
//! cargo run --release -p tf-app --example vol -- --gpu synchro
//! ```
//!
//! **`--gpu`** ajoute à chaque image ce que la coque fait après une arrivée :
//! `refaire` reconstruit la scène GPU entière (l'ancienne `regarnir`),
//! `synchro` n'envoie que ce que les arènes ont réécrit, `aucun` s'arrête aux
//! arènes. Sans l'option, les trois passent, si un adaptateur est là. Le
//! temps mesuré est celui du fil principal — copies vers les tampons de
//! transfert, créations, compilations — pas celui du GPU, qui travaille à
//! côté ; sous un pilote logiciel, ce dernier ne vaudrait de toute façon rien.
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
use tf_render::{Appareil, AtlasGpu, Scene};
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

/// Ce que la fenêtre fait au GPU après une arrivée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gpu {
    /// Rien : la mesure s'arrête aux arènes.
    Aucun,
    /// L'ancienne `regarnir` : atlas remonté, scène refaite, tout renvoyé.
    Refaire,
    /// `Scene::synchroniser` : seulement ce que les arènes ont réécrit.
    Synchro,
}

fn monter_atlas(app: &Appareil, a: &tf_assets::Atlas) -> AtlasGpu {
    AtlasGpu::avec_mips(app, a.cote, a.len() as u32, &a.pyramide())
}

/// La scène GPU d'un vol, et la clé de l'atlas qu'elle a monté.
struct CoteGpu<'a> {
    app: &'a Appareil,
    scene: Scene,
    atlas: (u32, usize, u32),
}

impl CoteGpu<'_> {
    /// Ce que fait la coque après une arrivée. Rend les octets envoyés.
    fn regarnir(&mut self, mode: Gpu, o: &mut Ouvert) -> u64 {
        match mode {
            Gpu::Aucun => 0,
            Gpu::Refaire => {
                let atlas = monter_atlas(self.app, &o.monde.atlas);
                self.scene = Scene::pour(
                    self.app,
                    &o.monde.arene,
                    &o.monde.modeles,
                    &atlas,
                    tf_render::scene::FORMAT,
                );
                // Une scène neuve : son compteur est TOUT ce qu'elle a reçu.
                self.scene.octets_envoyes()
            }
            Gpu::Synchro => {
                let cle = (o.rechargements, o.monde.atlas.len(), o.monde.atlas.cote);
                if cle != self.atlas {
                    self.scene
                        .changer_atlas(&monter_atlas(self.app, &o.monde.atlas));
                    self.atlas = cle;
                }
                self.scene
                    .synchroniser(&mut o.monde.arene, &mut o.monde.modeles)
            }
        }
    }
}

/// Ce qu'un vol a coûté.
struct Vol {
    images: Profil,
    /// Le temps de la seule étape GPU, sur les images où elle a eu lieu.
    gpu: Option<Profil>,
    cellules: usize,
    resident: usize,
    envoyes: u64,
}

fn voler(
    pack: &str,
    monde: &str,
    rayon: u32,
    pas: i32,
    images: usize,
    gpu: Option<(&Appareil, Gpu)>,
) -> Vol {
    let mut o = Ouvert::ouvrir(pack, Some(monde), [0, 0, 0, 0]).expect("monde ouvert");
    let mut p = Pilote::pour(&o, Dimension::Overworld, Niveau::Chunk, rayon, HAUTEUR);
    let mode = gpu.map_or(Gpu::Aucun, |(_, m)| m);
    let mut cote = gpu.map(|(app, _)| {
        let mut scene = Scene::vide(
            app,
            &monter_atlas(app, &o.monde.atlas),
            tf_render::scene::FORMAT,
        );
        scene.synchroniser(&mut o.monde.arene, &mut o.monde.modeles);
        CoteGpu {
            app,
            scene,
            atlas: (o.rechargements, o.monde.atlas.len(), o.monde.atlas.cote),
        }
    });
    let mut temps = Vec::with_capacity(images);
    let mut arrivees = 0;
    let mut envoyes = 0u64;
    let mut temps_gpu = Vec::new();
    for i in 0..images {
        let oeil = BlockPos::new(8 + i as i32 * pas, 64, 8 + COTE as i32 * 8);
        let t = Instant::now();
        let f = p
            .image(&mut o, oeil, EST, CELLULES_PAR_IMAGE)
            .expect("une image");
        if let Some(c) = &mut cote {
            let grandi = c.scene.agrandissements();
            if f.a_change() {
                let g = Instant::now();
                envoyes += c.regarnir(mode, &mut o);
                temps_gpu.push(ms(g.elapsed()));
                tf_app::scene::phase("gpu", g);
            }
            // Une soumission par image, comme la fenêtre : c'est elle qui
            // vide les écritures en attente. Sans elle, elles
            // s'accumuleraient et la mesure les paierait toutes à la fin.
            let g = Instant::now();
            c.app.queue.submit([]);
            c.app.device.poll(wgpu::Maintain::Poll);
            tf_app::scene::phase("soumettre", g);
            if c.scene.agrandissements() > grandi {
                tf_app::scene::phase_texte(&format!(
                    "tampons agrandis : {:?}",
                    c.scene.capacites()
                ));
            }
        }
        let d = t.elapsed();
        // Sous `TF_PHASES`, une ligne par IMAGE après ses phases : ce qui
        // s'imprime entre deux de ces lignes appartient à la seconde, et
        // c'est ce qui permet d'attribuer un pic à ce qui l'a causé.
        tf_app::scene::phase_texte(&format!(
            "IMAGE {i} {:.2} ms · {} arrivées · {} dégagées · {} sections remaillées · {} posées",
            ms(d),
            f.arrivees,
            f.degagees,
            if f.a_change() {
                o.sections_remaillees
            } else {
                0
            },
            f.posees
        ));
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
    Vol {
        images: profil(temps, 8.0),
        gpu: (!temps_gpu.is_empty()).then(|| profil(temps_gpu, 8.0)),
        cellules: arrivees,
        resident,
        envoyes,
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let mut rayon = 6u32;
    let mut pack: Option<String> = None;
    let mut modes = vec![Gpu::Aucun, Gpu::Refaire, Gpu::Synchro];
    while let Some(o) = a.next() {
        match o.as_str() {
            "--gpu" => {
                modes = match a.next().as_deref() {
                    Some("aucun") => vec![Gpu::Aucun],
                    Some("refaire") => vec![Gpu::Refaire],
                    Some("synchro") => vec![Gpu::Synchro],
                    autre => {
                        eprintln!("--gpu {autre:?} : aucun, refaire ou synchro");
                        return;
                    }
                }
            }
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
    let app = if modes.iter().any(|m| *m != Gpu::Aucun) {
        match Appareil::ouvrir() {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("pas d'adaptateur ({e}) : la mesure s'arrête aux arènes");
                modes = vec![Gpu::Aucun];
                None
            }
        }
    } else {
        None
    };
    println!(
        "  {:<9} {:<8} {:>8} {:>9} {:>9} {:>9} {:>8} {:>6} {:>9} {:>9}  {:>16}",
        "fixture",
        "gpu",
        "disque",
        "médiane",
        "p95",
        "pire",
        "> 8 ms",
        "cell.",
        "résident",
        "envoyé",
        "étape GPU méd/pire"
    );

    for (nom, bati) in [("Terrain", false), ("Build", true)] {
        let dir = racine.join(nom);
        let _ = std::fs::remove_dir_all(&dir);
        let disque = semer(&dir, bati);
        for &mode in &modes {
            let v = voler(
                &pack,
                dir.to_str().expect("chemin lisible"),
                rayon,
                // Quatre blocs par image : 1 600 blocs, soit cent chunks, ce
                // qui traverse les deux régions.
                4,
                400,
                app.as_ref()
                    .filter(|_| mode != Gpu::Aucun)
                    .map(|a| (a, mode)),
            );
            // **La prémisse, vérifiée.** Une mesure sans cellule posée ne dit
            // rien du coût d'une image de vol — c'est ce qu'a rendu le premier
            // essai, et il avait l'air excellent.
            assert!(
                v.cellules > 100,
                "{nom} : seulement {} cellules posées — la mesure ne mesure pas le vol",
                v.cellules
            );
            let p = &v.images;
            let gpu = v.gpu.as_ref().map_or("—".to_string(), |g| {
                format!("{:.2} / {:.1} ms", g.mediane, g.pire)
            });
            println!(
                "  {:<9} {:<8} {:>5.1} Mo {:>6.2} ms {:>6.2} ms {:>6.1} ms {:>4}/{:<3} {:>6} {:>6.1} Mo {:>6.0} Mo  {:>16}",
                nom,
                format!("{mode:?}").to_lowercase(),
                disque as f64 / 1e6,
                p.mediane,
                p.p95,
                p.pire,
                p.au_dela,
                p.n,
                v.cellules,
                v.resident as f64 / 1e6,
                v.envoyes as f64 / 1e6,
                gpu
            );
        }
    }
    println!();
    println!(
        "La MÉDIANE ne dit pas ce qu'on voit : une image sur vingt à 40 ms se \n\
         remarque, une médiane à 2 ms ne se remarque pas. C'est la colonne \n\
         « > 8 ms » qui porte la promesse de la phase 5."
    );
    let _ = std::fs::remove_dir_all(&racine);
}
