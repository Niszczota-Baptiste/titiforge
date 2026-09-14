//! Combien coûte le maillage d'un build Minefield, et où part le temps.
//!
//! La chaîne complète, depuis le `.mca` : décompresser, balayer, décoder,
//! bâtir le voisinage, mailler. C'est ce que l'application fera, et les
//! proportions entre ces étapes sont ce qui dimensionne la phase 2.

use std::collections::HashMap;
use std::time::Instant;

use tf_anvil::{decode_section, inflate, read, scan, Interner, StateId};
use tf_bench::catalogue::{Forme, BLOCS, PIRE_CAS};
use tf_bench::{build, Build};
use tf_mesh::forme::Cuboide;
use tf_mesh::{mailler, mailler_pour_gpu, TableFormes, Voisinage};

/// Fabrique un cuboïde plausible pour le n-ième élément d'un modèle.
///
/// On n'a pas la géométrie réelle ici — seulement le NOMBRE de cuboïdes, qui
/// est ce qui coûte. Des cuboïdes inventés mais de la bonne taille mesurent le
/// bon travail ; recopier les modèles du serveur serait redistribuer son pack.
fn cuboide(k: u8, total: u8) -> Cuboide {
    let t = total.max(1) as i32;
    let k = k as i32;
    let bas = (k * 16 / t) as f32;
    let haut = (((k + 1) * 16 / t).max(k * 16 / t + 1)) as f32;
    Cuboide {
        min: [0.0, bas, 0.0],
        max: [16.0, haut.min(16.0), 16.0],
        faces: 0x3F,
        // Seules les faces à ras portent `cullface`, comme dans un vrai modèle.
        cull: 0x3F,
    }
}

fn table_depuis(
    interner: &Interner,
    table: &mut TableFormes,
    connus: &mut HashMap<String, StateId>,
) {
    let formes: HashMap<&str, (Forme, u8)> = BLOCS
        .iter()
        .map(|(n, f, c)| (*n, (*f, *c)))
        .chain(std::iter::once((PIRE_CAS.0, (PIRE_CAS.1, PIRE_CAS.2))))
        .collect();

    for id in 0..interner.len() as StateId {
        let cle = interner.resolve(id).unwrap();
        if connus.contains_key(cle) {
            continue;
        }
        let nu = cle.split('|').next().unwrap();
        let (air, opaque, modele) = match nu {
            "minecraft:air" => (true, false, Vec::new()),
            _ => match formes.get(nu) {
                Some((Forme::Cube, _)) => (false, true, Vec::new()),
                Some((Forme::Modele, n)) => {
                    (false, false, (0..*n).map(|k| cuboide(k, *n)).collect())
                }
                Some((Forme::Vide, _)) | None => (true, false, Vec::new()),
            },
        };
        let attribue = table.pousser(air, opaque, modele);
        debug_assert_eq!(attribue, id, "la table suit l'ordre de l'interner");
        connus.insert(cle.to_string(), attribue);
    }
}

