//! Le round-trip lossless — l'invariant n° 2, et le seul qui protège la save
//! d'un utilisateur.
//!
//! La chaîne complète est bouclée par deux implémentations INDÉPENDANTES :
//!
//! ```text
//!   fixture (écrit les octets à la main, depuis la spec)
//!      → tf_anvil::read → décode → modifie → splice → tf_anvil::write
//!      → frozen (reconstruit un arbre NBT complet, depuis la spec)
//! ```
//!
//! Ni le producteur ni le consommateur ne partagent une ligne avec `src/`. Un
//! test où les deux bouts viennent du code testé dirait seulement « le code est
//! d'accord avec lui-même ».

mod common;
use common::fixture::{self, SectionSpec};
use common::frozen;

use std::borrow::Cow;
use tf_anvil::{
    decode_section, deflate, inflate, read, scan, section_edits, splice, write, Edit, Interner,
    Section, VOL,
};

// ── outils du test ──────────────────────────────────────────────────────────

fn petite_region() -> Vec<u8> {
    let mut rng = fixture::Rng::new(77);
    let mut chunks = Vec::new();
    for lz in 0..4u32 {
        for lx in 0..4u32 {
            let secs = vec![
                SectionSpec::uniform(-4, "minecraft:bedrock"),
                SectionSpec::with_palette_size(-3, 12, &mut rng),
                SectionSpec::with_palette_size(-2, 40, &mut rng),
                SectionSpec::with_palette_size(-1, 300, &mut rng),
                SectionSpec::uniform(0, "minecraft:air"),
            ];
            chunks.push((lx, lz, fixture::chunk_nbt(lx as i32, lz as i32, &secs)));
        }
    }
    // `gap_every: 3` fragmente le fichier, comme le jeu le fait.
    fixture::region_file(&chunks, 3)
}

/// Applique `f` aux sections d'UN chunk, par splice. Rend le nouveau `.mca`.
fn editer_chunk(
    src: &[u8],
    lx: i32,
    lz: i32,
    f: impl Fn(&mut Section, &Interner) -> bool,
) -> Vec<u8> {
    let mut region = read(src, 0, 0).unwrap();
    let raw = region.get(lx, lz).expect("chunk présent");
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
        if f(&mut section, &interner) {
            edits.extend(section_edits(&section, sc, &interner).expect("palette résoluble"));
        }
    }

    let neuf = splice(&inflated, &mut edits).unwrap();
    let payload = deflate(&neuf, compression).unwrap();
    region.get_mut(lx, lz).unwrap().payload = Cow::Owned(payload);
    write(&region).unwrap()
}

// ── 1. sans modification ────────────────────────────────────────────────────

#[test]
fn lire_puis_reecrire_sans_rien_toucher_preserve_chaque_charge_octet_pour_octet() {
    let src = petite_region();
    let region = read(&src, 0, 0).unwrap();
    assert_eq!(region.count(), 16);
    assert!(
        region.iter().all(|c| c.is_pristine()),
        "aucune charge ne doit être possédée"
    );

    let sortie = write(&region).unwrap();
    let relu = read(&sortie, 0, 0).unwrap();

    assert_eq!(relu.count(), region.count());
    for i in 0..32 {
        for j in 0..32 {
            match (region.get(i, j), relu.get(i, j)) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    assert_eq!(a.payload, b.payload, "charge du chunk ({i},{j})");
                    assert_eq!(a.compression, b.compression);
                    assert_eq!(a.timestamp, b.timestamp, "horodatage du chunk ({i},{j})");
                }
                _ => panic!("le chunk ({i},{j}) est apparu ou disparu"),
            }
        }
    }
}

#[test]
fn l_ecriture_est_idempotente() {
    // Notre disposition est compacte, celle du jeu est fragmentée : le fichier
    // produit ne peut pas être identique à une source fragmentée. Mais réécrire
    // ce qu'on vient d'écrire doit rendre exactement les mêmes octets, sinon
    // une sauvegarde ferait grossir le fichier à chaque fois.
    let src = petite_region();
    let un = write(&read(&src, 0, 0).unwrap()).unwrap();
    let deux = write(&read(&un, 0, 0).unwrap()).unwrap();
    assert_eq!(un, deux, "write ∘ read doit être un point fixe");
}

#[test]
fn le_contenu_survit_a_un_aller_retour_complet() {
    let src = petite_region();
    let sortie = write(&read(&src, 0, 0).unwrap()).unwrap();
    assert_eq!(
        frozen::census(&src),
        frozen::census(&sortie),
        "le décodeur indépendant doit voir exactement les mêmes blocs"
    );
}

