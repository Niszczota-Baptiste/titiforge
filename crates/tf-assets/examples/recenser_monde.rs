//! Recense un VRAI monde contre un VRAI pack.
//!
//! `recenser` mesure un pack ; `mailler_reel` maille une fixture avec la
//! géométrie du pack. Il manquait le troisième côté : ce qu'un joueur a
//! réellement posé.
//!
//! Tout ce que `docs/fixtures.md` annonce sur un build vient d'une fixture
//! qu'on a écrite soi-même, et le document le dit : « la densité de décor est
//! un réglage, pas une mesure ». Cet exemple remplace le réglage par un
//! relevé. Il ne copie rien — il lit la save de l'utilisateur et rend des
//! CHIFFRES.
//!
//! ```text
//! cargo run --release -p tf-assets --example recenser_monde -- <monde> <codex>
//! ```
//!
//! Le pack est facultatif : sans lui on relève la structure Anvil, ce qui
//! suffit à juger les fixtures de `tf-bench`. Avec lui on obtient la part de
//! blocs-modèles et le nombre de cuboïdes à émettre — le chiffre qui
//! dimensionne le rendu.

use std::collections::BTreeMap;
use std::path::Path;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId, VOL};
use tf_assets::catalogue::{blocs_translucides, table_formes, textures_citees};
use tf_assets::Atlas;
use tf_mesh::forme::Formes;
use tf_mesh::Grille;

/// Ce qu'un balayage retient du monde. Rien de matérialisé : des compteurs.
#[derive(Default)]
struct Releve {
    regions: usize,
    chunks: usize,
    sections: usize,
    sections_homogenes: usize,
    sections_sans_blocs: usize,
    /// Occurrences par état, indexé comme l'interner.
    par_etat: Vec<u64>,
    palettes: Vec<usize>,
    bits: BTreeMap<u8, usize>,
    versions: BTreeMap<i32, usize>,
    illisibles: usize,
    /// Blocs posés par chunk, avec ses coordonnées MONDE. C'est la
    /// distribution qui dimensionne le rendu, pas la moyenne : un monde plat
    /// dilue un château dans un océan d'air, et le mailleur, lui, travaille
    /// chunk par chunk.
    par_chunk: Vec<(i32, i32, u32)>,
    /// Blocs posés par section, pour les sections qui en portent au moins un.
    par_section: Vec<u32>,
}

impl Releve {
    fn compter(&mut self, id: StateId, n: u64) {
        let i = id as usize;
        if self.par_etat.len() <= i {
            self.par_etat.resize(i + 1, 0);
        }
        self.par_etat[i] += n;
    }
}

