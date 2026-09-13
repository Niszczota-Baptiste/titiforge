//! Chunks surdimensionnés : les charges déportées en `c.X.Z.mcc`.
//!
//! Au-delà de 255 secteurs (≈ 1 Mio compressé), le compte de secteurs ne tient
//! plus sur son octet. Minecraft range alors la charge dans un fichier à côté
//! de la région et ne laisse qu'un talon : longueur 1, et le bit 0x80 posé sur
//! l'octet de compression.
//!
//! Ce crate ne touche pas au disque — c'est l'invariant qui permettra de le
//! rebrancher sur autre chose qu'un système de fichiers. L'écriture RE**ND**
//! donc les fichiers à poser, et la lecture signale ce qu'il faut aller
//! chercher.

mod common;
use common::fixture::{self, SectionSpec};
use common::frozen;

use std::borrow::Cow;
use tf_anvil::{
    external_file_name, inflate, read, scan, write, Compression, EXTERNAL_FLAG, MAX_SECTORS, SECTOR,
};

/// Une charge que zlib ne peut pas comprimer, donc qui dépasse vraiment.
fn charge_incompressible(octets: usize) -> Vec<u8> {
    let mut s = 0x9E37_79B9u32;
    (0..octets)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s & 0xFF) as u8
        })
        .collect()
}

// ── le nom du fichier ───────────────────────────────────────────────────────

#[test]
fn le_nom_du_mcc_porte_les_coordonnees_monde_du_chunk() {
    // Pas les coordonnées locales : deux régions voisines auraient sinon des
    // fichiers du même nom dans le même dossier.
    assert_eq!(external_file_name(0, 0), "c.0.0.mcc");
    assert_eq!(external_file_name(-59, 39), "c.-59.39.mcc");
    assert_eq!(external_file_name(1023, -1024), "c.1023.-1024.mcc");
}

// ── lecture d'un talon ──────────────────────────────────────────────────────

#[test]
fn un_talon_est_reconnu_et_sa_compression_demasquee() {
    // Sans le masquage, `2 | 0x80` = 130 se lirait comme une compression
    // inconnue, et le chunk deviendrait illisible.
    for comp in [1u8, 2, 3] {
        let src = fixture::region_file_with_stub(5, 7, comp);
        let region = read(&src, 0, 0).unwrap();
        let c = region
            .get(5, 7)
            .expect("le talon doit être vu comme un chunk");

        assert!(c.external, "compression {comp}");
        assert_eq!(c.compression, Compression::from_byte(comp), "démasquée");
        assert!(c.payload.is_empty(), "un talon ne porte aucune charge");
        assert!(c.needs_external(), "et il faut aller la chercher");
        assert_eq!(c.timestamp, 42, "l'horodatage reste lisible");
    }
}

#[test]
fn un_chunk_en_ligne_n_est_jamais_marque_deporte() {
    let mut rng = fixture::Rng::new(1);
    let secs = vec![SectionSpec::with_palette_size(0, 12, &mut rng)];
    let src = fixture::region_file(&[(0, 0, fixture::chunk_nbt(0, 0, &secs))], 0);
    let c = read(&src, 0, 0).unwrap();
    let c = c.get(0, 0).unwrap();
    assert!(!c.external);
    assert!(!c.needs_external());
}

#[test]
fn une_charge_deportee_se_resout_et_se_decode_normalement() {
    let mut rng = fixture::Rng::new(2);
    let secs = vec![
        SectionSpec::uniform(0, "minecraft:bedrock"),
        SectionSpec::with_palette_size(1, 30, &mut rng),
    ];
    let nbt = fixture::chunk_nbt(5, 7, &secs);
    let mcc = fixture::zlib_bytes(&nbt);

    let src = fixture::region_file_with_stub(5, 7, 2);
    let mut region = read(&src, 0, 0).unwrap();
    let c = region.get_mut(5, 7).unwrap();
    assert!(c.needs_external());

    c.resolve_external(mcc);
    assert!(!c.needs_external(), "la charge est là");

    let c = region.get(5, 7).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();
    assert_eq!(s.sections.len(), 2);
    assert_eq!(s.x_pos, Some(5));
    assert_eq!(s.z_pos, Some(7));
}

// ── écriture : le déport est automatique ────────────────────────────────────

#[test]
fn un_chunk_trop_gros_part_dans_un_mcc_au_lieu_d_echouer() {
    let mut rng = fixture::Rng::new(3);
    let secs = vec![SectionSpec::with_palette_size(0, 8, &mut rng)];
    let src = fixture::region_file(&[(3, 4, fixture::chunk_nbt(3, 4, &secs))], 0);
    let mut region = read(&src, -1, 2).unwrap();

    // 2 Mio d'octets que zlib ne réduit pas : au-delà de 255 secteurs.
    let enorme = charge_incompressible(2 * 1024 * 1024);
    region.get_mut(3, 4).unwrap().payload = Cow::Owned(enorme.clone());

    let out = write(&region).unwrap();
    assert_eq!(out.external.len(), 1, "un fichier déporté à écrire");
    // Coordonnées MONDE : région (-1, 2) → chunk (-32 + 3, 64 + 4).
    assert_eq!(out.external[0].name, "c.-29.68.mcc");
    assert_eq!(out.external[0].bytes, enorme, "la charge part telle quelle");
    assert!(out.removed_external.is_empty());

    // Et le `.mca` ne porte plus qu'un talon.
    let relu = read(&out.region, -1, 2).unwrap();
    let c = relu.get(3, 4).unwrap();
    assert!(c.external);
    assert!(c.payload.is_empty());
    assert_eq!(
        c.compression,
        Compression::Zlib,
        "la compression survit au drapeau"
    );

    // La région entière tient en trois secteurs : l'en-tête plus le talon.
    assert_eq!(out.region.len(), 2 * SECTOR + SECTOR);
}

