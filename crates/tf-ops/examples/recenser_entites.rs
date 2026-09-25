//! Ce qu'un VRAI monde porte comme entités — et ce que le moteur en ferait.
//!
//! Le suivi des entités a été écrit sur une fixture : des cadres, un tableau,
//! un villageois, tous tirés du format du jeu, mais choisis par nous. Une
//! fixture ne peut trouver que ce qu'on savait déjà. Cet outil relève ce
//! qu'une save contient VRAIMENT, et fait passer chaque entité par le code
//! même des opérations — une rotation, un miroir — pour compter ce que le
//! moteur annoncerait comme APPROCHÉ. C'est ce chiffre-là qui dit quoi écrire
//! ensuite, pas une liste de cas qu'on imagine.
//!
//! ```text
//! cargo run --release -p tf-ops --example recenser_entites -- D:\monde
//! cargo run --release -p tf-ops --example recenser_entites -- D:\monde --zone "4,7,10,12"
//! cargo run --release -p tf-ops --example recenser_entites -- D:\monde --dim nether
//! ```
//!
//! `--zone` est un rectangle de CHUNKS, bornes comprises ; la virgule et
//! l'espace y sont acceptés tous les deux (PowerShell recolle un tableau avec
//! des espaces). Rien n'est écrit : l'outil ne fait que lire.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tf_anvil::mobiles::{balayer_chunk, Mobile};
use tf_anvil::{external_file_name, inflate, read};
use tf_blocks::Transfo;
use tf_ops::mobiles::transformer_mobile;
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::FsSource;

