//! Coordonnées, en-tête, et ce que le crate fait d'un fichier abîmé.
//!
//! Un `.mca` vient du disque d'un utilisateur : coupé par un crash, tronqué
//! par un téléchargement, renommé par Windows. Refuser d'ouvrir une région
//! pour un seul chunk illisible ferait perdre les 1023 autres.

mod common;
use common::fixture::{self, SectionSpec};

use tf_anvil::{
    chunk_of_block, deflate, floor_div, inflate, read, region_coords_from_name, region_file_name,
    region_of_chunk, scan, splice, write, CodecError, Compression, Edit, ReadError, SpliceError,
    WriteError, CHUNKS, HEADER, MAX_SECTORS, SECTOR,
};
use tf_nbt::Span;

// ── coordonnées ─────────────────────────────────────────────────────────────

#[test]
fn le_bloc_moins_un_est_dans_la_region_moins_un() {
    // Le piège le plus coûteux de we-engine : une division entière naïve
    // charge la mauvaise moitié du monde sans rien signaler, parce que -1/16
    // vaut 0 et non -1.
    assert_eq!(floor_div(-1, 16), -1);
    assert_eq!(floor_div(-16, 16), -1);
    assert_eq!(floor_div(-17, 16), -2);
    assert_eq!(floor_div(0, 16), 0);
    assert_eq!(floor_div(15, 16), 0);
    assert_eq!(floor_div(16, 16), 1);

    assert_eq!(chunk_of_block(-1, -1), (-1, -1));
    assert_eq!(chunk_of_block(-16, 0), (-1, 0));
    assert_eq!(chunk_of_block(-17, 31), (-2, 1));

    assert_eq!(region_of_chunk(-1, -1), (-1, -1));
    assert_eq!(region_of_chunk(-32, 0), (-1, 0));
    assert_eq!(region_of_chunk(-33, 63), (-2, 1));
    assert_eq!(region_of_chunk(31, 31), (0, 0));
    assert_eq!(region_of_chunk(32, 32), (1, 1));
}

#[test]
fn une_division_naive_donnerait_un_resultat_different_et_faux() {
    // On fige l'écart plutôt que de faire confiance à la relecture : c'est
    // exactement le genre de correction qu'une « simplification » annule.
    //
    // Nuance qui compte, et qui explique pourquoi le bug est si difficile à
    // voir : sur les MULTIPLES exacts, les deux divisions sont d'accord. Le
    // bloc -64 tombe bien dans le chunk -4 des deux façons. C'est partout
    // AILLEURS que la troncature remonte d'un cran vers zéro — donc quinze
    // colonnes sur seize, et jamais celle qu'on teste à la main en premier.
    let mut divergences = 0;
    for v in -64i32..0 {
        if v % 16 == 0 {
            assert_eq!(
                floor_div(v, 16),
                v / 16,
                "v = {v} est un multiple : les deux coïncident"
            );
        } else {
            assert_eq!(floor_div(v, 16), v / 16 - 1, "v = {v}");
            divergences += 1;
        }
    }
    assert_eq!(
        divergences, 60,
        "quinze valeurs sur seize, sur quatre chunks"
    );
}

#[test]
fn les_deux_conventions_de_nom_de_region_sont_acceptees() {
    assert_eq!(region_coords_from_name("r.0.0.mca"), Some((0, 0)));
    assert_eq!(region_coords_from_name("r.-1.2.mca"), Some((-1, 2)));
    assert_eq!(region_coords_from_name("r_-1_2.mca"), Some((-1, 2)));
    assert_eq!(
        region_coords_from_name("/monde/region/r.3.-4.mca"),
        Some((3, -4))
    );
    assert_eq!(
        region_coords_from_name(r"C:\monde\region\r.3.-4.mca"),
        Some((3, -4))
    );
}

#[test]
fn un_nom_de_fichier_qui_ment_est_refuse_pas_devine() {
    // `r.0.0 (16).mca` est le doublon de téléchargement de Windows. Le nom ne
    // doit rien rendre : le CONTENU porte ses propres coordonnées, et c'est
    // lui qui fera foi.
    for nom in [
        "r.0.0 (16).mca",
        "region.mca",
        "r.0.mca",
        "r..0.mca",
        "r.a.b.mca",
        "r.0.0.mcc",
        "r.0.0",
        "",
    ] {
        assert_eq!(
            region_coords_from_name(nom),
            None,
            "« {nom} » ne doit rien rendre"
        );
    }
}

