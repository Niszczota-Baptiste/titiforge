//! **Un chunk que le jeu n'a pas fini de générer.**
//!
//! Au bord de toute zone explorée, le jeu laisse une couronne de chunks à mi-
//! génération. Il la reprendra quand un joueur approchera — bruit, surface,
//! grottes, minerais, arbres — par-dessus ce qu'on y aurait écrit. Le
//! balayage doit donc savoir le reconnaître, dans les deux dispositions.

mod common;
use common::fixture::{self, SectionSpec};

use tf_anvil::{scan, statut_incomplet, STATUTS_INCOMPLETS};

/// Remplace la valeur de `Status` dans un chunk inflaté.
fn statut(nbt: &[u8], s: &str) -> Vec<u8> {
    let cle: &[u8] = &[8, 0, 6, b'S', b't', b'a', b't', b'u', b's'];
    let debut = nbt.windows(cle.len()).position(|w| w == cle).unwrap() + cle.len();
    let ancien = u16::from_be_bytes([nbt[debut], nbt[debut + 1]]) as usize;
    let mut out = nbt[..debut].to_vec();
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    out.extend_from_slice(&nbt[debut + 2 + ancien..]);
    out
}

/// **La liste nomme ce qu'on SAIT incomplet, et rien d'autre.** Un statut
/// fini — `full` des deux époques, `fullchunk` et `postprocessed` de 1.13 — ou
/// INCONNU — une version future, un mod — n'est pas incomplet : le deviner
/// tel cacherait le monde de quelqu'un.
#[test]
fn les_statuts_incomplets_sont_ceux_qu_on_connait() {
    for s in STATUTS_INCOMPLETS {
        assert!(statut_incomplet(s), "{s}");
        assert!(statut_incomplet(&format!("minecraft:{s}")), "minecraft:{s}");
    }
    for s in [
        "full",
        "minecraft:full",
        "fullchunk",
        "postprocessed",
        "c2me:quelque_chose",
        "",
    ] {
        assert!(!statut_incomplet(s), "« {s} » n'est pas incomplet");
    }
}

/// Le balayage le reconnaît à la RACINE (1.18+) et sous `Level` (avant), et
/// un chunk fini ne l'est pas.
#[test]
fn le_balayage_reconnait_un_chunk_a_mi_generation() {
    let sections = [SectionSpec::uniform(0, "minecraft:stone")];

    let fini = fixture::chunk_nbt(0, 0, &sections);
    assert!(!scan(&fini).unwrap().incomplet, "minecraft:full");
    let a_mi = statut(&fini, "minecraft:liquid_carvers");
    let sc = scan(&a_mi).unwrap();
    assert!(sc.incomplet, "minecraft:liquid_carvers");
    assert_eq!(sc.sections.len(), 1, "le reste du balayage ne change pas");

    let ancien = fixture::legacy_chunk_nbt(0, 0, &sections, false);
    assert!(!scan(&ancien).unwrap().incomplet, "full");
    assert!(
        scan(&statut(&ancien, "decorated")).unwrap().incomplet,
        "decorated (1.13)"
    );
    assert!(
        scan(&statut(&ancien, "features")).unwrap().incomplet,
        "features (1.14–1.17)"
    );
}
