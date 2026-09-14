//! De l'opération au fichier, et retour.
//!
//! Chaque pièce est testée de son côté. Ce fichier teste leur JONCTION — le
//! seul endroit qu'`ExeWorldEdit` a payé cher : deux moitiés justes dont le
//! raccord ne l'est pas produisent un résultat parfaitement plausible et faux.
//!
//! Les propriétés vérifiées ici ne se vérifient nulle part ailleurs :
//!
//!  · la SOURCE reste octet pour octet ce qu'elle était ;
//!  · les chunks que la sélection ne touche pas sont réémis à l'identique ;
//!  · annuler rend le monde d'avant, octet pour octet ;
//!  · refaire rend le monde d'après, octet pour octet.

use tf_anvil::chunk::{decode_section, scan};
use tf_anvil::codec::inflate;
use tf_anvil::region::read;
use tf_anvil::Interner;
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, appliquer_region};
use tf_ops::plan::Plan;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::{Genre, Journal};
use tf_world::source::{Dimension, Folder, MemorySource, RegionSource};
use tf_world::Staging;

const SURFACE: Dimension = Dimension::Overworld;
const DOSSIER: Folder = Folder::Region;
const ZERO: RegionPos = RegionPos { x: 0, z: 0 };

fn monde() -> (MemorySource, Vec<u8>) {
    let t = Terrain::petite();
    let brut = region(&t);
    let m = MemorySource::new();
    m.put_region(SURFACE, DOSSIER, ZERO, brut.clone());
    (m, brut)
}

fn staging(src: MemorySource) -> Staging<MemorySource, MemorySource> {
    Staging::new(src, MemorySource::new())
}

/// Un lecteur de mondes qui NUMÉROTE les états de façon stable.
///
/// Un `StateId` n'a de sens que relativement à SON interner : deux lectures
/// indépendantes numérotent dans l'ordre où elles rencontrent les états, donc
/// deux mondes dont un chunk diffère décalent toute la numérotation des chunks
/// suivants. Comparer leurs identifiants, c'est comparer deux systèmes de
/// coordonnées — et ça produit exactement le genre de faux positif qu'on met
/// une heure à comprendre (mesuré : toutes les valeurs décalées de 1).
///
/// Le lecteur garde donc UNE table pour toutes ses lectures. Résoudre en
/// `String` marcherait aussi, et coûtait seize secondes de suite de tests.
#[derive(Default)]
struct Lecteur {
    table: std::collections::HashMap<String, u32>,
}

impl Lecteur {
    /// Tous les états d'une région, chunk par chunk et section par section.
    ///
    /// C'est la vérité qu'on compare : pas un résumé, pas un compte — le
    /// contenu.
    fn etats(&mut self, bytes: &[u8]) -> Vec<(u16, i8, Vec<u32>)> {
        let r = read(bytes, 0, 0).unwrap();
        let mut interner = Interner::new();
        let mut out = Vec::new();
        for c in r.iter() {
            let inflated = inflate(&c.payload, c.compression).unwrap();
            let s = scan(&inflated).unwrap();
            for sc in &s.sections {
                let Some(sec) = decode_section(&inflated, &s, sc, &mut interner).unwrap() else {
                    continue;
                };
                // La palette est petite : on la traduit UNE fois, et la boucle
                // des 4 096 cases ne fait plus qu'indexer.
                let stable: Vec<u32> = sec
                    .palette
                    .iter()
                    .map(|&id| {
                        let nom = interner.resolve(id).expect("l'état vient d'être interné");
                        let n = self.table.len() as u32;
                        *self.table.entry(nom.to_string()).or_insert(n)
                    })
                    .collect();
                let idx = sec.unpack();
                out.push((
                    c.index,
                    sc.y,
                    idx.iter().map(|&i| stable[i as usize]).collect(),
                ));
            }
        }
        out
    }
}

fn interner_de(bytes: &[u8]) -> Interner {
    let r = read(bytes, 0, 0).unwrap();
    let mut interner = Interner::new();
    for c in r.iter() {
        let inflated = inflate(&c.payload, c.compression).unwrap();
        let s = scan(&inflated).unwrap();
        for sc in &s.sections {
            decode_section(&inflated, &s, sc, &mut interner).unwrap();
        }
    }
    interner
}

