//! Où part le temps d'une opération COMPLÈTE, de la lecture au fichier.
//!
//! Les trois étages mesurent le calcul ; ce bench mesure la chaîne. Et il la
//! découpe avant de la parallélser, parce que ce dépôt a déjà payé une fois le
//! fait d'avoir deviné : « prismarine-nbt dominera le chargement » était faux,
//! il pesait 12 % là où le dépack de sections en pesait 85.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};

use tf_anvil::chunk::{decode_section, scan, section_edits, splice};
use tf_anvil::codec::{deflate, inflate};
use tf_anvil::region::read;
use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::appliquer_region;
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos, SectionPos};
use tf_world::source::{Dimension, Folder, MemorySource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn interner_de(bytes: &[u8]) -> Interner {
    let r = read(bytes, 0, 0).unwrap();
    let mut interner = Interner::new();
    for c in r.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        for sc in &s.sections {
            decode_section(&inflated, &s, sc, &mut interner).unwrap();
        }
    }
    interner
}

fn partout() -> BBox {
    BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 511,
            y: 319,
            z: 511,
        },
    )
}

fn bench(c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let brut = region(&t);
    let interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").expect("la fixture en a");
    let terre = interner.get("minecraft:dirt").expect("et de la terre");

    let mut g = c.benchmark_group("edition");
    g.sample_size(10);
    g.throughput(Throughput::Elements(t.blocs() as u64));

    // ── La chaîne complète.
    g.bench_function("0_chaine_complete", |b| {
        b.iter_batched(
            || {
                let src = MemorySource::new();
                src.put_region(SURFACE, DOSSIER, ZERO, brut.clone());
                (Staging::new(src, MemorySource::new()), interner.clone())
            },
            |(st, inter)| {
                let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
                let r = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &partout(), &plan, &inter)
                    .unwrap();
                black_box(r.patches.len())
            },
            BatchSize::LargeInput,
        )
    });

    // ── Les phases, séparément. Sans ça on optimise au hasard.
    g.bench_function("1_inflate", |b| {
        b.iter(|| {
            let r = read(&brut, 0, 0).unwrap();
            let mut n = 0usize;
            for c in r.iter() {
                n += inflate(&c.payload, c.compression).unwrap().len();
            }
            black_box(n)
        })
    });

    let inflates: Vec<Vec<u8>> = {
        let r = read(&brut, 0, 0).unwrap();
        r.iter()
            .map(|c| inflate(&c.payload, c.compression).unwrap())
            .collect()
    };

    g.bench_function("2_scan_et_decode", |b| {
        b.iter(|| {
            let mut inter = interner.clone();
            let mut n = 0usize;
            for inf in &inflates {
                let s = scan(inf).unwrap();
                for sc in &s.sections {
                    if decode_section(inf, &s, sc, &mut inter).unwrap().is_some() {
                        n += 1;
                    }
                }
            }
            black_box(n)
        })
    });

    g.bench_function("3_appliquer_et_encoder", |b| {
        let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
        b.iter(|| {
            let mut inter = interner.clone();
            let mut n = 0usize;
            for (i, inf) in inflates.iter().enumerate() {
                let s = scan(inf).unwrap();
                for sc in &s.sections {
                    let Some(mut sec) = decode_section(inf, &s, sc, &mut inter).unwrap() else {
                        continue;
                    };
                    let spos = SectionPos {
                        x: (i % 32) as i32,
                        y: sc.y as i32,
                        z: (i / 32) as i32,
                    };
                    plan.appliquer(&mut sec, &partout(), spos);
                    n += section_edits(inf, &sec, sc, &inter).unwrap().len();
                }
            }
            black_box(n)
        })
    });

    g.bench_function("4_splice", |b| {
        let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
        let mut inter = interner.clone();
        let prepares: Vec<(Vec<u8>, Vec<tf_anvil::chunk::Edit>)> = inflates
            .iter()
            .enumerate()
            .map(|(i, inf)| {
                let s = scan(inf).unwrap();
                let mut edits = Vec::new();
                for sc in &s.sections {
                    let Some(mut sec) = decode_section(inf, &s, sc, &mut inter).unwrap() else {
                        continue;
                    };
                    let spos = SectionPos {
                        x: (i % 32) as i32,
                        y: sc.y as i32,
                        z: (i / 32) as i32,
                    };
                    plan.appliquer(&mut sec, &partout(), spos);
                    edits.extend(section_edits(inf, &sec, sc, &inter).unwrap());
                }
                (inf.clone(), edits)
            })
            .collect();
        b.iter(|| {
            let mut n = 0usize;
            for (inf, edits) in &prepares {
                let mut e = edits.clone();
                n += splice(inf, &mut e).unwrap().len();
            }
            black_box(n)
        })
    });

    g.bench_function("5_deflate", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for inf in &inflates {
                n += deflate(inf, tf_anvil::Compression::Zlib).unwrap().len();
            }
            black_box(n)
        })
    });

    g.finish();
}

criterion_group!(edition, bench);
criterion_main!(edition);