/// Ce qu'on compte, par `id`.
#[derive(Default)]
struct ParType {
    n: u64,
    passagers: u64,
    accrochees: u64,
    orientees: u64,
    souvenirs: u64,
    laisses: u64,
    poses: u64,
    octets: u64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(monde) = args.first() else {
        eprintln!(
            "usage : recenser_entites <monde> [--zone \"x0,z0,x1,z1\"] [--dim surface|nether|end]"
        );
        std::process::exit(2);
    };
    let mut zone: Option<[i32; 4]> = None;
    let mut dim = Dimension::Overworld;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--zone" => {
                let v: Vec<i32> = args
                    .get(i + 1)
                    .map(|s| {
                        s.split([',', ' '])
                            .filter(|x| !x.is_empty())
                            .filter_map(|x| x.parse().ok())
                            .collect()
                    })
                    .unwrap_or_default();
                if v.len() != 4 {
                    eprintln!("--zone attend quatre nombres : x0,z0,x1,z1 (en chunks)");
                    std::process::exit(2);
                }
                zone = Some([
                    v[0].min(v[2]),
                    v[1].min(v[3]),
                    v[0].max(v[2]),
                    v[1].max(v[3]),
                ]);
                i += 2;
            }
            "--dim" => {
                dim = match args.get(i + 1).map(String::as_str) {
                    Some("nether") => Dimension::Nether,
                    Some("end") => Dimension::End,
                    Some("surface") | Some("overworld") => Dimension::Overworld,
                    autre => {
                        eprintln!("dimension inconnue : {autre:?}");
                        std::process::exit(2);
                    }
                };
                i += 2;
            }
            autre => {
                eprintln!("argument inconnu : {autre}");
                std::process::exit(2);
            }
        }
    }

    let src = match FsSource::open(PathBuf::from(monde)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("monde illisible : {e:?}");
            std::process::exit(1);
        }
    };
    let regions = match src.overview(&dim, Folder::Entities) {
        Ok(o) => o.regions,
        Err(e) => {
            eprintln!("pas de dossier entities/ lisible ({e:?}) — un monde d'avant 1.17 ?");
            std::process::exit(1);
        }
    };

    let dedans = |cx: i32, cz: i32| {
        zone.is_none_or(|[x0, z0, x1, z1]| cx >= x0 && cx <= x1 && cz >= z0 && cz <= z1)
    };

    let mut par_type: BTreeMap<String, ParType> = BTreeMap::new();
    let mut versions: BTreeMap<Option<i32>, u64> = BTreeMap::new();
    let mut approches: BTreeMap<(String, &'static str), u64> = BTreeMap::new();
    let (mut n_regions, mut n_chunks, mut racines, mut deportes, mut illisibles) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut sans_position = 0u64;
    let mut cadres_au_sol_tournes = 0u64;
    let mut plus_peuple = (0usize, 0i32, 0i32);

    for r in regions {
        // Une région entièrement hors de la zone ne se lit pas.
        if let Some([x0, z0, x1, z1]) = zone {
            let (rx0, rz0) = (r.pos.x * 32, r.pos.z * 32);
            if rx0 + 31 < x0 || rx0 > x1 || rz0 + 31 < z0 || rz0 > z1 {
                continue;
            }
        }
        let octets = match src.read_region(&dim, Folder::Entities, r.pos) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("r.{}.{} illisible : {e:?}", r.pos.x, r.pos.z);
                illisibles += 1;
                continue;
            }
        };
        let Ok(mut region) = read(&octets, r.pos.x, r.pos.z) else {
            illisibles += 1;
            continue;
        };
        n_regions += 1;
        illisibles += region.illisibles as u64;
        for i in 0..1024usize {
            let (cx, cz) = (
                r.pos.x * 32 + (i % 32) as i32,
                r.pos.z * 32 + (i / 32) as i32,
            );
            if !dedans(cx, cz) {
                continue;
            }
            let Some(brut) = region.slots[i].as_mut() else {
                continue;
            };
            if brut.needs_external() {
                deportes += 1;
                match src.read_external(&dim, Folder::Entities, &external_file_name(cx, cz)) {
                    Ok(c) => brut.resolve_external(c),
                    Err(_) => {
                        illisibles += 1;
                        continue;
                    }
                }
            }
            let Ok(inflate) = inflate(&brut.payload, brut.compression) else {
                illisibles += 1;
                continue;
            };
            let Ok(chunk) = balayer_chunk(&inflate) else {
                illisibles += 1;
                continue;
            };
            n_chunks += 1;
            *versions.entry(chunk.data_version).or_default() += 1;
            if chunk.entrees.len() > plus_peuple.0 {
                plus_peuple = (chunk.entrees.len(), cx, cz);
            }
            for e in &chunk.entrees {
                racines += 1;
                if e.pos().is_none() {
                    sans_position += 1;
                }
                for (rang, k) in e.corps.iter().enumerate() {
                    let id = k.id.clone().unwrap_or_else(|| "(sans id)".to_string());
                    let p = par_type.entry(id).or_default();
                    p.n += 1;
                    if rang > 0 {
                        p.passagers += 1;
                    }
                    p.accrochees += k.tuile.is_some() as u64;
                    p.orientees += (k.facing.is_some() || k.attache.is_some()) as u64;
                    p.souvenirs += k.retenues.len() as u64;
                    p.laisses += k.laisse_uuid.is_some() as u64;
                    p.poses += k.pose as u64;
                    if rang == 0 {
                        p.octets += e.span.len() as u64;
                    }
                    if matches!(k.facing.map(|f| f.v), Some(0 | 1))
                        && k.rotation_objet.is_some_and(|r| r.v != 0)
                    {
                        cadres_au_sol_tournes += 1;
                    }
                }
                // Ce que le moteur ANNONCERAIT : le vrai code, sur la vraie
                // entité, sous une rotation et sous un miroir.
                let m = Mobile::depuis(&inflate, e, chunk.data_version);
                for t in [Transfo::Rot90, Transfo::MiroirX] {
                    let mut a = Vec::new();
                    transformer_mobile(&m, t, [16, 16, 16], &mut a);
                    for x in a {
                        *approches.entry((x.id, x.raison)).or_default() += 1;
                    }
                }
            }
        }
    }

    let total: u64 = par_type.values().map(|p| p.n).sum();
    println!(
        "entities/ ({}) : {n_regions} région(s), {n_chunks} chunk(s), {racines} entité(s) \
         dans les listes, {total} en comptant les passagers",
        dim.label()
    );
    if let Some(z) = zone {
        println!("zone (chunks) : {z:?}");
    }
    let v: Vec<String> = versions
        .iter()
        .map(|(dv, n)| match dv {
            Some(dv) => format!("{dv} × {n}"),
            None => format!("sans DataVersion × {n}"),
        })
        .collect();
    println!("DataVersion des chunks : {}", v.join(", "));
    println!(
        "chunks déportés (.mcc) : {deportes} · illisibles : {illisibles} · le plus peuplé : \
         {} entité(s), chunk ({}, {})",
        plus_peuple.0, plus_peuple.1, plus_peuple.2
    );

    println!();
    println!(
        "{:>8}  {:<40} {:>9} {:>9} {:>9} {:>9} {:>7} {:>6} {:>10}",
        "n",
        "type",
        "passagers",
        "accroch.",
        "orientées",
        "souvenirs",
        "laisses",
        "poses",
        "o/entité"
    );
    let mut lignes: Vec<_> = par_type.iter().collect();
    lignes.sort_by(|a, b| b.1.n.cmp(&a.1.n).then(a.0.cmp(b.0)));
    for (id, p) in lignes {
        let racines = (p.n - p.passagers).max(1);
        println!(
            "{:>8}  {:<40} {:>9} {:>9} {:>9} {:>9} {:>7} {:>6} {:>10}",
            p.n,
            id,
            p.passagers,
            p.accrochees,
            p.orientees,
            p.souvenirs,
            p.laisses,
            p.poses,
            p.octets / racines
        );
    }

    println!();
    println!("ce qui compte pour les opérations :");
    println!(
        "  cadres au sol ou au plafond dont l'objet est tourné : {cadres_au_sol_tournes} \
         (règle dérivée du rendu du jeu, à confirmer en jeu)"
    );
    println!("  entités sans position lisible (jamais déplacées) : {sans_position}");
    if approches.is_empty() {
        println!("  rien que le moteur annoncerait comme approché, sous rotation ni sous miroir");
    } else {
        println!("  ce que le moteur annoncerait comme APPROCHÉ (rotation + miroir) :");
        let mut a: Vec<_> = approches.into_iter().collect();
        a.sort_by(|x, y| y.1.cmp(&x.1));
        for ((id, raison), n) in a {
            println!("    {n:>7}  {id} — {raison}");
        }
    }
}