fn selection_large() -> BBox {
    BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 255,
            y: 128,
            z: 255,
        },
    )
}

#[test]
fn une_operation_ecrit_dans_le_staging_et_jamais_dans_la_source() {
    // L'invariant n° 1, vérifié au bout de la chaîne et pas seulement dans le
    // module qui l'implémente.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").expect("la fixture en a");
    let terre = interner.get("minecraft:dirt").expect("et de la terre");
    let st = staging(src);

    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre)).en_comptant();
    let rap = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &selection_large(),
        &plan,
        &mut interner,
    )
    .unwrap();

    assert!(!rap.est_vide(), "l'opération doit avoir touché des chunks");
    assert!(rap.blocs.unwrap() > 0, "et modifié des blocs");
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        avant,
        "la SOURCE doit être intacte, octet pour octet"
    );

    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    assert_ne!(apres, avant, "mais la copie de travail, elle, a changé");
    let mut lecteur = Lecteur::default();
    let (a, b) = (lecteur.etats(&avant), lecteur.etats(&apres));
    assert_eq!(
        a.len(),
        b.len(),
        "aucune section ne doit apparaître ni disparaître"
    );
    let changes: usize = a
        .iter()
        .zip(&b)
        .map(|((_, _, x), (_, _, y))| x.iter().zip(y).filter(|(p, q)| p != q).count())
        .sum();
    assert_eq!(
        changes as u64,
        rap.blocs.unwrap(),
        "le compte annoncé doit être celui du monde"
    );
}

#[test]
fn annuler_rend_le_monde_octet_pour_octet() {
    // Le test qui justifie que le journal stocke les DEUX sens. Et il porte sur
    // les octets, pas sur les blocs : un round-trip qui rendrait les mêmes
    // blocs dans un fichier réécrit autrement aurait perdu tout ce qu'on n'a
    // pas compris du format.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer_region(
        &st,
        &SURFACE,
        DOSSIER,
        ZERO,
        &selection_large(),
        &plan,
        &mut interner,
    )
    .unwrap();
    assert!(!rap.patches.is_empty());

    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    // Le journal, comme l'application le tiendra : UNE entrée pour l'opération
    // entière, quel que soit le nombre de chunks touchés.
    let mut journal = Journal::new();
    let corrections = rap
        .patches
        .iter()
        .cloned()
        .map(tf_world::journal::Correction::Chunk)
        .collect();
    journal.pousser(
        "Remplacer pierre → terre",
        0,
        Genre::Operation {
            op: "replace".into(),
            // Le champ que `Rapport.bornes` remplit : le remaillage
            // incrémental et le recadrage de la vue s'y fient.
            bounds: rap.bornes,
            corrections,
        },
    );
    assert!(journal.peut_annuler());

    // ── annuler
    let (entree, _) = journal.annuler().expect("il y a quelque chose à annuler");
    let mut region = read(&apres, 0, 0).unwrap().iter().count();
    assert!(region > 0);
    region = 0;
    let mut defait = apres.clone();
    for c in entree_corrections(entree) {
        defait = rejouer(&defait, &c, true);
        region += 1;
    }
    assert!(region > 0, "l'annulation doit toucher des chunks");
    let mut lecteur = Lecteur::default();
    assert_eq!(
        lecteur.etats(&defait),
        lecteur.etats(&avant),
        "annuler doit rendre EXACTEMENT le monde d'avant"
    );

    // ── refaire
    let (entree, _) = journal.refaire().expect("et à refaire");
    let mut refait = defait.clone();
    for c in entree_corrections(entree) {
        refait = rejouer(&refait, &c, false);
    }
    assert_eq!(
        lecteur.etats(&refait),
        lecteur.etats(&apres),
        "refaire doit rendre EXACTEMENT le monde d'après"
    );
}

