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
use tf_mesh::{
    fluides as passe_fluides, glouton, mailler, mailler_pour_gpu, modeles, Fluide, Formes,
    GenreFluide, TableFormes, Voisinage, COTE,
};

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
            return if k.is_multiple_of(4) { CUBE2 } else { CUBE };
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

/// **La passe de FLUIDES**, sur les profils où elle travaille — et sur celui
/// où elle ne devrait rien coûter.
///
/// Dans la vraie chaîne, une section dont la palette ne porte aucun fluide
/// ne l'appelle même pas (`Grille::porte_du_fluide`) ; « sans fluide » mesure
/// donc ce qu'elle coûterait si l'on s'en passait. Le nombre de faces est
/// imprimé une fois par profil : un temps ne dit rien sans ce qu'il produit.
fn fluides(c: &mut Criterion) {
    let (mut t, modeles) = table();
    let eau = |niveau| Fluide {
        genre: GenreFluide::Eau,
        niveau,
    };
    let source = t.pousser(true, false, Vec::new());
    t.marquer_fluide(source, eau(0));
    let courantes: Vec<StateId> = (1..8)
        .map(|n| {
            let id = t.pousser(true, false, Vec::new());
            t.marquer_fluide(id, eau(n));
            id
        })
        .collect();
    // Le décor INONDÉ : les mêmes modèles, avec de l'eau dans la case.
    let inondes: Vec<StateId> = modeles
        .iter()
        .map(|m| {
            let id = t.pousser(false, false, t.cuboides(*m).to_vec());
            t.marquer_fluide(id, eau(0));
            id
        })
        .collect();
    let f: &dyn Formes = &t;
    let n = COTE as i32;

    let mut profils: Vec<(&str, Voisinage)> = Vec::new();
    // La salle décorée des autres passes : aucun fluide.
    let salle = profils_sans_fluide();
    profils.push(("sans fluide", salle));
    // Le fond de la mer : de l'eau partout, dessus compris — aucune face.
    let mut v = Voisinage::new();
    v.remplir(|_, _, _| source);
    profils.push(("fond de mer", v));
    // Un lac : de l'eau jusqu'à mi-hauteur, de l'air au-dessus, jusque dans
    // la peau. Un seul dessus, fusionné sur toute la section.
    let mut v = Voisinage::new();
    v.remplir(|_, y, _| if y < 8 { source } else { AIR });
    profils.push(("lac", v));
    // Une rivière en pente : des niveaux qui changent d'une case à l'autre,
    // donc des coins tous différents et un courant partout — le pire cas de
    // la fusion.
    let mut v = Voisinage::new();
    v.remplir(|x, y, z| {
        if y < 3 {
            CUBE
        } else if y == 3 {
            courantes[((x + 2 * z).rem_euclid(7)) as usize]
        } else {
            AIR
        }
    });
    profils.push(("rivière", v));
    // Une salle inondée : des murs, de l'eau, et du décor inondé au sol.
    let mut k = 0usize;
    let mut v = Voisinage::new();
    v.remplir(|x, y, z| {
        k = k.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mur = x == 0 || z == 0 || y == 0 || x == n - 1 || z == n - 1;
        if mur {
            return CUBE;
        }
        if y == 1 && k % 100 < 55 {
            return inondes[k % inondes.len()];
        }
        if y < 10 {
            source
        } else {
            AIR
        }
    });
    profils.push(("salle inondée", v));

    let mut g = c.benchmark_group("fluides");
    for (nom, v) in &profils {
        let op = Opacite::relever(v, f);
        let mut faces = Vec::new();
        passe_fluides::mailler_avec(v, f, &op, &mut faces);
        eprintln!("fluides/{nom} : {} faces", faces.len());
        g.bench_function(format!("{nom}/passe"), |b| {
            b.iter_batched_ref(
                Vec::new,
                |out| passe_fluides::mailler_avec(v, f, &op, out),
                BatchSize::SmallInput,
            )
        });
    }
    g.finish();
}

/// **Un océan** : de la pierre, trois sections d'eau pleine, une surface, du
/// ciel — seize chunks de côté. Le cas qui décide ce que coûte l'eau PROFONDE,
/// dont un monde vanilla est couvert aux deux tiers.
fn ocean(c: &mut Criterion) {
    use tf_anvil::{bits_for, pack, Packing, Section};
    let mut t = TableFormes::new();
    let air = t.pousser(true, false, Vec::new());
    let pierre = t.pousser(false, true, Vec::new());
    let eau = t.pousser(true, false, Vec::new());
    t.marquer_fluide(
        eau,
        Fluide {
            genre: GenreFluide::Eau,
            niveau: 0,
        },
    );
    // La surface : de l'eau sur quatorze couches, de l'air au-dessus.
    let surface = {
        let idx: Vec<u16> = (0..4096)
            .map(|i| if i / 256 < 14 { 0 } else { 1 })
            .collect();
        let bits = bits_for(2);
        Section {
            y: 3,
            palette: vec![eau, air],
            bits,
            data: pack(&idx, bits as usize, Packing::NoStraddle).into_boxed_slice(),
            packing: Packing::NoStraddle,
        }
    };
    let cote = 16;
    let mut grille = tf_mesh::Grille::new();
    for cz in 0..cote {
        for cx in 0..cote {
            for sy in -4..=7i8 {
                let s = match sy {
                    -4..=-1 => Section::uniform(sy, pierre),
                    0..=2 => Section::uniform(sy, eau),
                    3 => surface.clone(),
                    _ => Section::uniform(sy, air),
                };
                grille.poser(cx, cz, s);
            }
        }
    }
    let ch = grille.mailler(&t);
    eprintln!(
        "océan : {} lots, {} quads, {} faces de fluide",
        ch.lots.len(),
        ch.quads(),
        ch.fluides()
    );
    let mut g = c.benchmark_group("ocean");
    g.sample_size(20);
    g.bench_function("mailler_1_fil", |b| b.iter(|| grille.mailler(&t)));
    g.finish();
}

/// La salle décorée de `profils`, seule.
fn profils_sans_fluide() -> Voisinage {
    profils()
        .into_iter()
        .find(|(nom, _)| *nom == "salle décorée")
        .map(|(_, v)| v)
        .expect("le profil existe")
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

criterion_group!(benches, passes, complet, fluides, ocean, region);
criterion_main!(benches);
