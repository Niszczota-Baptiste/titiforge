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
use tf_ops::catalogue::{construire, Params, Valeur, OPS};
use tf_ops::executer::{executer, Options};
use tf_ops::Volume;
use tf_world::coords::{BBox, BlockPos};
use tf_world::decoupe::Niveau;
use tf_world::selection::{Direction, Selection, DIRECTIONS};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::FsSource;
use tf_world::Staging;

struct Args {
    monde: PathBuf,
    dim: Dimension,
    /// L'identifiant CATALOGUE de l'opération demandée, et ses paramètres.
    ///
    /// **La ligne de commande ne connaît plus les opérations.** Elle traduit
    /// ses propres options en paramètres nommés et laisse `construire` puis
    /// `executer` faire le reste : une seule chaîne d'appels pour tous les
    /// hôtes, au lieu de deux qui finiraient par ne plus dire la même chose.
    op: Option<&'static str>,
    params: Params,
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
    /// Le VOLUME visé dans la sélection. Gardé en paramètres plutôt qu'en
    /// `Forme` construite : le centre vient de `--sel`, qui peut arriver
    /// APRÈS sur la ligne de commande, et l'ordre des options n'a jamais de
    /// sens.
    volume: Volume,
    creux: Option<f64>,
    /// **Un drapeau à part, appliqué à la FIN.** `--renversee` peut précéder
    /// `--pyramide` sur la ligne de commande, et l'ordre des options n'a
    /// jamais de sens. Le poser directement dans le volume le perdait quand
    /// il arrivait le premier — regression attrapée en essayant la commande.
    renversee: bool,
    /// Gestes de SÉLECTION, appliqués dans l'ordre AVANT l'opération.
    ///
    /// Une liste et pas trois champs : `--pousser est 3 --chunk` doit
    /// s'enchaîner dans l'ordre tapé, sinon l'utilisateur ne peut pas prévoir
    /// ce qu'il obtient.
    gestes: Vec<Geste>,
}

/// Ce qui bouge la SÉLECTION avant que l'opération ne parte.
#[derive(Debug, Clone, Copy)]
enum Geste {
    /// `//expand` / `//contract` : pousse une face. Négatif la ramène.
    Pousser(Direction, i32),
    /// `//chunk` : étend aux cellules entières du découpage.
    Aligner(Niveau),
    /// Étend la HAUTEUR aux sections entières — l'autre moitié de ce qui
    /// donne l'étage palette.
    AlignerSections,
}

