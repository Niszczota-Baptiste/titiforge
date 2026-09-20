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
//! .\editer.exe D:\monde-essai --sel "0,60,0,31,90,31" --copier-vers "100,0,0" --tourner 90 --pack %APPDATA%\.minefield_1_18
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
use tf_blocks::Transfo;
use tf_ops::edition::{appliquer, copier, deplacer, empiler};
use tf_ops::plan::{Operation, Plan};
use tf_ops::{Collage, Forme, Masque, Motif, Naturaliser, Pas, PoserBiome};
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
    /// Le pack d'où DÉRIVER les règles de rotation. Sans lui, un quart de
    /// tour déplace les cases sans réécrire les états : les escaliers
    /// regardent toujours dans l'ancienne direction. L'outil le DIT au lieu
    /// de le laisser découvrir en jeu.
    pack: Option<PathBuf>,
    avec_air: bool,
    /// Ce qui reste à la place de la source d'un `--deplacer`.
    remplir: String,
    /// Le VOLUME visé dans la sélection. Gardé en paramètres plutôt qu'en
    /// `Forme` construite : le centre vient de `--sel`, qui peut arriver
    /// APRÈS sur la ligne de commande, et l'ordre des options n'a jamais de
    /// sens.
    volume: Volume,
    creux: Option<f64>,
    renversee: bool,
    /// Surface, sous-sol, roche d'une naturalisation.
    couches: [String; 3],
    profondeur: u32,
}

#[derive(Debug, Clone, Copy)]
enum Volume {
    Aucun,
    Sphere(f64),
    Cylindre(f64, f64),
    Pyramide(f64, f64),
    /// Les quatre parois verticales de la sélection — `//walls`.
    Murs(f64),
    /// Ses six faces — `//faces`.
    Faces(f64),
}

enum Op {
    Poser(String),
    Remplacer(String, String),
    Melanger(Vec<(u32, String)>),
    /// `//copy` + `//rotate` + `//paste` en un geste. Une ligne de commande ne
    /// garde pas de presse-papiers entre deux appels : ce qui serait trois
    /// commandes dans le jeu en fait une ici.
    CopierVers {
        d: [i32; 3],
        transfo: Option<Transfo>,
    },
    /// `//move` : la source est effacée, le contenu part ailleurs.
    Deplacer {
        d: [i32; 3],
    },
    /// `//stack` : la sélection se répète, d'un PAS égal à sa propre taille
    /// le long d'une direction. C'est ce que fait WorldEdit, et c'est ce
    /// qu'on veut neuf fois sur dix — un mur qu'on prolonge.
    Empiler {
        fois: u32,
        dir: [i32; 3],
    },
    /// `//naturalize` : refaire la stratigraphie, colonne par colonne.
    Naturaliser,
    /// `//setbiome` : poser un biome. Grille de 4 × 4 × 4, pas de bloc.
    Biome(String),
}

