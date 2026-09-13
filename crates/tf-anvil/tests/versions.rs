//! Les formes qu'un chunk a prises au fil des versions.
//!
//! Une save réelle mélange des chunks de plusieurs versions : le jeu ne
//! réécrit un chunk que lorsqu'un joueur le visite. Un lecteur qui décide de
//! la forme au niveau du MONDE lit donc de travers la moitié d'une save
//! ancienne — sans rien signaler, parce qu'un indice de palette mal dépacké
//! reste un indice valide.

mod common;
use common::fixture::{self, SectionSpec};
use common::frozen;

use std::borrow::Cow;
use tf_anvil::{
    decode_section, deflate, detect_packing, inflate, longs_for, pack, read, scan, section_edits,
    splice, unpack_into, version_label, write, Edit, Interner, Layout, Packing, Section, VOL,
};

// ── le packing se déduit de la longueur ─────────────────────────────────────

#[test]
fn les_deux_packings_ne_coincident_qu_ou_ils_sont_identiques() {
    // C'est le fait qui autorise à se passer d'une table de DataVersion. Les
    // longueurs ne se confondent qu'à 4 et 8 bits — et là, `bits` divise 64,
    // donc les deux dispositions produisent exactement les mêmes octets.
    for bits in 4..=12usize {
        let sans = longs_for(VOL, bits, Packing::NoStraddle);
        let avec = longs_for(VOL, bits, Packing::Straddle);
        if 64 % bits == 0 {
            assert_eq!(
                sans, avec,
                "{bits} bits divisent 64 : les longueurs coïncident"
            );
            let idx: Vec<u16> = (0..VOL).map(|n| (n % (1 << bits)) as u16).collect();
            assert_eq!(
                pack(&idx, bits, Packing::NoStraddle),
                pack(&idx, bits, Packing::Straddle),
                "{bits} bits : les OCTETS doivent être identiques, sinon l'ambiguïté compte"
            );
        } else {
            assert_ne!(sans, avec, "{bits} bits : les longueurs doivent différer");
        }
    }
}

#[test]
fn detect_packing_retrouve_la_disposition_a_partir_de_la_longueur() {
    for bits in 4..=12usize {
        for packing in [Packing::NoStraddle, Packing::Straddle] {
            let n = longs_for(VOL, bits, packing);
            let detecte =
                detect_packing(VOL, bits, n).expect("une longueur valide doit se détecter");
            if 64 % bits == 0 {
                // Ambiguë, mais sans conséquence : les deux sont interchangeables.
                assert_eq!(detecte, Packing::NoStraddle);
            } else {
                assert_eq!(detecte, packing, "{bits} bits, {packing:?}");
            }
        }
    }
}

#[test]
fn une_longueur_qui_ne_correspond_a_rien_est_refusee_pas_devinee() {
    // Un tableau tronqué. Deviner écrirait des blocs faux dans une save ;
    // refuser laisse le chunk intact.
    for bits in [5usize, 6, 9, 11, 12] {
        let bon = longs_for(VOL, bits, Packing::NoStraddle);
        assert!(
            detect_packing(VOL, bits, bon - 1).is_none(),
            "{bits} bits, un long de moins"
        );
        assert!(
            detect_packing(VOL, bits, bon + 1).is_none(),
            "{bits} bits, un long de trop"
        );
        assert!(detect_packing(VOL, bits, 0).is_none());
    }
}

#[test]
fn pack_et_unpack_font_l_aller_retour_dans_les_deux_dispositions() {
    let mut rng = fixture::Rng::new(6161);
    for &n in &[2usize, 16, 17, 33, 64, 65, 200, 500, 2000, 4096] {
        let bits = {
            let mut b = 4usize;
            while (1usize << b) < n {
                b += 1;
            }
            b
        };
        let idx: Vec<u16> = (0..VOL).map(|_| rng.below(n) as u16).collect();
        for packing in [Packing::NoStraddle, Packing::Straddle] {
            let data = pack(&idx, bits, packing);
            assert_eq!(data.len(), longs_for(VOL, bits, packing));
            let mut out = vec![0u16; VOL];
            unpack_into(&data, VOL, bits, packing, &mut out);
            assert_eq!(out, idx, "palette de {n} ({bits} bits), {packing:?}");
        }
    }
}

