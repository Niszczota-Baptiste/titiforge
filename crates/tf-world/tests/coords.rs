//! L'adressage, éprouvé surtout du côté NÉGATIF.
//!
//! C'est là que vivent les erreurs : sur les coordonnées positives, une
//! division tronquée et une division plancher donnent le même résultat, donc
//! tout ce qu'on vérifie à la main passe. `we-engine` a payé ce piège en
//! chargeant la mauvaise moitié du monde, sans le moindre signal.

use tf_world::{
    floor_div, floor_mod, BBox, BlockPos, ChunkPos, Height, LocalBox, RegionPos, SectionPos,
};

// ── les deux divisions ──────────────────────────────────────────────────────

#[test]
fn la_division_plancher_differe_de_la_troncature_sur_quinze_valeurs_sur_seize() {
    let mut divergences = 0;
    for v in -256i32..256 {
        let plancher = floor_div(v, 16);
        let tronquee = v / 16;
        if v < 0 && v % 16 != 0 {
            assert_eq!(plancher, tronquee - 1, "v = {v}");
            divergences += 1;
        } else {
            assert_eq!(plancher, tronquee, "v = {v}");
        }
    }
    assert_eq!(
        divergences,
        15 * 16,
        "quinze valeurs sur seize, sur seize chunks négatifs"
    );
}

#[test]
fn le_reste_plancher_est_toujours_dans_le_domaine() {
    for v in -1000i32..1000 {
        for b in [4i32, 16, 32] {
            let r = floor_mod(v, b);
            assert!((0..b).contains(&r), "floor_mod({v}, {b}) = {r}");
            // L'identité qui doit tenir : a = floor_div(a,b)*b + floor_mod(a,b)
            assert_eq!(floor_div(v, b) * b + r, v, "v = {v}, b = {b}");
        }
    }
}

// ── bloc → section / chunk / région ─────────────────────────────────────────

#[test]
fn le_bloc_moins_un_est_dans_le_chunk_moins_un() {
    assert_eq!(BlockPos::new(-1, -1, -1).chunk(), ChunkPos::new(-1, -1));
    assert_eq!(BlockPos::new(-16, 0, -16).chunk(), ChunkPos::new(-1, -1));
    assert_eq!(
        BlockPos::new(-1, -1, -1).section(),
        SectionPos::new(-1, -1, -1)
    );
    assert_eq!(
        BlockPos::new(-1, -1, -1).chunk().region(),
        RegionPos::new(-1, -1)
    );
}

#[test]
fn les_frontieres_de_chunk_tombent_au_bon_endroit() {
    for (x, attendu) in [
        (-33i32, -3i32),
        (-32, -2),
        (-17, -2),
        (-16, -1),
        (-1, -1),
        (0, 0),
        (15, 0),
        (16, 1),
    ] {
        assert_eq!(BlockPos::new(x, 0, 0).chunk().x, attendu, "bloc x = {x}");
    }
}

#[test]
fn la_position_locale_reste_toujours_dans_zero_quinze() {
    for x in -100i32..100 {
        for y in [-100i32, -65, -64, -1, 0, 1, 63, 319] {
            for z in [-33i32, -16, -1, 0, 15, 16] {
                let (lx, ly, lz) = BlockPos::new(x, y, z).local();
                assert!(
                    lx < 16 && ly < 16 && lz < 16,
                    "({x},{y},{z}) → ({lx},{ly},{lz})"
                );
            }
        }
    }
}

#[test]
fn bloc_vers_section_puis_retour_est_coherent() {
    // La propriété qui doit tenir partout : la section d'un bloc contient ce
    // bloc, et sa position locale le redésigne exactement.
    for x in [-1_000_000i32, -33, -17, -16, -1, 0, 15, 16, 1_000_000] {
        for y in [-64i32, -49, -48, -1, 0, 255, 319] {
            for z in [-1_000_000i32, -17, -1, 0, 16, 1_000_000] {
                let p = BlockPos::new(x, y, z);
                let s = p.section();
                let (lx, ly, lz) = p.local();
                assert_eq!(s.min_block().x + lx as i32, x, "x de ({x},{y},{z})");
                assert_eq!(s.min_block().y + ly as i32, y, "y de ({x},{y},{z})");
                assert_eq!(s.min_block().z + lz as i32, z, "z de ({x},{y},{z})");
                // Et la section contient bien le bloc.
                let b = BBox {
                    min: s.min_block(),
                    max: s.max_block(),
                };
                assert!(b.contains(p), "({x},{y},{z}) hors de sa propre section");
            }
        }
    }
}