fn usage() -> ! {
    eprintln!(
        "usage : editer <monde> [options]

  --sel \"x1,y1,z1,x2,y2,z2\"  la sélection, en coordonnées MONDE. Les guillemets
                             sous PowerShell : sans eux le shell mange la virgule
  --poser <bloc>             //set
  --remplacer <de> <vers>    //replace
  --melanger p:bloc,p:bloc   un mélange pondéré, ex. 3:minecraft:stone,1:minecraft:dirt
  --copier-vers \"dx,dy,dz\"   //copy puis //paste décalé de (dx, dy, dz)
  --tourner 90|180|270       tourne l'extrait avant de le poser
  --miroir x|z               le reflète
  --deplacer \"dx,dy,dz\"     //move : déplace le contenu, la source est remplie
  --empiler <n> <direction>  //stack : répète la sélection n fois, d'un pas égal
                             à sa taille. est|ouest|nord|sud|haut|bas
                             (+X = Est, +Z = Sud, +Y = Haut)
  --remplir <bloc>           ce qui reste à la place d'un --deplacer
                             (défaut : minecraft:air)
  --naturaliser              //naturalize : la 1re couche solide de chaque
                             colonne devient de l'herbe, les 3 suivantes de la
                             terre, le reste de la pierre
  --couches <s> <ss> <r>     les trois blocs (défaut grass_block, dirt, stone)
  --profondeur <n>           l'épaisseur du sous-sol (défaut 3)
  --biome <nom>              //setbiome. ATTENTION : un biome se pose par
                             CELLULE de 4 × 4 × 4 blocs — une sélection qui ne
                             tombe pas sur un multiple de 4 déborde d'autant
  --sphere <rayon>           //sphere : restreint l'opération à une sphère
  --cylindre <rayon> <haut>  //cyl, axe vertical
  --pyramide <demi-base> <h> //pyramid ; --renversee pour la pointe en bas
  --creux <épaisseur>        creuse la forme (//hsphere, //hcyl…)
  --murs <épaisseur>         //walls : les 4 parois VERTICALES de la sélection
  --faces <épaisseur>        //faces : ses 6 faces, plancher et plafond compris
  Les formes sont CENTRÉES sur la sélection, et l'opération ne paie que la
  forme : une sphère de rayon 10 dans une sélection d'un million de blocs
  coûte une sphère de rayon 10.
  --avec-air                 l'air de l'extrait écrase ce qu'il recouvre
  --pack <chemin>            le pack, l'installation ou le codex d'où DÉRIVER
                             les règles de rotation. Sans lui, les cases
                             bougent mais les états ne sont pas réécrits
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
        pack: None,
        avec_air: false,
        remplir: "minecraft:air".to_string(),
        volume: Volume::Aucun,
        creux: None,
        renversee: false,
        couches: [
            "minecraft:grass_block".to_string(),
            "minecraft:dirt".to_string(),
            "minecraft:stone".to_string(),
        ],
        profondeur: 3,
    };
    // Rotation et miroir se donnent séparément de la destination : on les
    // recolle à la fin, parce que `--tourner` peut précéder `--copier-vers`
    // sur la ligne de commande et que l'ordre des options n'a jamais de sens.
    let mut transfo: Option<Transfo> = None;
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
            "--copier-vers" => {
                let v: Vec<i32> = a
                    .next()
                    .unwrap_or_else(|| usage())
                    .split(sep)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse().unwrap_or_else(|_| usage()))
                    .collect();
                if v.len() != 3 {
                    usage();
                }
                args.op = Some(Op::CopierVers {
                    d: [v[0], v[1], v[2]],
                    transfo: None,
                });
            }
            "--deplacer" => {
                let v: Vec<i32> = a
                    .next()
                    .unwrap_or_else(|| usage())
                    .split(sep)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse().unwrap_or_else(|_| usage()))
                    .collect();
                if v.len() != 3 {
                    usage();
                }
                args.op = Some(Op::Deplacer {
                    d: [v[0], v[1], v[2]],
                });
            }
            "--empiler" => {
                let fois = a
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage());
                // Le repère Minecraft, en toutes lettres : +X = Est,
                // +Z = Sud, +Y = Haut. Un « nord » qui irait vers +Z ferait
                // empiler du mauvais côté sans qu'aucune erreur le dise.
                let dir = match a.next().unwrap_or_else(|| usage()).as_str() {
                    "est" | "e" => [1, 0, 0],
                    "ouest" | "o" => [-1, 0, 0],
                    "sud" | "s" => [0, 0, 1],
                    "nord" | "n" => [0, 0, -1],
                    "haut" | "h" => [0, 1, 0],
                    "bas" | "b" => [0, -1, 0],
                    _ => usage(),
                };
                args.op = Some(Op::Empiler { fois, dir });
            }
            "--remplir" => args.remplir = a.next().unwrap_or_else(|| usage()),
            "--naturaliser" => args.op = Some(Op::Naturaliser),
            "--biome" => args.op = Some(Op::Biome(a.next().unwrap_or_else(|| usage()))),
            "--couches" => {
                args.couches = [
                    a.next().unwrap_or_else(|| usage()),
                    a.next().unwrap_or_else(|| usage()),
                    a.next().unwrap_or_else(|| usage()),
                ]
            }
            "--profondeur" => args.profondeur = nombre(a.next()) as u32,
            "--sphere" => args.volume = Volume::Sphere(nombre(a.next())),
            "--cylindre" => args.volume = Volume::Cylindre(nombre(a.next()), nombre(a.next())),
            "--pyramide" => args.volume = Volume::Pyramide(nombre(a.next()), nombre(a.next())),
            "--creux" => args.creux = Some(nombre(a.next())),
            "--murs" => args.volume = Volume::Murs(nombre(a.next())),
            "--faces" => args.volume = Volume::Faces(nombre(a.next())),
            "--renversee" => args.renversee = true,
            "--tourner" => {
                transfo = Some(match a.next().unwrap_or_else(|| usage()).as_str() {
                    "90" => Transfo::Rot90,
                    "180" => Transfo::Rot180,
                    "270" => Transfo::Rot270,
                    _ => usage(),
                })
            }
            "--miroir" => {
                transfo = Some(match a.next().unwrap_or_else(|| usage()).as_str() {
                    "x" | "X" => Transfo::MiroirX,
                    "z" | "Z" => Transfo::MiroirZ,
                    _ => usage(),
                })
            }
            "--avec-air" => args.avec_air = true,
            "--pack" => args.pack = Some(PathBuf::from(a.next().unwrap_or_else(|| usage()))),
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
    if let (Some(Op::CopierVers { transfo: t, .. }), Some(v)) = (args.op.as_mut(), transfo) {
        *t = Some(v);
    }
    args
}