#[test]
fn le_packing_avec_chevauchement_est_compatible_avec_l_implementation_independante() {
    // Être auto-cohérent ne prouve pas être compatible avec Minecraft : le
    // producteur du test implémente la spec de son côté.
    let mut rng = fixture::Rng::new(4141);
    for &n in &[17usize, 40, 300, 1000, 4096] {
        let bits = fixture::bits_for(n);
        let idx: Vec<u16> = (0..VOL).map(|_| rng.below(n) as u16).collect();
        assert_eq!(
            pack(&idx, bits, Packing::Straddle),
            fixture::pack_straddle(&idx, bits),
            "palette de {n}"
        );
    }
}

// ── lecture du format ancien ────────────────────────────────────────────────

fn region_ancienne(straddle: bool) -> Vec<u8> {
    let mut rng = fixture::Rng::new(if straddle { 101 } else { 202 });
    let mut chunks = Vec::new();
    for lz in 0..2u32 {
        for lx in 0..2u32 {
            let secs = vec![
                SectionSpec::uniform(0, "minecraft:bedrock"),
                SectionSpec::with_palette_size(1, 20, &mut rng), // 5 bits : distinguable
                SectionSpec::with_palette_size(2, 300, &mut rng), // 9 bits
                SectionSpec::with_palette_size(3, 16, &mut rng), // 4 bits : ambigu, donc identique
            ];
            chunks.push((
                lx,
                lz,
                fixture::legacy_chunk_nbt(lx as i32, lz as i32, &secs, straddle),
            ));
        }
    }
    fixture::region_file(&chunks, 0)
}

#[test]
fn la_disposition_se_detecte_sur_la_structure_pas_sur_le_dataversion() {
    for straddle in [true, false] {
        let src = region_ancienne(straddle);
        let region = read(&src, 0, 0).unwrap();
        let c = region.get(0, 0).unwrap();
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();

        assert_eq!(s.layout, Layout::Legacy, "straddle = {straddle}");
        assert_eq!(s.sections.len(), 4);
        // xPos et zPos vivent sous `Level` dans ce format : il faut être allé
        // les chercher là.
        assert_eq!(s.x_pos, Some(0));
        assert_eq!(s.z_pos, Some(0));
    }

    // Et le format moderne reste détecté comme tel.
    let mut rng = fixture::Rng::new(1);
    let secs = vec![SectionSpec::with_palette_size(0, 20, &mut rng)];
    let src = fixture::region_file(&[(0, 0, fixture::chunk_nbt(0, 0, &secs))], 0);
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    assert_eq!(scan(&inflated).unwrap().layout, Layout::Flat);
}

#[test]
fn le_packing_d_un_chunk_ancien_est_celui_de_son_fichier() {
    for (straddle, attendu) in [(true, Packing::Straddle), (false, Packing::NoStraddle)] {
        let src = region_ancienne(straddle);
        let region = read(&src, 0, 0).unwrap();
        let c = region.get(0, 0).unwrap();
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        let mut interner = Interner::new();

        let mut vus = Vec::new();
        for sc in &s.sections {
            if let Some(section) = decode_section(&inflated, &s, sc, &mut interner).unwrap() {
                if !section.data.is_empty() {
                    vus.push((section.bits, section.packing));
                }
            }
        }
        // 5 et 9 bits distinguent les deux dispositions ; 4 bits est ambigu et
        // se lit toujours comme « sans chevauchement », ce qui est correct
        // puisque les octets sont les mêmes.
        assert!(
            vus.iter().any(|&(b, p)| b == 5 && p == attendu),
            "5 bits, {vus:?}"
        );
        assert!(
            vus.iter().any(|&(b, p)| b == 9 && p == attendu),
            "9 bits, {vus:?}"
        );
        assert!(vus.iter().any(|&(b, p)| b == 4 && p == Packing::NoStraddle));
    }
}

#[test]
fn un_chunk_ancien_se_relit_avec_les_memes_blocs() {
    for straddle in [true, false] {
        let src = region_ancienne(straddle);
        let avant = frozen::census(&src);
        assert!(!avant.is_empty());
        let sortie = write(&read(&src, 0, 0).unwrap()).unwrap();
        assert_eq!(frozen::census(&sortie), avant, "straddle = {straddle}");
    }
}

// ── écriture du format ancien ───────────────────────────────────────────────

