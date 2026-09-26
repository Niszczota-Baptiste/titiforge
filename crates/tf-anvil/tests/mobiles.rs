//! Le balayage des chunks d'ENTITÉS face à ce qu'un fichier peut contenir :
//! tronqué, forgé, de la mauvaise forme — et relu par le décodeur GELÉ.
//!
//! Les cas réalistes (cadres, tableaux, villageois) sont dans
//! `tf-ops/tests/mobiles.rs`, qui a la fixture de `tf-bench`. Ici, un écrivain
//! minimal écrit ses octets à la main, comme `common/fixture.rs`.

mod common;

use common::frozen::{parse_nbt, Tag};
use tf_anvil::mobiles::{balayer_chunk, chunk_neuf, edition_mobiles, Mobile, MAX_PASSAGERS};
use tf_anvil::splice;
use tf_nbt::{tag, Writer};

fn doubles(w: &mut Writer, nom: &str, v: &[f64]) {
    w.field(tag::LIST, nom).list_header(tag::DOUBLE, v.len());
    for x in v {
        w.raw(&x.to_bits().to_be_bytes());
    }
}

/// Une entité : `id`, `Pos`, `UUID`, et ce que `extra` y ajoute.
fn entite(id: &str, pos: [f64; 3], uuid: [i32; 4], extra: impl Fn(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    w.field(tag::STRING, "id").raw_str(id);
    doubles(&mut w, "Pos", &pos);
    w.field(tag::INT_ARRAY, "UUID").i32_payload(4);
    for x in uuid {
        w.i32_payload(x);
    }
    extra(&mut w);
    w.end();
    w.into_bytes()
}

fn chunk(entites: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::new();
    w.field(tag::COMPOUND, "");
    w.field(tag::INT, "DataVersion").i32_payload(2975);
    w.field(tag::LIST, "Entities");
    w.list_header(tag::COMPOUND, entites.len());
    for e in entites {
        w.raw(e);
    }
    w.field(tag::INT_ARRAY, "Position")
        .i32_payload(2)
        .i32_payload(3)
        .i32_payload(-4);
    w.end();
    w.into_bytes()
}

#[test]
fn un_champ_de_la_mauvaise_forme_n_est_pas_releve_et_ressort_tel_quel() {
    let e = entite("t:bete", [1.0, 2.0, 3.0], [1, 2, 3, 4], |w| {
        // Deux doubles au lieu de trois, des doubles au lieu de flottants, un
        // UUID de trois entiers dans la laisse, un `TileX` sans ses frères.
        doubles(w, "Motion", &[0.5, 0.25]);
        doubles(w, "Rotation", &[10.0, 20.0]);
        w.field(tag::COMPOUND, "Leash");
        w.field(tag::INT_ARRAY, "UUID")
            .i32_payload(3)
            .i32_payload(7)
            .i32_payload(8)
            .i32_payload(9);
        w.end();
        w.field(tag::INT, "TileX").i32_payload(5);
    });
    let nbt = chunk(&[e]);
    let ch = balayer_chunk(&nbt).unwrap();
    let k = &ch.entrees[0].corps[0];
    assert_eq!(k.pos.unwrap().v, [1.0, 2.0, 3.0]);
    assert!(k.motion.is_none(), "deux doubles ne sont pas une vitesse");
    assert!(k.rotation.is_none(), "des doubles ne sont pas un lacet");
    assert!(k.laisse_uuid.is_none(), "trois entiers ne sont pas un UUID");
    assert!(
        k.tuile.is_none(),
        "une case à laquelle il manque deux axes n'en est pas une"
    );
    let m = Mobile::depuis(&nbt, &ch.entrees[0], ch.data_version);
    assert_eq!(m.octets(), ch.entrees[0].span.slice(&nbt));
}

#[test]
fn une_chaine_de_passagers_forgee_est_refusee_sans_deborder_la_pile() {
    // Chaque passager en porte un autre, bien au-delà de ce que fait le jeu.
    let mut e = entite("t:bete", [0.0; 3], [0; 4], |_| {});
    for _ in 0..(MAX_PASSAGERS as usize + 10) {
        let dedans = e.clone();
        e = entite("t:bete", [0.0; 3], [0; 4], |w| {
            w.field(tag::LIST, "Passengers")
                .list_header(tag::COMPOUND, 1);
            w.raw(&dedans);
        });
    }
    assert!(balayer_chunk(&chunk(&[e])).is_err());

    // Et une chaîne raisonnable passe, dans l'ordre de l'arbre.
    let bas = entite("t:c", [0.0; 3], [3; 4], |_| {});
    let milieu = entite("t:b", [0.0; 3], [2; 4], |w| {
        w.field(tag::LIST, "Passengers")
            .list_header(tag::COMPOUND, 1);
        w.raw(&bas);
    });
    let haut = entite("t:a", [0.0; 3], [1; 4], |w| {
        w.field(tag::LIST, "Passengers")
            .list_header(tag::COMPOUND, 1);
        w.raw(&milieu);
    });
    let ch = balayer_chunk(&chunk(&[haut])).unwrap();
    let ids: Vec<_> = ch.entrees[0]
        .corps
        .iter()
        .map(|k| k.uuid.unwrap().v[0])
        .collect();
    assert_eq!(ids, [1, 2, 3]);
}

#[test]
fn un_chunk_tronque_ne_fait_jamais_paniquer() {
    let e = entite("t:bete", [1.0, 2.0, 3.0], [1, 2, 3, 4], |w| {
        w.field(tag::INT, "TileX").i32_payload(1);
        w.field(tag::INT, "TileY").i32_payload(2);
        w.field(tag::INT, "TileZ").i32_payload(3);
        w.field(tag::BYTE, "Facing").i8_payload(3);
    });
    let nbt = chunk(&[e.clone(), e]);
    for n in 0..nbt.len() {
        // Rendre une erreur est permis ; paniquer sur la save de quelqu'un,
        // non.
        let _ = balayer_chunk(&nbt[..n]);
    }
}

/// **Un chunk neuf se relit par un décodeur qui ne doit rien au moteur**, et
/// il porte ce que le jeu attend : `DataVersion`, `Position` et la liste.
#[test]
fn un_chunk_neuf_se_relit_par_le_decodeur_gele() {
    let a = entite("t:a", [1.0, 2.0, 3.0], [1, 2, 3, 4], |_| {});
    let b = entite("t:b", [4.0, 5.0, 6.0], [5, 6, 7, 8], |_| {});
    let nbt = chunk_neuf(2975, -7, 12, &[a, b]);
    let (_, racine) = parse_nbt(&nbt);
    assert_eq!(racine.get("DataVersion").and_then(Tag::as_i32), Some(2975));
    assert_eq!(racine.get("Position"), Some(&Tag::IntArray(vec![-7, 12])));
    let liste = racine.get("Entities").and_then(Tag::as_list).unwrap();
    let ids: Vec<_> = liste
        .iter()
        .map(|e| e.get("id").and_then(Tag::as_str).unwrap())
        .collect();
    assert_eq!(ids, ["t:a", "t:b"]);

    // Et le balayage du moteur y retrouve ses petits.
    let ch = balayer_chunk(&nbt).unwrap();
    assert_eq!(ch.position, Some([-7, 12]));
    assert_eq!(ch.entrees.len(), 2);
}

#[test]
fn une_liste_vide_qui_le_reste_ne_produit_aucune_edition() {
    // Les deux écritures d'une liste vide : `TAG_End`, et `TAG_Compound` de
    // longueur nulle. Normaliser la seconde changerait les octets d'un chunk
    // que l'opération n'a pas touché.
    for et in [tag::END, tag::COMPOUND] {
        let mut w = Writer::new();
        w.field(tag::COMPOUND, "");
        w.field(tag::LIST, "Entities").list_header(et, 0);
        w.end();
        let nbt = w.into_bytes();
        let ch = balayer_chunk(&nbt).unwrap();
        assert!(ch.entrees.is_empty());
        assert_eq!(edition_mobiles(&nbt, &ch, &[]), None);
    }
}

#[test]
fn une_liste_absente_s_insere_avant_la_fin_de_la_racine() {
    let mut w = Writer::new();
    w.field(tag::COMPOUND, "");
    w.field(tag::INT, "DataVersion").i32_payload(2975);
    w.end();
    let nbt = w.into_bytes();
    let ch = balayer_chunk(&nbt).unwrap();
    assert!(ch.champ.is_none());
    let e = entite("t:a", [1.0, 2.0, 3.0], [1, 2, 3, 4], |_| {});
    let ed = edition_mobiles(&nbt, &ch, std::slice::from_ref(&e)).unwrap();
    let apres = splice(&nbt, &mut [ed]).unwrap();
    let (_, racine) = parse_nbt(&apres);
    assert_eq!(racine.get("DataVersion").and_then(Tag::as_i32), Some(2975));
    assert_eq!(
        racine.get("Entities").and_then(Tag::as_list).map(Vec::len),
        Some(1)
    );
}

#[test]
fn un_souvenir_qui_n_a_pas_la_forme_d_une_position_n_en_est_pas_une() {
    let e = entite("t:villageois", [0.0; 3], [1; 4], |w| {
        w.field(tag::COMPOUND, "Brain");
        w.field(tag::COMPOUND, "memories");
        // Une position globale, la seule forme qu'on reconnaît.
        w.field(tag::COMPOUND, "minecraft:home");
        w.field(tag::COMPOUND, "value");
        w.field(tag::INT_ARRAY, "pos")
            .i32_payload(3)
            .i32_payload(1)
            .i32_payload(2)
            .i32_payload(3);
        w.field(tag::STRING, "dimension")
            .raw_str("minecraft:overworld");
        w.end();
        w.end();
        // La même, avec un champ de plus : ce n'est plus une forme connue.
        w.field(tag::COMPOUND, "mod:inconnu");
        w.field(tag::COMPOUND, "value");
        w.field(tag::INT_ARRAY, "pos")
            .i32_payload(3)
            .i32_payload(9)
            .i32_payload(9)
            .i32_payload(9);
        w.field(tag::STRING, "dimension")
            .raw_str("minecraft:overworld");
        w.field(tag::INT, "rayon").i32_payload(4);
        w.end();
        w.end();
        // Un souvenir qui n'est pas une position du tout.
        w.field(tag::COMPOUND, "minecraft:last_slept");
        w.field(tag::LONG, "value").raw(&5i64.to_be_bytes());
        w.end();
        w.end();
        w.end();
    });
    let ch = balayer_chunk(&chunk(&[e])).unwrap();
    let r: Vec<_> = ch.entrees[0].corps[0]
        .retenues
        .iter()
        .map(|c| c.v)
        .collect();
    assert_eq!(r, [[1, 2, 3]]);
}

/// Une entité faite de ses seuls octets se situe comme dans un chunk — et des
/// octets en trop après son compound la font refuser.
#[test]
fn une_entite_se_fait_de_ses_seuls_octets() {
    let mut w = Writer::new();
    w.field(tag::STRING, "id").raw_str("minecraft:armor_stand");
    w.field(tag::LIST, "Pos").list_header(tag::DOUBLE, 3);
    for v in [1.5f64, 64.0, -2.5] {
        w.raw(&v.to_bits().to_be_bytes());
    }
    w.end();
    let nbt = w.into_bytes();
    let m = Mobile::depuis_compound(nbt.clone(), Some(2975)).unwrap();
    assert_eq!(m.pos(), Some([1.5, 64.0, -2.5]));
    assert_eq!(m.id(), Some("minecraft:armor_stand"));
    assert_eq!(m.octets(), nbt);
    let mut plus = nbt;
    plus.extend_from_slice(&[0, 0]);
    assert!(Mobile::depuis_compound(plus, None).is_err());
}