#[test]
fn l_index_local_suit_l_ordre_yzx_du_moteur() {
    // La même règle que `tf_anvil::local_index`. Deux tables d'indices qui
    // divergeraient poseraient les blocs à côté — et seulement pour les
    // coordonnées négatives, ce qui rendrait le bug très difficile à voir.
    for x in [-17i32, -16, -1, 0, 5, 15, 16] {
        for y in [-64i32, -1, 0, 7, 319] {
            for z in [-33i32, -1, 0, 11, 16] {
                let p = BlockPos::new(x, y, z);
                let (lx, ly, lz) = p.local();
                assert_eq!(
                    p.local_index(),
                    tf_anvil::local_index(lx, ly, lz),
                    "({x},{y},{z})"
                );
            }
        }
    }
}

// ── chunk → région ──────────────────────────────────────────────────────────

#[test]
fn le_chunk_moins_un_est_dans_la_region_moins_un_a_l_index_trente_et_un() {
    // Le piège en deux temps : la région est la −1 (division plancher), et
    // l'index DANS le fichier est 31 (reste plancher). Se tromper sur le
    // second écrit le chunk à l'autre bout de la région.
    let c = ChunkPos::new(-1, -1);
    assert_eq!(c.region(), RegionPos::new(-1, -1));
    assert_eq!(c.index_in_region(), 31 + 31 * 32);

    assert_eq!(ChunkPos::new(-32, 0).region(), RegionPos::new(-1, 0));
    assert_eq!(ChunkPos::new(-32, 0).index_in_region(), 0);
    assert_eq!(ChunkPos::new(-33, 0).region(), RegionPos::new(-2, 0));
    assert_eq!(ChunkPos::new(-33, 0).index_in_region(), 31);
    assert_eq!(ChunkPos::new(31, 31).region(), RegionPos::new(0, 0));
    assert_eq!(ChunkPos::new(31, 31).index_in_region(), 1023);
}

#[test]
fn chaque_index_de_region_est_atteint_une_fois_et_une_seule() {
    for (rx, rz) in [(0i32, 0i32), (-1, -1), (-2, 3), (7, -9)] {
        let base = RegionPos::new(rx, rz).min_chunk();
        let mut vus = vec![false; 1024];
        for dz in 0..32 {
            for dx in 0..32 {
                let c = ChunkPos::new(base.x + dx, base.z + dz);
                assert_eq!(c.region(), RegionPos::new(rx, rz), "{c:?}");
                let i = c.index_in_region();
                assert!(
                    !vus[i],
                    "index {i} atteint deux fois dans la région ({rx},{rz})"
                );
                vus[i] = true;
            }
        }
        assert!(
            vus.iter().all(|&v| v),
            "région ({rx},{rz}) : des index manquent"
        );
    }
}

#[test]
fn le_nom_de_fichier_suit_la_convention() {
    assert_eq!(RegionPos::new(0, 0).file_name(), "r.0.0.mca");
    assert_eq!(RegionPos::new(-1, 2).file_name(), "r.-1.2.mca");
    assert_eq!(
        tf_anvil::region_coords_from_name(&RegionPos::new(-3, 7).file_name()),
        Some((-3, 7)),
        "le moteur Anvil doit relire ce que l'adressage écrit"
    );
}

// ── boîte englobante ────────────────────────────────────────────────────────

#[test]
fn une_boite_se_normalise_quel_que_soit_l_ordre_des_coins() {
    let a = BlockPos::new(10, 5, -3);
    let b = BlockPos::new(-2, 60, 8);
    assert_eq!(BBox::new(a, b), BBox::new(b, a));
    assert_eq!(BBox::new(a, b).min, BlockPos::new(-2, 5, -3));
    assert_eq!(BBox::new(a, b).max, BlockPos::new(10, 60, 8));
}

#[test]
fn les_bornes_sont_incluses_des_deux_cotes() {
    // « de −10 à 10 » fait 21 blocs, pas 20. Une convention exclusive d'un
    // côté donne un décalage d'un bloc à chaque conversion, et personne ne le
    // voit avant qu'un mur sorte trop court.
    let b = BBox::new(BlockPos::new(-10, 0, 0), BlockPos::new(10, 0, 0));
    assert_eq!(b.volume(), 21);
    assert_eq!(b.size(), (21, 1, 1));
    assert!(b.contains(BlockPos::new(-10, 0, 0)));
    assert!(b.contains(BlockPos::new(10, 0, 0)));
    assert!(!b.contains(BlockPos::new(11, 0, 0)));

    assert_eq!(BBox::single(BlockPos::new(3, 4, 5)).volume(), 1);
}

