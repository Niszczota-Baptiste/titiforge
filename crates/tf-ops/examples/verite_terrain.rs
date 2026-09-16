//! Les invariants, vérifiés sur des fichiers que **Minecraft a écrits**.
//!
//! Tous les tests du dépôt travaillent sur des fixtures construites à la
//! volée. C'est délibéré — pas de binaire opaque en revue — mais ça laisse une
//! question ouverte : le décodeur et l'encodeur ne se sont jamais mesurés
//! qu'à eux-mêmes. Une fixture fausse dans le sens prudent reste fausse, et
//! deux moitiés qui partagent une erreur passent leurs tests ensemble.
//!
//! Cet exemple prend une vraie save et vérifie les deux invariants qui portent
//! tout le reste :
//!
//! 1. **Un chunk non modifié ressort octet pour octet.** Pas « équivalent
//!    après réencodage » : les MÊMES octets compressés.
//! 2. **Une opération est exactement réversible.** On applique, puis on
//!    applique le sens « annuler » du correctif, et on exige les octets
//!    d'origine — pas seulement la même empreinte.
//!
//! On y ajoute ce qu'un test synthétique ne peut pas prouver : que le balayage
//! NBT traverse sans dommage tout ce que le vrai format transporte
//! (heightmaps, entités de bloc, structures, données de serveur), puisque le
//! splice le laisse intact par construction.
//!
//! Les trois étages d'écriture y passent, et c'est le point : l'étage
//! *palette* ne touche AUCUN indice — un aller-retour qui ne réécrit rien
//! revient forcément juste. Seul l'étage *bloc*, qui repacke les 4096
//! indices, met le splice à l'épreuve.
//!
//! ```text
//! cargo run --release -p tf-ops --example verite_terrain -- <monde> [bloc]
//! ```
//!
//! Rien n'est écrit dans la save : tout vit dans une copie de travail.

use std::collections::HashMap;
use std::path::PathBuf;

use tf_anvil::{inflate, read, splice, Interner};
use tf_world::journal::empreinte as empreinte_de;
use tf_ops::edition::appliquer;
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::{FsSource, Staging};

/// Un compteur de contrôles, pour qu'un « tout va bien » sur zéro cas ne
/// puisse pas passer pour une réussite.
#[derive(Default)]
struct Bilan {
    verifies: usize,
    fautes: Vec<String>,
}

