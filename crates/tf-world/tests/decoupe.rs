//! Le découpage : voir la grille, et s'y aligner.
//!
//! Deux niveaux du même code — le chunk (16) que Minecraft montre avec F3+G,
//! et la région (512) qu'il ne montre PAS et qui décide pourtant ce qu'on
//! exporte et ce qu'une opération réécrit.

use std::collections::BTreeSet;
use tf_world::coords::{BBox, BlockPos};
use tf_world::decoupe::{cellules_autour, Niveau, RAYON_MAX};

fn p(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(x, y, z)
}

fn boite(a: (i32, i32, i32), b: (i32, i32, i32)) -> BBox {
    BBox::new(p(a.0, a.1, a.2), p(b.0, b.1, b.2))
}

// ── la cellule d'un bloc ────────────────────────────────────────────────────

/// **Le bloc −1 est dans le chunk −1, pas le chunk 0.**
///
/// L'invariant du dépôt, ici aussi. Une division qui tronque vers zéro fait
/// sauter la grille d'une cellule au passage de l'origine, et seulement du
/// côté négatif : un défaut qui ne se voit que dans un quart du monde, et
/// qu'on met longtemps à reproduire.
#[test]
fn la_cellule_se_prend_en_division_plancher() {
    for (bloc, chunk) in [
        (0, 0),
        (15, 0),
        (16, 1),
        (-1, -1),
        (-16, -1),
        (-17, -2),
        (511, 31),
        (-512, -32),
    ] {
        assert_eq!(
            Niveau::Chunk.cellule_axe(bloc),
            chunk,
            "le bloc {bloc} est dans le chunk {chunk}"
        );
    }
    for (bloc, region) in [(0, 0), (511, 0), (512, 1), (-1, -1), (-512, -1), (-513, -2)] {
        assert_eq!(
            Niveau::Region.cellule_axe(bloc),
            region,
            "le bloc {bloc} est dans la région {region}"
        );
    }
}

/// Les deux niveaux sont cohérents entre eux : 32 chunks font une région, et
/// la région d'un bloc est la même par les deux chemins.
#[test]
fn trente_deux_chunks_font_une_region() {
    assert_eq!(Niveau::Region.cote(), Niveau::Chunk.cote() * 32);
    for b in [-2000i32, -513, -512, -1, 0, 15, 16, 511, 512, 5000] {
        let par_chunk = tf_world::coords::floor_div(Niveau::Chunk.cellule_axe(b), 32);
        assert_eq!(
            par_chunk,
            Niveau::Region.cellule_axe(b),
            "bloc {b} : les deux chemins doivent donner la même région"
        );
    }
}

// ── les cellules visibles ───────────────────────────────────────────────────

/// Un rayon de zéro rend UNE cellule : la sienne.
#[test]
fn un_rayon_nul_rend_la_cellule_courante() {
    let c = cellules_autour(p(20, 70, -5), 0, Niveau::Chunk, (-64, 319));
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].x, c[0].z), (1, -1), "chunk 1, −1");
    assert_eq!(c[0].boite.min, p(16, -64, -16));
    assert_eq!(c[0].boite.max, p(31, 319, -1));
}

/// Le compte est borné par construction : `(2r + 1)²`. Il n'y a pas d'entrée
/// qui puisse faire exploser la sortie, donc pas de plafond de mémoire à
/// tenir — c'est la forme de l'API qui le garantit.
#[test]
fn le_compte_est_borne_par_le_rayon() {
    for r in [0u32, 1, 3, 8] {
        let n = cellules_autour(p(0, 0, 0), r, Niveau::Region, (0, 0)).len();
        assert_eq!(n, ((2 * r + 1) * (2 * r + 1)) as usize, "rayon {r}");
    }
    // Un rayon absurde est ÉCRÊTÉ, pas honoré : au-delà, ce n'est plus un
    // découpage qu'on montre, c'est un quadrillage illisible.
    let n = cellules_autour(p(0, 0, 0), u32::MAX, Niveau::Chunk, (0, 0)).len();
    assert_eq!(n, ((2 * RAYON_MAX + 1) * (2 * RAYON_MAX + 1)) as usize);
}

