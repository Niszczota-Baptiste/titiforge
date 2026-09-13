//! Tests du lecteur.
//!
//! Deux familles, et la seconde compte autant que la première : ce que le
//! lecteur fait d'une entrée BIEN formée, et ce qu'il fait d'une entrée
//! hostile. Un `.mca` vient du disque d'un utilisateur, éventuellement tronqué
//! par un crash ou par un téléchargement interrompu. Le lecteur doit rendre
//! une erreur, jamais paniquer, jamais tenter une allocation démesurée.

use tf_nbt::{tag, Cur, Span, Trunc};

// ── fabriques ────────────────────────────────────────────────────────────────

fn nbt_str(s: &str) -> Vec<u8> {
    let mut v = (s.len() as u16).to_be_bytes().to_vec();
    v.extend_from_slice(s.as_bytes());
    v
}

/// `TAG_Compound` racine avec un nom vide, puis les champs fournis, puis END.
fn root(fields: &[u8]) -> Vec<u8> {
    let mut v = vec![tag::COMPOUND];
    v.extend(nbt_str(""));
    v.extend_from_slice(fields);
    v.push(tag::END);
    v
}

fn field(t: u8, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![t];
    v.extend(nbt_str(name));
    v.extend_from_slice(payload);
    v
}

// ── formes bien formées ──────────────────────────────────────────────────────

#[test]
fn saute_chaque_type_a_taille_fixe() {
    for (t, n) in [
        (tag::BYTE, 1usize),
        (tag::SHORT, 2),
        (tag::INT, 4),
        (tag::FLOAT, 4),
        (tag::LONG, 8),
        (tag::DOUBLE, 8),
    ] {
        let buf = vec![0u8; n + 4];
        let mut c = Cur::new(&buf);
        c.skip_payload(t).unwrap();
        assert_eq!(c.pos(), n, "type {t} devrait consommer {n} octets");
    }
}

#[test]
fn saute_les_types_a_taille_variable() {
    // BYTE_ARRAY : i32 de longueur + n octets
    let mut b = 3i32.to_be_bytes().to_vec();
    b.extend_from_slice(&[1, 2, 3]);
    let mut c = Cur::new(&b);
    c.skip_payload(tag::BYTE_ARRAY).unwrap();
    assert_eq!(c.pos(), 7);

    // INT_ARRAY : i32 + n*4
    let mut b = 2i32.to_be_bytes().to_vec();
    b.extend_from_slice(&[0; 8]);
    let mut c = Cur::new(&b);
    c.skip_payload(tag::INT_ARRAY).unwrap();
    assert_eq!(c.pos(), 12);

    // LONG_ARRAY : i32 + n*8
    let mut b = 2i32.to_be_bytes().to_vec();
    b.extend_from_slice(&[0; 16]);
    let mut c = Cur::new(&b);
    c.skip_payload(tag::LONG_ARRAY).unwrap();
    assert_eq!(c.pos(), 20);

    // STRING : u16 + n
    let b = nbt_str("minecraft:stone");
    let mut c = Cur::new(&b);
    c.skip_payload(tag::STRING).unwrap();
    assert_eq!(c.pos(), 17);
}

#[test]
fn saute_une_liste_de_type_fixe_d_un_bloc() {
    // 300 shorts : doit se sauter par multiplication, pas par 300 itérations.
    let mut b = vec![tag::SHORT];
    b.extend_from_slice(&300i32.to_be_bytes());
    b.extend_from_slice(&vec![0u8; 600]);
    let mut c = Cur::new(&b);
    c.skip_payload(tag::LIST).unwrap();
    assert_eq!(c.pos(), 5 + 600);
}

#[test]
fn saute_une_liste_de_compounds() {
    let mut b = vec![tag::COMPOUND];
    b.extend_from_slice(&2i32.to_be_bytes());
    for _ in 0..2 {
        b.extend(field(tag::STRING, "Name", &nbt_str("minecraft:stone")));
        b.push(tag::END);
    }
    let n = b.len();
    let mut c = Cur::new(&b);
    c.skip_payload(tag::LIST).unwrap();
    assert_eq!(c.pos(), n);
}