impl Bilan {
    fn exiger(&mut self, ok: bool, quoi: impl FnOnce() -> String) {
        self.verifies += 1;
        if !ok {
            self.fautes.push(quoi());
        }
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let Some(monde) = a.next() else {
        eprintln!("usage : verite_terrain <monde> [bloc à remplacer]");
        std::process::exit(2);
    };
    let cible = a.next().unwrap_or_else(|| "minecraft:dirt".to_string());
    let monde = PathBuf::from(monde);

    let src = match FsSource::open(&monde) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("monde illisible : {e:?}");
            std::process::exit(1);
        }
    };
    let dim = Dimension::Overworld;
    let ov = src.overview(&dim, Folder::Region).expect("aperçu");
    println!("monde : {} · {} régions", monde.display(), ov.regions.len());

    let mut bilan = Bilan::default();

    // ── 1. ce que la save contient, avant qu'on y touche
    //
    // On garde l'INFLATÉ de chaque chunk. C'est la référence de comparaison :
    // l'octet compressé dépend du niveau de deflate, l'inflaté est le contenu.
    let mut avant: HashMap<(i32, i32, u16), Vec<u8>> = HashMap::new();
    let mut brut: HashMap<(i32, i32, u16), Vec<u8>> = HashMap::new();
    for info in &ov.regions {
        let octets = src
            .read_region(&dim, Folder::Region, info.pos)
            .expect("région lisible");
        let r = read(&octets, info.pos.x, info.pos.z).expect("région analysable");
        for i in 0..1024u16 {
            let Some(c) = r.get((i % 32) as i32, (i / 32) as i32) else {
                continue;
            };
            let inflated = inflate(&c.payload, c.compression).expect("charge inflatable");
            avant.insert((info.pos.x, info.pos.z, i), inflated);
            brut.insert((info.pos.x, info.pos.z, i), c.payload.as_ref().to_vec());
        }
    }
    println!("  {} chunks relevés", avant.len());
    if avant.is_empty() {
        eprintln!("aucun chunk : rien à vérifier");
        std::process::exit(1);
    }

    // ── 2. les trois étages d'écriture, sur une copie de travail
    let mut interner = Interner::new();
    let de = interner.intern(&cible);
    let pierre = interner.intern("minecraft:stone");
    let terre = interner.intern("minecraft:dirt");

    let (x0, x1) = (
        ov.regions.iter().map(|r| r.pos.x).min().unwrap() * 512,
        ov.regions.iter().map(|r| r.pos.x).max().unwrap() * 512 + 511,
    );
    let (z0, z1) = (
        ov.regions.iter().map(|r| r.pos.z).min().unwrap() * 512,
        ov.regions.iter().map(|r| r.pos.z).max().unwrap() * 512 + 511,
    );
    let tout = BBox::new(
        BlockPos { x: x0, y: -64, z: z0 },
        BlockPos { x: x1, y: 319, z: z1 },
    );

    let essais: Vec<(&str, Plan, BBox)> = vec![
        (
            "palette — un NOM réécrit, pas un indice",
            Plan::nouveau(Masque::Etat(de), Motif::Bloc(pierre)).en_comptant(),
            tout,
        ),
        (
            "bloc — 4096 indices repackés, motif dépendant de la position",
            Plan::nouveau(
                Masque::Tout,
                Motif::melange(vec![(1, pierre), (1, terre)]),
            )
            .en_comptant(),
            tout,
        ),
    ];

    for (etiquette, plan, sel) in essais {
        let couche = std::env::temp_dir().join(format!(
            "titiforge-verite-{}-{}",
            std::process::id(),
            etiquette.as_bytes()[0]
        ));
        let _ = std::fs::remove_dir_all(&couche);
        let _ = std::fs::create_dir_all(&couche);
        let overlay = FsSource::open(&couche).expect("copie de travail");
        // La source est relue à chaque essai : chacun part de la save
        // d'ORIGINE, sinon le second vérifierait le premier.
        let source = FsSource::open(&monde).expect("monde relisible");
        let staging = Staging::new(source, overlay);

        let t0 = std::time::Instant::now();
        let rap = appliquer(&staging, &dim, Folder::Region, &sel, &plan, &interner)
            .expect("opération applicable");
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "\n┌ {etiquette}\n│ {ms:.0} ms · {} chunks touchés · {} blocs · étages rien {} · section {} · palette {} · bloc {}",
            rap.patches.len(),
            rap.blocs.unwrap_or(0),
            rap.etages[0],
            rap.etages[1],
            rap.etages[2],
            rap.etages[3]
        );
        if rap.patches.is_empty() {
            bilan.exiger(false, || format!("{etiquette} : aucun chunk modifié"));
            let _ = std::fs::remove_dir_all(&couche);
            continue;
        }

        // ── invariant n° 2 : l'opération est exactement réversible
        //
        // On n'exige pas la même empreinte, on exige les mêmes OCTETS. Une
        // empreinte qui colle sur des octets différents serait une collision ;
        // comparer les octets ne laisse pas cette porte ouverte.
        let mut annules = 0usize;
        for p in &rap.patches {
            let cle = (p.cible.region.x, p.cible.region.z, p.cible.chunk);
            let Some(origine) = avant.get(&cle) else {
                bilan.exiger(false, || format!("correctif sur un chunk absent : {cle:?}"));
                continue;
            };
            bilan.exiger(p.avant_hash == empreinte_de(origine), || {
                format!("{cle:?} : avant_hash ne décrit pas le chunk d'origine")
            });

            let mut e = p.refaire.clone();
            let apres = match splice(origine, &mut e) {
                Ok(v) => v,
                Err(err) => {
                    bilan.exiger(false, || format!("{cle:?} : refaire impossible ({err:?})"));
                    continue;
                }
            };
            bilan.exiger(empreinte_de(&apres) == p.apres_hash, || {
                format!("{cle:?} : refaire ne mène pas à apres_hash")
            });

            let mut e = p.annuler.clone();
            let retour = match splice(&apres, &mut e) {
                Ok(v) => v,
                Err(err) => {
                    bilan.exiger(false, || format!("{cle:?} : annuler impossible ({err:?})"));
                    continue;
                }
            };
            bilan.exiger(retour == *origine, || {
                format!(
                    "{cle:?} : annuler ne rend pas les octets d'origine ({} vs {} octets)",
                    retour.len(),
                    origine.len()
                )
            });
            annules += 1;
        }
        println!("│ {annules} chunks refaits puis annulés, octet pour octet");

        // ── invariant n° 1 : un chunk NON modifié ressort octet pour octet
        //
        // Et pas seulement « le même contenu » : la même charge COMPRESSÉE.
        // C'est ce qui prouve qu'on ne l'a pas réencodé — un réencodage qui
        // tomberait juste aujourd'hui tomberait faux le jour où zlib change de
        // version.
        let touches: std::collections::HashSet<_> = rap
            .patches
            .iter()
            .map(|p| (p.cible.region.x, p.cible.region.z, p.cible.chunk))
            .collect();
        let mut intacts = 0usize;
        let mut reecrits = 0usize;
        for info in &ov.regions {
            let Ok(octets) = staging.overlay().read_region(&dim, Folder::Region, info.pos) else {
                continue; // région jamais réécrite : rien à comparer
            };
            reecrits += 1;
            let r = read(&octets, info.pos.x, info.pos.z).expect("région réécrite analysable");
            for i in 0..1024u16 {
                let cle = (info.pos.x, info.pos.z, i);
                let Some(c) = r.get((i % 32) as i32, (i / 32) as i32) else {
                    bilan.exiger(!avant.contains_key(&cle), || {
                        format!("{cle:?} : chunk DISPARU de la région réécrite")
                    });
                    continue;
                };
                if touches.contains(&cle) {
                    let inflated = inflate(&c.payload, c.compression).expect("charge inflatable");
                    let p = rap
                        .patches
                        .iter()
                        .find(|p| (p.cible.region.x, p.cible.region.z, p.cible.chunk) == cle)
                        .unwrap();
                    bilan.exiger(empreinte_de(&inflated) == p.apres_hash, || {
                        format!("{cle:?} : le fichier écrit ne porte pas apres_hash")
                    });
                    continue;
                }
                let Some(origine) = brut.get(&cle) else {
                    bilan.exiger(false, || format!("{cle:?} : chunk apparu de nulle part"));
                    continue;
                };
                bilan.exiger(c.payload.as_ref() == &origine[..], || {
                    format!(
                        "{cle:?} : chunk NON modifié réécrit ({} → {} octets compressés)",
                        origine.len(),
                        c.payload.len()
                    )
                });
                intacts += 1;
            }
        }
        println!("└ {reecrits} régions réécrites · {intacts} chunks intacts vérifiés au ZLIB près");
        let _ = std::fs::remove_dir_all(&couche);
    }

    println!("\n{} contrôles", bilan.verifies);
    if bilan.fautes.is_empty() {
        println!("AUCUNE FAUTE.");
    } else {
        println!("{} FAUTES :", bilan.fautes.len());
        for f in bilan.fautes.iter().take(20) {
            println!("  {f}");
        }
        std::process::exit(1);
    }
}