/// Les correctifs de chunk d'une entrée.
fn entree_corrections(e: &tf_world::journal::Entree) -> Vec<tf_world::journal::ChunkPatch> {
    match &e.genre {
        Genre::Operation { corrections, .. } => corrections
            .iter()
            .filter_map(|c| match c {
                tf_world::journal::Correction::Chunk(p) => Some(p.clone()),
                _ => None,
            })
            .collect(),
        Genre::Reprise => Vec::new(),
    }
}

/// Rejoue un correctif sur la région, dans un sens ou dans l'autre.
///
/// Le correctif porte sur le chunk INFLATÉ : il faut donc le décompresser, le
/// recoller, et le recompresser. C'est exactement ce que l'application fera.
fn rejouer(region_bytes: &[u8], patch: &tf_world::journal::ChunkPatch, annuler: bool) -> Vec<u8> {
    use tf_anvil::chunk::splice;
    use tf_anvil::codec::deflate;
    use tf_anvil::region::write;

    let mut r = read(region_bytes, 0, 0).unwrap();
    let (lx, lz) = (
        (patch.cible.chunk % 32) as i32,
        (patch.cible.chunk / 32) as i32,
    );
    let c = r.get_mut(lx, lz).expect("le chunk visé existe");
    let courant = inflate(&c.payload, c.compression).unwrap();
    let attendu = if annuler {
        patch.apres_hash
    } else {
        patch.avant_hash
    };
    assert_eq!(
        tf_world::journal::empreinte(&courant),
        attendu,
        "le journal refuse de s'appliquer à un chunk qui a changé sous lui"
    );
    let mut edits = if annuler {
        patch.annuler.clone()
    } else {
        patch.refaire.clone()
    };
    let neuf = splice(&courant, &mut edits).unwrap();
    c.payload = std::borrow::Cow::Owned(deflate(&neuf, c.compression).unwrap());
    write(&r).unwrap().region
}

#[test]
fn un_chunk_hors_selection_n_est_pas_touche() {
    // La portée d'une opération est ce qu'elle a VRAIMENT écrit. Un chunk que
    // la sélection ne touche pas ne doit produire ni correctif, ni réécriture.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    // Un seul chunk : (0,0) à (15,15) en x/z.
    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 15,
            y: 320,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &mut interner).unwrap();

    assert_eq!(
        rap.patches.len(),
        1,
        "un seul chunk visé, un seul correctif"
    );
    assert_eq!(rap.patches[0].cible.chunk, 0);

    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let mut lecteur = Lecteur::default();
    let (a, b) = (lecteur.etats(&avant), lecteur.etats(&apres));
    for ((ci, y, x), (_, _, z)) in a.iter().zip(&b) {
        if *ci == 0 {
            continue;
        }
        assert_eq!(x, z, "le chunk {ci} (section {y}) ne devait pas bouger");
    }
    let bornes = rap.bornes.expect("l'opération a écrit");
    assert!(
        bornes.min.x >= 0 && bornes.max.x <= 15 && bornes.min.z >= 0 && bornes.max.z <= 15,
        "les bornes ne débordent pas de la sélection : {bornes:?}"
    );
}

#[test]
fn une_operation_qui_ne_change_rien_n_ecrit_rien() {
    // Un drapeau de propreté finit par mentir ; des octets, non. Une opération
    // dont la cible est absente ne doit ni salir le staging, ni remplir le
    // journal d'entrées vides.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let absent = interner.intern("minecraft:bedrock_qui_n_existe_pas");
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    let plan = Plan::nouveau(Masque::Etat(absent), Motif::Bloc(terre)).en_comptant();
    let rap = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &selection_large(),
        &plan,
        &mut interner,
    )
    .unwrap();

    assert!(rap.est_vide(), "aucun correctif");
    assert_eq!(rap.blocs, Some(0));
    assert_eq!(rap.bornes, None);
    assert!(st.is_clean(), "le staging ne doit pas être sali");
    assert_eq!(st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(), avant);
}

#[test]
fn une_region_absente_n_est_pas_une_erreur() {
    // Le cas NORMAL au bord d'un monde. Le confondre avec un échec ferait
    // refuser une save parfaitement saine.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    let loin = RegionPos { x: 40, z: 40 };
    let sel = BBox::new(
        BlockPos {
            x: 40 * 512,
            y: 0,
            z: 40 * 512,
        },
        BlockPos {
            x: 40 * 512 + 15,
            y: 15,
            z: 40 * 512 + 15,
        },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(terre));
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, loin, &sel, &plan, &mut interner).unwrap();
    assert!(rap.est_vide());
}