#[test]
fn une_liste_de_tag_end_ne_porte_aucune_charge() {
    // C'est la forme d'une liste VIDE dans les fichiers réels. Minecraft écrit
    // parfois une longueur non nulle avec un type END : la charge est absente
    // quand même. La sauter en itérant lirait la suite du fichier comme des
    // éléments.
    for len in [0i32, 7, -1] {
        let mut b = vec![tag::END];
        b.extend_from_slice(&len.to_be_bytes());
        b.extend_from_slice(b"SUITE");
        let mut c = Cur::new(&b);
        c.skip_payload(tag::LIST).unwrap();
        assert_eq!(c.pos(), 5, "longueur {len} : la charge doit être vide");
    }
}

#[test]
fn traverse_un_compound_imbrique() {
    let inner = field(tag::INT, "x", &1i32.to_be_bytes());
    let mut mid = inner.clone();
    mid.push(tag::END);
    let outer = field(tag::COMPOUND, "sous", &mid);
    let buf = root(&outer);

    let mut c = Cur::new(&buf);
    assert_eq!(c.enter_root().unwrap(), "");
    let (t, name) = c.next_field().unwrap().unwrap();
    assert_eq!((t, name), (tag::COMPOUND, "sous"));
    c.skip_payload(t).unwrap();
    assert!(c.next_field().unwrap().is_none(), "le END de la racine");
}

