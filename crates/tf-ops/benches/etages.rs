//! Ce que chaque étage coûte, sur une RÉGION PLEINE.
//!
//! Le but n'est pas de mesurer une opération : c'est de vérifier que la
//! répartition **se déclenche**. Le prototype a fait l'erreur exactement une
//! fois — il annonçait un chemin rapide, et la mesure a répondu « 0 sections
//! rapides sur 9 216 ». Un bench qui ne compte pas les étages ne dit rien.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};

use tf_anvil::chunk::{decode_section, scan};
use tf_anvil::codec::inflate;
use tf_anvil::region::read;
use tf_anvil::{Interner, Section, StateId};
use tf_bench::{region, Terrain};
use tf_ops::plan::{Etage, Plan};
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, SectionPos};

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

/// Une sélection qui couvre tout, et une qui rogne d'un bloc.
fn partout() -> BBox {
    BBox::new(
        BlockPos {
            x: i32::MIN / 2,
            y: i32::MIN / 2,
            z: i32::MIN / 2,
        },
        BlockPos {
            x: i32::MAX / 2,
            y: i32::MAX / 2,
            z: i32::MAX / 2,
        },
    )
}

fn joue(plan: &Plan, secs: &mut [Section], sel: &BBox) -> usize {
    let mut n = 0;
    for (i, s) in secs.iter_mut().enumerate() {
        let pos = SectionPos {
            x: (i % 32) as i32,
            y: (i / 32 % 24) as i32 - 4,
            z: (i / 768) as i32,
        };
        if plan.appliquer(s, sel, pos).etage != Etage::Rien {
            n += 1;
        }
    }
    n
}

fn compte_etages(plan: &Plan, secs: &[Section], sel: &BBox) -> [usize; 4] {
    let mut c = [0usize; 4];
    for (i, s) in secs.iter().enumerate() {
        let pos = SectionPos {
            x: (i % 32) as i32,
            y: (i / 32 % 24) as i32 - 4,
            z: (i / 768) as i32,
        };
        let k = match plan.etage(s, sel, pos) {
            Etage::Rien => 0,
            Etage::Section => 1,
            Etage::Palette => 2,
            Etage::Bloc => 3,
        };
        c[k] += 1;
    }
    c
}

fn bench(c: &mut Criterion) {
    let t = Terrain::region_pleine();
    let src = region(&t);
    let (sections, interner) = sections_de(&src);
    let pierre: StateId = interner.get("minecraft:stone").expect("la fixture en a");
    let terre: StateId = interner.get("minecraft:dirt").expect("et de la terre");

    let sel = partout();
    // Une sélection décalée d'un bloc : aucune section n'est entièrement
    // couverte, tout descend à l'étage bloc.
    let borde = BBox::new(
        BlockPos { x: 1, y: 1, z: 1 },
        BlockPos {
            x: 511,
            y: 318,
            z: 511,
        },
    );

    let remplacer = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let poser = Plan::nouveau(Masque::Tout, Motif::Bloc(terre));
    let melanger =
        Plan::nouveau(Masque::Tout, Motif::melange(vec![(3, pierre), (1, terre)])).avec_seed(7);
    let absent = Plan::nouveau(Masque::Etat(StateId::MAX), Motif::Bloc(terre));

    // On ANNONCE la répartition avant de mesurer : un chiffre sans le compte
    // des étages ne prouve pas que le chemin rapide s'est déclenché.
    for (nom, p, s) in [
        ("remplacer (palette)", &remplacer, &sel),
        ("poser (section)", &poser, &sel),
        ("mélanger (bloc)", &melanger, &sel),
        ("remplacer, sélection bordée", &remplacer, &borde),
        ("cible absente", &absent, &sel),
    ] {
        let [rien, section, palette, bloc] = compte_etages(p, &sections, s);
        println!(
            "{nom:32} rien {rien:5} · section {section:5} · palette {palette:5} · bloc {bloc:5}"
        );
    }

    let mut g = c.benchmark_group("etages");
    g.sample_size(10);
    g.throughput(Throughput::Elements(t.blocs() as u64));

    for (nom, plan, sel) in [
        ("1_cible_absente", &absent, &sel),
        ("2_set_uniforme", &poser, &sel),
        ("3_replace_palette", &remplacer, &sel),
        ("4_replace_bloc_borde", &remplacer, &borde),
        ("5_melange_bloc", &melanger, &sel),
    ] {
        g.bench_function(nom, |b| {
            b.iter_batched_ref(
                || sections.clone(),
                |secs| black_box(joue(plan, secs, sel)),
                BatchSize::LargeInput,
            )
        });
    }

    // Le COMPTAGE exact à l'étage palette : c'est le parcours qu'on vient
    // d'éviter, et le chiffre dit s'il faut le proposer ou l'imposer.
    let compte = remplacer.clone().en_comptant();
    g.bench_function("6_replace_palette_en_comptant", |b| {
        b.iter_batched_ref(
            || sections.clone(),
            |secs| black_box(joue(&compte, secs, &sel)),
            BatchSize::LargeInput,
        )
    });

    g.finish();
}

criterion_group!(etages, bench);
criterion_main!(etages);