#[test]
fn le_nom_de_fichier_se_reconstruit_a_l_identique() {
    for (x, z) in [(0, 0), (-1, 2), (31, -47)] {
        let nom = region_file_name(x, z);
        assert_eq!(
            region_coords_from_name(&nom),
            Some((x, z)),
            "aller-retour sur {nom}"
        );
    }
}

#[test]
fn les_coordonnees_monde_d_un_chunk_suivent_la_region() {
    let mut rng = fixture::Rng::new(11);
    let secs = vec![SectionSpec::with_palette_size(0, 6, &mut rng)];
    let src = fixture::region_file(&[(5, 7, fixture::chunk_nbt(-59, 39, &secs))], 0);
    let (rx, rz) = (-2i32, 1i32);
    let region = read(&src, rx, rz).unwrap();
    let c = region.get(5, 7).unwrap();
    assert_eq!(region.chunk_coords(c), (rx * 32 + 5, rz * 32 + 7));
    assert_eq!(region.chunk_coords(c), (-59, 39));
}

// ── en-tête ─────────────────────────────────────────────────────────────────

#[test]
fn un_fichier_plus_court_que_l_en_tete_est_refuse() {
    for n in [0usize, 1, 100, HEADER - 1] {
        let buf = vec![0u8; n];
        assert_eq!(
            read(&buf, 0, 0).unwrap_err(),
            ReadError::TooShort,
            "taille {n}"
        );
    }
    // Exactement l'en-tête : région valide et vide.
    let buf = vec![0u8; HEADER];
    let r = read(&buf, 0, 0).unwrap();
    assert_eq!(r.count(), 0);
    assert_eq!(r.slots.len(), CHUNKS);
}

#[test]
fn une_entree_incoherente_est_sautee_sans_perdre_les_autres() {
    let mut rng = fixture::Rng::new(21);
    let secs = vec![SectionSpec::with_palette_size(0, 8, &mut rng)];
    let mut src = fixture::region_file(
        &[
            (0, 0, fixture::chunk_nbt(0, 0, &secs)),
            (1, 0, fixture::chunk_nbt(1, 0, &secs)),
            (2, 0, fixture::chunk_nbt(2, 0, &secs)),
        ],
        0,
    );
    assert_eq!(read(&src, 0, 0).unwrap().count(), 3);

    // On pointe le chunk (1,0) très au-delà de la fin du fichier.
    let i = 1usize;
    let loc = (0xFF_FFFFu32 << 8) | 1;
    src[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());

    let region = read(&src, 0, 0).unwrap();
    assert_eq!(region.count(), 2, "les deux autres chunks restent lisibles");
    assert!(region.get(1, 0).is_none());
    assert!(region.get(0, 0).is_some() && region.get(2, 0).is_some());
}

#[test]
fn une_longueur_de_chunk_incoherente_est_sautee() {
    let mut rng = fixture::Rng::new(22);
    let secs = vec![SectionSpec::with_palette_size(0, 8, &mut rng)];
    let mut src = fixture::region_file(&[(0, 0, fixture::chunk_nbt(0, 0, &secs))], 0);

    // Longueur annoncée : 0 (incohérent — elle compte l'octet de compression).
    src[SECTOR * 2..SECTOR * 2 + 4].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(read(&src, 0, 0).unwrap().count(), 0);

    // Longueur qui déborde du fichier.
    src[SECTOR * 2..SECTOR * 2 + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(read(&src, 0, 0).unwrap().count(), 0);
}

#[test]
fn un_chunk_trop_gros_est_refuse_explicitement() {
    // Au-delà de 255 secteurs, Minecraft range le chunk dans un `.mcc`
    // externe. Écrire un en-tête tronqué le rendrait illisible pour le jeu ET
    // pour nous : on refuse, en nommant le chunk.
    let mut rng = fixture::Rng::new(23);
    let secs = vec![SectionSpec::with_palette_size(0, 8, &mut rng)];
    let src = fixture::region_file(&[(0, 0, fixture::chunk_nbt(0, 0, &secs))], 0);
    let mut region = read(&src, 0, 0).unwrap();

    // Une charge incompressible de 2 Mio : elle ne peut pas tenir en 255 secteurs.
    let enorme: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|i| (i * 2654435761u64 % 251) as u8)
        .collect();
    region.get_mut(0, 0).unwrap().payload = std::borrow::Cow::Owned(enorme);

    match write(&region) {
        Err(WriteError::ChunkTooLarge { index, sectors }) => {
            assert_eq!(index, 0);
            assert!(sectors > MAX_SECTORS, "{sectors} secteurs");
        }
        other => panic!("attendu ChunkTooLarge, obtenu {other:?}"),
    }
}