#[test]
fn un_chunk_juste_sous_la_limite_reste_en_ligne() {
    // La frontière exacte : 255 secteurs tiennent, 256 non. Un `>=` au lieu
    // d'un `>` déporterait des chunks qui n'en ont pas besoin, et un `>` de
    // trop écrirait un compte de secteurs tronqué à 0.
    let mut rng = fixture::Rng::new(4);
    let secs = vec![SectionSpec::with_palette_size(0, 8, &mut rng)];
    let src = fixture::region_file(&[(0, 0, fixture::chunk_nbt(0, 0, &secs))], 0);

    for (octets, deporte) in [
        (MAX_SECTORS * SECTOR - 5, false), // pile 255 secteurs
        (MAX_SECTORS * SECTOR - 4, true),  // un octet de trop → 256
    ] {
        let mut region = read(&src, 0, 0).unwrap();
        region.get_mut(0, 0).unwrap().payload = Cow::Owned(charge_incompressible(octets));
        let out = write(&region).unwrap();
        assert_eq!(out.external.is_empty(), !deporte, "{octets} octets");

        let relu = read(&out.region, 0, 0).unwrap();
        assert_eq!(relu.get(0, 0).unwrap().external, deporte, "{octets} octets");
    }
}

#[test]
fn un_chunk_qui_redevient_petit_signale_son_ancien_mcc() {
    // Laisser le `.mcc` ne casserait rien pour le jeu — il ne le lit que si le
    // talon le désigne — mais il occuperait le disque pour toujours, et une
    // save qui grossit sans raison finit par être signalée comme un bug.
    let src = fixture::region_file_with_stub(1, 1, 2);
    let mut region = read(&src, 0, 0).unwrap();

    let mut rng = fixture::Rng::new(5);
    let secs = vec![SectionSpec::with_palette_size(0, 10, &mut rng)];
    let petit = fixture::zlib_bytes(&fixture::chunk_nbt(1, 1, &secs));
    region.get_mut(1, 1).unwrap().resolve_external(petit);

    let out = write(&region).unwrap();
    assert!(out.external.is_empty(), "il tient en ligne maintenant");
    assert_eq!(out.removed_external, vec!["c.1.1.mcc".to_string()]);

    let relu = read(&out.region, 0, 0).unwrap();
    assert!(!relu.get(1, 1).unwrap().external);
}

#[test]
fn un_chunk_deporte_qui_le_reste_ne_signale_aucune_suppression() {
    let src = fixture::region_file_with_stub(2, 2, 2);
    let mut region = read(&src, 0, 0).unwrap();
    region
        .get_mut(2, 2)
        .unwrap()
        .resolve_external(charge_incompressible(2 * 1024 * 1024));

    let out = write(&region).unwrap();
    assert_eq!(out.external.len(), 1);
    assert_eq!(out.external[0].name, "c.2.2.mcc");
    assert!(
        out.removed_external.is_empty(),
        "il était déporté et le reste : rien à supprimer"
    );
}

// ── le tout ensemble ────────────────────────────────────────────────────────

#[test]
fn une_region_melangeant_chunks_en_ligne_et_deportes_fait_l_aller_retour() {
    let mut rng = fixture::Rng::new(6);
    let mut chunks = Vec::new();
    for k in 0..4u32 {
        let secs = vec![SectionSpec::with_palette_size(0, 10 + k as usize, &mut rng)];
        chunks.push((k, 0, fixture::chunk_nbt(k as i32, 0, &secs)));
    }
    let src = fixture::region_file(&chunks, 0);
    let avant = frozen::census(&src);

    let mut region = read(&src, 0, 0).unwrap();
    // On fait grossir le chunk 2 au-delà de la limite.
    region.get_mut(2, 0).unwrap().payload = Cow::Owned(charge_incompressible(2 * 1024 * 1024));

    let out = write(&region).unwrap();
    assert_eq!(out.external.len(), 1);
    assert_eq!(out.external[0].name, "c.2.0.mcc");

    let relu = read(&out.region, 0, 0).unwrap();
    assert_eq!(relu.count(), 4, "les quatre chunks restent référencés");
    for k in 0..4i32 {
        assert_eq!(relu.get(k, 0).unwrap().external, k == 2, "chunk {k}");
    }

    // Les trois chunks restés en ligne sont intacts, octet pour octet.
    let source = read(&src, 0, 0).unwrap();
    for k in [0i32, 1, 3] {
        assert_eq!(
            source.get(k, 0).unwrap().payload,
            relu.get(k, 0).unwrap().payload,
            "chunk {k}"
        );
    }

    // Et le décodeur indépendant lit toujours les trois — il ne voit pas le
    // quatrième, dont la charge est dans un fichier qu'il n'a pas.
    // Le décodeur indépendant ne voit que le `.mca` : il lit les trois chunks
    // en ligne et saute le talon, faute d'avoir le `.mcc`.
    let apres = frozen::census(&out.region);
    assert!(!apres.is_empty());
    assert!(
        apres.values().sum::<usize>() < avant.values().sum::<usize>(),
        "un chunk de moins à compter, puisque sa charge est ailleurs"
    );
    assert_eq!(frozen::decode_region(&out.region).len(), 3);
}

#[test]
fn le_drapeau_externe_ne_se_confond_pas_avec_une_compression() {
    // Les quatre octets possibles, drapeau posé ou non.
    for comp in [1u8, 2, 3, 9] {
        assert_eq!((comp | EXTERNAL_FLAG) & !EXTERNAL_FLAG, comp);
        assert!(
            comp & EXTERNAL_FLAG == 0,
            "une compression tient sur 7 bits"
        );
    }
}