fn usage() -> ! {
    eprintln!(
        "usage : editer <monde> [options]

  --sel \"x1,y1,z1,x2,y2,z2\"  la sélection, en coordonnées MONDE. Les guillemets
                             sous PowerShell : sans eux le shell mange la virgule

 OPÉRATIONS — une seule par commande :
{}
 Paramètres des opérations, dans l'ordre où le descripteur les déclare. Les
 valeurs par défaut viennent du catalogue, pas d'ici :
{}
 Gestes de SÉLECTION, appliqués dans l'ordre tapé, AVANT l'opération :
  --pousser <dir> <n>        //expand : pousse UNE face de n blocs. n négatif la
                             ramène (//contract), sans jamais traverser la face
                             opposée. est|ouest|nord|sud|haut|bas
  --chunk                    //chunk : étend la sélection aux CHUNKS entiers.
                             Ce n'est pas cosmétique — une sélection alignée
                             couvre des sections entières, donc l'étage palette ;
                             décalée d'un bloc, c'est × 21
  --mca                      la même chose, mais aux fichiers r.X.Z.mca
  --sections                 étend la HAUTEUR aux tranches de 16. --chunk ne le
                             fait pas — c'est la convention de WorldEdit — et
                             les DEUX sont nécessaires pour l'étage palette

 FORMES — le volume visé DANS la sélection :
  --sphere <rayon>           //sphere
  --cylindre <rayon> <haut>  //cyl, axe vertical
  --pyramide <demi-base> <h> //pyramid ; --renversee pour la pointe en bas
  --murs <épaisseur>         //walls : les 4 parois VERTICALES de la sélection
  --faces <épaisseur>        //faces : ses 6 faces, plancher et plafond compris
  --creux <épaisseur>        creuse la FORME (//hsphere, //hcyl…) — géométrique :
                             il retire le centre du volume qu'on vient de poser.
                             Ne pas confondre avec --creuser, qui INSPECTE
  Les formes sont CENTRÉES sur la sélection, et l'opération ne paie que la
  forme : une sphère de rayon 10 dans une sélection d'un million de blocs
  coûte une sphère de rayon 10.

 RÉGLAGES :
  --avec-air                 l'air de l'extrait écrase ce qu'il recouvre
  --pack <chemin>            le pack, l'installation ou le codex d'où DÉRIVER
                             les règles de rotation. Sans lui, les cases
                             bougent mais les états ne sont pas réécrits
  --dim nether|end           (défaut : le monde principal)
  --seed <n>                 la graine du mélange (défaut 0)
  --compter                  compter les blocs modifiés — coûte 31 × l'opération
  --ecrire                   ÉCRIRE dans la save (sinon : essai à blanc)

Une commande par LIGNE : PowerShell ne connaît pas la continuation \\ d'un shell Unix.

Sans opération, se contente de décrire le monde.",
        lignes_operations(),
        lignes_parametres()
    );
    std::process::exit(2)
}

/// **La liste des opérations vient du CATALOGUE.**
///
/// Écrite à la main, elle a déjà vieilli une fois dans ce fichier — et rien ne
/// l'aurait dit : un texte d'aide faux se lit exactement comme un texte d'aide
/// juste. Le descripteur porte déjà le nom, les noms WorldEdit et le résumé ;
/// les recopier ici serait la cinquième table qui diverge.
fn lignes_operations() -> String {
    let mut out = String::new();
    for d in OPS {
        let resume = d.resume.split(". ").next().unwrap_or(d.resume);
        out.push_str(&format!(
            "  --{:<22} {}\n{:26} {}\n",
            d.id,
            d.we.join(" "),
            "",
            resume.trim()
        ));
    }
    out
}

/// Et leurs paramètres, avec leurs bornes et leurs défauts — les vrais, ceux
/// que `normaliser` appliquera.
fn lignes_parametres() -> String {
    use tf_ops::catalogue::Saisie;
    let mut out = String::new();
    for d in OPS {
        if d.params.is_empty() {
            continue;
        }
        let champs: Vec<String> = d
            .params
            .iter()
            .map(|p| {
                let genre = match p.saisie {
                    Saisie::Bloc => "bloc".to_string(),
                    Saisie::Biome => "biome".to_string(),
                    Saisie::Melange => "p:bloc,p:bloc".to_string(),
                    Saisie::Entier { min, max } => format!("{min}..{max}"),
                    Saisie::Vecteur => "dx,dy,dz".to_string(),
                    Saisie::Direction => "est|ouest|nord|sud|haut|bas".to_string(),
                    Saisie::Transformation => "90|180|270|x|z".to_string(),
                };
                match p.defaut {
                    Some(v) => format!("{} <{genre}> [{}]", p.nom, v.valeur()),
                    None => format!("{} <{genre}> REQUIS", p.nom),
                }
            })
            .collect();
        out.push_str(&format!("  --{:<22} {}\n", d.id, champs.join(" · ")));
    }
    out
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
    // **Aucun défaut n'est posé ici.** Les défauts vivent dans le descripteur
    // de chaque opération, et `normaliser` les applique. En poser un second
    // jeu ferait deux sources pour la même valeur — et la ligne de commande
    // enverrait `remplir` à un `//set` qui n'en veut pas, donc une erreur
    // franche là où il n'y avait qu'une option de trop.
    let mut args = Args {
        monde,
        dim: Dimension::Overworld,
        op: None,
        params: Params::new(),
        sel: None,
        ecrire: false,
        compter: false,
        seed: 0,
        pack: None,
        avec_air: false,
        volume: Volume::Aucun,
        creux: None,
        renversee: false,
        gestes: Vec::new(),
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
            "--poser" => {
                args.op = Some("poser");
                args.params.poser("bloc", texte(a.next()));
            }
            "--remplacer" => {
                args.op = Some("remplacer");
                args.params.poser("de", texte(a.next()));
                args.params.poser("vers", texte(a.next()));
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
                args.op = Some("melanger");
                args.params.poser("melange", Valeur::Melange(entrees));
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
                args.op = Some("copier-vers");
                args.params
                    .poser("decalage", Valeur::Vecteur([v[0], v[1], v[2]]));
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
                args.op = Some("deplacer");
                args.params
                    .poser("decalage", Valeur::Vecteur([v[0], v[1], v[2]]));
            }
            "--empiler" => {
                let fois = a
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage());
                args.op = Some("empiler");
                args.params.poser("fois", Valeur::Entier(fois));
                args.params
                    .poser("direction", Valeur::Direction(direction(a.next())));
            }
            "--remplir" => {
                args.params.poser("remplir", texte(a.next()));
            }
            "--naturaliser" => args.op = Some("naturaliser"),
            "--biome" => {
                args.op = Some("biome");
                args.params.poser("biome", texte(a.next()));
            }
            // Le nombre de passes est une option à part et non un second
            // argument positionnel : `std::env::Args` ne se relit pas, donc
            // « est-ce un nombre ou l'option suivante ? » ne se décide pas
            // sans consommer. Une option nommée ne pose pas la question.
            "--lisser" => {
                args.op = Some("lisser");
                args.params
                    .poser("rayon", Valeur::Entier(nombre(a.next()) as i64));
            }
            "--passes" => {
                args.params
                    .poser("passes", Valeur::Entier(nombre(a.next()) as i64));
            }
            // Le repère Minecraft en toutes lettres, comme `--empiler` :
            // +X = Est, +Z = Sud, +Y = Haut.
            "--pousser" => {
                let d = direction(a.next());
                args.gestes.push(Geste::Pousser(d, nombre(a.next()) as i32));
            }
            "--chunk" => args.gestes.push(Geste::Aligner(Niveau::Chunk)),
            "--mca" => args.gestes.push(Geste::Aligner(Niveau::Region)),
            "--sections" => args.gestes.push(Geste::AlignerSections),
            "--creuser" => args.op = Some("creuser"),
            "--epaisseur" => {
                args.params
                    .poser("epaisseur", Valeur::Entier(nombre(a.next()) as i64));
            }
            "--couches" => {
                args.params.poser("surface", texte(a.next()));
                args.params.poser("sous-sol", texte(a.next()));
                args.params.poser("roche", texte(a.next()));
            }
            "--profondeur" => {
                args.params
                    .poser("profondeur", Valeur::Entier(nombre(a.next()) as i64));
            }
            "--sphere" => {
                args.volume = Volume::Sphere {
                    rayon: nombre(a.next()),
                }
            }
            "--cylindre" => {
                args.volume = Volume::Cylindre {
                    rayon: nombre(a.next()),
                    hauteur: nombre(a.next()),
                }
            }
            "--pyramide" => {
                args.volume = Volume::Pyramide {
                    demi_base: nombre(a.next()),
                    hauteur: nombre(a.next()),
                    renversee: false,
                }
            }
            "--creux" => args.creux = Some(nombre(a.next())),
            "--murs" => {
                args.volume = Volume::Murs {
                    epaisseur: nombre(a.next()),
                }
            }
            "--faces" => {
                args.volume = Volume::Faces {
                    epaisseur: nombre(a.next()),
                }
            }
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
    // Rotation et miroir se recollent ici : `--tourner` peut précéder
    // `--copier-vers` sur la ligne de commande, et l'ordre des options n'a
    // jamais de sens.
    if let Some(v) = transfo {
        args.params
            .poser("transformation", Valeur::Transformation(Some(v)));
    }
    if args.renversee {
        if let Volume::Pyramide { renversee, .. } = &mut args.volume {
            *renversee = true;
        }
    }
    args
}

/// Un texte obligatoire, ou l'usage.
fn texte(v: Option<String>) -> Valeur {
    Valeur::Texte(v.unwrap_or_else(|| usage()))
}

/// Une direction du repère Minecraft — **+X = Est, +Z = Sud, +Y = Haut**.
///
/// Les noms vivent dans `Direction::nom`, pas ici : cet exemple en portait
/// DEUX tables identiques (`--empiler` et `--pousser`), ce qui est déjà une de
/// trop, et une coque en aurait écrit une troisième.
fn direction(v: Option<String>) -> Direction {
    let n = v.unwrap_or_else(|| usage());
    Direction::depuis_nom(&n)
        .or_else(|| {
            // Les initiales, pour la frappe rapide — « n » est le NORD, donc
            // −Z. C'est le seul endroit où l'abréviation est décidée.
            DIRECTIONS
                .iter()
                .copied()
                .find(|d| d.nom().starts_with(&n) && n.len() == 1)
        })
        .unwrap_or_else(|| usage())
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

// La sauvegarde horodatée vit dans `tf_world::sauvegarder` — invariant n° 6,
// et elle est écrite UNE fois : la coque la faisait déjà, et deux
// sauvegardes qui divergent d'un hôte à l'autre sont deux sauvegardes qu'on
// découvre incomplètes le jour où on en a besoin.

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

    let (Some(nom_op), Some(sel)) = (args.op, args.sel) else {
        // La liste vient du CATALOGUE, pas d'une phrase écrite à la main :
        // celle-ci a déjà vieilli une fois, et rien ne l'aurait dit.
        let noms: Vec<&str> = OPS.iter().flat_map(|d| d.we.iter().copied()).collect();
        println!("\n(pas d'opération demandée — {})", noms.join(" "));
        return;
    };

    let mut interner = Interner::new();

    // ── les gestes de sélection, DANS L'ORDRE TAPÉ
    //
    // Avant tout le reste : c'est la sélection finale qui décide de la portée,
    // des formes, et de ce que le rapport annoncera. Les appliquer après
    // rendrait un chiffre qui ne correspond à rien.
    let sel = if args.gestes.is_empty() {
        sel
    } else {
        let mut s = Selection::nouvelle();
        s.poser_coin1(sel.min);
        s.poser_coin2(sel.max);
        for g in &args.gestes {
            let fait = match *g {
                Geste::Pousser(d, n) => s.agrandir(d, n),
                Geste::Aligner(niveau) => s.aligner(niveau),
                Geste::AlignerSections => s.aligner_sections(),
            };
            // Le DIRE quand ça ne change rien : un geste silencieux qui n'a
            // rien fait est un geste qu'on croit avoir fait.
            if !fait {
                println!("geste sans effet : {g:?}");
            }
        }
        let apres = s.boite().expect("la sélection avait deux coins");
        let (ax, ay, az) = apres.size();
        println!(
            "gestes : {} × {} × {} → {ax} × {ay} × {az} · {},{},{} → {},{},{}",
            sel.size().0,
            sel.size().1,
            sel.size().2,
            apres.min.x,
            apres.min.y,
            apres.min.z,
            apres.max.x,
            apres.max.y,
            apres.max.z
        );
        apres
    };

    let (sx, sy, sz) = sel.size();
    println!(
        "\nsélection : {sx} × {sy} × {sz} = {} blocs · {} régions",
        sel.volume(),
        sel.regions().count()
    );
    // **Ce qui décide l'étage se COMPTE, il ne se déduit pas.** « Alignée sur
    // les chunks » est vrai et ne prouve rien : une sélection de y = −40 à
    // −20 est alignée en x et z et ne couvre AUCUNE section entière, parce que
    // les sections vont de −48 à −33 puis de −32 à −17. Mesuré en écrivant ce
    // message : douze sections à l'étage bloc pendant que l'outil annonçait
    // « alignée ».
    let (entieres, total) = sel.sections_entieres();
    if total > 0 {
        println!(
            "sections entièrement couvertes : {entieres} / {total}{}",
            if entieres == total {
                " — l'étage palette est atteignable"
            } else {
                " — le reste passera par l'étage BLOC (--chunk aligne x et z, --sections la hauteur)"
            }
        );
    }

    // Les formes sont centrées sur la SÉLECTION, et `Volume::forme` est la
    // seule table qui le sache — la coque y lit la même chose.
    let forme = args.volume.forme(&sel, args.creux);
    if let Some(b) = forme.bornes() {
        let (fx, fy, fz) = b.size();
        // Une enveloppe n'est pas « centrée » : elle est PRISE sur la
        // sélection. Le dire autrement laisserait croire qu'on peut la
        // déplacer avec un centre.
        let ou = if args.volume.enveloppe() {
            "prise sur la sélection".to_string()
        } else {
            format!(
                "centrée sur {},{},{}",
                (sel.min.x + sel.max.x).div_euclid(2),
                (sel.min.y + sel.max.y).div_euclid(2),
                (sel.min.z + sel.max.z).div_euclid(2)
            )
        };
        println!("forme : {} · {fx} × {fy} × {fz} · {ou}", args.volume.nom());
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

    // ── L'EXÉCUTION.
    //
    // Tout ce que cet exemple portait — copier, transformer, creuser, coller,
    // déplacer, empiler, lisser, naturaliser — vit maintenant dans
    // `tf_ops::executer`. Deux cent cinquante lignes d'aiguillage en moins, et
    // surtout : la coque prend exactement le même chemin. Deux chaînes
    // d'appels auraient fini par ne plus dire la même chose, et la première à
    // se tromper l'aurait fait en silence.
    let table = regles(args.pack.as_deref());
    let regle = |cle: &str, t: Transfo| table.as_ref().and_then(|tb| tb.transformer(cle, t));
    let opts = Options {
        compter: args.compter,
        seed: args.seed,
        avec_air: args.avec_air,
        forme,
        regle: table.as_ref().map(|_| &regle as tf_ops::Regle),
    };

    let travail = match construire(nom_op, &args.params, &mut interner) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let t0 = std::time::Instant::now();
    let cr = match executer(
        &travail,
        &staging,
        &args.dim,
        Folder::Region,
        &sel,
        &mut interner,
        &opts,
    ) {
        Ok(cr) => cr,
        Err(e) => {
            eprintln!("opération refusée : {e}");
            std::process::exit(1);
        }
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let rap = &cr.rapport;

    // ── Ce que le moteur a rapporté, dit à l'utilisateur.
    //
    // **Le moteur ne parle à personne** : il rend des données, et c'est ici
    // qu'elles deviennent des phrases. La coque en fera des étiquettes à
    // partir des mêmes chiffres, donc aucune des deux ne peut se tromper sur
    // un nombre que l'autre a juste.
    if let Some([tx, ty, tz]) = cr.extrait {
        println!(
            "extrait : {tx} × {ty} × {tz} · {} block entities",
            cr.entites_copiees
        );
    }
    if cr.cases_materialisees > 0 {
        println!(
            "matérialisé : {:.1} Mo — la seule opération qui paie tout le volume",
            cr.cases_materialisees as f64 / 1e6
        );
    }
    if let Some([dx, dy, dz]) = cr.pas {
        println!("pas : {dx},{dy},{dz}");
    }
    if cr.colonnes_relevees > 0 {
        println!("relief : {} colonnes relevées", cr.colonnes_relevees);
    }
    // Ce qu'on n'a pas su transformer est NOMMÉ. Le taire produirait un build
    // à moitié tourné, et rien à l'écran pour le dire. Sans pack, au
    // contraire, les nommer un par un noierait la seule information utile.
    match (cr.sans_regle, cr.intacts.len()) {
        (_, 0) => {}
        (true, n) => println!(
            "ATTENTION : aucun pack, donc AUCUN des {n} états n'est réécrit — \
             les cases bougent, pas les orientations. `--pack <chemin>` les dérive."
        ),
        (false, n) => {
            let exemples: Vec<&str> = cr
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
        let vers = tf_world::sauvegarder(&monde)?;
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
