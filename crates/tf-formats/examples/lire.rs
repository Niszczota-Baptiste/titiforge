//! Ce qu'un fichier d'échange contient, tel que titiforge le lit : le format,
//! la boîte, l'ancre, les états les plus fréquents, les block entities, les
//! entités — et ce que la lecture n'a pas su porter.
//!
//! C'est la vérification « contre un fichier produit par l'outil d'origine » :
//! un `.litematic` sauvé par Litematica, un `.schem` de WorldEdit, un `.nbt`
//! de bloc de structure.
//!
//! ```text
//! cargo run --release -p tf-formats --example lire -- <fichier>
//! ```

use std::collections::BTreeMap;

use tf_anvil::Interner;

fn main() {
    let Some(chemin) = std::env::args().nth(1) else {
        eprintln!("usage : lire <fichier .schem | .litematic | .nbt>");
        std::process::exit(2);
    };
    let octets = std::fs::read(&chemin).unwrap_or_else(|e| {
        eprintln!("{chemin} : {e}");
        std::process::exit(1);
    });
    let mut interner = Interner::new();
    let t0 = std::time::Instant::now();
    let lu = match tf_formats::lire(&octets, &mut interner) {
        Ok(lu) => lu,
        Err(e) => {
            eprintln!("{chemin} : {e}");
            std::process::exit(1);
        }
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let p = &lu.presse;
    println!("{chemin}");
    println!("  lu comme : {} en {ms:.0} ms", lu.lecture);
    if let Some(dv) = lu.data_version {
        println!("  DataVersion : {dv}");
    }
    if let Some(n) = &lu.nom {
        println!("  nom : {n}");
    }
    if let Some(a) = &lu.auteur {
        println!("  auteur : {a}");
    }
    println!(
        "  boîte : {} × {} × {} = {} cases, ancre {:?}",
        p.taille[0],
        p.taille[1],
        p.taille[2],
        p.blocs.len(),
        p.ancre
    );
    let mut compte: BTreeMap<&str, usize> = BTreeMap::new();
    for &id in &p.blocs {
        *compte
            .entry(interner.resolve(id).unwrap_or("?"))
            .or_default() += 1;
    }
    let mut v: Vec<_> = compte.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  {} états distincts ; les plus fréquents :", v.len());
    for (cle, n) in v.iter().take(12) {
        println!("    {n:>10}  {cle}");
    }
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    for e in &p.entites {
        *ids.entry(e.id().unwrap_or("?").to_string()).or_default() += 1;
    }
    println!("  {} block entities : {ids:?}", p.entites.len());
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    for m in &p.mobiles {
        *ids.entry(m.id().unwrap_or("?").to_string()).or_default() += 1;
    }
    println!("  {} entités : {ids:?}", p.mobiles.len());
    for m in p
        .mobiles
        .iter()
        .filter(|m| m.corps[0].tuile.is_some())
        .take(5)
    {
        let k = &m.corps[0];
        println!(
            "    {} à {:?}, accroché en {:?}",
            m.id().unwrap_or("?"),
            k.pos.map(|p| p.v),
            k.tuile.as_ref().map(|t| t.v)
        );
    }
    if lu.remarques.is_empty() {
        println!("  rien qui n'ait pas suivi");
    }
    for r in &lu.remarques {
        println!("  ! {r}");
    }
}