fn main() {
    for (etiquette, b) in [
        ("décor 20 %", Build::petit().avec_decor(20)),
        ("décor 55 %", Build::petit()),
        ("décor 90 %", Build::petit().avec_decor(90)),
    ] {
        let octets = build::region(&b);
        let r = read(&octets, 0, 0).unwrap();

        let mut t_charge = 0u128;
        let mut t_voisinage = 0u128;
        let mut t_maille = 0u128;
        let mut t_gpu = 0u128;
        let mut quads_g = 0usize;
        let mut quads_m = 0usize;
        let mut poses = 0usize;
        let mut sections = 0usize;

        let debut = Instant::now();
        for cz in 0..b.side as i32 {
            for cx in 0..b.side as i32 {
                let t0 = Instant::now();
                let brut = r.get(cx, cz).unwrap();
                let inflated = inflate(&brut.payload, brut.compression).unwrap();
                let sc = scan(&inflated).unwrap();
                let mut interner = Interner::new();
                let mut decodees = Vec::new();
                for s in &sc.sections {
                    if let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap() {
                        decodees.push((s.y, sec));
                    }
                }
                let mut table = TableFormes::new();
                let mut connus = HashMap::new();
                table_depuis(&interner, &mut table, &mut connus);
                t_charge += t0.elapsed().as_nanos();

                for (sy, sec) in &decodees {
                    let t1 = Instant::now();
                    let idx = sec.unpack();
                    let mut v = Voisinage::new();
                    // La peau vient des sections voisines ; ici on la prend
                    // dans la même colonne quand elle existe, et de l'air
                    // sinon. Un vrai hôte irait la chercher chez le voisin.
                    v.remplir(|x, y, z| {
                        if !Voisinage::dedans(x, y, z) {
                            let vy = *sy as i32 * 16 + y;
                            let voisine = decodees
                                .iter()
                                .find(|(s, _)| *s as i32 * 16 <= vy && vy < *s as i32 * 16 + 16);
                            if let Some((s2, sec2)) = voisine {
                                if (0..16).contains(&x) && (0..16).contains(&z) {
                                    let ly = vy - *s2 as i32 * 16;
                                    return sec2
                                        .get(x as usize, ly as usize, z as usize)
                                        .unwrap_or(0);
                                }
                            }
                            return 0;
                        }
                        sec.palette[idx[(y * 256 + z * 16 + x) as usize] as usize]
                    });
                    t_voisinage += t1.elapsed().as_nanos();

                    let t2 = Instant::now();
                    let m = mailler(&v, &table);
                    t_maille += t2.elapsed().as_nanos();
                    quads_g += m.quads_glouton;
                    quads_m += m.quads_modele;

                    let t3 = Instant::now();
                    let (_, inst) = mailler_pour_gpu(&v, &table);
                    t_gpu += t3.elapsed().as_nanos();
                    poses += inst.len();
                    sections += 1;
                }
            }
        }
        let total = debut.elapsed();
        let ms = |n: u128| n as f64 / 1e6;
        let quads = quads_g + quads_m;

        println!("\n╭─ build Minefield · {etiquette} · {} blocs", b.blocs());
        println!(
            "│ total            {:.0} ms sur {sections} sections",
            total.as_secs_f64() * 1000.0
        );
        println!(
            "│   charger        {:8.0} ms  ({:4.1} %)",
            ms(t_charge),
            100.0 * t_charge as f64 / total.as_nanos() as f64
        );
        println!(
            "│   voisinage      {:8.0} ms  ({:4.1} %)",
            ms(t_voisinage),
            100.0 * t_voisinage as f64 / total.as_nanos() as f64
        );
        println!(
            "│   MAILLER        {:8.0} ms  ({:4.1} %)",
            ms(t_maille),
            100.0 * t_maille as f64 / total.as_nanos() as f64
        );
        println!("│ quads            {quads}");
        println!(
            "│   glouton        {quads_g:10}  ({:4.1} %)",
            100.0 * quads_g as f64 / quads as f64
        );
        println!(
            "│   modèles        {quads_m:10}  ({:4.1} %)",
            100.0 * quads_m as f64 / quads as f64
        );
        println!("│");
        println!("│ ── le même maillage, modèles en INSTANCES ──");
        println!(
            "│   temps          {:8.0} ms  (× {:.2})",
            ms(t_gpu),
            t_maille as f64 / t_gpu as f64
        );
        println!(
            "│   éléments       {:10}  (× {:.1} moins que {quads})",
            quads_g + poses,
            quads as f64 / (quads_g + poses) as f64
        );
        println!(
            "│   poses          {poses:10}  soit {:.2} Mo pour le GPU",
            poses as f64 * 8.0 / 1e6
        );
        println!(
            "│   quads de modèles évités : {:.1} Mo à 16 octets",
            quads_m as f64 * 16.0 / 1e6
        );
        println!(
            "╰─ {:.2} µs par section maillée · {:.0} quads par section",
            ms(t_maille) * 1000.0 / sections as f64,
            quads as f64 / sections as f64
        );
    }
}
