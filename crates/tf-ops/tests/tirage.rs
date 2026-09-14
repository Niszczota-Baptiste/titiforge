//! Le tirage : rejouable, sans motif, et aux bonnes proportions.
//!
//! Un hachage de position bon marché est exactement le genre d'optimisation
//! qui passe toutes les moyennes et sort un damier à l'écran. Une proportion
//! juste ne prouve rien : il faut vérifier qu'il n'y a de structure sur AUCUN
//! plan, et qu'une case ne ressemble pas à sa voisine.

use tf_ops::hash3;
use tf_ops::motif::Motif;

/// Le tirage binaire d'une position, pour un mélange 1:1.
fn face(x: i32, y: i32, z: i32, seed: u64) -> bool {
    hash3(x, y, z, seed) & 1 == 1
}

#[test]
fn le_meme_point_et_la_meme_graine_donnent_toujours_la_meme_chose() {
    for (x, y, z) in [
        (0, 0, 0),
        (-1, 64, 7),
        (i32::MIN, 0, i32::MAX),
        (5, -300, 9),
    ] {
        let a = hash3(x, y, z, 42);
        assert_eq!(a, hash3(x, y, z, 42), "({x},{y},{z})");
        assert_ne!(a, hash3(x, y, z, 43), "une autre graine, un autre tirage");
    }
}

#[test]
fn permuter_les_axes_ne_donne_pas_le_meme_tirage() {
    // Sans un multiplicateur par axe, `(x, y, z)` et `(y, x, z)` tomberaient
    // sur la même valeur — et un mur vertical montrerait le motif du sol.
    // Trois composantes DISTINCTES, sinon une permutation rend le triplet de
    // départ et on comparerait une valeur à elle-même.
    for (a, b, c) in [(1, 2, 3), (7, 0, 5), (-4, 9, 12), (100, -100, 3)] {
        let h = hash3(a, b, c, 1);
        assert_ne!(h, hash3(b, a, c, 1));
        assert_ne!(h, hash3(a, c, b, 1));
        assert_ne!(h, hash3(c, b, a, 1));
    }
}

#[test]
fn aucun_plan_ne_montre_de_structure() {
    // Sur un cube de 64³, chaque tranche perpendiculaire à chaque axe doit
    // tirer à peu près à pile ou face. Un hachage qui laisserait passer un bit
    // d'axe sans le mélanger donnerait une tranche à 0 % ou 100 % — c'est LE
    // défaut qu'on voit à l'écran et qu'une moyenne globale cache.
    const N: i32 = 64;
    let seed = 0xDEAD_BEEF;
    for axe in 0..3 {
        for t in 0..N {
            let mut vrais = 0usize;
            for a in 0..N {
                for b in 0..N {
                    let (x, y, z) = match axe {
                        0 => (t, a, b),
                        1 => (a, t, b),
                        _ => (a, b, t),
                    };
                    if face(x, y, z, seed) {
                        vrais += 1;
                    }
                }
            }
            let part = vrais as f64 / (N * N) as f64;
            assert!(
                (part - 0.5).abs() < 0.05,
                "axe {axe}, tranche {t} : {part:.3} au lieu de 0,5"
            );
        }
    }
}

#[test]
fn une_case_ne_ressemble_pas_a_sa_voisine() {
    // La corrélation entre voisins est ce qui fabrique des BANDES. Deux cases
    // adjacentes doivent différer une fois sur deux, sur les trois axes.
    const N: i32 = 48;
    let seed = 7;
    for (dx, dy, dz) in [(1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 1)] {
        let mut differents = 0usize;
        let mut total = 0usize;
        for x in 0..N {
            for y in 0..N {
                for z in 0..N {
                    if face(x, y, z, seed) != face(x + dx, y + dy, z + dz, seed) {
                        differents += 1;
                    }
                    total += 1;
                }
            }
        }
        let part = differents as f64 / total as f64;
        assert!(
            (part - 0.5).abs() < 0.02,
            "décalage ({dx},{dy},{dz}) : {part:.3} de différences au lieu de 0,5"
        );
    }
}

#[test]
fn les_bits_bas_valent_les_bits_hauts() {
    // Le tirage prend les bits HAUTS (méthode de Lemire) mais un masque `& 1`
    // prend les bas : les deux doivent être également mélangés, sinon un test
    // passerait pendant que l'application montre un motif.
    const N: i32 = 40;
    let mut bas = 0usize;
    let mut haut = 0usize;
    let mut total = 0usize;
    for x in 0..N {
        for y in 0..N {
            for z in 0..N {
                let h = hash3(x, y, z, 3);
                bas += (h & 1) as usize;
                haut += (h >> 63) as usize;
                total += 1;
            }
        }
    }
    for (nom, n) in [("bas", bas), ("haut", haut)] {
        let part = n as f64 / total as f64;
        assert!((part - 0.5).abs() < 0.02, "bit {nom} : {part:.3}");
    }
}

#[test]
fn les_proportions_suivent_les_poids_meme_tres_desequilibrees() {
    // Un poids de 1 contre 99 est le cas où un tirage bâclé se voit : il rend
    // zéro, ou beaucoup trop.
    const N: i32 = 64;
    for (a, b, attendu) in [(1u32, 99u32, 0.01), (50, 50, 0.5), (99, 1, 0.99)] {
        let m = Motif::melange(vec![(a, 1), (b, 2)]);
        let tirage = m.tirage();
        let mut premiers = 0usize;
        for x in 0..N {
            for y in 0..N {
                for z in 0..N {
                    if tirage.indice(x, y, z, 11) == Some(0) {
                        premiers += 1;
                    }
                }
            }
        }
        let part = premiers as f64 / (N * N * N) as f64;
        assert!(
            (part - attendu).abs() < 0.01,
            "{a}:{b} → {part:.4} au lieu de {attendu}"
        );
    }
}