/// Les cellules PAVENT le plan : pas de trou, pas de recouvrement, et elles
/// se touchent exactement.
#[test]
fn les_cellules_pavent_sans_trou_ni_recouvrement() {
    let cs = cellules_autour(p(0, 70, 0), 2, Niveau::Chunk, (-64, 319));
    let mut vues: BTreeSet<(i32, i32)> = BTreeSet::new();
    for c in &cs {
        assert!(
            vues.insert((c.x, c.z)),
            "cellule ({}, {}) en double",
            c.x,
            c.z
        );
        let (sx, _, sz) = c.boite.size();
        assert_eq!((sx, sz), (16, 16), "un chunk fait 16 × 16");
        // Son voisin de droite commence là où elle finit, plus un.
        if let Some(v) = cs.iter().find(|v| v.x == c.x + 1 && v.z == c.z) {
            assert_eq!(v.boite.min.x, c.boite.max.x + 1, "pas de trou entre deux");
        }
    }
    assert_eq!(vues.len(), 25);
}

/// **La hauteur n'est PAS alignée.** Un chunk fait 16 × 16 en horizontal et
/// toute la hauteur du monde en vertical. L'aligner sur 16 alignerait sur les
/// SECTIONS, qui sont un autre découpage — et personne qui demande « mon
/// chunk » ne demande la tranche de seize blocs où il se trouve.
#[test]
fn la_hauteur_demandee_est_rendue_telle_quelle() {
    let c = cellules_autour(p(0, 0, 0), 0, Niveau::Chunk, (-64, 319));
    assert_eq!((c[0].boite.min.y, c[0].boite.max.y), (-64, 319));
    // Et dans n'importe quel ordre : deux clics ne sont pas triés.
    let c = cellules_autour(p(0, 0, 0), 0, Niveau::Chunk, (319, -64));
    assert_eq!((c[0].boite.min.y, c[0].boite.max.y), (-64, 319));
}

// ── montrer les .mca ────────────────────────────────────────────────────────

/// **Chaque cellule sait de quel `.mca` elle relève** — c'est ça, « montrer
/// visuellement les différents `.mca` ». Une cellule de CHUNK la porte aussi :
/// c'est ce qui permet de teinter les chunks par leur région, donc de voir la
/// frontière de fichier sans dessiner un second quadrillage par-dessus.
#[test]
fn chaque_cellule_nomme_son_fichier() {
    // Autour du coin de quatre régions : les quatre doivent apparaître.
    let cs = cellules_autour(p(0, 70, 0), 1, Niveau::Chunk, (-64, 319));
    let fichiers: BTreeSet<String> = cs.iter().map(|c| c.fichier()).collect();
    assert_eq!(
        fichiers.iter().cloned().collect::<Vec<_>>(),
        vec![
            "r.-1.-1.mca".to_string(),
            "r.-1.0.mca".into(),
            "r.0.-1.mca".into(),
            "r.0.0.mca".into()
        ],
        "les quatre .mca qui se touchent à l'origine"
    );

    // Et au niveau région, la cellule EST la région.
    let cs = cellules_autour(p(600, 70, -100), 0, Niveau::Region, (0, 0));
    assert_eq!(cs[0].fichier(), "r.1.-1.mca");
    assert_eq!((cs[0].x, cs[0].z), (1, -1));
}

/// **Deux cellules voisines n'ont jamais la même parité.**
///
/// C'est le minimum qu'il faut pour qu'un œil SÉPARE deux `.mca` adjacents :
/// un quadrillage d'une seule couleur montre où sont les bords, pas à quel
/// fichier appartient ce qu'il y a entre eux. Et la parité se prend en
/// euclidien — `(-1) % 2` vaut `-1` en Rust, ce qui donnerait trois valeurs au
/// lieu de deux et un damier qui se casse à l'origine.
#[test]
fn le_damier_ne_se_casse_pas_a_l_origine() {
    let cs = cellules_autour(p(0, 0, 0), 3, Niveau::Region, (0, 0));
    for c in &cs {
        assert!(
            c.parite() <= 1,
            "({}, {}) : parité {}",
            c.x,
            c.z,
            c.parite()
        );
        for v in &cs {
            let voisine = (v.x - c.x).abs() + (v.z - c.z).abs() == 1;
            if voisine {
                assert_ne!(
                    c.parite(),
                    v.parite(),
                    "({}, {}) et ({}, {}) sont voisines et de même parité",
                    c.x,
                    c.z,
                    v.x,
                    v.z
                );
            }
        }
    }
}

// ── s'aligner ───────────────────────────────────────────────────────────────

