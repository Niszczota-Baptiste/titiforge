//! Mesures du moteur Anvil.
//!
//! Ces benchs existent pour que les chiffres de `docs/RESULTATS.md` cessent de
//! venir d'un prototype jetable et deviennent une mesure CONTINUE. C'est le
//! piège n° 1 du projet — « optimiser sans mesurer » — et sa version longue :
//! optimiser sur une mesure d'il y a trois mois.
//!
//! ```bash
//! cargo bench -p tf-bench                       # mesurer
//! cargo bench -p tf-bench -- --save-baseline v0 # figer la référence
//! cargo bench -p tf-bench -- --baseline v0      # comparer
//! ```
//!
//! Deux règles héritées du bench de `we-engine` :
//!
//! - Un écart ne compte qu'au-delà de **25 %**. Mesuré là-bas : à code
//!   identique, un scénario a bougé de 18 % entre deux exécutions.
//! - Ces mesures viennent d'un conteneur partagé. Ce qui est exploitable,
//!   ce sont les **rapports entre scénarios**, pas les valeurs absolues.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use std::hint::black_box;

use tf_anvil::{
    decode_section, deflate, inflate, pack, read, scan, section_edits, splice, unpack_into, write,
    Compression, Edit, Interner, Packing, Section, VOL,
};
use tf_bench::{region, rss_bytes, Terrain};

/// La région de référence, construite une fois pour toutes les mesures.
fn region_pleine() -> Vec<u8> {
    region(&Terrain::region_pleine())
}

/// Décode toutes les sections d'une région. C'est le chemin de chargement.
fn decoder(src: &[u8]) -> (usize, Interner) {
    let r = read(src, 0, 0).unwrap();
    let mut interner = Interner::new();
    let mut n = 0usize;
    for c in r.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        for sc in &s.sections {
            if decode_section(&inflated, &s, sc, &mut interner)
                .unwrap()
                .is_some()
            {
                n += 1;
            }
        }
    }
    (n, interner)
}

/// Matérialise toutes les sections, pour les benchs d'opérations.
fn sections_de(src: &[u8]) -> (Vec<Section>, Interner) {
    let r = read(src, 0, 0).unwrap();
    let mut interner = Interner::new();
    let mut out = Vec::new();
    for c in r.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        for sc in &s.sections {
            if let Some(sec) = decode_section(&inflated, &s, sc, &mut interner).unwrap() {
                out.push(sec);
            }
        }
    }
    (out, interner)
}

// ── chargement ──────────────────────────────────────────────────────────────

fn bench_chargement(c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let src = region_pleine();

    let mut g = c.benchmark_group("chargement");
    g.sample_size(10);
    g.throughput(Throughput::Elements(t.sections_total() as u64));

    // Les trois phases séparément : sans ça, on optimise au hasard. Le bench de
    // we-engine a montré que le dépack de sections pesait 85 % là où on
    // soupçonnait le parseur NBT.
    g.bench_function("1_inflate_seul", |b| {
        let r = read(&src, 0, 0).unwrap();
        let charges: Vec<(Vec<u8>, Compression)> = r
            .iter()
            .map(|c| (c.payload.to_vec(), c.compression))
            .collect();
        b.iter(|| {
            let mut total = 0usize;
            for (p, comp) in &charges {
                total += inflate(black_box(p), *comp).unwrap().len();
            }
            black_box(total)
        })
    });

    g.bench_function("2_scan_seul", |b| {
        let r = read(&src, 0, 0).unwrap();
        let inflates: Vec<Vec<u8>> = r
            .iter()
            .map(|c| inflate(&c.payload, c.compression).unwrap())
            .collect();
        b.iter(|| {
            let mut n = 0usize;
            for i in &inflates {
                n += scan(black_box(i)).unwrap().sections.len();
            }
            black_box(n)
        })
    });

    g.bench_function("3_region_complete", |b| {
        b.iter(|| black_box(decoder(black_box(&src))).0)
    });

    g.finish();
}

// ── opérations ──────────────────────────────────────────────────────────────