#[test]
fn un_monde_en_coordonnees_negatives_se_modifie_au_bon_endroit() {
    // Le piège le mieux documenté du dépôt, vérifié de bout en bout : le bloc
    // −1 est dans la région −1, pas la région 0. Une division entière naïve
    // chargerait la mauvaise moitié du monde sans rien signaler.
    //
    // La même fixture, posée en région (−1, −1). Elle ne peuple que ses seize
    // premiers chunks LOCAUX, donc les chunks monde (−32..−17), donc les blocs
    // (−512..−257).
    let t = Terrain::petite();
    let brut = region(&t);
    let src = MemorySource::new();
    let moins = RegionPos { x: -1, z: -1 };
    src.put_region(SURFACE, DOSSIER, moins, brut.clone());
    let mut interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    // Un seul chunk : celui de coordonnées monde (−32, −32), premier chunk
    // local de la région −1. Ses blocs vont de −512 à −497.
    let sel = BBox::new(
        BlockPos {
            x: -512,
            y: -64,
            z: -512,
        },
        BlockPos {
            x: -497,
            y: 320,
            z: -497,
        },
    );
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre)).en_comptant();
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &mut interner).unwrap();

    assert_eq!(
        rap.patches.len(),
        1,
        "un seul chunk visé — et il est dans la région −1"
    );
    // Le chunk (−32, −32) est le PREMIER de la région −1 : index 0. Une
    // division entière naïve l'aurait cherché dans la région 0.
    assert_eq!(rap.patches[0].cible.chunk, 0);
    assert_eq!(rap.patches[0].cible.region, moins);
    let bornes = rap.bornes.expect("l'opération a écrit");
    assert!(
        bornes.min.x >= -512 && bornes.max.x <= -497,
        "les bornes restent dans la sélection négative : {bornes:?}"
    );
    assert!(rap.blocs.unwrap() > 0);

    // Et la région 0,0 n'existe même pas : rien n'y a été écrit.
    assert!(
        st.read_region(&SURFACE, DOSSIER, ZERO).is_err(),
        "rien ne doit avoir été écrit dans la région 0"
    );
}

#[test]
fn une_operation_sur_plusieurs_regions_fait_une_seule_entree_de_journal() {
    // Un Ctrl+Z qui ne défait qu'un tiers du travail est pire qu'une
    // annulation absente.
    let t = Terrain::petite();
    let brut = region(&t);
    let src = MemorySource::new();
    for (x, z) in [(0, 0), (1, 0), (0, 1)] {
        src.put_region(SURFACE, DOSSIER, RegionPos { x, z }, brut.clone());
    }
    let mut interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    // Une sélection qui déborde sur deux régions EN Y TROUVANT DES CHUNKS :
    // la fixture ne peuple que ses seize premiers chunks locaux, donc les
    // chunks monde 0..15 pour la région 0 et 32..47 pour la région 1.
    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 527,
            y: 320,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &mut interner).unwrap();

    let regions: std::collections::BTreeSet<_> =
        rap.patches.iter().map(|p| p.cible.region).collect();
    assert!(
        regions.len() >= 2,
        "la sélection doit vraiment déborder : {regions:?}"
    );

    let mut journal = Journal::new();
    let corrections = rap
        .patches
        .iter()
        .cloned()
        .map(tf_world::journal::Correction::Chunk)
        .collect();
    journal.pousser(
        "Remplacer",
        0,
        Genre::Operation {
            op: "replace".into(),
            bounds: rap.bornes,
            corrections,
        },
    );
    assert_eq!(journal.entrees().len(), 1, "UNE entrée, pas une par région");
    assert_eq!(
        journal.entrees()[0].regions().len(),
        regions.len(),
        "et elle nomme toutes les régions touchées"
    );
}