fn balayer(
    monde: &Path,
    zone: Option<[i32; 4]>,
    rel: &mut Releve,
    interner: &mut Interner,
    grille: Option<&mut Grille>,
) {
    let dossier = monde.join("region");
    let Ok(entrees) = std::fs::read_dir(&dossier) else {
        eprintln!("pas de dossier `region/` sous {}", monde.display());
        std::process::exit(1);
    };
    let mut fichiers: Vec<_> = entrees
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "mca").unwrap_or(false))
        .collect();
    fichiers.sort();

    let mut idx = vec![0u16; VOL];
    // Une grille de TOUT le monde ne tient pas en mémoire au-delà de quelques
    // régions. On ne la remplit que si on a demandé à mailler, et l'appelant
    // reste responsable de restreindre la zone.
    let mut grille = grille;
    for f in fichiers {
        let Ok(octets) = std::fs::read(&f) else {
            continue;
        };
        // Les coordonnées passées à `read` ne servent qu'aux messages : le
        // contenu porte les siennes.
        let Ok(r) = read(&octets, 0, 0) else {
            rel.illisibles += 1;
            continue;
        };
        rel.regions += 1;
        for brut in r.iter() {
            let Ok(inflated) = inflate(&brut.payload, brut.compression) else {
                rel.illisibles += 1;
                continue;
            };
            let Ok(sc) = scan(&inflated) else {
                rel.illisibles += 1;
                continue;
            };
            // Les coordonnées viennent du CONTENU : un nom de fichier est une
            // métadonnée qui peut mentir.
            let (cx, cz) = (sc.x_pos.unwrap_or(0), sc.z_pos.unwrap_or(0));
            if let Some([x0, z0, x1, z1]) = zone {
                if cx < x0 || cx > x1 || cz < z0 || cz > z1 {
                    continue;
                }
            }
            rel.chunks += 1;
            *rel.versions.entry(sc.data_version).or_insert(0) += 1;
            let mut poses_chunk = 0u32;
            for s in &sc.sections {
                rel.sections += 1;
                if s.spans.is_none() {
                    rel.sections_sans_blocs += 1;
                    continue;
                }
                let Ok(Some(sec)) = decode_section(&inflated, &sc, s, interner) else {
                    rel.illisibles += 1;
                    continue;
                };
                rel.palettes.push(sec.palette.len());
                *rel.bits.entry(sec.bits).or_insert(0) += 1;
                if sec.is_uniform() {
                    rel.sections_homogenes += 1;
                    if let Some(&id) = sec.palette.first() {
                        rel.compter(id, VOL as u64);
                        if !air_cle(interner.resolve(id).unwrap_or("")) {
                            rel.par_section.push(VOL as u32);
                            poses_chunk += VOL as u32;
                        }
                    }
                    // Une section homogène entre quand même dans la grille :
                    // un sol plat en est fait, et la sauter dessinerait un
                    // monde sans sol. C'est le mailleur qui décide d'ignorer
                    // celles qui sont vraiment vides.
                    if let Some(g) = grille.as_deref_mut() {
                        g.poser(cx, cz, sec);
                    }
                    continue;
                }
                sec.unpack_into(&mut idx);
                let mut local = vec![0u32; sec.palette.len()];
                for &i in idx.iter() {
                    // Un indice hors palette existe dans un fichier abîmé ;
                    // le compter comme la première entrée mentirait.
                    if let Some(c) = local.get_mut(i as usize) {
                        *c += 1;
                    }
                }
                let mut poses_sec = 0u32;
                for (i, &n) in local.iter().enumerate() {
                    if n > 0 {
                        rel.compter(sec.palette[i], n as u64);
                        if !air_cle(interner.resolve(sec.palette[i]).unwrap_or("")) {
                            poses_sec += n;
                        }
                    }
                }
                if poses_sec > 0 {
                    rel.par_section.push(poses_sec);
                    poses_chunk += poses_sec;
                }
                if let Some(g) = grille.as_deref_mut() {
                    g.poser(cx, cz, sec);
                }
            }
            rel.par_chunk.push((cx, cz, poses_chunk));
        }
    }
}

/// Un état est-il de l'air ? **Une seule règle** dans ce fichier : deux
/// versions finiraient par diverger, et l'une des deux compterait l'air
/// comme un bloc posé.
fn air_cle(cle: &str) -> bool {
    let (n, _) = tf_assets::catalogue::decouper(cle);
    n.ends_with(":air") || n.ends_with(":cave_air") || n.ends_with(":void_air")
}

fn pourcent(n: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        n as f64 * 100.0 / total as f64
    }
}

