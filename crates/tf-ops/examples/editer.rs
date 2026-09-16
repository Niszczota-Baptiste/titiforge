//! Une opération sur un VRAI monde, en ligne de commande.
//!
//! C'est le premier bout de titiforge qu'on peut lancer sur sa propre save.
//! Il ne remplace pas l'application ; il prouve que la chaîne complète tient
//! sur des fichiers que Minecraft a écrits, et pas seulement sur des fixtures.
//!
//! **Une commande par LIGNE.** Windows est la cible, et PowerShell ne connaît
//! pas la continuation `\` d'un shell Unix : il attend la suite et rend une
//! erreur de syntaxe qui ne parle pas du programme.
//!
//! ```text
//! .\editer.exe D:\monde-essai
//! .\editer.exe D:\monde-essai --remplacer minecraft:stone minecraft:dirt --sel "0,-64,0,511,320,511" --compter
//! .\editer.exe D:\monde-essai --poser minecraft:air --sel "0,60,0,15,70,15" --ecrire
//! ```
//!
//! Les guillemets autour de `--sel` ne sont pas décoratifs sous PowerShell :
//! sans eux, `0,-64,0` est lu comme un tableau et recollé avec des espaces.
//! L'outil accepte les deux — mais la documentation montre la forme sûre.
//!
//! **Invariant n° 1 : on ne touche jamais au fichier source.** Sans `--ecrire`,
//! tout vit dans un dossier temporaire et la save n'est même pas ouverte en
//! écriture. Avec `--ecrire`, l'ordre est celui que le projet s'impose :
//! refuser si Minecraft tient le monde, sauvegarder, puis écrire.

use std::path::{Path, PathBuf};

use tf_anvil::Interner;
use tf_ops::edition::appliquer;
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::FsSource;
use tf_world::Staging;

struct Args {
    monde: PathBuf,
    dim: Dimension,
    op: Option<Op>,
    sel: Option<BBox>,
    ecrire: bool,
    compter: bool,
    seed: u64,
}

enum Op {
    Poser(String),
    Remplacer(String, String),
    Melanger(Vec<(u32, String)>),
}

fn usage() -> ! {
    eprintln!(
        "usage : editer <monde> [options]

  --sel \"x1,y1,z1,x2,y2,z2\"  la sélection, en coordonnées MONDE. Les guillemets
                             sous PowerShell : sans eux le shell mange la virgule
  --poser <bloc>             //set
  --remplacer <de> <vers>    //replace
  --melanger p:bloc,p:bloc   un mélange pondéré, ex. 3:minecraft:stone,1:minecraft:dirt
  --dim nether|end           (défaut : le monde principal)
  --seed <n>                 la graine du mélange (défaut 0)
  --compter                  compter les blocs modifiés — coûte 31 × l'opération
  --ecrire                   ÉCRIRE dans la save (sinon : essai à blanc)

Une commande par LIGNE : PowerShell ne connaît pas la continuation \\ d'un shell Unix.

Sans opération, se contente de décrire le monde."
    );
    std::process::exit(2)
}

/// Le séparateur d'une liste passée en argument : la virgule ET l'espace.
///
/// Ce n'est pas de la complaisance. PowerShell lit `0,-64,0,511` en position
/// d'argument comme un TABLEAU et le recolle avec des espaces avant de le
/// passer à l'exe : la virgule a disparu avant que le programme ne voie quoi
/// que ce soit, alors que l'utilisateur a tapé exactement ce que la
/// documentation dit. Windows est la cible ; un outil qui refuse le rendu
/// naturel de son propre shell est un outil cassé.
fn sep(c: char) -> bool {
    c == ',' || c.is_whitespace()
}