fn nombre(v: Option<String>) -> f64 {
    v.unwrap_or_else(|| usage())
        .trim()
        .replace(',', ".")
        .parse()
        .unwrap_or_else(|_| usage())
}

/// Les règles de rotation, dérivées du pack qu'on nous désigne.
///
/// **Rien n'est écrit à la main.** 910 blocs Minefield portent un état et
/// 22 627 variantes déclarent une rotation : une table écrite à la main est
/// impossible, et une table incomplète est pire qu'aucune — elle tourne la
/// moitié d'un mur.
///
/// Sans pack, on rend `None` et le collage laisse les états TELS QUELS. C'est
/// annoncé, pas subi : un build à moitié tourné est faux d'une façon qu'aucune
/// capture d'écran ne montre.
fn regles(pack: Option<&Path>) -> Option<tf_blocks::Table> {
    let chemin = pack?;
    match tf_assets::jeu::catalogue(chemin) {
        Ok((cat, _, genre)) => {
            let table = tf_blocks::Table::deriver(&cat);
            println!(
                "pack : {} ({genre:?}) · {} blocs, règles pour {} états au quart de tour",
                chemin.display(),
                cat.nb_blocs(),
                table.couverture(Transfo::Rot90)
            );
            Some(table)
        }
        Err(e) => {
            eprintln!("pack illisible ({e}) — les états ne seront PAS réécrits");
            None
        }
    }
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
        println!(
            "\n(pas d'opération demandée — voir --poser / --remplacer / --melanger / \
             --copier-vers / --deplacer / --empiler / --naturaliser / --biome)"
        );
        return;
    };

    let mut interner = Interner::new();
    let air = interner.intern("minecraft:air");
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
        // Ces trois-là se construisent plus bas : elles ont besoin du staging
        // pour lire ce qu'elles vont reposer.
        // Celles-là ne sont pas des plans : elles se construisent plus bas,
        // parce qu'elles demandent le staging ou une autre forme de travail
        // que « un masque et un motif ».
        Op::CopierVers { .. }
        | Op::Deplacer { .. }
        | Op::Empiler { .. }
        | Op::Naturaliser
        | Op::Biome(_) => Plan::nouveau(Masque::Tout, Motif::Garder),
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

    // Les formes sont centrées sur la SÉLECTION. Le milieu se prend en
    // division PLANCHER : `(-9 + -1) / 2` vaut −5 en Rust comme en euclidien,
    // mais `(-9 + 0) / 2` vaut −4 dans un sens et −5 dans l'autre — et le
    // bloc −1 est dans la région −1, pas la région 0.
    let centre = [
        (sel.min.x + sel.max.x).div_euclid(2),
        (sel.min.y + sel.max.y).div_euclid(2),
        (sel.min.z + sel.max.z).div_euclid(2),
    ];
    let forme = match args.volume {
        Volume::Aucun => Forme::Boite,
        Volume::Sphere(r) => Forme::sphere(centre, r),
        Volume::Cylindre(r, h) => Forme::cylindre(centre, r, h),
        // La pyramide se pose sur le BAS de la sélection, pas sur son centre :
        // une pyramide flottante n'est ce que personne ne demande.
        Volume::Pyramide(b, h) => Forme::pyramide(
            [
                centre[0],
                if args.renversee { sel.max.y } else { sel.min.y },
                centre[2],
            ],
            b,
            h,
            args.renversee,
        ),
        // Les murs et les faces se prennent sur la SÉLECTION, pas sur un
        // centre et un rayon : c'est une enveloppe, pas un volume posé.
        Volume::Murs(e) => Forme::murs(sel, e),
        Volume::Faces(e) => Forme::faces(sel, e),
    };
    let forme = match args.creux {
        Some(e) => forme.creuse(e),
        None => forme,
    };
    let plan = plan.dans(forme);
    if let Some(b) = plan.forme.bornes() {
        let (fx, fy, fz) = b.size();
        // Une enveloppe n'est pas « centrée » : elle est PRISE sur la
        // sélection. Le dire autrement laisserait croire qu'on peut la
        // déplacer avec un centre.
        let ou = match args.volume {
            Volume::Murs(_) | Volume::Faces(_) => "prise sur la sélection".to_string(),
            _ => format!("centrée sur {},{},{}", centre[0], centre[1], centre[2]),
        };
        println!("forme : {fx} × {fy} × {fz} · {ou}");
    }

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

    // ── Le presse-papiers, quand l'opération en demande un.
    //
    // `//copy` est la seule opération du crate qui n'écrit rien : elle lit à
    // travers le staging et rend un extrait détaché. Rien n'est réservé au
    // monde tant que le collage n'est pas appliqué.
    let mut presse = None;
    if let Op::CopierVers { transfo, .. } = &op {
        let t0 = std::time::Instant::now();
        let mut p = match copier(&staging, &args.dim, Folder::Region, &sel, &mut interner) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("copie refusée : {e}");
                std::process::exit(1);
            }
        };
        println!(
            "copié : {} × {} × {} en {:.0} ms · {} état(s) distinct(s) · {} block entities",
            p.taille[0],
            p.taille[1],
            p.taille[2],
            t0.elapsed().as_secs_f64() * 1000.0,
            p.palette().len(),
            p.entites.len()
        );
        if let Some(t) = transfo {
            let table = regles(args.pack.as_deref());
            let r = p.transformer(*t, &mut interner, &|cle, t| {
                table.as_ref().and_then(|tb| tb.transformer(cle, t))
            });
            // Ce qu'on n'a pas su transformer est NOMMÉ. Le taire produirait
            // un build à moitié tourné, et rien à l'écran pour le dire.
            //
            // Sans pack, TOUT est intact et les nommer un par un noierait le
            // message dans une liste de pierres qui n'auraient rien tourné de
            // toute façon : la seule information utile est qu'il manque un
            // pack. Avec pack, au contraire, chaque nom compte — c'est un
            // trou de la table, et il se répare.
            match (table.is_none(), r.intacts.len()) {
                (_, 0) => {}
                (true, n) => println!(
                    "ATTENTION : aucun pack, donc AUCUN des {n} états n'est réécrit — \
                     les cases bougent, pas les orientations. `--pack <chemin>` les dérive."
                ),
                (false, n) => {
                    let exemples: Vec<&str> = r
                        .intacts
                        .iter()
                        .filter_map(|id| interner.resolve(*id))
                        .take(5)
                        .collect();
                    println!(
                        "ATTENTION : {n} état(s) laissés TELS QUELS — {}{}",
                        exemples.join(", "),
                        if n > exemples.len() { ", …" } else { "" }
                    );
                }
            }
            p = r.presse;
        }
        presse = Some(p);
    }

    let collage = presse.as_ref().map(|p| {
        let [dx, dy, dz] = match &op {
            Op::CopierVers { d, .. } => *d,
            _ => [0, 0, 0],
        };
        Collage {
            presse: p,
            coin: BlockPos {
                x: sel.min.x + dx,
                y: sel.min.y + dy,
                z: sel.min.z + dz,
            },
            avec_air: args.avec_air,
            air,
            compter: args.compter,
        }
    });
    // Une opération ne paie que sa PORTÉE : un collage paie son extrait, pas
    // la sélection d'où il vient.
    let (operation, portee): (&dyn Operation, BBox) = match &collage {
        Some(c) => (c, c.bornes()),
        // Une opération ne paie que sa PORTÉE : la forme resserre la
        // sélection avant qu'un seul chunk ne soit lu.
        None => (&plan, plan.portee(&sel)),
    };

    let remplissage = interner.intern(&args.remplir);
    let un_pas = |d: [i32; 3]| Pas {
        d,
        avec_air: args.avec_air,
        air,
        compter: args.compter,
    };

    let t0 = std::time::Instant::now();
    // `deplacer` et `empiler` sont COMPOSÉES : elles enchaînent plusieurs
    // passes et rendent leurs correctifs bout à bout, pour une seule entrée
    // de journal. Un `Ctrl+Z` défera le déplacement entier.
    let fait = match &op {
        Op::Deplacer { d } => deplacer(
            &staging,
            &args.dim,
            Folder::Region,
            &sel,
            un_pas(*d),
            remplissage,
            &mut interner,
        ),
        Op::Empiler { fois, dir } => {
            let (sx, sy, sz) = sel.size();
            let d = [dir[0] * sx as i32, dir[1] * sy as i32, dir[2] * sz as i32];
            println!("pas : {},{},{} · {fois} fois", d[0], d[1], d[2]);
            empiler(
                &staging,
                &args.dim,
                Folder::Region,
                &sel,
                un_pas(d),
                *fois,
                &mut interner,
            )
        }
        Op::Biome(nom) => {
            let b = interner.intern(nom);
            let op = PoserBiome {
                biome: b,
                compter: args.compter,
            };
            appliquer(&staging, &args.dim, Folder::Region, &sel, &op, &interner)
        }
        Op::Naturaliser => {
            let mut n = Naturaliser::nouveau(
                interner.intern(&args.couches[0]),
                interner.intern(&args.couches[1]),
                interner.intern(&args.couches[2]),
                air,
            );
            n.profondeur = args.profondeur;
            n.compter = args.compter;
            println!(
                "naturalisation : {} puis {} × {} puis {}",
                args.couches[0], args.couches[1], args.profondeur, args.couches[2]
            );
            appliquer(&staging, &args.dim, Folder::Region, &sel, &n, &interner)
        }
        _ => appliquer(
            &staging,
            &args.dim,
            Folder::Region,
            &portee,
            operation,
            &interner,
        ),
    };
    let rap = match fait {
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
    if rap.entites_posees > 0 || rap.entites_retirees > 0 {
        println!(
            "block entities : {} posée(s) · {} retirée(s) (leur bloc a disparu)",
            rap.entites_posees, rap.entites_retirees
        );
    }
    if rap.biomes > 0 {
        println!(
            "biomes : {} section(s) — la grille est de 4 × 4 × 4 blocs, la \
             sélection a pu déborder d'autant",
            rap.biomes
        );
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