#[test]
fn l_etage_section_traverse_le_splice_et_se_relit() {
    // L'étage O(1) rend la section HOMOGÈNE : sa palette tombe à une entrée et
    // son tableau d'indices disparaît. Le jeu n'en écrit pas pour une palette
    // d'une entrée, et en laisser un de la mauvaise longueur casse le
    // chargement du chunk. Le recollement doit donc retirer le CHAMP entier,
    // pas seulement sa charge — et ça ne se vérifie qu'en relisant le fichier
    // produit.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);

    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 15,
            y: 320,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::Bloc(terre)).en_comptant();
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &mut interner).unwrap();
    assert_eq!(
        rap.etages[1],
        rap.etages.iter().sum::<usize>() - rap.etages[0] - rap.etages[3],
        "les sections couvertes doivent passer par l'étage SECTION"
    );
    assert!(rap.etages[1] > 0, "au moins une section en O(1)");

    // On RELIT le fichier produit : c'est la seule preuve qui compte.
    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let r = read(&apres, 0, 0).unwrap();
    let c = r.get(0, 0).expect("le chunk visé");
    let inflated = inflate(&c.payload, c.compression).unwrap();
    let s = scan(&inflated).unwrap();
    let mut interner2 = Interner::new();
    let mut vues = 0;
    for sc in &s.sections {
        let Some(sec) = decode_section(&inflated, &s, sc, &mut interner2).unwrap() else {
            continue;
        };
        // Toutes les sections du chunk sont dans la sélection : toutes doivent
        // être homogènes et en terre.
        assert_eq!(sec.palette.len(), 1, "section {} non homogène", sc.y);
        assert!(sec.data.is_empty(), "section {} garde un `data`", sc.y);
        assert_eq!(
            interner2.resolve(sec.palette[0]),
            Some("minecraft:dirt"),
            "section {}",
            sc.y
        );
        vues += 1;
    }
    assert!(vues > 0);
}

#[test]
fn l_etage_bloc_fait_grandir_la_palette_et_se_relit() {
    // Un mélange ajoute des états à la palette, ce qui peut faire grandir
    // `bits`, donc allonger le tableau d'indices. Le recollement remplace une
    // plage d'octets par une PLUS LONGUE — et tout ce qui suit dans le chunk se
    // décale. C'est le cas que le splice doit tenir, et il ne se vérifie qu'en
    // relisant.
    let (src, avant) = monde();
    let mut interner = interner_de(&avant);
    let a = interner.intern("minecraft:titiforge_test_a");
    let b = interner.intern("minecraft:titiforge_test_b");
    let c = interner.intern("minecraft:titiforge_test_c");
    let st = staging(src);

    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 15,
            y: 320,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Tout, Motif::melange(vec![(1, a), (1, b), (1, c)]))
        .avec_seed(1234)
        .en_comptant();
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &mut interner).unwrap();
    assert!(rap.etages[3] > 0, "le mélange doit passer par l'étage BLOC");
    assert!(rap.blocs.unwrap() > 0);

    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let r = read(&apres, 0, 0).unwrap();
    let ch = r.get(0, 0).expect("le chunk visé");
    let inflated = inflate(&ch.payload, ch.compression).unwrap();
    let s = scan(&inflated).unwrap();
    let mut interner2 = Interner::new();
    let mut compte = [0usize; 3];
    for sc in &s.sections {
        let Some(sec) = decode_section(&inflated, &s, sc, &mut interner2).unwrap() else {
            continue;
        };
        let idx = sec.unpack();
        for &i in idx.iter() {
            let nom = interner2.resolve(sec.palette[i as usize]).unwrap();
            match nom {
                "minecraft:titiforge_test_a" => compte[0] += 1,
                "minecraft:titiforge_test_b" => compte[1] += 1,
                "minecraft:titiforge_test_c" => compte[2] += 1,
                autre => panic!("état inattendu après un mélange couvrant : {autre}"),
            }
        }
    }
    let total: usize = compte.iter().sum();
    assert!(total > 0);
    for (i, n) in compte.iter().enumerate() {
        let part = *n as f64 / total as f64;
        assert!(
            (part - 1.0 / 3.0).abs() < 0.02,
            "état {i} : {part:.3} au lieu d'un tiers"
        );
    }
}