fn lire_args() -> Args {
    let mut a = std::env::args().skip(1);
    let monde = PathBuf::from(a.next().unwrap_or_else(|| usage()));
    let mut args = Args {
        monde,
        dim: Dimension::Overworld,
        op: None,
        sel: None,
        ecrire: false,
        compter: false,
        seed: 0,
    };
    while let Some(o) = a.next() {
        match o.as_str() {
            "--sel" => {
                let v: Vec<i32> = a
                    .next()
                    .unwrap_or_else(|| usage())
                    .split(sep)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse().unwrap_or_else(|_| usage()))
                    .collect();
                if v.len() != 6 {
                    usage();
                }
                args.sel = Some(BBox::new(
                    BlockPos {
                        x: v[0],
                        y: v[1],
                        z: v[2],
                    },
                    BlockPos {
                        x: v[3],
                        y: v[4],
                        z: v[5],
                    },
                ));
            }
            "--poser" => args.op = Some(Op::Poser(a.next().unwrap_or_else(|| usage()))),
            "--remplacer" => {
                let de = a.next().unwrap_or_else(|| usage());
                let vers = a.next().unwrap_or_else(|| usage());
                args.op = Some(Op::Remplacer(de, vers));
            }
            "--melanger" => {
                let v = a.next().unwrap_or_else(|| usage());
                let entrees = v
                    .split(sep)
                    .filter(|e| !e.is_empty())
                    .map(|e| {
                        let (p, b) = e.split_once(':').unwrap_or_else(|| usage());
                        (p.trim().parse().unwrap_or_else(|_| usage()), b.to_string())
                    })
                    .collect();
                args.op = Some(Op::Melanger(entrees));
            }
            "--dim" => {
                args.dim = match a.next().unwrap_or_else(|| usage()).as_str() {
                    "nether" => Dimension::Nether,
                    "end" => Dimension::End,
                    _ => Dimension::Overworld,
                }
            }
            "--seed" => args.seed = a.next().unwrap_or_else(|| usage()).parse().unwrap_or(0),
            "--compter" => args.compter = true,
            "--ecrire" => args.ecrire = true,
            _ => usage(),
        }
    }
    args
}

/// Une copie de sauvegarde du monde, à côté de lui.
///
/// **Invariant n° 6 : aucune écriture sans sauvegarde préalable**, et dans cet
/// ordre. Une sauvegarde prise après la première écriture ne sauvegarde plus
/// rien.
fn sauvegarder(monde: &Path) -> Result<PathBuf, String> {
    let quand = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let nom = format!(
        "{}.sauvegarde-{quand}",
        monde
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("monde")
    );
    let vers = monde.with_file_name(nom);
    copier_dossier(monde, &vers).map_err(|e| e.to_string())?;
    Ok(vers)
}

fn copier_dossier(de: &Path, vers: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(vers)?;
    for e in std::fs::read_dir(de)? {
        let e = e?;
        let cible = vers.join(e.file_name());
        if e.file_type()?.is_dir() {
            copier_dossier(&e.path(), &cible)?;
        } else {
            std::fs::copy(e.path(), &cible)?;
        }
    }
    Ok(())
}