/// **`aligner` ÉTEND, elle ne rétrécit jamais.**
///
/// Rétrécir ferait perdre en silence des blocs que l'utilisateur avait
/// sélectionnés, et il ne le verrait qu'après l'opération. Entre deux erreurs
/// on prend celle qui se voit.
#[test]
fn aligner_etend_et_ne_retrecit_jamais() {
    let sel = boite((3, 10, 5), (20, 50, 40));
    let a = sel.aligner(Niveau::Chunk);
    assert_eq!(a.min, p(0, 10, 0), "le coin bas descend au chunk");
    assert_eq!(a.max, p(31, 50, 47), "le coin haut monte au bout du chunk");
    assert!(
        a.contains(sel.min) && a.contains(sel.max),
        "elle CONTIENT l'ancienne"
    );
    assert!(a.volume() >= sel.volume());
}

/// La hauteur n'est pas touchée — même raison que pour les cellules.
#[test]
fn aligner_ne_touche_pas_a_la_hauteur() {
    let sel = boite((3, 7, 5), (20, 53, 40));
    for n in [Niveau::Chunk, Niveau::Region] {
        let a = sel.aligner(n);
        assert_eq!((a.min.y, a.max.y), (7, 53), "{n:?}");
    }
}

/// Du côté négatif aussi — et c'est là que la division plancher se voit.
#[test]
fn aligner_marche_du_cote_negatif() {
    let sel = boite((-1, 0, -17), (-1, 0, -17));
    let a = sel.aligner(Niveau::Chunk);
    assert_eq!(a.min, p(-16, 0, -32));
    assert_eq!(
        a.max,
        p(-1, 0, -17),
        "z = −17 est dans le chunk −2, qui va de −32 à −17"
    );
}

/// Aligner est IDEMPOTENT : une boîte déjà alignée ne bouge plus. Sans ça,
/// appuyer deux fois sur le bouton agrandirait la sélection à chaque fois.
#[test]
fn aligner_est_idempotent() {
    for n in [Niveau::Chunk, Niveau::Region] {
        for sel in [
            boite((3, 10, 5), (20, 50, 40)),
            boite((-1000, -64, 700), (-3, 319, 1200)),
            boite((0, 0, 0), (0, 0, 0)),
        ] {
            let une = sel.aligner(n);
            assert_eq!(une, une.aligner(n), "{n:?} sur {sel:?}");
            assert!(une.est_alignee(n));
            // Et elle dit la vérité sur ce qui ne l'est pas.
            if sel != une {
                assert!(!sel.est_alignee(n));
            }
        }
    }
}

/// **Aligner sur la région implique aligné sur le chunk**, puisque 32 chunks
/// font une région. L'inverse est faux. Une hiérarchie qui ne tiendrait pas
/// ferait qu'« aligner sur le .mca » désalignerait les chunks.
#[test]
fn aligner_sur_la_region_aligne_aussi_les_chunks() {
    let sel = boite((3, 10, 5), (900, 50, -40));
    let r = sel.aligner(Niveau::Region);
    assert!(r.est_alignee(Niveau::Region));
    assert!(
        r.est_alignee(Niveau::Chunk),
        "une boîte alignée région doit l'être aussi en chunk"
    );
}

/// L'intérêt n'est pas cosmétique : une sélection alignée couvre des sections
/// ENTIÈRES, donc l'étage palette. Le test le dit sur ce qui décide —
/// `covers_section`.
#[test]
fn une_selection_alignee_couvre_des_sections_entieres() {
    let sel = boite((3, -64, 5), (20, 319, 40));
    let a = sel.aligner(Niveau::Chunk);
    let mut entieres = 0;
    let mut total = 0;
    for s in a.sections() {
        total += 1;
        if a.covers_section(s) {
            entieres += 1;
        }
    }
    assert!(total > 0);
    assert_eq!(
        entieres, total,
        "toutes les sections doivent être couvertes"
    );
    // Et décalée d'un bloc, ça ne tient plus — c'est le × 21 mesuré.
    let decalee = boite((3, -64, 5), (21, 319, 40)).aligner(Niveau::Chunk);
    let decalee = boite(
        (decalee.min.x + 1, decalee.min.y, decalee.min.z),
        (decalee.max.x, decalee.max.y, decalee.max.z),
    );
    assert!(decalee.sections().any(|s| !decalee.covers_section(s)));
}