#[test]
fn chaque_chunk_ecrit_commence_sur_une_frontiere_de_secteur() {
    let mut rng = fixture::Rng::new(24);
    let mut chunks = Vec::new();
    for k in 0..6u32 {
        let secs = vec![SectionSpec::with_palette_size(0, 20 + k as usize, &mut rng)];
        chunks.push((k, 0, fixture::chunk_nbt(k as i32, 0, &secs)));
    }
    let src = fixture::region_file(&chunks, 0);
    let sortie = write(&read(&src, 0, 0).unwrap()).unwrap();

    assert_eq!(
        sortie.len() % SECTOR,
        0,
        "le fichier fait un nombre entier de secteurs"
    );
    for i in 0..CHUNKS {
        let loc = u32::from_be_bytes(sortie[i * 4..i * 4 + 4].try_into().unwrap());
        let off = (loc >> 8) as usize;
        let cnt = (loc & 0xff) as usize;
        if off == 0 && cnt == 0 {
            continue;
        }
        assert!(off >= 2, "aucun chunk ne peut commencer dans l'en-tête");
        assert!(
            (off + cnt) * SECTOR <= sortie.len(),
            "le chunk {i} déborde du fichier"
        );
    }
}

// ── splice ──────────────────────────────────────────────────────────────────

#[test]
fn splice_sans_edition_rend_le_tampon_identique() {
    let src = b"abcdefghij".to_vec();
    assert_eq!(splice(&src, &mut Vec::new()).unwrap(), src);
}

#[test]
fn splice_remplace_et_recopie_le_reste() {
    let src = b"abcdefghij".to_vec();
    let mut edits = vec![
        Edit {
            span: Span { start: 6, end: 8 },
            bytes: b"ZZZZ".to_vec(),
        },
        Edit {
            span: Span { start: 1, end: 3 },
            bytes: b"X".to_vec(),
        },
    ];
    // Les éditions sont données dans le désordre : splice doit les trier.
    assert_eq!(splice(&src, &mut edits).unwrap(), b"aXdefZZZZij".to_vec());
}

#[test]
fn splice_refuse_deux_plages_qui_se_chevauchent() {
    // Un bug d'appelant, pas une entrée à assainir : produire une sortie
    // plausible écrirait n'importe quoi dans la save de quelqu'un.
    let src = vec![0u8; 100];
    let mut edits = vec![
        Edit {
            span: Span { start: 10, end: 30 },
            bytes: vec![1],
        },
        Edit {
            span: Span { start: 20, end: 40 },
            bytes: vec![2],
        },
    ];
    assert_eq!(splice(&src, &mut edits), Err(SpliceError::Overlap));
}

#[test]
fn splice_refuse_une_plage_hors_du_tampon() {
    let src = vec![0u8; 10];
    for span in [Span { start: 0, end: 11 }, Span { start: 5, end: 3 }] {
        let mut edits = vec![Edit {
            span,
            bytes: vec![9],
        }];
        assert!(matches!(
            splice(&src, &mut edits),
            Err(SpliceError::OutOfBounds) | Err(SpliceError::Overlap)
        ));
    }
}