fn main() {
    let args = lire_args();
    let src: FsSource = match FsSource::open(&args.monde) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("monde illisible : {e:?}");
            if !FsSource::looks_like_world(&args.monde) {
                eprintln!("(ce dossier ne ressemble pas à une save : pas de `region/`)");
            }
            std::process::exit(1);
        }
    };

    // Une save ne s'ouvre JAMAIS en entier : l'aperçu ne lit que les noms et
    // les tailles de fichiers.
    println!("monde : {}", args.monde.display());
    for dim in src.dimensions().unwrap_or_default() {
        for folder in Folder::ALL {
            let Ok(ov) = src.overview(&dim, folder) else {
                continue;
            };
            if ov.regions.is_empty() {
                continue;
            }
            let octets: u64 = ov.regions.iter().map(|r| r.bytes).sum();
            // Un fichier plus court que l'en-tête Anvil n'est pas une région,
            // quel que soit son nom. On le dit ICI plutôt que de laisser
            // l'utilisateur lire « 0,0 Mo » et chercher pourquoi.
            let tronquees: Vec<String> = ov
                .regions
                .iter()
                .filter(|r| r.est_tronquee())
                .map(|r| format!("r.{}.{}.mca ({} o)", r.pos.x, r.pos.z, r.bytes))
                .collect();
            let (x0, x1) = (
                ov.regions.iter().map(|r| r.pos.x).min().unwrap(),
                ov.regions.iter().map(|r| r.pos.x).max().unwrap(),
            );
            let (z0, z1) = (
                ov.regions.iter().map(|r| r.pos.z).min().unwrap(),
                ov.regions.iter().map(|r| r.pos.z).max().unwrap(),
            );
            println!(
                "  {:12} {:5} régions · {:8.1} Mo · x {x0}..{x1} · z {z0}..{z1} (blocs {}..{})",
                format!("{}/{}", dim.label(), folder.dir_name()),
                ov.regions.len(),
                octets as f64 / 1e6,
                x0 * 512,
                x1 * 512 + 511
            );
            if !tronquees.is_empty() {
                println!(
                    "  ATTENTION : {} fichier(s) plus court(s) que l'en-tête Anvil de 8 Kio, \
                     donc pas des régions — {}",
                    tronquees.len(),
                    tronquees.join(", ")
                );
                println!(
                    "             (un `.mca` qui commence par `bplist00` est un ALIAS macOS \
                     ou iOS : le fichier n'a pas été envoyé, seulement un raccourci vers lui)"
                );
            }
        }
    }

    let verrou = src.probe_lock();
    println!(
        "verrou : {}",
        match (verrou.locked, verrou.reliable) {
            (true, _) => "Minecraft TIENT ce monde — aucune écriture possible",
            (false, true) => "libre",
            (false, false) => "indéterminé (verrou consultatif hors Windows)",
        }
    );

    let (Some(op), Some(sel)) = (args.op, args.sel) else {
        println!("\n(pas d'opération demandée — voir --poser / --remplacer / --melanger)");
        return;
    };

    let mut interner = Interner::new();
    let plan = match &op {
        Op::Poser(b) => Plan::nouveau(Masque::Tout, Motif::Bloc(interner.intern(b))),
        Op::Remplacer(de, vers) => Plan::nouveau(
            Masque::Etat(interner.intern(de)),
            Motif::Bloc(interner.intern(vers)),
        ),
        Op::Melanger(v) => Plan::nouveau(
            Masque::Tout,
            Motif::melange(v.iter().map(|(p, b)| (*p, interner.intern(b))).collect()),
        ),
    }
    .avec_seed(args.seed);
    let plan = if args.compter {
        plan.en_comptant()
    } else {
        plan
    };

    let (sx, sy, sz) = sel.size();
    println!(
        "\nsélection : {sx} × {sy} × {sz} = {} blocs · {} régions",
        sel.volume(),
        sel.regions().count()
    );

    // La copie de travail vit à côté, dans un dossier temporaire. La save
    // d'origine n'est pas ouverte en écriture tant qu'on n'a pas demandé.
    let couche = std::env::temp_dir().join(format!("titiforge-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&couche);
    let overlay = match FsSource::open(&couche) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("copie de travail impossible : {e:?}");
            std::process::exit(1);
        }
    };
    let staging = Staging::new(src, overlay);

    let t0 = std::time::Instant::now();
    let rap = match appliquer(&staging, &args.dim, Folder::Region, &sel, &plan, &interner) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("opération refusée : {e}");
            std::process::exit(1);
        }
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;

    println!(
        "\n{ms:.0} ms · {} chunks modifiés · étages : rien {} · section {} · palette {} · bloc {}",
        rap.patches.len(),
        rap.etages[0],
        rap.etages[1],
        rap.etages[2],
        rap.etages[3]
    );
    match rap.blocs {
        Some(n) => println!("blocs modifiés : {n}"),
        None => println!("blocs modifiés : non comptés (--compter les compte, 31 × plus cher)"),
    }
    match rap.bornes {
        Some(b) => println!(
            "portée réelle : {},{},{} → {},{},{}",
            b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
        ),
        None => println!("portée réelle : rien n'a été écrit"),
    }

    if !args.ecrire {
        println!("\nESSAI À BLANC — la save n'a pas été touchée. `--ecrire` pour de vrai.");
        let _ = std::fs::remove_dir_all(&couche);
        return;
    }
    if rap.patches.is_empty() {
        println!("\nrien à écrire.");
        let _ = std::fs::remove_dir_all(&couche);
        return;
    }

    // Refuser si le jeu tient le monde, SAUVEGARDER, puis écrire — dans cet
    // ordre, et le staging le fait respecter.
    let monde = args.monde.clone();
    let mut copie = None;
    let sink = FsSource::open(&args.monde).expect("le monde est ouvrable");
    match staging.commit(&sink, verrou, true, &mut || {
        let vers = sauvegarder(&monde)?;
        println!("sauvegarde : {}", vers.display());
        copie = Some(vers);
        Ok(())
    }) {
        Ok(r) => println!(
            "écrit : {} régions, {} charges déportées",
            r.regions_ecrites, r.externes_ecrites
        ),
        Err(e) => {
            eprintln!("\nécriture refusée : {e:?}");
            std::process::exit(1);
        }
    }
    let _ = std::fs::remove_dir_all(&couche);
}