#[test]
fn les_longs_sont_lus_en_big_endian() {
    // Le piège qui fait sortir un build en bouillie sans rien signaler.
    let buf = [0x01u8, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
    let mut c = Cur::new(&buf);
    assert_eq!(c.u64().unwrap(), 0x0123_4567_89AB_CDEF);
}

#[test]
fn long_array_rend_les_longs_dans_l_ordre() {
    let mut b = 2i32.to_be_bytes().to_vec();
    b.extend_from_slice(&1u64.to_be_bytes());
    b.extend_from_slice(&u64::MAX.to_be_bytes());
    let mut c = Cur::new(&b);
    assert_eq!(c.long_array().unwrap(), vec![1, u64::MAX]);
}

#[test]
fn span_of_payload_delimite_exactement_la_charge() {
    let payload = nbt_str("abc");
    let mut buf = vec![0xAA, 0xBB]; // préfixe quelconque
    let start = buf.len();
    buf.extend_from_slice(&payload);
    buf.push(0xCC);

    let mut c = Cur::at(&buf, start);
    let span = c.span_of_payload(tag::STRING).unwrap();
    assert_eq!(
        span,
        Span {
            start,
            end: start + payload.len()
        }
    );
    assert_eq!(span.slice(&buf), &payload[..]);
    assert_eq!(span.len(), 5);
}

#[test]
fn list_header_rend_type_et_longueur() {
    let mut b = vec![tag::COMPOUND];
    b.extend_from_slice(&24i32.to_be_bytes());
    let mut c = Cur::new(&b);
    assert_eq!(c.list_header().unwrap(), (tag::COMPOUND, 24));
}

// ── entrées hostiles ─────────────────────────────────────────────────────────

#[test]
fn une_charge_tronquee_rend_une_erreur_pour_chaque_type() {
    for t in [
        tag::BYTE,
        tag::SHORT,
        tag::INT,
        tag::LONG,
        tag::FLOAT,
        tag::DOUBLE,
        tag::BYTE_ARRAY,
        tag::STRING,
        tag::LIST,
        tag::COMPOUND,
        tag::INT_ARRAY,
        tag::LONG_ARRAY,
    ] {
        let buf: [u8; 0] = [];
        let mut c = Cur::new(&buf);
        assert_eq!(c.skip_payload(t), Err(Trunc), "type {t} sur un tampon vide");
    }
}

#[test]
fn une_longueur_negative_vaut_zero_et_n_alloue_rien() {
    // NBT encode les longueurs en i32 SIGNÉ. Convertir -1 en usize donnerait
    // 18 446 744 073 709 551 615 : une réservation qui tue le processus.
    let b = (-1i32).to_be_bytes();
    let mut c = Cur::new(&b);
    assert_eq!(c.long_array().unwrap(), Vec::<u64>::new());

    let mut c = Cur::new(&b);
    c.skip_payload(tag::BYTE_ARRAY).unwrap();
    assert_eq!(c.pos(), 4);
}

#[test]
fn une_longueur_colossale_rend_une_erreur_sans_allouer() {
    // i32::MAX longs = 17 Go annoncés dans un fichier de 4 octets.
    let b = i32::MAX.to_be_bytes();
    let mut c = Cur::new(&b);
    assert_eq!(c.long_array(), Err(Trunc));

    let mut c = Cur::new(&b);
    assert_eq!(c.skip_payload(tag::LONG_ARRAY), Err(Trunc));

    let mut c = Cur::new(&b);
    assert_eq!(c.skip_payload(tag::INT_ARRAY), Err(Trunc));
}

#[test]
fn une_imbrication_pathologique_rend_une_erreur_au_lieu_de_deborder_la_pile() {
    // 100 000 compounds ouverts et jamais fermés. Sans garde-fou de
    // profondeur, `skip_payload` récurse jusqu'au débordement de pile — qui
    // n'est PAS rattrapable en Rust : le processus meurt sans message.
    let mut b = Vec::new();
    for _ in 0..100_000 {
        b.push(tag::COMPOUND);
        b.extend(nbt_str(""));
    }
    let mut c = Cur::new(&b);
    assert_eq!(c.skip_payload(tag::COMPOUND), Err(Trunc));
}

#[test]
fn un_type_de_tag_inconnu_rend_une_erreur() {
    for t in [13u8, 42, 200, 255] {
        let buf = [0u8; 32];
        let mut c = Cur::new(&buf);
        assert_eq!(c.skip_payload(t), Err(Trunc), "type {t}");
    }
    // Y compris comme type d'élément de liste …
    let mut b = vec![99u8];
    b.extend_from_slice(&1i32.to_be_bytes());
    let mut c = Cur::new(&b);
    assert_eq!(c.skip_payload(tag::LIST), Err(Trunc));
    // … et comme type de champ d'un compound.
    let mut b = vec![99u8];
    b.extend(nbt_str("x"));
    let mut c = Cur::new(&b);
    assert_eq!(c.skip_payload(tag::COMPOUND), Err(Trunc));
}

#[test]
fn une_chaine_non_utf8_rend_une_erreur() {
    // Plutôt qu'un remplacement silencieux : un nom de bloc approximatif
    // donnerait un état valide mais FAUX, et on préfère garder le chunk intact.
    let mut b = 2u16.to_be_bytes().to_vec();
    b.extend_from_slice(&[0xFF, 0xFE]);
    let mut c = Cur::new(&b);
    assert_eq!(c.str(), Err(Trunc));
}

#[test]
fn enter_root_refuse_ce_qui_n_est_pas_un_compound() {
    for t in [tag::END, tag::INT, tag::LIST, tag::STRING] {
        let mut b = vec![t];
        b.extend(nbt_str(""));
        let mut c = Cur::new(&b);
        assert_eq!(c.enter_root(), Err(Trunc), "racine de type {t}");
    }
}

#[test]
fn aucune_suite_d_octets_ne_fait_paniquer_le_lecteur() {
    // Générateur déterministe : un échec se rejoue à l'identique. Un xorshift
    // suffit, et il évite une dépendance pour dix lignes.
    let mut s: u32 = 0x2545_F491;
    let mut rand = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };

    for tour in 0..4_000 {
        let n = (rand() % 512) as usize;
        let buf: Vec<u8> = (0..n).map(|_| (rand() & 0xFF) as u8).collect();

        // Le résultat n'a aucune importance : seul compte le fait qu'on en
        // obtienne un. Une panique ici serait un déni de service déclenché par
        // un fichier de région abîmé.
        let mut c = Cur::new(&buf);
        let _ = c.enter_root();
        let mut c = Cur::new(&buf);
        let _ = c.skip_payload(tag::COMPOUND);
        let mut c = Cur::new(&buf);
        let _ = c.long_array();
        let mut c = Cur::new(&buf);
        while let Ok(Some((t, _))) = c.next_field() {
            if c.skip_payload(t).is_err() {
                break;
            }
        }
        assert!(tour < 4_000);
    }
}
