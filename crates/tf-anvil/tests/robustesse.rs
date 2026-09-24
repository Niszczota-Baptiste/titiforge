//! Les trous trouvés en auditant, et leurs garde-fous.
//!
//! Aucun de ces cas ne faisait planter quoi que ce soit. Tous rendaient un
//! résultat plausible et faux — la seule forme de bug qui compte vraiment sur
//! ce produit, parce qu'elle sort dans la save d'un utilisateur et qu'on ne
//! l'apprend qu'après.

mod common;
use common::fixture::{self, SectionSpec};
use common::frozen;

use std::borrow::Cow;
use tf_anvil::{
    bits_for, decode_section, deflate, encode_section, in_section, inflate, read, scan,
    section_edits, splice, write, Edit, EncodeError, Interner, Layout, Packing, Section, MAX_BITS,
    MAX_PALETTE, VOL,
};
use tf_nbt::Span;

fn section_de(palette_len: usize, indices: &[u16]) -> Section {
    let mut s = Section {
        y: 0,
        palette: (0..palette_len as u32).collect(),
        bits: bits_for(palette_len),
        data: Box::new([]),
        packing: Packing::NoStraddle,
    };
    s.repack(indices);
    s
}

// ── A. la palette ne peut pas dépasser ce que le format porte ───────────────

#[test]
fn bits_for_plafonne_a_douze_quoi_qu_on_lui_demande() {
    // Une section n'a que 4096 blocs : elle ne peut pas porter plus de 4096
    // états distincts, et le jeu ne lit pas au-delà de 12 bits. Sans le
    // plafond, une palette de 5000 produisait 13 bits et un fichier
    // qu'aucun Minecraft ne relit — sans la moindre erreur.
    assert_eq!(bits_for(MAX_PALETTE), MAX_BITS);
    for n in [4097usize, 5000, 8192, 70_000, usize::MAX / 2] {
        assert_eq!(bits_for(n), MAX_BITS, "palette de {n}");
    }
    // Et au-delà de 16 bits, les indices ne tiendraient même plus dans le u16
    // du dépack : ils sortaient tronqués à 65535. En bloc `const`, c'est le
    // COMPILATEUR qui le garantit — un test ne peut pas échouer sur quelque
    // chose qui ne compile pas.
    const _: () = assert!(MAX_BITS <= 16);
}

#[test]
fn repack_compacte_plutot_que_d_ecrire_une_section_illisible() {
    // On fabrique une palette de 5000 entrées dont seules quelques-unes sont
    // réellement référencées — exactement ce que produirait une suite de
    // `replace_state` suivie d'ajouts.
    let mut s = Section {
        y: 0,
        palette: (0..5000u32).collect(),
        bits: bits_for(5000),
        data: Box::new([]),
        packing: Packing::NoStraddle,
    };
    // 20 états distincts sur les 4096 blocs.
    let idx: Vec<u16> = (0..VOL).map(|n| (n % 20) as u16).collect();
    s.repack(&idx);

    assert!(
        s.palette.len() <= MAX_PALETTE,
        "repack doit compacter : {} entrées",
        s.palette.len()
    );
    assert_eq!(
        s.palette.len(),
        20,
        "seules les entrées référencées survivent"
    );
    assert!(s.bits <= MAX_BITS);

    // Et le contenu est intact : chaque bloc vaut toujours le même état.
    let relu = s.unpack();
    for (n, &v) in relu.iter().enumerate() {
        assert_eq!(s.palette[v as usize], (n % 20) as u32, "bloc {n}");
    }
}

#[test]
fn une_palette_de_plus_de_4096_refuse_l_ecriture_en_disant_pourquoi() {
    let mut interner = Interner::new();
    let ids: Vec<u32> = (0..5000)
        .map(|i| interner.intern(&format!("test:bloc_{i}")))
        .collect();
    let s = Section {
        y: 0,
        palette: ids,
        bits: MAX_BITS,
        data: vec![0u64; 820].into_boxed_slice(),
        packing: Packing::NoStraddle,
    };
    assert_eq!(
        encode_section(&s, &interner).unwrap_err(),
        EncodeError::PaletteTooLarge(5000)
    );
}

// ── B. les coordonnées d'un bloc sont bornées ──────────────────────────────

