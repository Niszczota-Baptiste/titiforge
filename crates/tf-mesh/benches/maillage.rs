//! Le coût du maillage, par passe et par profil de section.
//!
//! Séparer les passes n'est pas cosmétique : la gloutonne produit 10 % des
//! quads et prenait l'essentiel du temps, ce qu'aucune mesure globale ne
//! montrait. Et le profil compte autant que la passe — une section vide, un
//! mur plein et une salle décorée ne coûtent pas la même chose, et c'est le
//! rapport entre les trois qui dit si le coût suit le CONTENU ou le VOLUME.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::collections::HashMap;

use tf_anvil::StateId;
use tf_bench::catalogue::{Forme, BLOCS};
use tf_bench::{build, Build};
use tf_mesh::forme::Cuboide;
use tf_mesh::opacite::Opacite;
use tf_mesh::{glouton, mailler, mailler_pour_gpu, modeles, Formes, TableFormes, Voisinage, COTE};

/// Les identifiants que les scénarios partagent.
const AIR: StateId = 0;
const CUBE: StateId = 1;
const CUBE2: StateId = 2;

/// Table de formes : air, deux cubes, puis un bloc-modèle par entrée du
/// catalogue, avec son VRAI nombre de cuboïdes.
fn table() -> (TableFormes, Vec<StateId>) {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t.pousser(false, true, Vec::new());
    let mut modeles = Vec::new();
    for (_, f, n) in BLOCS {
        if *f != Forme::Modele {
            continue;
        }
        let cub: Vec<Cuboide> = (0..*n)
            .map(|k| {
                let t = (*n).max(1) as i32;
                let bas = (k as i32 * 16 / t) as f32;
                let haut = (((k as i32 + 1) * 16 / t).max(bas as i32 + 1).min(16)) as f32;
                Cuboide {
                    min: [0.0, bas, 0.0],
                    max: [16.0, haut, 16.0],
                    faces: 0x3F,
                    cull: 0x3F,
                }
            })
            .collect();
        modeles.push(t.pousser(false, false, cub));
    }
    (t, modeles)
}

/// Les profils de section qu'on mesure.
fn profils() -> Vec<(&'static str, Voisinage)> {
    let (_, modeles) = table();
    let n = COTE as i32;
    let mut out = Vec::new();

    // 1. Vide. Le plancher : ce qu'une section sans rien coûte quand même.
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| AIR);
    out.push(("vide", v));

    // 2. Pleine de cubes. Six quads en sortie, 4 096 blocs en entrée.
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| CUBE);
    out.push(("pleine", v));

    // 3. Un mur : ce que le glouton sait le mieux faire.
    let mut v = Voisinage::new();
    v.remplir(|x, y, z| {
        if Voisinage::adressable(x, y, z) && z == 8 && (0..n).contains(&x) && (0..n).contains(&y) {
            CUBE
        } else {
            AIR
        }
    });
    out.push(("mur", v));

    // 4. Une salle décorée : le cas réel, murs de cubes et décor en modèles.
    let mut k = 0usize;
    let mut v = Voisinage::new();
    v.remplir(|x, y, z| {
        k = k.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mur = x == 0 || z == 0 || y == 0 || x == n - 1 || z == n - 1;
        if mur {
            return if k % 4 == 0 { CUBE2 } else { CUBE };
        }
        if y == 1 && k % 100 < 55 {
            return modeles[k % modeles.len()];
        }
        AIR
    });
    out.push(("salle décorée", v));

    // 5. Du bruit : le pire cas du glouton, rien ne fusionne.
    let mut k = 1usize;
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| {
        k = k.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        match k % 3 {
            0 => CUBE,
            1 => CUBE2,
            _ => AIR,
        }
    });
    out.push(("bruit", v));

    out
}