fn bench_operations(c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let src = region_pleine();
    let (sections, interner) = sections_de(&src);
    let pierre = interner
        .get("minecraft:stone")
        .expect("la fixture en contient");
    let terre = interner.get("minecraft:dirt").expect("et de la terre");

    let mut g = c.benchmark_group("operations");
    g.sample_size(10);
    g.throughput(Throughput::Elements(t.blocs() as u64));

    // Étage PALETTE : on réécrit des entrées, aucun indice n'est touché.
    g.bench_function("replace_par_palette", |b| {
        b.iter_batched_ref(
            || sections.clone(),
            |secs| {
                let mut n = 0usize;
                for s in secs.iter_mut() {
                    n += s.replace_state(black_box(pierre), black_box(terre));
                }
                black_box(n)
            },
            BatchSize::LargeInput,
        )
    });

    // Étage BLOC : le même résultat, en visitant chaque case. C'est la
    // comparaison qui justifie l'étage palette — sans elle, « 0,26 ms » ne
    // veut rien dire.
    g.bench_function("replace_par_bloc", |b| {
        b.iter_batched_ref(
            || sections.clone(),
            |secs| {
                let mut n = 0usize;
                for s in secs.iter_mut() {
                    if !s.contains_state(pierre) {
                        continue;
                    }
                    n += s.map_blocks(|id| if id == pierre { terre } else { id });
                }
                black_box(n)
            },
            BatchSize::LargeInput,
        )
    });

    // Étage SECTION : palette d'une entrée, tableau d'indices supprimé.
    g.bench_function("set_uniforme", |b| {
        b.iter_batched_ref(
            || sections.clone(),
            |secs| {
                for s in secs.iter_mut() {
                    s.set_uniform(black_box(pierre));
                }
                black_box(secs.len())
            },
            BatchSize::LargeInput,
        )
    });

    // `count_of` : la version en `contains()` par bloc coûtait O(palette ×
    // 4096). Ce bench le fige, parce qu'une régression y serait invisible —
    // le résultat resterait juste, seulement lent.
    g.bench_function("count_of", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for s in sections.iter() {
                n += s.count_of(black_box(pierre));
            }
            black_box(n)
        })
    });

    // Le compactage, qui ne tourne qu'à l'écriture.
    g.bench_function("compact_palette", |b| {
        b.iter_batched_ref(
            || {
                let mut s = sections.clone();
                for sec in s.iter_mut() {
                    sec.replace_state(pierre, terre);
                }
                s
            },
            |secs| {
                let mut n = 0usize;
                for s in secs.iter_mut() {
                    n += s.compact_palette();
                }
                black_box(n)
            },
            BatchSize::LargeInput,
        )
    });

    g.finish();
}

// ── packing ─────────────────────────────────────────────────────────────────

fn bench_packing(c: &mut Criterion) {
    let mut g = c.benchmark_group("packing");
    g.throughput(Throughput::Elements(VOL as u64));

    // 5 bits : la largeur qui distingue les deux dispositions. 4 et 8 bits sont
    // les seules où elles coïncident, donc les seules où mesurer ne dirait rien.
    let idx: Vec<u16> = (0..VOL).map(|n| (n % 21) as u16).collect();

    for (nom, packing) in [
        ("sans_chevauchement", Packing::NoStraddle),
        ("avec_chevauchement", Packing::Straddle),
    ] {
        g.bench_function(format!("pack_{nom}"), |b| {
            b.iter(|| black_box(pack(black_box(&idx), 5, packing)))
        });
        let data = pack(&idx, 5, packing);
        g.bench_function(format!("unpack_{nom}"), |b| {
            let mut out = vec![0u16; VOL];
            b.iter(|| {
                unpack_into(black_box(&data), VOL, 5, packing, &mut out);
                black_box(out[0])
            })
        });
    }
    g.finish();
}

// ── écriture ────────────────────────────────────────────────────────────────