// ── 2. avec modification : la non-destruction ───────────────────────────────

#[test]
fn modifier_un_chunk_laisse_les_autres_octet_pour_octet() {
    let src = petite_region();
    let avant = read(&src, 0, 0).unwrap();

    let sortie = editer_chunk(&src, 2, 1, |s, i| {
        let Some(cible) = i.get("minecraft:bloc_5") else {
            return false;
        };
        s.set_uniform(cible);
        true
    });

    let apres = read(&sortie, 0, 0).unwrap();
    let mut touches = 0;
    for lz in 0..4 {
        for lx in 0..4 {
            let a = avant.get(lx, lz).unwrap();
            let b = apres.get(lx, lz).unwrap();
            if (lx, lz) == (2, 1) {
                assert_ne!(a.payload, b.payload, "le chunk visé doit avoir changé");
                touches += 1;
            } else {
                assert_eq!(
                    a.payload, b.payload,
                    "le chunk ({lx},{lz}) ne devait pas bouger"
                );
            }
        }
    }
    assert_eq!(touches, 1);
}

#[test]
fn les_donnees_de_mod_inconnues_survivent_a_une_modification_de_blocs() {
    // LE test. `we-engine` ré-encode l'arbre NBT dès qu'un chunk est modifié :
    // tout ce que son parseur a mal compris disparaît. Le splice ne peut pas
    // les toucher, parce qu'il ne les lit même pas.
    let src = petite_region();

    let extraire = |mca: &[u8]| -> (Vec<u8>, String, Vec<i64>, usize) {
        let chunks = frozen::decode_region(mca);
        let c = &chunks[&(2, 1)];
        let att = c
            .root
            .get("neoforge:attachments")
            .expect("les attachments doivent être là");
        let kinetic = att
            .get("create:kinetic")
            .and_then(|t| t.as_bytes())
            .unwrap()
            .clone();
        let channel = att
            .get("ae2:channel")
            .and_then(|t| t.as_str())
            .unwrap()
            .to_string();
        let hm = c
            .root
            .get("Heightmaps")
            .and_then(|t| t.get("MOTION_BLOCKING"))
            .and_then(|t| t.as_longs())
            .unwrap()
            .clone();
        let be = c
            .root
            .get("block_entities")
            .and_then(|t| t.as_list())
            .unwrap()
            .len();
        (kinetic, channel, hm, be)
    };

    let avant = extraire(&src);

    let sortie = editer_chunk(&src, 2, 1, |s, i| {
        let Some(cible) = i.get("minecraft:bloc_2") else {
            return false;
        };
        s.set_uniform(cible);
        true
    });
    let apres = extraire(&sortie);

    assert_eq!(
        avant.0, apres.0,
        "create:kinetic (256 octets de données de mod)"
    );
    assert_eq!(avant.1, apres.1, "ae2:channel, y compris le hangeul");
    assert_eq!(avant.1, "témoin — ne doit jamais bouger 한국어");
    assert_eq!(avant.2, apres.2, "Heightmaps/MOTION_BLOCKING");
    assert_eq!(avant.3, apres.3, "block_entities");
    assert_eq!(apres.3, 1);
}