fn passes(c: &mut Criterion) {
    let (t, _) = table();
    let f: &dyn Formes = &t;

    let mut g = c.benchmark_group("passes");
    for (nom, v) in profils() {
        let op = Opacite::relever(&v, f);

        g.bench_function(format!("{nom}/opacite"), |b| {
            b.iter(|| Opacite::relever(&v, f))
        });
        g.bench_function(format!("{nom}/glouton"), |b| {
            b.iter_batched_ref(
                tf_mesh::Maillage::new,
                |m| glouton::mailler_avec(&v, f, &op, m),
                BatchSize::SmallInput,
            )
        });
        g.bench_function(format!("{nom}/modeles_quads"), |b| {
            b.iter_batched_ref(
                tf_mesh::Maillage::new,
                |m| modeles::mailler_avec(&v, f, &op, m),
                BatchSize::SmallInput,
            )
        });
        g.bench_function(format!("{nom}/modeles_instances"), |b| {
            b.iter_batched_ref(
                tf_mesh::Instances::new,
                |i| modeles::instancier_avec(&v, f, &op, i),
                BatchSize::SmallInput,
            )
        });
    }
    g.finish();
}

fn complet(c: &mut Criterion) {
    let (t, _) = table();
    let f: &dyn Formes = &t;
    let mut g = c.benchmark_group("complet");
    for (nom, v) in profils() {
        g.bench_function(format!("{nom}/quads"), |b| b.iter(|| mailler(&v, f)));
        g.bench_function(format!("{nom}/gpu"), |b| b.iter(|| mailler_pour_gpu(&v, f)));
    }
    g.finish();
}

/// Une région bâtie, de bout en bout : c'est le chiffre qui compte pour
/// l'utilisateur, et le seul qui intègre le coût de lecture.
///
/// La peau est VRAIE ici — elle traverse les chunks. La remplir d'air, comme
/// le faisait la première mesure, invente un mur de faces fantômes le long de
/// chaque frontière et gonfle le compte de quads.
fn region(c: &mut Criterion) {
    let b = Build::petit();
    let octets = build::region(&b);
    let (grille, table) = charger(&octets, &b);

    let mut g = c.benchmark_group("region");
    g.sample_size(10);
    g.bench_function("mailler_1_fil", |bencher| {
        bencher.iter(|| grille.mailler(&table))
    });
    g.bench_function("mailler_paralleles", |bencher| {
        bencher.iter(|| grille.mailler_parallele(&table))
    });
    g.finish();
}

/// Décode une région entière dans une grille, et bâtit la table de formes.
fn charger(octets: &[u8], b: &Build) -> (tf_mesh::Grille, TableFormes) {
    use tf_anvil::{decode_section, inflate, read, scan, Interner};
    let r = read(octets, 0, 0).unwrap();
    let formes: HashMap<&str, (Forme, u8)> = BLOCS.iter().map(|(n, f, c)| (*n, (*f, *c))).collect();
    let mut grille = tf_mesh::Grille::new();
    // Un SEUL interner pour toute la région : deux tables donneraient des
    // identifiants qui ne veulent rien dire l'un chez l'autre, et la peau
    // entre deux chunks poserait les mauvais blocs.
    let mut interner = Interner::new();
    for cz in 0..b.side as i32 {
        for cx in 0..b.side as i32 {
            let brut = r.get(cx, cz).unwrap();
            let inflated = inflate(&brut.payload, brut.compression).unwrap();
            let sc = scan(&inflated).unwrap();
            for s in &sc.sections {
                if let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap() {
                    grille.poser(cx, cz, sec);
                }
            }
        }
    }
    let mut t = TableFormes::new();
    for id in 0..interner.len() as StateId {
        let cle = interner.resolve(id).unwrap();
        let nu = cle.split('|').next().unwrap();
        match nu {
            "minecraft:air" => t.pousser(true, false, Vec::new()),
            _ => match formes.get(nu) {
                Some((Forme::Cube, _)) => t.pousser(false, true, Vec::new()),
                Some((Forme::Modele, n)) => t.pousser(
                    false,
                    false,
                    (0..*n)
                        .map(|k| {
                            let tt = (*n).max(1) as i32;
                            let bas = (k as i32 * 16 / tt) as f32;
                            let haut =
                                (((k as i32 + 1) * 16 / tt).max(bas as i32 + 1).min(16)) as f32;
                            Cuboide {
                                min: [0.0, bas, 0.0],
                                max: [16.0, haut, 16.0],
                                faces: 0x3F,
                                cull: 0x3F,
                            }
                        })
                        .collect(),
                ),
                _ => t.pousser(true, false, Vec::new()),
            },
        };
    }
    (grille, t)
}

criterion_group!(benches, passes, complet, region);
criterion_main!(benches);