#[test]
fn get_hors_de_la_section_rend_none_et_pas_le_bloc_du_voisin() {
    // Le piège : `local_index(16, 0, 0)` vaut 16, c'est-à-dire (0, 0, 1). Sans
    // borne, `get(16, 0, 0)` rendait le bloc de la case d'à côté — un résultat
    // parfaitement plausible, et faux.
    let mut idx = vec![0u16; VOL];
    idx[tf_anvil::local_index(0, 0, 1)] = 7;
    let s = section_de(8, &idx);

    assert_eq!(s.get(0, 0, 1), Some(7), "la vraie case");
    assert_eq!(s.get(16, 0, 0), None, "hors section : None, pas le voisin");
    assert_eq!(s.get(0, 16, 0), None);
    assert_eq!(s.get(0, 0, 16), None);
    assert_eq!(s.get(999, 999, 999), None);

    // Toutes les cases valides répondent, elles.
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                assert!(s.get(x, y, z).is_some(), "({x},{y},{z})");
            }
        }
    }
}

#[test]
fn in_section_delimite_exactement_le_domaine() {
    assert!(in_section(0, 0, 0));
    assert!(in_section(15, 15, 15));
    assert!(!in_section(16, 0, 0));
    assert!(!in_section(0, 16, 0));
    assert!(!in_section(0, 0, 16));
}

// ── C. count_of ne doit pas être quadratique ───────────────────────────────

#[test]
fn count_of_reste_juste_avec_une_palette_pleine_de_doublons() {
    // La version en `contains()` coûtait O(palette × 4096) : mesuré à 340 µs
    // pour une section, soit 8,4 s pour une région pleine. Ce test fige la
    // JUSTESSE ; le coût, lui, est mesuré par le bench.
    let n = 2000usize;
    let mut s = Section {
        y: 0,
        palette: (0..n as u32).map(|i| i % 3).collect(),
        bits: bits_for(n),
        data: Box::new([]),
        packing: Packing::NoStraddle,
    };
    let idx: Vec<u16> = (0..VOL).map(|i| (i % n) as u16).collect();
    s.repack(&idx);

    // `repack` ne compacte QUE s'il le doit — au-delà de 4096 entrées.
    // Compacter systématiquement coûterait le prix d'un compactage à chaque
    // écriture, mesuré à 103 ms pour une région. La palette garde donc ses
    // 2000 entrées et ses 667 doublons par état : c'est exactement le cas que
    // `count_of` doit tenir.
    assert_eq!(s.palette.len(), n, "repack ne compacte pas sous le plafond");
    assert!(
        s.palette.iter().filter(|&&e| e == 1).count() > 600,
        "le test doit bien produire les doublons qu'il prétend éprouver"
    );

    let attendu = |cible: u32| idx.iter().filter(|&&v| (v as u32) % 3 == cible).count();
    for cible in 0..3u32 {
        assert_eq!(s.count_of(cible), attendu(cible), "état {cible}");
    }
    assert_eq!(
        (0..3).map(|c| s.count_of(c)).sum::<usize>(),
        VOL,
        "chaque bloc compté une fois et une seule, malgré les doublons"
    );
}

// ── D. un chunk qui porte les deux dispositions ────────────────────────────

#[test]
fn la_racine_gagne_sur_level_quand_un_chunk_porte_les_deux() {
    // Un monde en cours de conversion peut porter les deux le temps d'une
    // migration. Prendre `Level` lirait la version PÉRIMÉE du chunk, et
    // l'écriture écraserait la neuve.
    let mut rng = fixture::Rng::new(1);
    let moderne = vec![SectionSpec::with_palette_size(0, 20, &mut rng)];
    let ancien = vec![
        SectionSpec::with_palette_size(5, 20, &mut rng),
        SectionSpec::with_palette_size(6, 20, &mut rng),
    ];
    let a = fixture::chunk_nbt(0, 0, &moderne);
    let b = fixture::legacy_chunk_nbt(0, 0, &ancien, true);
    let mut mixte = a[..a.len() - 1].to_vec(); // sans le END de la racine
    mixte.extend_from_slice(&b[3..b.len() - 1]); // les champs de b, sans sa racine
    mixte.push(0);

    let src = fixture::region_file(&[(0, 0, mixte)], 0);
    let r = read(&src, 0, 0).unwrap();
    let c = r.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();

    assert_eq!(s.layout, Layout::Flat, "la racine doit l'emporter");
    assert_eq!(
        s.sections.len(),
        1,
        "celles de `sections`, pas celles de `Level`"
    );
    assert_eq!(s.sections[0].y, 0);
}

// ── E. une palette ancienne qui GRANDIT ────────────────────────────────────