#[test]
fn le_champ_inconnu_a_l_interieur_de_la_section_survit_aussi() {
    // Celui-là est voisin immédiat de `block_states` dans le même compound :
    // un splice qui se tromperait d'une borne l'écraserait.
    let src = petite_region();
    let sortie = editer_chunk(&src, 0, 0, |s, i| {
        let Some(cible) = i.get("minecraft:bloc_1") else {
            return false;
        };
        s.set_uniform(cible);
        true
    });

    let chunks = frozen::decode_region(&sortie);
    let sections = chunks[&(0, 0)]
        .root
        .get("sections")
        .and_then(|t| t.as_list())
        .unwrap();
    assert_eq!(sections.len(), 5);
    for s in sections {
        let d = s
            .get("modtest:donnees_de_section")
            .and_then(|t| t.as_bytes())
            .expect("le champ voisin de block_states doit survivre");
        assert_eq!(d, &vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }
}

#[test]
fn un_replace_par_palette_donne_exactement_le_bon_recensement() {
    let src = petite_region();
    let avant = frozen::census(&src);
    let de = "minecraft:bloc_7".to_string();
    let vers = "minecraft:bloc_1".to_string();
    let n_de = *avant.get(&de).unwrap_or(&0);
    let n_vers = *avant.get(&vers).unwrap_or(&0);
    assert!(n_de > 0, "le bloc à remplacer doit exister dans la fixture");

    // On applique sur les 16 chunks.
    let mut courant = src.clone();
    for lz in 0..4 {
        for lx in 0..4 {
            courant = editer_chunk(&courant, lx, lz, |s, i| {
                let (Some(a), Some(b)) = (i.get(&de), i.get(&vers)) else {
                    return false;
                };
                s.replace_state(a, b) > 0
            });
        }
    }

    let apres = frozen::census(&courant);
    assert_eq!(
        apres.get(&de),
        None,
        "le bloc remplacé ne doit plus exister nulle part"
    );
    assert_eq!(
        *apres.get(&vers).unwrap(),
        n_de + n_vers,
        "chaque bloc remplacé doit se retrouver dans la cible"
    );

    // Et le total reste le même : on remplace, on ne crée ni ne perd de blocs.
    let total = |m: &std::collections::BTreeMap<String, usize>| m.values().sum::<usize>();
    assert_eq!(total(&avant), total(&apres));
}

#[test]
fn le_doublon_de_palette_produit_par_replace_se_relit_correctement() {
    // La contrepartie assumée du « sans dédoublonnage » : le fichier écrit
    // contient deux entrées de palette identiques. Le jeu l'accepte ; il faut
    // vérifier qu'un décodeur indépendant aussi.
    let src = petite_region();
    let sortie = editer_chunk(&src, 1, 1, |s, i| {
        // bloc_1 et bloc_2 : la fixture ne leur donne PAS de propriétés, donc
        // leur clé canonique est leur nom nu. bloc_3 en a, et sa clé est
        // `minecraft:bloc_3|facing=north,half=top` — la chercher sous son nom
        // nu ne trouve rien, et l'édition n'aurait jamais lieu.
        let (Some(a), Some(b)) = (i.get("minecraft:bloc_1"), i.get("minecraft:bloc_2")) else {
            return false;
        };
        s.replace_state(a, b) > 0
    });

    let chunks = frozen::decode_region(&sortie);
    let sections = chunks[&(1, 1)]
        .root
        .get("sections")
        .and_then(|t| t.as_list())
        .unwrap();

    let mut vu_doublon = false;
    for s in sections {
        let Some(pal) = s
            .get("block_states")
            .and_then(|b| b.get("palette"))
            .and_then(|p| p.as_list())
        else {
            continue;
        };
        let noms: Vec<&str> = pal
            .iter()
            .filter_map(|e| e.get("Name").and_then(|n| n.as_str()))
            .collect();
        let mut tries = noms.clone();
        tries.sort_unstable();
        let avant = tries.len();
        tries.dedup();
        if tries.len() < avant {
            vu_doublon = true;
        }
        // Et chaque section se dépacke sans erreur.
        let etats = frozen::section_states(s).unwrap();
        assert_eq!(etats.len(), VOL);
    }
    assert!(
        vu_doublon,
        "le test doit bien produire le doublon qu'il prétend vérifier"
    );
}

#[test]
fn les_proprietes_d_etat_traversent_l_aller_retour() {
    // La fixture pose `facing=north, half=top` sur une entrée sur sept.
    let src = petite_region();
    let avant = frozen::census(&src);
    let avec_props: Vec<&String> = avant.keys().filter(|k| k.contains('|')).collect();
    assert!(
        !avec_props.is_empty(),
        "la fixture doit contenir des blocs à propriétés"
    );
    assert!(avec_props
        .iter()
        .any(|k| k.contains("facing=north") && k.contains("half=top")));

    // Un aller-retour qui RÉÉCRIT les sections (donc ré-émet les propriétés).
    let sortie = editer_chunk(&src, 3, 3, |s, _| {
        s.compact_palette();
        true
    });
    let apres = frozen::census(&sortie);

    // Le chunk (3,3) a été compacté : le recensement global ne doit pas bouger,
    // puisque compacter ne change aucun bloc.
    assert_eq!(avant, apres, "compacter une palette ne change aucun bloc");
}

// ── 3. bornes et cas limites ────────────────────────────────────────────────

#[test]
fn une_region_fragmentee_se_lit_correctement() {
    // Le fichier de la fixture laisse un secteur vide tous les 3 chunks. Un
    // lecteur qui supposerait une disposition compacte lirait du remplissage
    // comme des chunks — sans rien signaler.
    let src = petite_region();
    let region = read(&src, 0, 0).unwrap();
    assert_eq!(region.count(), 16);
    for c in region.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        assert_eq!(s.data_version, 3465);
        assert_eq!(s.sections.len(), 5);
        assert_eq!(
            s.x_pos,
            Some(c.local_x()),
            "xPos vient du CONTENU, pas du nom"
        );
        assert_eq!(s.z_pos, Some(c.local_z()));
    }
}

