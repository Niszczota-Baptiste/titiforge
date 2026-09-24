//! La fixture « build Minefield » doit rester HONNÊTE.
//!
//! Une fixture qui dérive vers du terrain ferait passer tous les benchs du
//! rendu au vert en mesurant le mauvais chemin. Ces assertions sont là pour
//! échouer le jour où ça arrive.

use std::collections::{BTreeMap, BTreeSet};

use tf_anvil::{decode_section, inflate, read, scan, Interner};
use tf_bench::catalogue::{Forme, BLOCS, CUBOIDES_MOYENS_CODEX, PIRE_CAS};
use tf_bench::{build, Build};

/// Recense ce que la fixture produit vraiment, en relisant le `.mca`.
struct Releve {
    sections: usize,
    homogenes: usize,
    palettes: Vec<usize>,
    blocs: BTreeMap<String, usize>,
    air: usize,
    total: usize,
}

fn relever(b: &Build) -> Releve {
    let octets = build::region(b);
    let r = read(&octets, 0, 0).unwrap();
    let mut out = Releve {
        sections: 0,
        homogenes: 0,
        palettes: Vec::new(),
        blocs: BTreeMap::new(),
        air: 0,
        total: 0,
    };
    for cz in 0..b.side as i32 {
        for cx in 0..b.side as i32 {
            let brut = r.get(cx, cz).expect("chunk présent");
            let inflated = inflate(&brut.payload, brut.compression).unwrap();
            let sc = scan(&inflated).unwrap();
            let mut interner = Interner::new();
            for s in &sc.sections {
                let Some(sec) = decode_section(&inflated, &sc, s, &mut interner).unwrap() else {
                    continue;
                };
                out.sections += 1;
                out.palettes.push(sec.palette.len());
                if sec.palette.len() == 1 {
                    out.homogenes += 1;
                }
                let indices = sec.unpack();
                for i in 0..4096 {
                    let id = sec.palette[indices[i] as usize];
                    let nom = interner.resolve(id).unwrap();
                    let nu = nom.split('|').next().unwrap().to_string();
                    out.total += 1;
                    if nu == "minecraft:air" {
                        out.air += 1;
                    } else {
                        *out.blocs.entry(nu).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    out
}

fn forme_de(nom: &str) -> Option<Forme> {
    BLOCS.iter().find(|(n, ..)| *n == nom).map(|(_, f, _)| *f)
}

#[test]
fn le_build_ressemble_a_un_batiment_pas_a_du_terrain() {
    let b = Build::petit();
    let r = relever(&b);

    let plein = 100.0 * (r.total - r.air) as f64 / r.total as f64;
    assert!(
        (4.0..40.0).contains(&plein),
        "un bâtiment est surtout du vide, mais pas vide : {plein:.1} % de blocs posés"
    );

    let part_homogene = 100.0 * r.homogenes as f64 / r.sections as f64;
    assert!(
        part_homogene < 75.0,
        "trop de sections homogènes ({part_homogene:.0} %) : c'est le profil du \
         terrain, pas d'un build, et le maillage n'y travaillerait jamais"
    );

    let bâties: Vec<usize> = r.palettes.iter().copied().filter(|n| *n > 1).collect();
    let mediane = {
        let mut v = bâties.clone();
        v.sort_unstable();
        v[v.len() / 2]
    };
    assert!(
        (5..=80).contains(&mediane),
        "palette médiane d'une section bâtie : {mediane} entrées — hors de ce \
         qu'un build produit, donc largeurs de bits irréalistes"
    );
}

#[test]
fn les_blocs_modeles_dominent_comme_dans_le_codex() {
    let b = Build::petit();
    let r = relever(&b);

    let distincts: BTreeSet<&String> = r.blocs.keys().collect();
    let mut par_forme = BTreeMap::new();
    for n in &distincts {
        let f = forme_de(n).unwrap_or_else(|| panic!("{n} n'est pas au catalogue"));
        *par_forme.entry(f).or_insert(0usize) += 1;
    }
    let modeles = *par_forme.get(&Forme::Modele).unwrap_or(&0);
    assert!(
        modeles * 2 > distincts.len(),
        "les blocs-modèles sont deux tiers du catalogue Minefield : ils doivent \
         dominer les états DISTINCTS du build ({modeles} sur {})",
        distincts.len()
    );

    // En VOLUME c'est l'inverse, et c'est voulu : un mur est fait de cubes,
    // le décor est semé. Le maillage doit voir les deux régimes.
    let volume_modele: usize = r
        .blocs
        .iter()
        .filter(|(n, _)| forme_de(n) == Some(Forme::Modele))
        .map(|(_, c)| *c)
        .sum();
    let pose = r.total - r.air;
    let part = 100.0 * volume_modele as f64 / pose as f64;
    assert!(
        (1.0..50.0).contains(&part),
        "le décor doit être MINORITAIRE en volume et présent partout : {part:.1} %"
    );
}

#[test]
fn le_build_se_rejoue_a_l_identique() {
    let b = Build::minuscule();
    assert_eq!(
        build::region(&b),
        build::region(&b),
        "deux appels doivent rendre les mêmes octets"
    );
}

#[test]
fn le_tirage_ne_depend_pas_de_l_ordre_de_parcours() {
    // L'invariant n° 5 : un tirage par bloc se hache sur la POSITION. S'il
    // dépendait d'un générateur à état, lire les cases dans un autre ordre
    // donnerait un autre build — et une parallélisation le casserait sans bruit.
    let b = Build::minuscule();
    let avant: Vec<_> = (0..24)
        .flat_map(|y| (0..24).map(move |x| (x, y)))
        .map(|(x, y)| b.bloc(x, y, 3))
        .collect();
    let apres: Vec<_> = (0..24)
        .flat_map(|y| (0..24).map(move |x| (x, y)))
        .rev()
        .map(|(x, y)| b.bloc(x, y, 3))
        .collect();
    let mut apres = apres;
    apres.reverse();
    assert_eq!(avant, apres);
}

#[test]
fn une_graine_differente_donne_un_autre_build() {
    let a = Build::minuscule();
    let b = Build {
        seed: 4242,
        ..Build::minuscule()
    };
    assert_ne!(build::region(&a), build::region(&b));
}

#[test]
fn le_catalogue_couvre_les_trois_formes_et_le_pire_cas() {
    let mut par_forme = BTreeMap::new();
    let mut pire = 0u8;
    for (nom, f, n) in BLOCS {
        *par_forme.entry(*f).or_insert(0usize) += 1;
        pire = pire.max(*n);
        assert!(
            nom.starts_with("minefield:"),
            "{nom} : le catalogue décrit la cible PRINCIPALE"
        );
    }
    let total = BLOCS.len();
    let modeles = par_forme[&Forme::Modele];
    let cubes = par_forme[&Forme::Cube];
    // Le pack réel : 73,7 % de modèles, 25,1 % de cubes — un cube que ses
    // TEXTURES trouent compte comme modèle. On tolère 3 points.
    let pm = 100.0 * modeles as f64 / total as f64;
    let pc = 100.0 * cubes as f64 / total as f64;
    assert!(
        (70.7..76.7).contains(&pm),
        "la table doit garder la forme du pack : {pm:.1} % de modèles au lieu de 73,7"
    );
    assert!(
        (22.1..28.1).contains(&pc),
        "{pc:.1} % de cubes au lieu de 25,1"
    );
    let _ = pire;

    // La forme de la distribution, pas seulement ses proportions. Un premier
    // échantillon forçait les blocs les plus lourds en tête : moyenne 7,58
    // contre 3,58, soit un bench deux fois plus pessimiste que la cible.
    let cub: Vec<u32> = BLOCS
        .iter()
        .filter(|(_, f, _)| *f == Forme::Modele)
        .map(|(_, _, c)| *c as u32)
        .collect();
    let moyenne = 100 * cub.iter().sum::<u32>() / cub.len() as u32;
    let ecart = moyenne.abs_diff(CUBOIDES_MOYENS_CODEX) * 100 / CUBOIDES_MOYENS_CODEX;
    assert!(
        ecart <= 15,
        "cuboïdes par modèle : {:.2} ici contre {:.2} dans le codex ({ecart} % \
         d'écart). Une fixture fausse dans le sens prudent reste fausse — elle \
         ferait rejeter une optimisation qui suffisait.",
        moyenne as f64 / 100.0,
        CUBOIDES_MOYENS_CODEX as f64 / 100.0
    );
}

#[test]
fn le_pire_cas_est_a_part_et_pas_dans_la_moyenne() {
    assert_eq!(PIRE_CAS.2, 82);
    assert_eq!(PIRE_CAS.1, Forme::Modele);
    assert!(
        !BLOCS.iter().any(|(n, ..)| *n == PIRE_CAS.0),
        "un bloc à 82 cuboïdes sur 147 tirerait la moyenne de 3,58 à 4,12 : il \
         mérite sa propre mesure, pas de peser sur toutes les autres"
    );
}

#[test]
fn un_bloc_modele_n_est_jamais_opaque() {
    for (nom, f, _) in BLOCS {
        assert_eq!(
            f.opaque(),
            *f == Forme::Cube,
            "{nom} : un bloc non-cube marqué opaque effacerait les faces de ses \
             voisins — l'escalier creuserait un trou dans le mur qu'il touche"
        );
    }
}

/// **Un build posé ailleurs annonce où il est, et reste le MÊME bâtiment.**
///
/// Le contenu d'un `.mca` porte ses propres coordonnées. Écrire la même région
/// dans `r.2.-1.mca` sans corriger `xPos`/`zPos` ferait trouver, à tout ce qui
/// lit le contenu, plusieurs régions empilées au même endroit — le piège déjà
/// fermé pour `Terrain::region_en`, et qu'un vol mesuré sur deux régions
/// bâties aurait rouvert. Mais le bâtiment lui-même ne doit pas bouger : deux
/// mesures prises à deux endroits du monde doivent rester comparables.
#[test]
fn un_build_pose_ailleurs_annonce_sa_place_et_garde_son_contenu() {
    let b = Build::minuscule();
    let (rx, rz) = (2, -1);
    let (octets_ici, octets_la) = (build::region(&b), build::region_en(&b, rx, rz));
    let ici = read(&octets_ici, 0, 0).unwrap();
    let la = read(&octets_la, rx, rz).unwrap();

    let mut vus = 0;
    for cz in 0..b.side as i32 {
        for cx in 0..b.side as i32 {
            let (Some(a), Some(z)) = (ici.get(cx, cz), la.get(cx, cz)) else {
                continue;
            };
            let a = inflate(&a.payload, a.compression).unwrap();
            let z = inflate(&z.payload, z.compression).unwrap();
            let (sa, sz) = (scan(&a).unwrap(), scan(&z).unwrap());
            assert_eq!(
                (sz.x_pos, sz.z_pos),
                (Some(rx * 32 + cx), Some(rz * 32 + cz)),
                "le chunk ({cx}, {cz}) de r.{rx}.{rz} doit annoncer sa place MONDE"
            );
            assert_eq!((sa.x_pos, sa.z_pos), (Some(cx), Some(cz)));
            // Le même contenu, section par section, dans une même table d'états.
            let mut i = Interner::new();
            for (x, y) in sa.sections.iter().zip(sz.sections.iter()) {
                let p = decode_section(&a, &sa, x, &mut i).unwrap();
                let q = decode_section(&z, &sz, y, &mut i).unwrap();
                assert_eq!(
                    p, q,
                    "le chunk ({cx}, {cz}) a changé de contenu en changeant de place"
                );
            }
            vus += 1;
        }
    }
    assert_eq!(vus, (b.side * b.side) as usize, "tous les chunks comparés");
}