fn mediane(v: &mut [usize]) -> usize {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

fn main() {
    let mut a = std::env::args().skip(1);
    let Some(monde) = a.next() else {
        eprintln!(
            "usage : recenser_monde <monde> [codex] [--zone \"cx0,cz0,cx1,cz1\"]

  --zone     restreint le relevé à un rectangle de CHUNKS. C'est ce qui sépare
             « un monde » de « un build » : sur un monde plat, 99 % de l'air
             dilue tout ce qu'on cherche à mesurer.
  --mailler  maille pour de vrai ce qui a été relevé. Demande le pack, et garde
             tout le monde relevé en mémoire — à restreindre avec --zone."
        );
        std::process::exit(2);
    };
    let mut pack = None;
    let mut zone = None;
    let mut mailler = false;
    let mut reste = a.collect::<Vec<_>>().into_iter();
    while let Some(o) = reste.next() {
        if o == "--zone" {
            let v: Vec<i32> = reste
                .next()
                .unwrap_or_default()
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if v.len() != 4 {
                eprintln!("--zone attend quatre entiers : cx0,cz0,cx1,cz1");
                std::process::exit(2);
            }
            zone = Some([
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ]);
        } else if o == "--mailler" {
            mailler = true;
        } else {
            pack = Some(o);
        }
    }

    let t0 = std::time::Instant::now();
    let mut interner = Interner::new();
    let mut rel = Releve::default();
    let mut grille = Grille::new();
    balayer(
        Path::new(&monde),
        zone,
        &mut rel,
        &mut interner,
        mailler.then_some(&mut grille),
    );
    let ms_lecture = t0.elapsed().as_secs_f64() * 1000.0;

    println!("monde : {monde}");
    if let Some([x0, z0, x1, z1]) = zone {
        println!(
            "  zone : chunks {x0}..{x1} × {z0}..{z1} (blocs {}..{} × {}..{})",
            x0 * 16,
            x1 * 16 + 15,
            z0 * 16,
            z1 * 16 + 15
        );
    }
    println!(
        "  {} régions · {} chunks · {} sections ({} homogènes, {} sans blocs)",
        rel.regions, rel.chunks, rel.sections, rel.sections_homogenes, rel.sections_sans_blocs
    );
    if rel.illisibles > 0 {
        println!("  {} charges illisibles (ignorées)", rel.illisibles);
    }
    println!("  lecture complète : {ms_lecture:.0} ms");
    print!("  DataVersion :");
    for (v, n) in &rel.versions {
        print!(" {} ({} chunks)", tf_anvil::version_label(*v), n);
    }
    println!();

    let mut pal = rel.palettes.clone();
    let med = mediane(&mut pal);
    let max = pal.last().copied().unwrap_or(0);
    let moy = if pal.is_empty() {
        0.0
    } else {
        pal.iter().sum::<usize>() as f64 / pal.len() as f64
    };
    println!("  palette d'une section : médiane {med} · moyenne {moy:.1} · max {max}");
    print!("  bits par indice :");
    for (b, n) in &rel.bits {
        print!(" {b}→{n}");
    }
    println!();

    // ── ce qui est POSÉ
    let cles: Vec<String> = (0..interner.len() as StateId)
        .map(|i| interner.resolve(i).unwrap().to_string())
        .collect();
    let est_air = |c: &str| air_cle(c);
    let total: u64 = rel.par_etat.iter().sum();
    let poses: u64 = rel
        .par_etat
        .iter()
        .enumerate()
        .filter(|(i, _)| !est_air(&cles[*i]))
        .map(|(_, n)| n)
        .sum();
    println!(
        "\n  {total} blocs balayés · {poses} posés ({:.1} %) · {} états distincts",
        pourcent(poses, total),
        cles.len()
    );

    let mut par_ns: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for (i, &n) in rel.par_etat.iter().enumerate() {
        let (nom, _) = tf_assets::catalogue::decouper(&cles[i]);
        let ns = nom.split_once(':').map(|(a, _)| a).unwrap_or("?");
        let e = par_ns.entry(ns).or_insert((0, 0));
        e.0 += 1;
        if !est_air(&cles[i]) {
            e.1 += n;
        }
    }
    for (ns, (etats, n)) in &par_ns {
        println!(
            "    {ns:14} {etats:5} états · {n:12} blocs posés ({:.1} %)",
            pourcent(*n, poses)
        );
    }

    // ── la DISTRIBUTION, pas la moyenne
    //
    // Le mailleur travaille chunk par chunk : ce qui le dimensionne est le
    // chunk le plus chargé, pas le chunk moyen. Sur un monde plat la moyenne
    // est un chiffre vrai qui ne décrit aucun chunk existant.
    let mut dens: Vec<u32> = rel.par_chunk.iter().map(|&(_, _, n)| n).collect();
    dens.sort_unstable();
    let vides = dens.iter().take_while(|&&n| n == 0).count();
    let q = |p: f64| -> u32 {
        if dens.is_empty() {
            0
        } else {
            dens[((dens.len() - 1) as f64 * p) as usize]
        }
    };
    println!(
        "\n  blocs posés par chunk : {vides} chunks vides · médiane {} · p90 {} · p99 {} · max {}",
        q(0.5),
        q(0.9),
        q(0.99),
        dens.last().copied().unwrap_or(0)
    );
    let mut sec = rel.par_section.clone();
    sec.sort_unstable();
    println!(
        "  sections portant un bloc : {} / {} · médiane {} · p90 {} · max {} (sur 4096)",
        sec.len(),
        rel.sections,
        if sec.is_empty() {
            0
        } else {
            sec[sec.len() / 2]
        },
        if sec.is_empty() {
            0
        } else {
            sec[(sec.len() - 1) * 9 / 10]
        },
        sec.last().copied().unwrap_or(0)
    );
    let mut chunks = rel.par_chunk.clone();
    chunks.sort_unstable_by(|a, b| b.2.cmp(&a.2));
    println!("  les huit chunks les plus chargés :");
    for &(cx, cz, n) in chunks.iter().take(8) {
        println!(
            "    chunk {cx:5},{cz:5} (bloc {:7},{:7}) : {n:6} posés",
            cx * 16,
            cz * 16
        );
    }

    let mut top: Vec<(u64, &str)> = rel
        .par_etat
        .iter()
        .enumerate()
        .filter(|(i, _)| !est_air(&cles[*i]))
        .map(|(i, &n)| (n, cles[i].as_str()))
        .collect();
    top.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    println!("\n  les quinze plus posés :");
    for (n, c) in top.iter().take(15) {
        println!("    {n:10} ({:5.2} %) {c}", pourcent(*n, poses));
    }

    // ── ce que le pack en dit
    let Some(pack) = pack else {
        println!("\n(pas de pack donné — la part de blocs-modèles demande le codex)");
        return;
    };
    let tp = std::time::Instant::now();
    // Codex, pack, ou INSTALLATION de launcher — le genre se reconnaît au
    // contenu, pas à ce que l'utilisateur en dit.
    let (cat, src, genre) = match tf_assets::jeu::catalogue(&pack) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("assets illisibles : {e}");
            std::process::exit(1);
        }
    };
    let disposition = genre.disposition();
    let citees = textures_citees(&cat);
    let atlas = Atlas::batir(&src, citees.iter().cloned(), &|n| {
        disposition.chemins_texture(n)
    });
    let translucides = blocs_translucides(&cat, &atlas);
    let table = table_formes(&cat, cles.iter().cloned(), &|n| translucides.contains(n));
    println!(
        "\nassets : {genre:?} · {} blocs, {} modèles, {} introuvables, {:.0} ms",
        cat.nb_blocs(),
        cat.nb_modeles(),
        cat.introuvables.len(),
        tp.elapsed().as_secs_f64() * 1000.0
    );

    let (mut n_cube, mut n_modele, mut n_vide, mut n_inconnu) = (0u64, 0u64, 0u64, 0u64);
    let (mut e_cube, mut e_modele, mut e_vide, mut e_inconnu) = (0usize, 0usize, 0usize, 0usize);
    let mut cuboides = 0u64;
    let mut inconnus: Vec<(u64, &str)> = Vec::new();
    for (i, &n) in rel.par_etat.iter().enumerate() {
        let id = i as StateId;
        if est_air(&cles[i]) {
            continue;
        }
        let (nom, _) = tf_assets::catalogue::decouper(&cles[i]);
        if cat.blockstate(nom).is_none() {
            n_inconnu += n;
            e_inconnu += 1;
            if n > 0 {
                inconnus.push((n, cles[i].as_str()));
            }
            continue;
        }
        let cub = table.cuboides(id);
        if table.opaque(id) {
            n_cube += n;
            e_cube += 1;
        } else if table.est_air(id) {
            // Le pack connaît le bloc mais son modèle n'a aucun élément.
            n_vide += n;
            e_vide += 1;
        } else {
            n_modele += n;
            e_modele += 1;
            cuboides += n * cub.len() as u64;
        }
    }
    let connus = n_cube + n_modele + n_vide;
    println!("\n  ce que le mailleur devra faire des {poses} blocs posés :");
    println!(
        "    cubes pleins    {n_cube:12} ({:5.1} %) · {e_cube} états",
        pourcent(n_cube, poses)
    );
    println!(
        "    blocs-modèles   {n_modele:12} ({:5.1} %) · {e_modele} états",
        pourcent(n_modele, poses)
    );
    println!(
        "    sans géométrie  {n_vide:12} ({:5.1} %) · {e_vide} états",
        pourcent(n_vide, poses)
    );
    println!(
        "    hors du pack    {n_inconnu:12} ({:5.1} %) · {e_inconnu} états",
        pourcent(n_inconnu, poses)
    );
    println!(
        "\n    CUBOÏDES à émettre : {cuboides} ({:.2} par bloc-modèle)",
        if n_modele == 0 {
            0.0
        } else {
            cuboides as f64 / n_modele as f64
        }
    );
    if connus > 0 {
        println!(
            "    (sur les seuls blocs connus du pack : {:.1} % de modèles)",
            pourcent(n_modele, connus)
        );
    }
    // ── ce qui PÈSE dans la passe de modèles
    //
    // La moyenne du pack (3,5 cuboïdes par modèle) n'est pas celle de ce
    // qu'on POSE. Un pack contient un sac de friandises à 82 cuboïdes ; un
    // build est fait de dalles et d'escaliers. Ce qui dimensionne le rendu
    // est le produit blocs × cuboïdes, pas l'un ou l'autre.
    let mut poids: Vec<(u64, u64, usize, &str)> = Vec::new();
    for (i, &n) in rel.par_etat.iter().enumerate() {
        let id = i as StateId;
        if n == 0 || est_air(&cles[i]) || table.opaque(id) || table.est_air(id) {
            continue;
        }
        let k = table.cuboides(id).len();
        if k > 0 {
            poids.push((n * k as u64, n, k, cles[i].as_str()));
        }
    }
    poids.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    if !poids.is_empty() {
        println!("\n  ce qui pèse dans la passe de modèles :");
        for (p, n, k, c) in poids.iter().take(10) {
            println!(
                "    {p:9} cuboïdes ({:4.1} %) = {n:7} × {k:3} · {c}",
                pourcent(*p, cuboides)
            );
        }
    }

    inconnus.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    if !inconnus.is_empty() {
        println!("\n  les dix plus posés que le pack ne connaît pas :");
        for (n, c) in inconnus.iter().take(10) {
            println!("    {n:10} {c}");
        }
    }

    // ── mailler pour de vrai
    //
    // C'est le seul chiffre qui dimensionne le rendu. Les benchs de `tf-mesh`
    // maillent une fixture ; ici la grille vient de la save et la géométrie du
    // pack, donc plus rien n'est inventé.
    if mailler {
        let t = std::time::Instant::now();
        let c = grille.mailler_parallele(&table);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("\n  MAILLAGE de {} sections posées", grille.len());
        println!("    {ms:8.0} ms");
        println!(
            "    {} maillées · {} sautées (vides ou entièrement masquées)",
            c.maillees(),
            c.sautees
        );
        println!("    {:10} quads gloutons", c.quads());
        println!("    {:10} poses de modèles", c.poses());
        println!("    {:.2} Mo pour le GPU", c.octets() as f64 / 1e6);
    }
}