#[test]
fn un_chunk_absent_reste_absent() {
    let mut rng = fixture::Rng::new(3);
    let secs = vec![SectionSpec::with_palette_size(0, 5, &mut rng)];
    let src = fixture::region_file(&[(7, 9, fixture::chunk_nbt(7, 9, &secs))], 0);

    let region = read(&src, 0, 0).unwrap();
    assert_eq!(region.count(), 1);
    assert!(region.get(7, 9).is_some());
    assert!(region.get(0, 0).is_none());

    let sortie = write(&region).unwrap();
    let relu = read(&sortie, 0, 0).unwrap();
    assert_eq!(relu.count(), 1);
    assert!(
        relu.get(7, 9).is_some(),
        "l'index du chunk doit être conservé"
    );
    assert_eq!(
        relu.get(7, 9).unwrap().payload,
        region.get(7, 9).unwrap().payload
    );
}

#[test]
fn une_section_sans_block_states_est_ignoree_sans_etre_detruite() {
    // Elles existent : sections purement d'éclairage aux bords du monde. Il ne
    // faut ni les décoder ni les réécrire.
    let mut n = fixture::Nbt::new();
    use fixture::t;
    n.field(t::COMPOUND, "");
    n.field(t::INT, "DataVersion").i32v(3465);
    n.field(t::LIST, "sections").list(t::COMPOUND, 1);
    n.field(t::BYTE, "Y").i8v(5);
    n.field(t::BYTE_ARRAY, "SkyLight")
        .bytes(&vec![0xFFu8; 2048]);
    n.end();
    n.end();

    let src = fixture::region_file(&[(0, 0, n.b)], 0);
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();

    assert_eq!(s.sections.len(), 1);
    assert_eq!(s.sections[0].y, 5);
    assert!(
        s.sections[0].spans.is_none(),
        "pas de champs de blocs à repérer"
    );

    let mut interner = Interner::new();
    assert!(decode_section(&inflated, &s, &s.sections[0], &mut interner)
        .unwrap()
        .is_none());

    // Et la SkyLight est intacte après un aller-retour.
    let sortie = write(&region).unwrap();
    let chunks = frozen::decode_region(&sortie);
    let sl = chunks[&(0, 0)]
        .root
        .get("sections")
        .and_then(|t| t.as_list())
        .unwrap()[0]
        .get("SkyLight")
        .and_then(|t| t.as_bytes())
        .unwrap();
    assert_eq!(sl.len(), 2048);
    assert!(sl.iter().all(|&b| b == 0xFF));
}

#[test]
fn un_etat_a_proprietes_ne_se_trouve_pas_sous_son_nom_nu() {
    // Le piège qui a fait échouer le test du doublon en l'écrivant. Un bloc
    // avec état a pour clé `nom|k=v,k=v` trié : le chercher sous son nom seul
    // rend `None`, et une opération qui ne vérifie pas son `Option` ne ferait
    // simplement RIEN — sans erreur, sans trace.
    let src = petite_region();
    let region = read(&src, 0, 0).unwrap();
    let c = region.get(0, 0).unwrap();
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let scanned = scan(&inflated).unwrap();

    let mut interner = Interner::new();
    for sc in &scanned.sections {
        let _ = decode_section(&inflated, &scanned, sc, &mut interner).unwrap();
    }

    // bloc_3 porte `facing=north, half=top` dans la fixture.
    assert!(
        interner.get("minecraft:bloc_3").is_none(),
        "le nom nu ne doit PAS résoudre un état qui porte des propriétés"
    );
    assert!(
        interner
            .get("minecraft:bloc_3|facing=north,half=top")
            .is_some(),
        "la clé canonique, elle, doit résoudre"
    );
    // Et un bloc sans propriétés se trouve bien sous son nom nu.
    assert!(interner.get("minecraft:bloc_1").is_some());
}

#[test]
fn les_proprietes_sont_triees_dans_la_cle_quel_que_soit_l_ordre_du_fichier() {
    // NBT ne donne aucun sens à l'ordre des champs d'un compound. Si la clé
    // dépendait de l'ordre de lecture, le même bloc écrit par deux versions du
    // jeu occuperait deux entrées de palette — et un //replace en raterait la
    // moitié.
    use tf_anvil::state_key;
    let mut a = vec![
        ("half".to_string(), "top".to_string()),
        ("facing".to_string(), "north".to_string()),
    ];
    let mut b = vec![
        ("facing".to_string(), "north".to_string()),
        ("half".to_string(), "top".to_string()),
    ];
    assert_eq!(state_key("x:y", &mut a), state_key("x:y", &mut b));
    assert_eq!(state_key("x:y", &mut a), "x:y|facing=north,half=top");
}
