//! Quel décompresseur, et est-ce que ça se voit ?
//!
//! `inflate` pèse 75 % du chargement d'une région. C'est donc la première
//! cible — mais « miniz_oxide est lent, zlib est rapide » est une réputation,
//! pas une mesure, et ce dépôt a une règle là-dessus.
//!
//! Deux `cargo bench` successifs ne répondent PAS à la question : mesuré ici,
//! un scénario qui ne décompresse rien du tout a « gagné » 14 % entre deux
//! exécutions. C'est le bruit d'un conteneur partagé, et il est du même ordre
//! que l'écart qu'on cherche.
//!
//! Les deux backends tournent donc dans le MÊME processus, sur les MÊMES
//! octets, en alternance : ce qui reste après ça est un vrai écart.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use std::io::Read;

use tf_anvil::{read, Compression};
use tf_bench::{region, Terrain};

/// Les charges compressées d'une région pleine, telles qu'elles sont sur le
/// disque.
fn charges() -> Vec<Vec<u8>> {
    let src = region(&Terrain::region_pleine());
    let r = read(&src, 0, 0).unwrap();
    r.iter()
        .map(|c| {
            assert_eq!(c.compression, Compression::Zlib);
            c.payload.to_vec()
        })
        .collect()
}

fn bench_backends(c: &mut Criterion) {
    let charges = charges();
    let octets_clairs: usize = charges
        .iter()
        .map(|p| flate2_inflate(p).len())
        .sum();

    let mut g = c.benchmark_group("inflate_backends");
    g.sample_size(20);
    // Le débit se compte en octets DÉCOMPRESSÉS : c'est le travail réel, et
    // c'est comparable d'un backend à l'autre quel que soit le taux.
    g.throughput(Throughput::Bytes(octets_clairs as u64));

    g.bench_function("flate2_zlib_rs", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for p in &charges {
                n += flate2_inflate(black_box(p)).len();
            }
            black_box(n)
        })
    });

    g.bench_function("miniz_oxide", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for p in &charges {
                n += miniz_inflate(black_box(p)).len();
            }
            black_box(n)
        })
    });

    g.finish();

    // Et une vérification, parce qu'un décompresseur rapide qui rend autre
    // chose n'est pas un décompresseur rapide.
    for p in charges.iter().take(8) {
        assert_eq!(
            flate2_inflate(p),
            miniz_inflate(p),
            "les deux backends doivent rendre les mêmes octets"
        );
    }
}

fn flate2_inflate(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(payload)
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn miniz_inflate(payload: &[u8]) -> Vec<u8> {
    miniz_oxide::inflate::decompress_to_vec_zlib(payload).unwrap()
}

/// L'autre moitié de la question, et la plus coûteuse.
///
/// Réécrire une région modifiée recompresse chaque chunk touché. Mesuré dans
/// `anvil.rs` : 1,10 ms pour un chunk, donc de l'ordre de la seconde pour une
/// région entière. C'est beaucoup plus cher que la décompression, et c'est là
/// que la réputation de zlib-ng porte vraiment.
fn bench_compression(c: &mut Criterion) {
    let clairs: Vec<Vec<u8>> = charges()
        .iter()
        .take(64) // la compression est lente : 64 chunks suffisent à trancher
        .map(|p| flate2_inflate(p))
        .collect();
    let octets: usize = clairs.iter().map(|c| c.len()).sum();

    let mut g = c.benchmark_group("deflate_backends");
    g.sample_size(10);
    g.throughput(Throughput::Bytes(octets as u64));

    // Niveau 6 : le défaut de zlib, et celui que le jeu utilise. Un fichier de
    // région n'est PAS un cache — il part sur le disque de l'utilisateur.
    g.bench_function("flate2_zlib_rs", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for c in &clairs {
                n += flate2_deflate(black_box(c)).len();
            }
            black_box(n)
        })
    });

    g.bench_function("miniz_oxide", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for c in &clairs {
                n += miniz_deflate(black_box(c)).len();
            }
            black_box(n)
        })
    });

    g.finish();

    // La taille compte autant que la vitesse : un backend deux fois plus
    // rapide qui produit des fichiers 20 % plus gros n'est pas un gain sur une
    // save de plusieurs gigaoctets.
    let a: usize = clairs.iter().map(|c| flate2_deflate(c).len()).sum();
    let b: usize = clairs.iter().map(|c| miniz_deflate(c).len()).sum();
    eprintln!();
    eprintln!("── taille produite, 64 chunks ──────────────────────────────────");
    eprintln!("  clair              {:>10} o", octets);
    eprintln!("  flate2 / zlib-rs   {:>10} o   ({:.1} %)", a, 100.0 * a as f64 / octets as f64);
    eprintln!("  miniz_oxide        {:>10} o   ({:.1} %)", b, 100.0 * b as f64 / octets as f64);
    eprintln!();

    // Et les deux doivent se relire l'un l'autre : un format, pas deux.
    for c in clairs.iter().take(4) {
        assert_eq!(&flate2_inflate(&miniz_deflate(c)), c);
        assert_eq!(&miniz_inflate(&flate2_deflate(c)), c);
    }
}

fn flate2_deflate(clair: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    e.write_all(clair).unwrap();
    e.finish().unwrap()
}

fn miniz_deflate(clair: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(clair, 6)
}

criterion_group!(benches, bench_backends, bench_compression);
criterion_main!(benches);
