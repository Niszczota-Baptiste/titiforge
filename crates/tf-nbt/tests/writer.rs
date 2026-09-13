//! Tests de l'écrivain.
//!
//! Le test central est le round-trip : ce que l'écrivain produit, le lecteur
//! doit le relire à l'identique. Deux moitiés testées séparément peuvent très
//! bien passer chacune leurs tests et diverger à leur jonction — c'est
//! exactement ce qui est arrivé à `we-engine` avec `toHeights` /
//! `applyHeightmap`, où chaque moitié était juste et l'ensemble faux.

use tf_nbt::{block_states_payload, tag, Cur, PaletteEntryRef, Writer};

/// Une palette relue : le nom, et ses propriétés triées.
type PaletteRelue = Vec<(String, Vec<(String, String)>)>;

fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Relit une charge de `block_states` et rend `(palette, data)` sous une forme
/// comparable : le nom, les propriétés triées, et les longs.
fn relire(payload: &[u8]) -> (PaletteRelue, Vec<u64>) {
    let mut c = Cur::new(payload);
    let mut palette: PaletteRelue = Vec::new();
    let mut data = Vec::new();
    while let Some((t, key)) = c.next_field().unwrap() {
        match (t, key) {
            (tag::LIST, "palette") => {
                let (et, n) = c.list_header().unwrap();
                assert_eq!(et, tag::COMPOUND);
                for _ in 0..n {
                    let mut name = String::new();
                    let mut ps: Vec<(String, String)> = Vec::new();
                    while let Some((pt, pk)) = c.next_field().unwrap() {
                        match (pt, pk) {
                            (tag::STRING, "Name") => name = c.str().unwrap().to_string(),
                            (tag::COMPOUND, "Properties") => {
                                while let Some((qt, qk)) = c.next_field().unwrap() {
                                    assert_eq!(qt, tag::STRING);
                                    ps.push((qk.to_string(), c.str().unwrap().to_string()));
                                }
                            }
                            _ => c.skip_payload(pt).unwrap(),
                        }
                    }
                    ps.sort();
                    palette.push((name, ps));
                }
            }
            (tag::LONG_ARRAY, "data") => data = c.long_array().unwrap(),
            _ => c.skip_payload(t).unwrap(),
        }
    }
    (palette, data)
}

#[test]
fn round_trip_d_une_palette_simple() {
    let names = ["minecraft:air", "minecraft:stone", "minecraft:dirt"];
    let vide: Vec<(String, String)> = Vec::new();
    let pal: Vec<PaletteEntryRef> = names
        .iter()
        .map(|n| PaletteEntryRef {
            name: n,
            props: &vide,
        })
        .collect();
    let data = vec![0x0123_4567_89AB_CDEF, 42, u64::MAX];

    let (relu_pal, relu_data) = relire(&block_states_payload(&pal, &data));
    assert_eq!(
        relu_pal.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        names
    );
    assert!(relu_pal.iter().all(|(_, p)| p.is_empty()));
    assert_eq!(relu_data, data);
}

#[test]
fn round_trip_avec_proprietes() {
    let p0 = props(&[("facing", "north"), ("half", "top"), ("shape", "straight")]);
    let p1 = props(&[("axis", "y")]);
    let vide: Vec<(String, String)> = Vec::new();
    let pal = vec![
        PaletteEntryRef {
            name: "minecraft:air",
            props: &vide,
        },
        PaletteEntryRef {
            name: "minecraft:oak_stairs",
            props: &p0,
        },
        PaletteEntryRef {
            name: "minecraft:oak_log",
            props: &p1,
        },
    ];
    let (relu, _) = relire(&block_states_payload(&pal, &[1, 2]));
    assert_eq!(relu[0], ("minecraft:air".into(), vec![]));
    assert_eq!(relu[1], ("minecraft:oak_stairs".into(), p0));
    assert_eq!(relu[2], ("minecraft:oak_log".into(), p1));
}

#[test]
fn une_palette_d_une_entree_n_ecrit_pas_de_data() {
    // Exactement ce que Minecraft écrit pour une section homogène. Émettre un
    // tableau d'indices tous nuls serait valide, mais ferait diverger nos
    // octets des siens et grossir le fichier sans raison.
    let vide: Vec<(String, String)> = Vec::new();
    let pal = vec![PaletteEntryRef {
        name: "minecraft:stone",
        props: &vide,
    }];
    let payload = block_states_payload(&pal, &[7, 7, 7]);
    let (relu, data) = relire(&payload);
    assert_eq!(relu.len(), 1);
    assert!(
        data.is_empty(),
        "`data` ne doit pas être écrit pour une palette de 1"
    );

    // Et le tampon ne contient littéralement pas le champ.
    assert!(!payload.windows(4).any(|w| w == b"data"));
}