fn editer_ancien(
    src: &[u8],
    lx: i32,
    lz: i32,
    mut f: impl FnMut(&mut Section, &mut Interner) -> bool,
) -> Vec<u8> {
    let mut region = read(src, 0, 0).unwrap();
    let raw = region.get(lx, lz).unwrap();
    let compression = raw.compression;
    let inflated = inflate(&raw.payload, compression).unwrap();
    let scanned = scan(&inflated).unwrap();
    let mut interner = Interner::new();
    let mut edits: Vec<Edit> = Vec::new();

    for sc in &scanned.sections {
        let Some(mut section) = decode_section(&inflated, &scanned, sc, &mut interner).unwrap()
        else {
            continue;
        };
        if f(&mut section, &mut interner) {
            edits.extend(
                section_edits(&section, sc, &interner)
                    .expect("la palette doit se résoudre dans CET interner"),
            );
        }
    }
    let neuf = splice(&inflated, &mut edits).unwrap();
    region.get_mut(lx, lz).unwrap().payload = Cow::Owned(deflate(&neuf, compression).unwrap());
    write(&region).unwrap()
}

#[test]
fn un_replace_sur_un_chunk_ancien_preserve_la_disposition_et_le_packing() {
    for straddle in [true, false] {
        let src = region_ancienne(straddle);
        let avant = frozen::census(&src);
        let de = "minecraft:bloc_5";
        let vers = "minecraft:bloc_6";
        let n_de = *avant.get(de).unwrap_or(&0);
        let n_vers = *avant.get(vers).unwrap_or(&0);
        assert!(
            n_de > 0,
            "straddle = {straddle} : la fixture doit contenir {de}"
        );

        let mut courant = src.clone();
        for lz in 0..2 {
            for lx in 0..2 {
                courant = editer_ancien(&courant, lx, lz, |s, i| {
                    let (Some(a), Some(b)) = (i.get(de), i.get(vers)) else {
                        return false;
                    };
                    s.replace_state(a, b) > 0
                });
            }
        }

        // Le décodeur gelé déduit le packing de la longueur : s'il avait changé
        // à l'écriture, il lèverait plutôt que de rendre des blocs faux.
        let apres = frozen::census(&courant);
        assert_eq!(apres.get(de), None, "straddle = {straddle}");
        assert_eq!(*apres.get(vers).unwrap(), n_de + n_vers);
        let total = |m: &std::collections::BTreeMap<String, usize>| m.values().sum::<usize>();
        assert_eq!(total(&avant), total(&apres));

        // Et la disposition reste ancienne : on ne convertit jamais un monde
        // en le modifiant. Un chunk 1.15 réécrit en 1.18 serait illisible pour
        // le jeu de l'utilisateur.
        let region = read(&courant, 0, 0).unwrap();
        let c = region.get(0, 0).unwrap();
        let inflated = inflate(&c.payload, c.compression).unwrap();
        assert_eq!(scan(&inflated).unwrap().layout, Layout::Legacy);
    }
}

#[test]
fn les_champs_inconnus_d_un_chunk_ancien_survivent_aussi() {
    let src = region_ancienne(true);
    let lire = |mca: &[u8]| -> (Vec<u8>, Vec<usize>) {
        let chunks = frozen::decode_region(mca);
        let level = chunks[&(0, 0)].root.get("Level").unwrap();
        let marque = level
            .get("modtest:legacy")
            .and_then(|t| t.as_bytes())
            .unwrap()
            .clone();
        let sky: Vec<usize> = level
            .get("Sections")
            .and_then(|t| t.as_list())
            .unwrap()
            .iter()
            .map(|s| {
                s.get("SkyLight")
                    .and_then(|t| t.as_bytes())
                    .map(|b| b.len())
                    .unwrap_or(0)
            })
            .collect();
        (marque, sky)
    };
    let avant = lire(&src);
    assert_eq!(avant.0, vec![0xC0, 0xFF, 0xEE]);
    assert_eq!(avant.1, vec![2048; 4]);

    let sortie = editer_ancien(&src, 0, 0, |s, i| {
        let Some(cible) = i.get("minecraft:bloc_2") else {
            return false;
        };
        s.set_uniform(cible);
        true
    });
    assert_eq!(
        lire(&sortie),
        avant,
        "les champs voisins de Palette/BlockStates"
    );
}