#[test]
fn une_palette_ancienne_qui_grandit_se_splice_sans_abimer_ses_voisins() {
    // Le splice remplace des plages d'octets. Quand la nouvelle charge est plus
    // LONGUE que l'ancienne, tout ce qui suit se décale — et le champ voisin
    // doit malgré tout ressortir intact.
    let mut rng = fixture::Rng::new(2);
    let secs = vec![SectionSpec::with_palette_size(0, 20, &mut rng)];
    let src = fixture::region_file(&[(0, 0, fixture::legacy_chunk_nbt(0, 0, &secs, true))], 0);

    let mut region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let compression = c.compression;
    let inflated = inflate(&c.payload, compression).unwrap();
    let s = scan(&inflated).unwrap();
    let mut interner = Interner::new();
    let sc = &s.sections[0];
    let mut sec = decode_section(&inflated, &s, sc, &mut interner)
        .unwrap()
        .unwrap();

    let avant_len = sec.palette.len();
    let avant_bits = sec.bits;
    for i in 0..10 {
        sec.palette
            .push(interner.intern(&format!("unmodtreslongnamespace:bloc_tres_long_{i}")));
    }
    // On référence les nouvelles entrées, sinon `repack` les compacte.
    let mut idx = sec.unpack().to_vec();
    for (k, slot) in idx.iter_mut().take(10).enumerate() {
        *slot = (avant_len + k) as u16;
    }
    sec.repack(&idx);
    assert_eq!(sec.palette.len(), avant_len + 10);
    assert_eq!(
        sec.bits, avant_bits,
        "20 → 30 entrées tient toujours en 5 bits"
    );

    let mut edits = section_edits(&inflated, &sec, sc, &interner).unwrap();
    assert_eq!(edits.len(), 2, "Palette et BlockStates, deux champs frères");
    let neuf = splice(&inflated, &mut edits).unwrap();
    assert!(neuf.len() > inflated.len(), "la charge a grossi");

    region.get_mut(0, 0).unwrap().payload = Cow::Owned(deflate(&neuf, compression).unwrap());
    let out = write(&region).unwrap().region;

    let chunks = frozen::decode_region(&out);
    let sect = &chunks[&(0, 0)]
        .root
        .get("Level")
        .unwrap()
        .get("Sections")
        .unwrap()
        .as_list()
        .unwrap()[0];
    assert_eq!(frozen::section_states(sect).unwrap().len(), VOL);
    assert_eq!(
        sect.get("SkyLight")
            .and_then(|t| t.as_bytes())
            .unwrap()
            .len(),
        2048,
        "le champ voisin, décalé par le splice, doit être intact"
    );
}

// ── F. le splice doit être déterministe ────────────────────────────────────

#[test]
fn le_resultat_d_un_splice_ne_depend_pas_de_l_ordre_du_vecteur() {
    // Trier sur le SEUL début rendait le résultat dépendant de l'ordre
    // d'insertion : une insertion en `p` et un remplacement commençant en `p`
    // passaient ou rendaient `Overlap` selon lequel arrivait en premier.
    let base = b"ABCDEF".to_vec();
    let inserer = Edit {
        span: Span { start: 2, end: 2 },
        bytes: b"!".to_vec(),
    };
    let remplacer = Edit {
        span: Span { start: 2, end: 4 },
        bytes: b"__".to_vec(),
    };

    let mut a = vec![inserer.clone(), remplacer.clone()];
    let mut b = vec![remplacer, inserer];
    let ra = splice(&base, &mut a);
    let rb = splice(&base, &mut b);

    assert_eq!(ra, rb, "les deux ordres doivent donner le MÊME résultat");
    assert_eq!(ra.unwrap(), b"AB!__EF".to_vec(), "l'insertion vient avant");
}

#[test]
fn deux_insertions_au_meme_point_se_suivent_dans_l_ordre_donne() {
    let base = b"ABCDEF".to_vec();
    let mut e = vec![
        Edit {
            span: Span { start: 3, end: 3 },
            bytes: b"<1>".to_vec(),
        },
        Edit {
            span: Span { start: 3, end: 3 },
            bytes: b"<2>".to_vec(),
        },
    ];
    assert_eq!(splice(&base, &mut e).unwrap(), b"ABC<1><2>DEF".to_vec());
}

// ── l'API que le parallélisme réclame ──────────────────────────────────────