fn bench_ecriture(c: &mut Criterion) {
    let src = region_pleine();

    let mut g = c.benchmark_group("ecriture");
    g.sample_size(10);

    // Le cas qui doit être quasi gratuit : rien n'a changé, donc chaque charge
    // est recopiée telle quelle. S'il coûtait cher, l'invariant de
    // non-destruction serait payé au prix fort à chaque sauvegarde.
    g.bench_function("region_intacte", |b| {
        let r = read(&src, 0, 0).unwrap();
        b.iter(|| black_box(write(black_box(&r)).unwrap().region.len()))
    });

    // Le chemin complet d'une opération : décoder, modifier, splicer,
    // recompresser. C'est ici que vit le coût réel d'un //replace.
    g.bench_function("splice_et_recompression", |b| {
        let r = read(&src, 0, 0).unwrap();
        let c0 = r.get(0, 0).unwrap();
        let inflated = inflate(&c0.payload, c0.compression).unwrap();
        let scanned = scan(&inflated).unwrap();
        let mut interner = Interner::new();
        let secs: Vec<Option<Section>> = scanned
            .sections
            .iter()
            .map(|sc| decode_section(&inflated, &scanned, sc, &mut interner).unwrap())
            .collect();

        b.iter(|| {
            let mut edits: Vec<Edit> = Vec::new();
            for (sc, sec) in scanned.sections.iter().zip(secs.iter()) {
                if let Some(sec) = sec {
                    edits.extend(section_edits(sec, sc, &interner).unwrap());
                }
            }
            let neuf = splice(black_box(&inflated), &mut edits).unwrap();
            black_box(deflate(&neuf, Compression::Zlib).unwrap().len())
        })
    });

    g.finish();
}

// ── empreinte ───────────────────────────────────────────────────────────────

/// Pas un bench de temps : une mesure d'EMPREINTE, imprimée une fois.
///
/// Criterion ne sait mesurer que des durées. Or le mur qui a tué `we-engine`
/// était la mémoire — 154 octets par bloc — pas la vitesse. Un harnais qui ne
/// mesure que le temps laisserait ce mur revenir sans un signal.
fn bench_empreinte(_c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let src = region_pleine();
    let (sections, interner) = sections_de(&src);

    let packed: usize = sections.iter().map(|s| s.packed_bytes()).sum();
    let blocs = sections.len() * VOL;

    eprintln!();
    eprintln!("── empreinte d'une région pleine ───────────────────────────────");
    eprintln!(
        "  fichier                {:>9.2} Mio",
        src.len() as f64 / 1_048_576.0
    );
    eprintln!("  sections               {:>9}", sections.len());
    eprintln!("  blocs                  {:>9}", blocs);
    eprintln!("  états distincts        {:>9}", interner.len());
    eprintln!(
        "  structure packée       {:>9.1} Mio   = {:.2} o/bloc",
        packed as f64 / 1_048_576.0,
        packed as f64 / blocs as f64
    );
    // Le RSS TOTAL, pas un delta : un delta mesuré autour du décodage rend
    // zéro, parce que la fabrication de la fixture a déjà fait grossir le tas
    // et que l'allocateur ne rend pas les pages libérées à l'OS. Un chiffre
    // qui vaut toujours zéro n'est pas une bonne nouvelle, c'est une mesure
    // cassée — et elle laisserait une régression de mémoire passer.
    eprintln!(
        "  RSS du processus       {:>9.1} Mio   (fixture comprise)",
        rss_bytes() as f64 / 1_048_576.0
    );
    eprintln!("  (we-engine, région équivalente : 633 Mio, 6,59 o/bloc)");
    eprintln!();

    assert_eq!(blocs, t.blocs(), "la fixture doit être une région pleine");
    // Un garde-fou, pas une mesure : si l'empreinte repassait au-dessus d'un
    // octet par bloc, c'est que la représentation packée a été perdue quelque
    // part — et c'est le mur n° 1 qui revient.
    assert!(
        (packed as f64 / blocs as f64) < 1.0,
        "empreinte packée au-dessus d'un octet par bloc"
    );

    // Pas d'entrée criterion ici : chronométrer `sections.len()` donnerait
    // 300 picosecondes et une jolie courbe qui ne mesure rien.
}

criterion_group!(
    benches,
    bench_chargement,
    bench_operations,
    bench_packing,
    bench_ecriture,
    bench_empreinte
);
criterion_main!(benches);
