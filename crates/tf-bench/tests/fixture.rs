//! La fixture de mesure doit être un vrai fichier de région.
//!
//! Un bench qui tourne sur une fixture cassée mesure le chemin d'erreur, et
//! annonce des chiffres flatteurs.

use tf_anvil::{decode_section, inflate, read, scan, Interner, Layout, Packing};
use tf_bench::{region, Terrain};

#[test]
fn la_fixture_produit_une_region_lisible_et_complete() {
    let t = Terrain::petite();
    let src = region(&t);
    let r = read(&src, 0, 0).unwrap();
    assert_eq!(r.count(), (t.side * t.side) as usize);

    let mut sections = 0usize;
    let mut homogenes = 0usize;
    let mut interner = Interner::new();

    for c in r.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        assert_eq!(s.layout, Layout::Flat);
        assert_eq!(s.data_version, 3465);
        assert_eq!(s.sections.len(), t.sections);
        assert_eq!(s.x_pos, Some(c.local_x()));

        for sc in &s.sections {
            let section = decode_section(&inflated, &s, sc, &mut interner)
                .unwrap()
                .expect("chaque section porte des blocs");
            sections += 1;
            if section.is_uniform() {
                homogenes += 1;
            }
        }
    }

    assert_eq!(sections, t.sections_total());
    // Le terrain doit contenir des sections HOMOGÈNES : c'est la moitié d'un
    // vrai monde, et c'est le cas que le format optimise. Une fixture sans
    // elles mesurerait un monde qui n'existe pas.
    assert!(
        homogenes > 0,
        "aucune section homogène : la fixture est du bruit"
    );
    assert!(homogenes < sections, "et pas QUE des sections homogènes");

    // Une palette de terrain fait une dizaine d'entrées, pas des milliers.
    assert!(
        (5..50).contains(&interner.len()),
        "{} états distincts — une vraie région en a quelques dizaines",
        interner.len()
    );
}

#[test]
fn la_fixture_est_reproductible_depuis_sa_graine() {
    let t = Terrain::petite();
    assert_eq!(region(&t), region(&t), "même graine, mêmes octets");
    let autre = Terrain { seed: 99, ..t };
    assert_ne!(
        region(&t),
        region(&autre),
        "graine différente, autre contenu"
    );
}

#[test]
fn la_fixture_sait_produire_le_packing_ancien() {
    let t = Terrain {
        packing: Packing::Straddle,
        ..Terrain::petite()
    };
    let src = region(&t);
    let r = read(&src, 0, 0).unwrap();
    let c = r.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();
    assert_eq!(
        s.packing,
        Packing::Straddle,
        "détecté sur la longueur du tableau"
    );
}

#[test]
fn une_region_pleine_fait_bien_cent_millions_de_blocs() {
    let t = Terrain::region_pleine();
    assert_eq!(t.blocs(), 100_663_296, "512 × 384 × 512");
    assert_eq!(t.sections_total(), 24_576);
}
