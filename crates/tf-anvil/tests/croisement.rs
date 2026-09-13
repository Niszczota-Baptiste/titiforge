//! Croisement avec un producteur TIERS : le moteur JS d'`ExeWorldEdit`.
//!
//! Les autres tests bouclent la chaîne entre deux implémentations écrites ici.
//! C'est déjà beaucoup, mais les deux partagent une lecture de la spec — la
//! mienne. Un fichier produit par `prismarine-nbt` et le `writeRegion` de
//! `we-engine` ne partage rien du tout : il vient d'un autre langage, d'une
//! autre bibliothèque NBT, et d'un code écrit sans connaître celui-ci.
//!
//! Le fichier n'est pas commité — pas de fixture binaire dans le dépôt. Le
//! test est donc conditionné à une variable d'environnement, et se saute en
//! disant pourquoi plutôt que de passer en silence :
//!
//! ```bash
//! node proto/fixtures/gen.mjs
//! TF_MCA_TIERS=./r.0.1.mca cargo test -p tf-anvil --test croisement -- --nocapture
//! ```

mod common;
use common::frozen;

use std::borrow::Cow;
use tf_anvil::{
    decode_section, deflate, inflate, read, scan, section_edits, splice, write, Edit, Interner,
};

fn fichier_tiers() -> Option<Vec<u8>> {
    let p = std::env::var("TF_MCA_TIERS").ok()?;
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(e) => panic!("TF_MCA_TIERS pointe sur « {p} », illisible : {e}"),
    }
}

#[test]
fn un_mca_produit_par_we_engine_fait_l_aller_retour_sans_perte() {
    let Some(src) = fichier_tiers() else {
        eprintln!("SAUTÉ : poser TF_MCA_TIERS sur un .mca pour croiser avec un producteur tiers");
        return;
    };

    let region = read(&src, 0, 0).unwrap();
    assert!(
        region.count() > 0,
        "le fichier tiers doit contenir des chunks"
    );
    assert!(region.iter().all(|c| c.is_pristine()));
    println!("  {} chunks lus depuis un fichier tiers", region.count());

    let sortie = write(&region).unwrap();
    assert_eq!(
        frozen::census(&src),
        frozen::census(&sortie),
        "le recensement des blocs doit être identique après un aller-retour"
    );

    // Et chaque charge est recopiée octet pour octet.
    let relu = read(&sortie, 0, 0).unwrap();
    for lz in 0..32 {
        for lx in 0..32 {
            match (region.get(lx, lz), relu.get(lx, lz)) {
                (None, None) => {}
                (Some(a), Some(b)) => assert_eq!(a.payload, b.payload, "chunk ({lx},{lz})"),
                _ => panic!("le chunk ({lx},{lz}) est apparu ou disparu"),
            }
        }
    }
}

#[test]
fn un_replace_sur_un_mca_tiers_donne_le_bon_recensement() {
    let Some(src) = fichier_tiers() else {
        eprintln!("SAUTÉ : TF_MCA_TIERS non défini");
        return;
    };
    let avant = frozen::census(&src);
    let de = "minecraft:stone";
    let vers = "minecraft:dirt";
    let n_de = *avant.get(de).unwrap_or(&0);
    let n_vers = *avant.get(vers).unwrap_or(&0);
    assert!(n_de > 0, "le fichier tiers doit contenir de la pierre");
    assert!(
        n_vers > 0,
        "…et de la terre, pour que la collision de palette se produise"
    );

    let mut region = read(&src, 0, 0).unwrap();
    let mut touches = 0usize;

    for lz in 0..32 {
        for lx in 0..32 {
            let Some(raw) = region.get(lx, lz) else {
                continue;
            };
            let compression = raw.compression;
            let inflated = inflate(&raw.payload, compression).unwrap();
            let scanned = scan(&inflated).unwrap();

            let mut interner = Interner::new();
            let mut sections = Vec::new();
            for sc in &scanned.sections {
                sections.push(decode_section(&inflated, &scanned, sc, &mut interner).unwrap());
            }
            let (Some(a), Some(b)) = (interner.get(de), interner.get(vers)) else {
                continue;
            };

            let mut edits: Vec<Edit> = Vec::new();
            for (sc, section) in scanned.sections.iter().zip(sections.iter_mut()) {
                let Some(section) = section else { continue };
                if section.replace_state(a, b) > 0 {
                    edits.extend(section_edits(section, sc, &interner).unwrap());
                }
            }
            if edits.is_empty() {
                continue;
            }
            touches += 1;
            let neuf = splice(&inflated, &mut edits).unwrap();
            region.get_mut(lx, lz).unwrap().payload =
                Cow::Owned(deflate(&neuf, compression).unwrap());
        }
    }

    assert!(
        touches > 0,
        "au moins un chunk devait contenir de la pierre"
    );
    println!("  {touches} chunks modifiés, {n_de} blocs de pierre remplacés");

    let apres = frozen::census(&write(&region).unwrap());
    assert_eq!(apres.get(de), None, "plus une seule pierre");
    assert_eq!(*apres.get(vers).unwrap(), n_de + n_vers);
    let total = |m: &std::collections::BTreeMap<String, usize>| m.values().sum::<usize>();
    assert_eq!(total(&avant), total(&apres), "aucun bloc créé ni perdu");
}