#[test]
fn splice_gere_une_plage_en_tete_et_en_queue() {
    let src = b"abcdef".to_vec();
    let mut e = vec![Edit {
        span: Span { start: 0, end: 2 },
        bytes: b"__".to_vec(),
    }];
    assert_eq!(splice(&src, &mut e).unwrap(), b"__cdef".to_vec());
    let mut e = vec![Edit {
        span: Span { start: 4, end: 6 },
        bytes: b"__".to_vec(),
    }];
    assert_eq!(splice(&src, &mut e).unwrap(), b"abcd__".to_vec());
    let mut e = vec![Edit {
        span: Span { start: 0, end: 6 },
        bytes: b"!".to_vec(),
    }];
    assert_eq!(splice(&src, &mut e).unwrap(), b"!".to_vec());
}

// ── compression ─────────────────────────────────────────────────────────────

#[test]
fn les_trois_compressions_du_format_font_l_aller_retour() {
    let clair = b"minecraft:stone".repeat(500);
    for c in [Compression::Zlib, Compression::Gzip, Compression::None] {
        let comprime = deflate(&clair, c).unwrap();
        assert_eq!(inflate(&comprime, c).unwrap(), clair, "{c:?}");
    }
}

#[test]
fn une_compression_inconnue_est_signalee_pas_devinee() {
    assert_eq!(
        inflate(b"nimporte", Compression::Other(9)),
        Err(CodecError::Unsupported(9))
    );
    assert_eq!(
        deflate(b"nimporte", Compression::Other(9)),
        Err(CodecError::Unsupported(9))
    );
    // Et l'octet du format est conservé tel quel dans les deux sens.
    assert_eq!(Compression::from_byte(9), Compression::Other(9));
    assert_eq!(Compression::Other(9).to_byte(), 9);
}

#[test]
fn une_charge_illisible_rend_une_erreur() {
    assert_eq!(
        inflate(b"ceci n'est pas du zlib", Compression::Zlib),
        Err(CodecError::Corrupt)
    );
    assert_eq!(inflate(b"", Compression::Gzip), Err(CodecError::Corrupt));
}

#[test]
fn une_bombe_zip_est_arretee_par_le_plafond() {
    // 256 Mio de zéros se compressent en quelques centaines de kilo-octets. Un
    // `.mca` forgé contenant ça ferait allouer jusqu'à la mort du processus.
    let bombe = deflate(&vec![0u8; 256 * 1024 * 1024], Compression::Zlib).unwrap();
    assert!(
        bombe.len() < 1_000_000,
        "la bombe doit bien être petite ({} o)",
        bombe.len()
    );
    assert_eq!(
        inflate(&bombe, Compression::Zlib),
        Err(CodecError::TooLarge)
    );
}

// ── entrées hostiles ────────────────────────────────────────────────────────

#[test]
fn aucun_fichier_de_region_ne_fait_paniquer_la_lecture() {
    let mut rng = fixture::Rng::new(0xBADF00D);
    for _ in 0..300 {
        // En-tête valide en taille, contenu entièrement aléatoire : le cas
        // d'un fichier écrasé par autre chose.
        let n = HEADER + rng.below(8 * SECTOR);
        let buf: Vec<u8> = (0..n).map(|_| (rng.next() & 0xFF) as u8).collect();

        let region = read(&buf, 0, 0).unwrap();
        for c in region.iter() {
            // Décompresser du bruit échoue presque toujours ; ce qui compte,
            // c'est qu'on obtienne un résultat.
            if let Ok(inflated) = inflate(&c.payload, c.compression) {
                let _ = scan(&inflated);
            }
        }
        // Et réécrire ce qu'on a lu ne doit pas paniquer non plus.
        let _ = write(&region);
    }
}

#[test]
fn un_en_tete_qui_pointe_dans_l_en_tete_est_sans_danger() {
    // Secteur 0 ou 1 = l'en-tête lui-même. Le lecteur ne doit pas se mettre à
    // lire sa propre table de localisation comme un chunk.
    let mut buf = vec![0u8; HEADER + SECTOR];
    for (i, sector) in [(0usize, 0u32), (1, 1)] {
        let loc = (sector << 8) | 1;
        buf[i * 4..i * 4 + 4].copy_from_slice(&loc.to_be_bytes());
    }
    let region = read(&buf, 0, 0).unwrap();
    // Le secteur 0 est exclu par la règle `sector_off == 0`. Le secteur 1 est
    // lu, mais sa longueur vaut 0, donc il est sauté.
    assert_eq!(region.count(), 0);
}