#[test]
fn le_volume_du_monde_entier_ne_deborde_pas() {
    // Le monde Minecraft fait 60 millions de blocs de côté : 1,38 × 10¹⁸, ce
    // qui TIENT dans un u64 avec treize fois de marge.
    let monde = BBox::new(
        BlockPos::new(-30_000_000, -64, -30_000_000),
        BlockPos::new(30_000_000, 319, 30_000_000),
    );
    let cote = 60_000_001u128;
    assert_eq!(monde.volume(), cote * cote * 384);
    assert!(
        monde.volume() < u64::MAX as u128,
        "le vrai monde tient en u64"
    );

    // Mais rien dans le TYPE ne borne une boîte au monde jouable : `BlockPos`
    // porte des i32, et une sélection fabriquée à partir de nombres saisis, ou
    // dérivée d'un fichier abîmé, peut couvrir tout le domaine. Là, le volume
    // vaut 7,9 × 10²⁸ — neuf ordres de grandeur au-dessus d'un u64. Le
    // débordement rendrait un volume minuscule, donc une opération annoncée
    // « à faible coût » qui ne finit jamais.
    let extreme = BBox::new(
        BlockPos::new(i32::MIN, i32::MIN, i32::MIN),
        BlockPos::new(i32::MAX, i32::MAX, i32::MAX),
    );
    let domaine = 4_294_967_296u128;
    assert_eq!(extreme.volume(), domaine * domaine * domaine);
    assert!(
        extreme.volume() > u64::MAX as u128,
        "et celui-là déborde bien"
    );
}

#[test]
fn intersection_et_recouvrement() {
    let a = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10));
    let b = BBox::new(BlockPos::new(5, 5, 5), BlockPos::new(20, 20, 20));
    assert!(a.intersects(&b));
    assert_eq!(
        a.intersection(&b).unwrap(),
        BBox::new(BlockPos::new(5, 5, 5), BlockPos::new(10, 10, 10))
    );

    // Deux boîtes qui se TOUCHENT sur une face se croisent bien : les bornes
    // sont incluses.
    let c = BBox::new(BlockPos::new(10, 0, 0), BlockPos::new(20, 10, 10));
    assert!(a.intersects(&c));
    // Une tranche d'un bloc d'épaisseur sur 11 × 11.
    assert_eq!(a.intersection(&c).unwrap().size(), (1, 11, 11));
    assert_eq!(a.intersection(&c).unwrap().volume(), 121);

    let loin = BBox::new(BlockPos::new(11, 0, 0), BlockPos::new(20, 10, 10));
    assert!(!a.intersects(&loin));
    assert_eq!(a.intersection(&loin), None);
}

#[test]
fn extend_agrandit_dans_les_deux_sens() {
    let mut b = BBox::single(BlockPos::new(0, 0, 0));
    b.extend(BlockPos::new(5, -3, 2));
    b.extend(BlockPos::new(-7, 10, -1));
    assert_eq!(b.min, BlockPos::new(-7, -3, -1));
    assert_eq!(b.max, BlockPos::new(5, 10, 2));
}

#[test]
fn le_balayage_des_sections_couvre_exactement_ce_qui_est_touche() {
    // Une boîte d'un seul bloc ne touche qu'une section — mais posée sur une
    // frontière, elle en touche huit.
    let un = BBox::single(BlockPos::new(5, 5, 5));
    assert_eq!(un.sections().count(), 1);

    let coin = BBox::new(BlockPos::new(15, 15, 15), BlockPos::new(16, 16, 16));
    assert_eq!(coin.sections().count(), 8, "à cheval sur huit sections");

    // Et chaque section listée doit vraiment croiser la boîte.
    for s in coin.sections() {
        let sb = BBox {
            min: s.min_block(),
            max: s.max_block(),
        };
        assert!(coin.intersects(&sb), "{s:?} listée sans être touchée");
    }

    // Négatif : la même chose de l'autre côté de zéro.
    let neg = BBox::new(BlockPos::new(-17, -1, -17), BlockPos::new(-16, 0, -16));
    assert_eq!(neg.sections().count(), 8);
    for s in neg.sections() {
        let sb = BBox {
            min: s.min_block(),
            max: s.max_block(),
        };
        assert!(neg.intersects(&sb), "{s:?}");
    }
}

#[test]
fn aucune_section_touchee_n_est_oubliee() {
    // La propriété qui compte vraiment : si un bloc de la boîte vit dans une
    // section, cette section DOIT être listée. L'oublier ferait lire de l'air
    // là où il y a de la pierre.
    let b = BBox::new(BlockPos::new(-20, -5, -20), BlockPos::new(20, 20, 20));
    let listees: std::collections::HashSet<SectionPos> = b.sections().collect();
    for x in b.min.x..=b.max.x {
        for y in b.min.y..=b.max.y {
            for z in b.min.z..=b.max.z {
                let s = BlockPos::new(x, y, z).section();
                assert!(listees.contains(&s), "section de ({x},{y},{z}) non listée");
            }
        }
    }
}