#[test]
fn une_palette_multiple_sans_data_n_ecrit_pas_de_data_non_plus() {
    let vide: Vec<(String, String)> = Vec::new();
    let pal = vec![
        PaletteEntryRef {
            name: "minecraft:air",
            props: &vide,
        },
        PaletteEntryRef {
            name: "minecraft:stone",
            props: &vide,
        },
    ];
    let (_, data) = relire(&block_states_payload(&pal, &[]));
    assert!(data.is_empty());
}

#[test]
fn des_proprietes_vides_ne_produisent_pas_de_compound_properties() {
    // `{}` et l'absence doivent donner les MÊMES octets. Sinon le même bloc
    // décrit de deux façons occuperait deux entrées de palette — le piège
    // `grass_block[snowy=false]` de we-engine, sous une autre forme.
    let vide: Vec<(String, String)> = Vec::new();
    let a = block_states_payload(
        &[PaletteEntryRef {
            name: "minecraft:stone",
            props: &vide,
        }],
        &[],
    );
    let b = block_states_payload(
        &[PaletteEntryRef {
            name: "minecraft:stone",
            props: &[],
        }],
        &[],
    );
    assert_eq!(a, b);
    assert!(!a.windows(10).any(|w| w == b"Properties"));
}

#[test]
fn l_ordre_des_proprietes_ne_change_pas_les_octets() {
    // NBT ne donne aucun sens à l'ordre des champs d'un compound, mais deux
    // ordres donnent deux suites d'octets. L'écriture trie, donc elle est
    // déterministe — c'est ce qui rend une comparaison d'octets possible dans
    // les tests, et une régression détectable.
    let p1 = props(&[("a", "1"), ("b", "2"), ("c", "3")]);
    let p2 = props(&[("c", "3"), ("a", "1"), ("b", "2")]);
    let f = |p: &Vec<(String, String)>| {
        block_states_payload(
            &[PaletteEntryRef {
                name: "x:y",
                props: p,
            }],
            &[],
        )
    };
    assert_eq!(f(&p1), f(&p2));
}

#[test]
fn le_writer_ecrit_les_entiers_en_big_endian() {
    let mut w = Writer::new();
    w.i32_payload(0x0102_0304);
    assert_eq!(w.as_slice(), &[0x01, 0x02, 0x03, 0x04]);

    let mut w = Writer::new();
    w.long_array_payload(&[0x0102_0304_0506_0708]);
    assert_eq!(w.as_slice(), &[0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn une_chaine_porte_sa_longueur_en_u16_be() {
    let mut w = Writer::new();
    w.raw_str("abc");
    assert_eq!(w.as_slice(), &[0, 3, b'a', b'b', b'c']);
}

#[test]
#[should_panic(expected = "65535")]
fn une_chaine_trop_longue_refuse_plutot_que_de_tronquer() {
    // Tronquer donnerait un nom de bloc valide mais FAUX — le pire des deux
    // mondes : le fichier se relit, et il ne contient pas ce qu'on a demandé.
    let mut w = Writer::new();
    w.raw_str(&"a".repeat(70_000));
}

#[test]
fn la_charge_produite_se_termine_par_le_end_du_compound() {
    let vide: Vec<(String, String)> = Vec::new();
    let payload = block_states_payload(
        &[PaletteEntryRef {
            name: "minecraft:stone",
            props: &vide,
        }],
        &[],
    );
    assert_eq!(*payload.last().unwrap(), tag::END);

    // Et le lecteur la traverse jusqu'au bout sans rien laisser derrière.
    let mut c = Cur::new(&payload);
    while c.next_field().unwrap().is_some() {
        let t = tag::LIST;
        let _ = t;
    }
}

#[test]
fn round_trip_sur_une_palette_de_quatre_mille_entrees() {
    // La borne réelle du format : 4096 entrées, soit 12 bits par indice.
    let noms: Vec<String> = (0..4096).map(|i| format!("modtest:bloc_{i}")).collect();
    let vide: Vec<(String, String)> = Vec::new();
    let pal: Vec<PaletteEntryRef> = noms
        .iter()
        .map(|n| PaletteEntryRef {
            name: n,
            props: &vide,
        })
        .collect();
    // `wrapping_mul` est délibéré : on veut des motifs de bits variés, et le
    // débordement est vérifié dans le profil de test — un `*` nu échouerait.
    let data: Vec<u64> = (0..820)
        .map(|i| (i as u64).wrapping_mul(0x0101_0101_0101_0101))
        .collect();

    let (relu_pal, relu_data) = relire(&block_states_payload(&pal, &data));
    assert_eq!(relu_pal.len(), 4096);
    assert_eq!(relu_pal[0].0, "modtest:bloc_0");
    assert_eq!(relu_pal[4095].0, "modtest:bloc_4095");
    assert_eq!(relu_data, data);
}