#[test]
fn une_section_ancienne_homogene_qui_cesse_de_l_etre_recoit_son_blockstates() {
    // Le cas qui demande une INSERTION plutôt qu'un remplacement : la section
    // n'avait pas de `BlockStates` du tout. Sans ça, la palette grandirait et
    // le fichier prétendrait toujours que tout vaut l'entrée 0.
    let secs = vec![SectionSpec::uniform(0, "minecraft:stone")];
    let src = fixture::region_file(&[(0, 0, fixture::legacy_chunk_nbt(0, 0, &secs, true))], 0);

    // Confirmation que la fixture n'écrit bien AUCUN BlockStates.
    {
        let chunks = frozen::decode_region(&src);
        let s = &chunks[&(0, 0)]
            .root
            .get("Level")
            .unwrap()
            .get("Sections")
            .unwrap()
            .as_list()
            .unwrap()[0];
        assert!(
            s.get("BlockStates").is_none(),
            "la fixture doit être homogène"
        );
    }

    // On rend la section non homogène : moitié pierre, moitié terre.
    let sortie = editer_ancien(&src, 0, 0, |s, i| {
        // On interne dans l'interner DU CHUNK. Fabriquer un interner local
        // rendrait des identifiants que `section_edits` ne saurait pas
        // résoudre — et il refuserait, plutôt que d'écrire une palette fausse.
        let pierre = i.intern("minecraft:stone");
        let terre = i.intern("minecraft:dirt");
        s.palette = vec![pierre, terre];
        let idx: Vec<u16> = (0..VOL).map(|n| (n % 2) as u16).collect();
        s.repack(&idx);
        // `repack` a écrit avec le packing de la section ; on garde celui du
        // fichier, qui était « avec chevauchement ».
        assert_eq!(s.packing, Packing::Straddle);
        true
    });

    let chunks = frozen::decode_region(&sortie);
    let section = &chunks[&(0, 0)]
        .root
        .get("Level")
        .unwrap()
        .get("Sections")
        .unwrap()
        .as_list()
        .unwrap()[0];
    assert!(
        section.get("BlockStates").is_some(),
        "le champ doit avoir été inséré"
    );
    let etats = frozen::section_states(section).unwrap();
    assert_eq!(
        etats.iter().filter(|s| *s == "minecraft:stone").count(),
        VOL / 2
    );
    assert_eq!(
        etats.iter().filter(|s| *s == "minecraft:dirt").count(),
        VOL / 2
    );
    // Et la SkyLight voisine n'a pas bougé.
    assert_eq!(
        section
            .get("SkyLight")
            .and_then(|t| t.as_bytes())
            .unwrap()
            .len(),
        2048
    );
}

// ── DataVersion : conservé, jamais décisionnaire ────────────────────────────

#[test]
fn le_dataversion_est_lu_et_conserve_mais_ne_decide_de_rien() {
    let src = region_ancienne(true);
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();
    assert_eq!(s.data_version, 2230, "lu…");
    assert_eq!(version_label(s.data_version), "1.15", "…et étiqueté");

    // La preuve qu'il ne décide de rien : on le met à une valeur absurde et
    // la lecture est INCHANGÉE, parce que la forme vient de la structure.
    // Le champ NBT complet : type (1 o) + longueur u16 (2 o) + le nom (11 o),
    // puis la charge i32. La fenêtre fait donc 14 octets, pas 11.
    const ENTETE: &[u8] = b"\x03\x00\x0bDataVersion";
    assert_eq!(ENTETE.len(), 14);
    let pos = inflated
        .windows(ENTETE.len())
        .position(|w| w == ENTETE)
        .expect("le champ doit être là");
    let mut trafique = inflated.clone();
    trafique[pos + 14..pos + 18].copy_from_slice(&999_999i32.to_be_bytes());

    let s2 = scan(&trafique).unwrap();
    assert_eq!(s2.data_version, 999_999);
    assert_eq!(
        s2.layout,
        Layout::Legacy,
        "la disposition ne dépend pas du DataVersion"
    );
    assert_eq!(s2.sections.len(), s.sections.len());

    let mut i1 = Interner::new();
    let mut i2 = Interner::new();
    for (a, b) in s.sections.iter().zip(s2.sections.iter()) {
        let x = decode_section(&inflated, &s, a, &mut i1).unwrap();
        let y = decode_section(&trafique, &s2, b, &mut i2).unwrap();
        assert_eq!(
            x.map(|s| (s.bits, s.packing, s.data.len())),
            y.map(|s| (s.bits, s.packing, s.data.len())),
            "un DataVersion absurde ne doit rien changer au décodage"
        );
    }
}