#[test]
fn le_balayage_des_regions_utilise_bien_la_division_plancher() {
    // Une boîte qui commence au bloc −1 touche la région −1. Une division
    // naïve chargerait la région 0 et lirait l'autre bout du monde.
    let b = BBox::new(BlockPos::new(-1, 0, -1), BlockPos::new(0, 0, 0));
    let regions: Vec<RegionPos> = b.regions().collect();
    assert!(regions.contains(&RegionPos::new(-1, -1)), "{regions:?}");
    assert!(regions.contains(&RegionPos::new(0, 0)), "{regions:?}");
    assert_eq!(regions.len(), 4);

    let chunks: Vec<ChunkPos> = b.chunks().collect();
    assert_eq!(chunks.len(), 4);
    assert!(chunks.contains(&ChunkPos::new(-1, -1)));
}

#[test]
fn covers_section_ne_ment_jamais_par_exces() {
    // C'est ce test qui envoie une opération à l'étage palette plutôt qu'à
    // l'étage bloc. Le rendre trop permissif écrirait HORS de la sélection.
    let s = SectionPos::new(0, 0, 0);
    let pile = BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(15, 15, 15));
    assert!(pile.covers_section(s));

    // Un bloc de moins sur n'importe quel axe, et ce n'est plus couvert.
    for manque in [
        BBox::new(BlockPos::new(1, 0, 0), BlockPos::new(15, 15, 15)),
        BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(14, 15, 15)),
        BBox::new(BlockPos::new(0, 1, 0), BlockPos::new(15, 15, 15)),
        BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(15, 15, 14)),
    ] {
        assert!(
            !manque.covers_section(s),
            "{manque:?} ne couvre pas entièrement"
        );
    }

    // Et une section négative, où l'arithmétique est la plus fragile.
    let sn = SectionPos::new(-1, -1, -1);
    assert!(BBox::new(BlockPos::new(-16, -16, -16), BlockPos::new(-1, -1, -1)).covers_section(sn));
    assert!(!BBox::new(BlockPos::new(-15, -16, -16), BlockPos::new(-1, -1, -1)).covers_section(sn));
}

#[test]
fn clip_to_section_rend_des_bornes_locales_justes() {
    let s = SectionPos::new(-1, 0, 2);
    let b = BBox::new(BlockPos::new(-10, 3, 35), BlockPos::new(-5, 8, 40));
    let l = b.clip_to_section(s).unwrap();
    // Section (-1,0,2) commence au bloc (-16, 0, 32).
    assert_eq!((l.x0, l.y0, l.z0), (6, 3, 3));
    assert_eq!((l.x1, l.y1, l.z1), (11, 8, 8));
    assert!(l.x1 < 16 && l.y1 < 16 && l.z1 < 16);
    assert!(!l.is_full());
    assert_eq!(l.count(), 6 * 6 * 6);
    assert_eq!(l.indices().count(), l.count());

    // Une section hors de la boîte ne rend rien.
    assert_eq!(b.clip_to_section(SectionPos::new(5, 0, 0)), None);

    // Et une section entièrement couverte rend tout le domaine.
    let tout = BBox::new(BlockPos::new(-16, 0, 32), BlockPos::new(-1, 15, 47));
    assert_eq!(tout.clip_to_section(s), Some(LocalBox::PLEINE));
    assert!(LocalBox::PLEINE.is_full());
    assert_eq!(LocalBox::PLEINE.count(), 4096);

    // Les index parcourus doivent être exactement ceux des cases couvertes.
    let index: std::collections::HashSet<usize> = l.indices().collect();
    assert_eq!(index.len(), l.count(), "aucun doublon");
    for i in &index {
        let (x, y, z) = (i & 15, i >> 8, (i >> 4) & 15);
        assert!(
            (l.x0..=l.x1).contains(&x) && (l.y0..=l.y1).contains(&y) && (l.z0..=l.z1).contains(&z)
        );
    }
}

// ── hauteur du monde ────────────────────────────────────────────────────────

#[test]
fn les_deux_hauteurs_du_jeu() {
    assert_eq!(Height::MODERNE.sections(), 24);
    assert_eq!(Height::MODERNE.min_section_y(), -4);
    assert_eq!(Height::MODERNE.max_section_y(), 19);
    assert!(Height::MODERNE.contains(-64));
    assert!(Height::MODERNE.contains(319));
    assert!(!Height::MODERNE.contains(-65));
    assert!(!Height::MODERNE.contains(320));

    assert_eq!(Height::ANCIENNE.sections(), 16);
    assert_eq!(Height::ANCIENNE.min_section_y(), 0);
    assert_eq!(Height::ANCIENNE.max_section_y(), 15);
    assert!(!Height::ANCIENNE.contains(-1));
}