#[test]
fn merge_from_refond_deux_tables_sans_perdre_un_etat() {
    let mut a = Interner::new();
    let pierre = a.intern("minecraft:stone");
    let terre = a.intern("minecraft:dirt");

    let mut b = Interner::new();
    let b_terre = b.intern("minecraft:dirt"); // commun, dans un autre ordre
    let b_herbe = b.intern("minecraft:grass_block"); // nouveau
    let b_pierre = b.intern("minecraft:stone");

    let table = a.merge_from(&b);
    assert_eq!(table.len(), b.len());
    assert_eq!(
        table[b_terre as usize], terre,
        "un état commun garde son identité"
    );
    assert_eq!(table[b_pierre as usize], pierre);
    assert_eq!(a.len(), 3, "un seul état nouveau ajouté");
    assert_eq!(
        a.resolve(table[b_herbe as usize]),
        Some("minecraft:grass_block")
    );

    // Et le remap d'une palette suit.
    let mut palette = vec![b_herbe, b_terre, b_pierre];
    Interner::remap_palette(&table, &mut palette);
    assert_eq!(palette[1], terre);
    assert_eq!(palette[2], pierre);
    assert_eq!(a.resolve(palette[0]), Some("minecraft:grass_block"));
}

#[test]
fn merge_from_est_idempotent() {
    let mut a = Interner::new();
    a.intern("minecraft:stone");
    let mut b = Interner::new();
    b.intern("minecraft:stone");
    b.intern("minecraft:dirt");

    let un = a.merge_from(&b);
    let apres = a.len();
    let deux = a.merge_from(&b);
    assert_eq!(un, deux, "refondre deux fois donne la même table");
    assert_eq!(a.len(), apres, "et n'ajoute rien de plus");
}

#[test]
fn un_indice_hors_palette_ne_tue_pas_le_compactage() {
    // `bits` se DÉDUIT de la longueur de palette : deux entrées se lisent sur
    // quatre bits, donc seize valeurs sont représentables pour deux valides. Un
    // `.mca` corrompu, tronqué ou forgé en porte — et un moteur qui panique
    // dessus panique sur la sauvegarde de quelqu'un.
    //
    // Le compactage ne peut pas conserver un tel indice : il renumérote la
    // palette. Mais il doit rendre un résultat LISIBLE plutôt que mourir.
    let mut s = section_de(2, &[0; VOL]);
    // Une palette au-delà du plafond, pour forcer le compactage.
    for _ in 0..MAX_PALETTE + 10 {
        s.palette.push(42);
    }
    let mut idx = vec![0u16; VOL];
    idx[7] = 60_000; // très au-delà de tout
    idx[9] = 3;
    s.repack(&idx);

    assert!(
        s.palette.len() <= MAX_PALETTE,
        "la palette doit être compactée"
    );
    let relu = s.unpack();
    for (i, &v) in relu.iter().enumerate() {
        assert!(
            (v as usize) < s.palette.len(),
            "case {i} : l'indice {v} sort encore de la palette"
        );
    }
}

#[test]
fn un_emplacement_qui_pointe_hors_du_fichier_est_compte_pas_tu() {
    // Un emplacement illisible est laissé vide pour sauver les 1023 autres —
    // et il est COMPTÉ : sans ce nombre, une région corrompue se lisait
    // exactement comme une région vide, et l'utilisateur voyait du vide sans
    // savoir pourquoi.
    let mut buf = vec![0u8; 8192 + 4096];
    // L'emplacement 0 : un chunk sain d'un secteur, au secteur 2.
    buf[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    buf[8192..8196].copy_from_slice(&2u32.to_be_bytes());
    buf[8196] = 2; // zlib
                   // L'emplacement 1 : un secteur bien au-delà de la fin du fichier.
    buf[4..8].copy_from_slice(&((900u32 << 8) | 1).to_be_bytes());
    // L'emplacement 2 : au secteur 2 aussi, mais une longueur qui déborde.
    buf[8..12].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    let r = tf_anvil::region::read(&buf, 0, 0).expect("en-tête lisible");
    assert!(r.slots[0].is_some(), "le sain est repéré");
    assert!(
        r.slots[1].is_none(),
        "l'offset hors fichier est laissé vide"
    );
    assert_eq!(r.illisibles, 1, "et compté");

    let mut debordant = buf.clone();
    debordant[8192..8196].copy_from_slice(&100_000u32.to_be_bytes());
    let r = tf_anvil::region::read(&debordant, 0, 0).expect("en-tête lisible");
    assert_eq!(
        r.illisibles, 3,
        "une longueur qui déborde rend illisibles les emplacements qui la partagent"
    );

    let saine = vec![0u8; 8192];
    assert_eq!(tf_anvil::region::read(&saine, 0, 0).unwrap().illisibles, 0);
}
