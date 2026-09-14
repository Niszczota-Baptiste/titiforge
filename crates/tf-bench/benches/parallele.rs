//! Ce que le parallélisme rapporte vraiment sur le chargement.
//!
//! `inflate` pèse 75 % du chargement d'une région et aucun backend n'y change
//! rien (voir `inflate.rs`). Le seul levier restant est le nombre de cœurs —
//! la décompression est parfaitement parallélisable par chunk, puisque chaque
//! chunk est un flux zlib indépendant.
//!
//! Ce bench mesure le gain AVANT qu'on décide où le parallélisme doit vivre.
//! `tf-anvil` est un crate de FORMAT : il décrit ce qu'un fichier contient, pas
//! comment l'exploiter. Décider de lancer huit fils est une politique, et une
//! politique appartient à l'appelant. Mais une politique dont on n'a pas mesuré
//! le rendement est une supposition.
//!
//! Point de conception que la mesure met au jour : l'interner est de l'état
//! MUTABLE PARTAGÉ. Le mettre derrière un verrou sérialiserait exactement ce
//! qu'on essaie de paralléliser. Chaque fil interne donc dans une table locale,
//! et la fusion se fait après — c'est ce que `tf-world` devra reprendre.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use rayon::prelude::*;
use std::hint::black_box;

use tf_anvil::{decode_section, inflate, read, scan, Interner, Section};
use tf_bench::{region, Terrain};

/// Un chunk décodé : ses sections, et la table d'états LOCALE qui les nomme.
struct ChunkDecode {
    sections: Vec<Section>,
    local: Interner,
}

fn decode_chunk(payload: &[u8], compression: tf_anvil::Compression) -> Option<ChunkDecode> {
    let inflated = inflate(payload, compression).ok()?;
    let s = scan(&inflated).ok()?;
    let mut local = Interner::new();
    let mut sections = Vec::with_capacity(s.sections.len());
    for sc in &s.sections {
        if let Ok(Some(sec)) = decode_section(&inflated, &s, sc, &mut local) {
            sections.push(sec);
        }
    }
    Some(ChunkDecode { sections, local })
}

/// Refond les identifiants locaux dans une table globale.
///
/// Le coût réel du découpage : il faut bien réunifier les palettes après coup.
/// S'il dépassait le gain, le parallélisme ne vaudrait rien — c'est exactement
/// ce qui est arrivé à `we-engine`, où transférer l'arbre NBT coûtait plus que
/// de le décoder.
fn fusionner(parts: Vec<ChunkDecode>) -> (Vec<Section>, Interner) {
    let mut global = Interner::new();
    let mut out = Vec::new();
    for mut part in parts {
        // La table se calcule UNE fois par chunk et s'applique à chacune de
        // ses sections. Les refondre ensemble la ferait recalculer par section.
        let table = global.merge_from(&part.local);
        for s in part.sections.iter_mut() {
            Interner::remap_palette(&table, &mut s.palette);
        }
        out.extend(part.sections);
    }
    (out, global)
}

fn bench_parallele(c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let src = region(&t);
    let r = read(&src, 0, 0).unwrap();
    let charges: Vec<(Vec<u8>, tf_anvil::Compression)> = r
        .iter()
        .map(|c| (c.payload.to_vec(), c.compression))
        .collect();

    let mut g = c.benchmark_group("chargement_parallele");
    g.sample_size(10);
    g.throughput(Throughput::Elements(t.sections_total() as u64));

    g.bench_function("1_monofil", |b| {
        b.iter(|| {
            let parts: Vec<ChunkDecode> = charges
                .iter()
                .filter_map(|(p, comp)| decode_chunk(black_box(p), *comp))
                .collect();
            black_box(fusionner(parts).0.len())
        })
    });

    g.bench_function("2_rayon", |b| {
        b.iter(|| {
            let parts: Vec<ChunkDecode> = charges
                .par_iter()
                .filter_map(|(p, comp)| decode_chunk(black_box(p), *comp))
                .collect();
            black_box(fusionner(parts).0.len())
        })
    });

    // Le décodage seul, sans la fusion : dit combien coûte la réunification des
    // palettes, donc si le découpage se paie lui-même.
    g.bench_function("3_rayon_sans_fusion", |b| {
        b.iter(|| {
            let parts: Vec<ChunkDecode> = charges
                .par_iter()
                .filter_map(|(p, comp)| decode_chunk(black_box(p), *comp))
                .collect();
            black_box(parts.len())
        })
    });

    g.finish();

    eprintln!();
    eprintln!("── cœurs disponibles : {} ──", rayon::current_num_threads());

    // Et le parallélisme doit donner EXACTEMENT le même monde. Un décodage
    // rapide qui rend autre chose n'est pas un décodage rapide.
    let seq = fusionner(
        charges
            .iter()
            .filter_map(|(p, comp)| decode_chunk(p, *comp))
            .collect(),
    );
    let par = fusionner(
        charges
            .par_iter()
            .filter_map(|(p, comp)| decode_chunk(p, *comp))
            .collect(),
    );
    assert_eq!(seq.0.len(), par.0.len(), "même nombre de sections");
    assert_eq!(seq.1.len(), par.1.len(), "même nombre d'états distincts");
    for (a, b) in seq.0.iter().zip(par.0.iter()) {
        assert_eq!(a.palette.len(), b.palette.len());
        assert_eq!(a.data, b.data, "les indices doivent être identiques");
    }
    // `collect` de rayon préserve l'ordre : les identifiants internés doivent
    // donc coïncider un à un, et pas seulement en nombre.
    for i in 0..seq.1.len() as u32 {
        assert_eq!(seq.1.resolve(i), par.1.resolve(i), "état {i}");
    }
    eprintln!("── séquentiel et parallèle produisent le même monde ──");
    eprintln!();
}

criterion_group!(benches, bench_parallele);
criterion_main!(benches);