#[test]
fn version_label_couvre_les_paliers_et_l_absence() {
    assert_eq!(version_label(0), "inconnue");
    assert_eq!(version_label(100), "antérieure à 1.13");
    assert_eq!(version_label(1519), "1.13");
    assert_eq!(version_label(2230), "1.15");
    assert_eq!(version_label(2724), "1.17");
    assert_eq!(version_label(3465), "1.20");
    assert_eq!(version_label(99_999), "1.21+");
}

#[test]
fn section_edits_refuse_une_palette_qui_vient_d_un_autre_interner() {
    // Le garde-fou qui a attrapé un bug de ce fichier même en l'écrivant.
    // Mélanger deux interners donne des identifiants qui pointent sur les
    // mauvais états ; écrire une palette « plausible » corromprait la save
    // sans la moindre erreur.
    let src = region_ancienne(true);
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let scanned = scan(&inflated).unwrap();

    let mut vrai = Interner::new();
    let sc = scanned.sections.iter().find(|s| s.spans.is_some()).unwrap();
    let mut section = decode_section(&inflated, &scanned, sc, &mut vrai)
        .unwrap()
        .unwrap();
    assert!(
        section_edits(&section, sc, &vrai).is_some(),
        "avec le bon interner"
    );

    // Un identifiant qui n'existe pas dans cet interner.
    section.palette[0] = vrai.len() as u32 + 500;
    assert!(
        section_edits(&section, sc, &vrai).is_none(),
        "un état non résoluble doit refuser l'écriture, pas la deviner"
    );
}

#[test]
fn un_chunk_entierement_homogene_retombe_sur_le_dataversion() {
    // Le SEUL cas où la version décide. Un chunk dont toutes les sections sont
    // homogènes ne porte aucun tableau d'indices : il n'y a rien à mesurer.
    use tf_anvil::packing_de_repli;
    assert_eq!(packing_de_repli(2230), Packing::Straddle, "1.15");
    assert_eq!(
        packing_de_repli(2528),
        Packing::Straddle,
        "juste avant 20w17a"
    );
    assert_eq!(packing_de_repli(2529), Packing::NoStraddle, "20w17a");
    assert_eq!(packing_de_repli(2724), Packing::NoStraddle, "1.17");
    assert_eq!(
        packing_de_repli(0),
        Packing::NoStraddle,
        "DataVersion absent"
    );

    for (dv, attendu) in [(2230i32, Packing::Straddle), (2724, Packing::NoStraddle)] {
        let secs = vec![SectionSpec::uniform(0, "minecraft:stone")];
        let src = fixture::region_file(
            &[(0, 0, fixture::legacy_chunk_nbt(0, 0, &secs, dv < 2529))],
            0,
        );
        let region = read(&src, 0, 0).unwrap();
        let c = region.get(0, 0).unwrap();
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        assert_eq!(s.data_version, dv);
        assert_eq!(s.packing, attendu, "DataVersion {dv}");
    }

    // Et dès qu'UNE section porte des indices, la mesure reprend la main —
    // même si le DataVersion dit le contraire.
    let mut rng = fixture::Rng::new(9);
    let secs = vec![
        SectionSpec::uniform(0, "minecraft:stone"),
        SectionSpec::with_palette_size(1, 20, &mut rng), // 5 bits : distinguable
    ];
    // DataVersion 2230 (donc « avec chevauchement » en repli), mais le fichier
    // est écrit SANS chevauchement : la structure doit l'emporter.
    let mut nbt = fixture::legacy_chunk_nbt(0, 0, &secs, false);
    let pos = nbt
        .windows(14)
        .position(|w| w == b"\x03\x00\x0bDataVersion")
        .unwrap();
    nbt[pos + 14..pos + 18].copy_from_slice(&2230i32.to_be_bytes());

    let src = fixture::region_file(&[(0, 0, nbt)], 0);
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();
    assert_eq!(s.data_version, 2230);
    assert_eq!(
        s.packing,
        Packing::NoStraddle,
        "la mesure doit l'emporter sur un DataVersion qui ment"
    );
}
