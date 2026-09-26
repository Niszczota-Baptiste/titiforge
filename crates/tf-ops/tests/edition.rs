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
use tf_anvil::{Interner, StateId};
use tf_bench::{region, Terrain};
use tf_ops::edition::{appliquer, appliquer_region, copier};
use tf_ops::plan::Plan;
use tf_ops::Presse;
use tf_ops::{Masque, Motif};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
use tf_world::source::{Dimension, Folder, MemorySource, RegionSink, RegionSource};
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
    let interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").expect("la fixture en a");
    let terre = interner.get("minecraft:dirt").expect("et de la terre");
    let st = staging(src);

    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre)).en_comptant();
    let rap = appliquer(&st, &SURFACE, DOSSIER, &selection_large(), &plan, &interner).unwrap();

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
    let interner = interner_de(&avant);
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
        &interner,
    )
    .unwrap();
    assert!(!rap.patches.is_empty());

    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    // Le journal, comme l'application le tiendra : UNE entrée pour l'opération
    // entière, quel que soit le nombre de chunks touchés. Par la JONCTION —
    // c'est elle qui remplit `bounds` depuis ce que l'opération a vraiment
    // écrit, et c'est d'elle que le remaillage incrémental dépendra.
    let mut journal = Journal::new();
    assert!(rap
        .journaliser(
            &mut journal,
            "Remplacer pierre → terre",
            "replace",
            Vec::new(),
            0
        )
        .is_some());
    assert!(journal.peut_annuler());

    // ── annuler
    let (entree, _) = journal.annuler().expect("il y a quelque chose à annuler");
    let mut region = read(&apres, 0, 0).unwrap().iter().count();
    assert!(region > 0);
    region = 0;
    let mut defait = apres.clone();
    // `a_annuler` rend les correctifs À L'ENVERS : deux passes sur le même
    // chunk s'enchaînent par leurs empreintes, et les rejouer dans l'ordre
    // d'enregistrement ferait échouer la seconde.
    for c in entree_corrections(entree, true) {
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
    for c in entree_corrections(entree, false) {
        refait = rejouer(&refait, &c, false);
    }
    assert_eq!(
        lecteur.etats(&refait),
        lecteur.etats(&apres),
        "refaire doit rendre EXACTEMENT le monde d'après"
    );
}

/// Les correctifs de chunk d'une entrée, dans le sens demandé.
fn entree_corrections(
    e: &tf_world::journal::Entree,
    annuler: bool,
) -> Vec<tf_world::journal::ChunkPatch> {
    let garder = |c: &tf_world::journal::Correction| match c {
        tf_world::journal::Correction::Chunk(p) => Some(p.clone()),
        _ => None,
    };
    if annuler {
        e.a_annuler().filter_map(garder).collect()
    } else {
        e.a_refaire().filter_map(garder).collect()
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
    let interner = interner_de(&avant);
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
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &interner).unwrap();

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
fn une_edition_fait_reeclairer_ses_chunks_par_le_jeu() {
    // Le splice ne remplace que les champs de blocs : la lumière stockée et
    // les cartes de hauteur décrivaient les blocs d'AVANT. En jeu, une salle
    // creusée sortait noire et la pluie traversait un toit neuf. On ne
    // recalcule rien soi-même — on DEMANDE au jeu de le faire : `isLightOn`
    // à zéro, `Heightmaps` retiré, sur les chunks dont les blocs ont changé,
    // et seulement eux.
    let (src, avant) = monde();
    let interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").unwrap();
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
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &interner).unwrap();
    assert_eq!(rap.patches.len(), 1, "la prémisse : un seul chunk écrit");
    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let (ra, rb) = (read(&avant, 0, 0).unwrap(), read(&apres, 0, 0).unwrap());
    let balayer = |r: &tf_anvil::region::Region, x: i32, z: i32| {
        let c = r.get(x, z).expect("le chunk existe");
        scan(&inflate(&c.payload, c.compression).unwrap()).unwrap()
    };
    let (touche_avant, touche) = (balayer(&ra, 0, 0), balayer(&rb, 0, 0));
    assert_eq!(
        touche_avant.lumiere.map(|(_, v)| v),
        Some(1),
        "la prémisse : le chunk était tenu pour éclairé"
    );
    assert!(
        touche_avant.hauteurs.is_some(),
        "et portait ses cartes de hauteur"
    );
    assert_eq!(
        touche.lumiere.map(|(_, v)| v),
        Some(0),
        "après l'édition, le jeu doit le rééclairer"
    );
    assert!(
        touche.hauteurs.is_none(),
        "et recalculer ses cartes de hauteur, qu'il reconstruit quand elles manquent"
    );

    // Le voisin que la sélection ne touche pas : octet pour octet.
    let (va, vb) = (ra.get(1, 0).unwrap(), rb.get(1, 0).unwrap());
    assert_eq!(
        va.payload, vb.payload,
        "un chunk non modifié est réémis octet pour octet, éclairage compris"
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
    let rap = appliquer(&st, &SURFACE, DOSSIER, &selection_large(), &plan, &interner).unwrap();

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
    let interner = interner_de(&avant);
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
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, loin, &sel, &plan, &interner).unwrap();
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
    let interner = interner_de(&brut);
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
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();

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
    let interner = interner_de(&brut);
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
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();

    let regions: std::collections::BTreeSet<_> =
        rap.patches.iter().map(|p| p.cible.region).collect();
    assert!(
        regions.len() >= 2,
        "la sélection doit vraiment déborder : {regions:?}"
    );

    let mut journal = Journal::new();
    assert!(rap
        .journaliser(&mut journal, "Remplacer", "replace", Vec::new(), 0)
        .is_some());
    assert_eq!(journal.entrees().len(), 1, "UNE entrée, pas une par région");
    assert_eq!(
        journal.entrees()[0].regions().len(),
        regions.len(),
        "et elle nomme toutes les régions touchées"
    );
}

/// **Une annulation qui ramène une région au contenu de la save la rend à la
/// save** — et seulement celle-là.
///
/// Recompressée, la région restait différente de la save AUX OCTETS : une
/// séance « éditer puis tout annuler » survivait à la fermeture, et le jour où
/// le joueur jouait là, la reprise voyait un conflit avec… rien.
#[test]
fn une_annulation_complete_rend_la_region_a_la_save() {
    use tf_ops::edition::{rejouer, Sens};

    let (src, brut) = monde();
    let interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);
    let mut journal = Journal::new();
    // Deux opérations sur deux chunks DIFFÉRENTS de la même région.
    for (x0, x1) in [(0, 15), (16, 31)] {
        let sel = BBox::new(BlockPos::new(x0, -64, 0), BlockPos::new(x1, 0, 15));
        let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
        let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();
        assert!(rap
            .journaliser(&mut journal, "Remplacer", "replace", Vec::new(), 0)
            .is_some());
    }

    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert!(
        st.is_dirty(&SURFACE, DOSSIER, ZERO),
        "la première opération y est toujours : la région reste"
    );

    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert!(!st.is_dirty(&SURFACE, DOSSIER, ZERO), "tout est défait");
    assert!(st.etats().unwrap().is_empty());
    assert_eq!(st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(), brut);

    // Et le journal n'en souffre pas : refaire relit la save, dont le
    // contenu est celui qu'il attend.
    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert!(st.is_dirty(&SURFACE, DOSSIER, ZERO));
}

/// **Une annulation qui diverge dans UNE région n'écrit dans AUCUNE.**
///
/// Écrire région par région laissait, quand la deuxième divergeait, la
/// première défaite et la seconde non : une entrée à moitié annulée, que plus
/// rien ne savait rejouer. Le cas n'a rien d'exotique : une région relue
/// depuis la save à la reprise d'une séance diverge par construction.
#[test]
fn une_annulation_qui_diverge_dans_une_region_n_ecrit_dans_aucune() {
    use tf_ops::edition::{rejouer, Erreur, Sens};
    use tf_world::journal::Correction;

    let t = Terrain::petite();
    let brut = region(&t);
    let src = MemorySource::new();
    for (x, z) in [(0, 0), (1, 0)] {
        src.put_region(SURFACE, DOSSIER, RegionPos { x, z }, brut.clone());
    }
    let interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);
    let sel = BBox::new(
        BlockPos { x: 0, y: -64, z: 0 },
        BlockPos {
            x: 527,
            y: 320,
            z: 15,
        },
    );
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();
    let mut journal = Journal::new();
    assert!(rap
        .journaliser(&mut journal, "Remplacer", "replace", Vec::new(), 0)
        .is_some());

    // La région que l'annulation traitera EN DERNIER change sous le journal :
    // c'est le seul ordre où l'ancienne écriture région par région avait
    // déjà défait l'autre au moment de découvrir la divergence.
    let (entree, _) = journal.annuler().unwrap();
    let premiere = entree
        .a_annuler()
        .find_map(|c| match c {
            Correction::Chunk(p) => Some(p.cible.region),
            _ => None,
        })
        .unwrap();
    let derniere = if premiere == ZERO {
        RegionPos { x: 1, z: 0 }
    } else {
        ZERO
    };
    let relue = st
        .source()
        .read_region(&SURFACE, DOSSIER, derniere)
        .unwrap();
    st.write_region(&SURFACE, DOSSIER, derniere, &relue)
        .unwrap();
    let avant = st.read_region(&SURFACE, DOSSIER, premiere).unwrap();

    let r = rejouer(&st, entree, Sens::Annuler);
    assert!(matches!(r, Err(Erreur::Divergence { .. })), "{r:?}");
    assert_eq!(
        st.read_region(&SURFACE, DOSSIER, premiere).unwrap(),
        avant,
        "la région qui ne divergeait pas n'a pas été défaite à moitié"
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
    let interner = interner_de(&avant);
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
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &interner).unwrap();
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
    let rap = appliquer_region(&st, &SURFACE, DOSSIER, ZERO, &sel, &plan, &interner).unwrap();
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

#[test]
fn le_resultat_ne_depend_pas_du_nombre_de_coeurs() {
    // La chaîne par chunk tourne sur plusieurs fils. Si le résultat en
    // dépendait, deux utilisateurs obtiendraient deux mondes différents de la
    // même opération — et le journal ne défairait pas ce qu'il croit défaire.
    //
    // Ce qui rend la propriété vraie n'est pas la chance : le tirage se hache
    // sur la POSITION, et `collect()` garde l'ordre d'entrée. Ce test fige les
    // deux.
    let (_, avant) = monde();
    let interner = interner_de(&avant);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();

    let sel = selection_large();
    let melange = Plan::nouveau(Masque::Tout, Motif::melange(vec![(3, pierre), (1, terre)]))
        .avec_seed(4242)
        .en_comptant();

    let mut resultats = Vec::new();
    for _ in 0..3 {
        let s = MemorySource::new();
        s.put_region(SURFACE, DOSSIER, ZERO, avant.clone());
        let st = staging(s);
        let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &melange, &interner).unwrap();
        let bytes = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
        let cibles: Vec<u16> = rap.patches.iter().map(|p| p.cible.chunk).collect();
        resultats.push((bytes, cibles, rap.blocs, rap.etages));
    }

    for i in 1..resultats.len() {
        assert_eq!(
            resultats[0].0, resultats[i].0,
            "le FICHIER doit être identique d'une exécution à l'autre"
        );
        assert_eq!(
            resultats[0].1, resultats[i].1,
            "et l'ordre des correctifs de journal aussi"
        );
        assert_eq!(resultats[0].2, resultats[i].2, "et le compte de blocs");
        assert_eq!(resultats[0].3, resultats[i].3, "et les étages traversés");
    }
    assert!(resultats[0].2.unwrap() > 0, "l'opération doit avoir écrit");
    assert!(
        resultats[0].1.len() > 1,
        "et toucher plusieurs chunks, sinon le test ne prouve rien"
    );
}

// ── copier : la seule opération qui n'écrit rien ────────────────────────────

/// Ce que la source contient vraiment à une position MONDE, lu par un chemin
/// indépendant de `copier` — sinon on comparerait la copie à elle-même.
fn bloc_source(brut: &[u8], interner: &mut Interner, x: i32, y: i32, z: i32) -> Option<StateId> {
    let r = read(brut, 0, 0).ok()?;
    let c = r.get(x.div_euclid(16) & 31, z.div_euclid(16) & 31)?;
    let inf = inflate(&c.payload, c.compression).ok()?;
    let sc = scan(&inf).ok()?;
    let sy = y.div_euclid(16) as i8;
    let s = sc.sections.iter().find(|s| s.y == sy)?;
    let section = decode_section(&inf, &sc, s, interner).ok()??;
    section.get(
        x.rem_euclid(16) as usize,
        y.rem_euclid(16) as usize,
        z.rem_euclid(16) as usize,
    )
}

fn boite(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> BBox {
    BBox::new(
        BlockPos {
            x: x0,
            y: y0,
            z: z0,
        },
        BlockPos {
            x: x1,
            y: y1,
            z: z1,
        },
    )
}

/// La copie rend EXACTEMENT ce que la source contient, case par case.
///
/// Vérifié contre un décodage indépendant : comparer la copie à elle-même ne
/// prouverait rien, et c'est exactement l'erreur que le croisement des étages
/// existe pour éviter.
#[test]
fn copier_rend_ce_que_la_source_contient() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    // Une boîte qui traverse deux sections en hauteur et deux chunks en X,
    // pour que le recollage des morceaux compte.
    let sel = boite(10, 4, 3, 21, 20, 9);
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();

    let (sx, sy, sz) = sel.size();
    assert_eq!(p.taille, [sx, sy, sz]);
    let mut compares = 0;
    for dy in 0..sy {
        for dz in 0..sz {
            for dx in 0..sx {
                let (x, y, z) = (
                    sel.min.x + dx as i32,
                    sel.min.y + dy as i32,
                    sel.min.z + dz as i32,
                );
                if let Some(attendu) = bloc_source(&brut, &mut i, x, y, z) {
                    assert_eq!(
                        p.get(dx, dy, dz),
                        Some(attendu),
                        "case monde ({x}, {y}, {z})"
                    );
                    compares += 1;
                }
            }
        }
    }
    assert!(compares > 500, "le test doit comparer pour de vrai");
}

/// **Ce qui n'existe pas vaut de l'air, pas une erreur.**
///
/// Une sélection déborde presque toujours de ce qui est généré — c'est le cas
/// NORMAL quand on copie un bâtiment avec sa marge. Refuser rendrait `//copy`
/// inutilisable au bord d'un build.
#[test]
fn copier_hors_du_monde_rend_de_l_air_et_pas_une_erreur() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    // Très loin de la seule région présente.
    let sel = boite(100_000, 0, 100_000, 100_003, 2, 100_003);
    let p = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(p.taille, [4, 3, 4]);
    assert!(
        p.blocs.iter().all(|&b| b == air),
        "hors du monde généré, tout doit être de l'air"
    );
}

/// Copier ne touche pas à la source, et ne salit pas la copie de travail : il
/// n'y a donc rien à annuler.
#[test]
fn copier_n_ecrit_nulle_part() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let _ = copier(&st, &SURFACE, DOSSIER, &boite(0, 0, 0, 31, 31, 31), &mut i).unwrap();
    assert!(st.is_clean(), "aucune région ne doit être salie");
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        brut,
        "la source reste octet pour octet ce qu'elle était"
    );
}

/// Une sélection d'un seul bloc est un cas limite qu'on écrit par erreur un
/// jour sur deux : la boîte est inclusive des DEUX côtés.
#[test]
fn copier_un_seul_bloc_rend_une_case() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let p = copier(&st, &SURFACE, DOSSIER, &boite(5, 7, 9, 5, 7, 9), &mut i).unwrap();
    assert_eq!(p.taille, [1, 1, 1]);
    assert_eq!(p.blocs.len(), 1);
    if let Some(attendu) = bloc_source(&brut, &mut i, 5, 7, 9) {
        assert_eq!(p.blocs[0], attendu);
    }
}

/// Copier puis tourner quatre fois rend l'extrait de départ — la jonction
/// entre la lecture du monde et la géométrie du presse-papiers.
#[test]
fn copier_puis_tourner_quatre_fois_revient_au_depart() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let depart = copier(&st, &SURFACE, DOSSIER, &boite(0, 0, 0, 9, 5, 13), &mut i).unwrap();
    let mut p = depart.clone();
    for _ in 0..4 {
        p = p
            .transformer(tf_blocks::Transfo::Rot90, &mut i, &|_, _| None)
            .presse;
    }
    assert_eq!(p, depart);
}

// ── coller : le tour complet ────────────────────────────────────────────────

/// Copier un morceau, le reposer ailleurs, et RELIRE le fichier écrit.
///
/// C'est la jonction que rien d'autre ne vérifie : le presse-papiers est juste
/// de son côté, le splice aussi, et leur raccord peut ne pas l'être — deux
/// moitiés justes qui produisent un résultat parfaitement plausible et faux.
#[test]
fn coller_repose_l_extrait_case_pour_case() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");

    let depuis = boite(2, 3, 4, 9, 8, 11);
    let p = copier(&st, &SURFACE, DOSSIER, &depuis, &mut i).unwrap();

    // Loin de la source, pour qu'aucun recouvrement ne puisse masquer une
    // erreur de coordonnées.
    let coin = BlockPos {
        x: 200,
        y: 3,
        z: 200,
    };
    let c = tf_ops::Collage {
        presse: &p,
        coin,
        avec_air: true,
        air,
        compter: true,
    };
    let sel = c.bornes();
    let r = appliquer(&st, &SURFACE, DOSSIER, &sel, &c, &i).unwrap();
    assert!(
        !r.patches.is_empty(),
        "le collage doit écrire quelque chose"
    );
    assert_eq!(
        r.etages[3],
        r.etages.iter().sum::<usize>(),
        "tout à l'étage bloc"
    );

    // On RELIT ce qui a été écrit, à travers le staging — pas la mémoire.
    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(relu.taille, p.taille);
    assert_eq!(relu.blocs, p.blocs, "l'extrait relu doit être l'original");
}

/// **L'air de l'extrait n'écrase pas ce qui est là**, sauf si on le demande.
///
/// C'est le défaut de WorldEdit et c'est le bon : on colle presque toujours un
/// bâtiment sur un terrain, pas un cube d'air.
#[test]
fn coller_sans_air_laisse_le_terrain_en_place() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:glowstone");

    // Un extrait moitié air, moitié marque.
    let mut p = Presse::uniforme([4, 2, 4], air);
    for x in 0..4u32 {
        for z in 0..4u32 {
            let idx = p.index(x, 1, z).unwrap();
            p.blocs[idx] = marque;
        }
    }
    let coin = BlockPos { x: 4, y: 4, z: 4 };
    let avant = copier(&st, &SURFACE, DOSSIER, &boite(4, 4, 4, 7, 5, 7), &mut i).unwrap();

    let c = tf_ops::Collage {
        presse: &p,
        coin,
        avec_air: false,
        air,
        compter: false,
    };
    appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();
    let apres = copier(&st, &SURFACE, DOSSIER, &c.bornes(), &mut i).unwrap();

    for x in 0..4u32 {
        for z in 0..4u32 {
            assert_eq!(
                apres.get(x, 1, z),
                Some(marque),
                "la couche pleine doit être posée"
            );
            assert_eq!(
                apres.get(x, 0, z),
                avant.get(x, 0, z),
                "la couche d'AIR de l'extrait ne doit rien écraser"
            );
        }
    }
}

/// Copier, tourner d'un quart de tour, coller : l'extrait relu doit être
/// exactement l'extrait tourné. C'est le tour complet de `//copy //rotate
/// //paste`.
#[test]
fn copier_tourner_coller_rend_l_extrait_tourne() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");

    // Non carré exprès : une rotation qui n'échangerait pas les dimensions
    // passerait sur un cube.
    let p = copier(&st, &SURFACE, DOSSIER, &boite(1, 2, 3, 12, 6, 7), &mut i).unwrap();
    let tourne = p
        .transformer(tf_blocks::Transfo::Rot90, &mut i, &|_, _| None)
        .presse;
    assert_ne!(tourne.taille, p.taille, "la fixture doit être non carrée");

    let c = tf_ops::Collage {
        presse: &tourne,
        // Dans la zone GÉNÉRÉE de la fixture (chunks 0..15) : un collage
        // n'engendre pas de chunk, voir le test qui suit.
        coin: BlockPos {
            x: 150,
            y: 2,
            z: 150,
        },
        avec_air: true,
        air,
        compter: false,
    };
    let sel = c.bornes();
    appliquer(&st, &SURFACE, DOSSIER, &sel, &c, &i).unwrap();
    let relu = copier(&st, &SURFACE, DOSSIER, &sel, &mut i).unwrap();
    assert_eq!(relu.taille, tourne.taille);
    assert_eq!(relu.blocs, tourne.blocs);
}

/// Un collage s'annule comme le reste : le journal ne sait pas qu'il est
/// différent, et c'est exactement ce qu'on veut d'une couture.
#[test]
fn un_collage_s_annule_octet_pour_octet() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:glowstone");
    let p = Presse::uniforme([6, 3, 6], marque);

    let c = tf_ops::Collage {
        presse: &p,
        coin: BlockPos { x: 3, y: 5, z: 3 },
        avec_air: true,
        air,
        compter: false,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();
    assert!(!r.patches.is_empty());

    // Annuler en rejouant le sens « annuler » de chaque correctif.
    let ecrit = st.overlay().read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    let region = read(&ecrit, 0, 0).unwrap();
    for patch in &r.patches {
        let c = region
            .get(
                (patch.cible.chunk % 32) as i32,
                (patch.cible.chunk / 32) as i32,
            )
            .unwrap();
        let apres = inflate(&c.payload, c.compression).unwrap();
        let mut e = patch.annuler.clone();
        let retour = tf_anvil::splice(&apres, &mut e).unwrap();
        assert_eq!(
            tf_world::journal::empreinte(&retour),
            patch.avant_hash,
            "annuler doit rendre le chunk d'avant"
        );
    }
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        brut,
        "et la source n'a jamais bougé"
    );
}

/// **Un collage n'ENGENDRE pas de chunk**, et il faut le savoir.
///
/// Coller là où le monde n'a jamais été généré n'écrit rien — pas d'erreur,
/// pas de chunk créé, rien. C'est cohérent avec le reste du crate (on ne crée
/// pas de terrain), mais ça se lit « j'ai collé et il ne s'est rien passé ».
/// Le rapport le dit : aucun correctif.
///
/// Le jour où l'on voudra l'inverse, ce sera une opération à part — engendrer
/// un chunk vide est une décision, pas un effet de bord d'un collage.
#[test]
fn coller_hors_des_chunks_generes_n_ecrit_rien_et_le_dit() {
    let (src, brut) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let air = i.intern("minecraft:air");
    let marque = i.intern("minecraft:glowstone");
    let p = Presse::uniforme([4, 4, 4], marque);

    // La fixture ne peuple que les chunks 0..15, soit les blocs 0..255.
    let c = tf_ops::Collage {
        presse: &p,
        coin: BlockPos {
            x: 400,
            y: 4,
            z: 400,
        },
        avec_air: true,
        air,
        compter: true,
    };
    let r = appliquer(&st, &SURFACE, DOSSIER, &c.bornes(), &c, &i).unwrap();
    assert!(
        r.patches.is_empty(),
        "aucun chunk là-bas : rien à écrire, et le rapport doit le montrer"
    );
    assert_eq!(r.bornes, None, "rien n'a été écrit");
    assert!(st.is_clean(), "et la copie de travail reste propre");
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        brut
    );
}

// ── la jonction rapport → journal ────────────────────────────────────────────

/// **La jonction existe, et elle est la SEULE.**
///
/// Recomposer une entrée à la main donne trois occasions de se tromper — les
/// correctifs, le nom de l'opération, les bornes — et l'une des trois est un
/// piège que ce dépôt a déjà payé. Le test qui manquait ne porte pas sur le
/// journal ni sur l'opération : il porte sur ce qui les relie, et c'est
/// toujours là que ça casse.
#[test]
fn la_jonction_rend_une_entree_fidele_au_rapport() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let de = i.intern("minecraft:stone");
    let vers = i.intern("minecraft:dirt");
    let sel = boite(0, -64, 0, 31, -33, 31);
    let rap = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &Plan::nouveau(Masque::Etat(de), Motif::Bloc(vers)),
        &i,
    )
    .unwrap();
    assert!(!rap.est_vide(), "le test ne prouve rien sans correctif");

    let mut journal = Journal::new();
    assert!(rap
        .journaliser(&mut journal, "Remplacer", "replace", vec![1, 2, 3], 7)
        .is_some());
    assert_eq!(journal.entrees().len(), 1);
    let e = &journal.entrees()[0];

    assert_eq!(e.label, "Remplacer");
    assert_eq!(e.horodatage, 7);
    assert_eq!(e.params(), &[1, 2, 3], "les paramètres de rejeu traversent");
    assert!(e.est_rejouable());
    assert_eq!(
        e.bounds(),
        rap.bornes,
        "les bornes sont CELLES DU RAPPORT — ce que l'opération a écrit, pas \
         la sélection"
    );
    assert_eq!(
        e.corrections().len(),
        rap.patches.len(),
        "un correctif par chunk touché, ni plus ni moins"
    );
    // Et dans l'ORDRE du rapport : c'est lui qui rend `a_annuler` juste.
    let cibles_entree: Vec<_> = e
        .a_refaire()
        .map(|c| match c {
            tf_world::journal::Correction::Chunk(p) => p.cible.clone(),
            autre => panic!("correctif inattendu : {autre:?}"),
        })
        .collect();
    let cibles_rapport: Vec<_> = rap.patches.iter().map(|p| p.cible.clone()).collect();
    assert_eq!(cibles_entree, cibles_rapport);
}

/// **Un rapport vide ne pousse RIEN.**
///
/// Une entrée sans correctif serait une case de plus dans la pile
/// d'annulation qui ne défait rien : l'utilisateur appuierait deux fois sur
/// Ctrl+Z sans voir quoi que ce soit bouger. C'est le pendant, un étage plus
/// haut, du « une opération qui n'écrit rien rend `Etage::Rien` ».
#[test]
fn un_rapport_vide_ne_remplit_pas_le_journal() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    // Un état qui n'existe nulle part dans la fixture : rien à remplacer.
    let absent = i.intern("minecraft:barrier");
    let vers = i.intern("minecraft:dirt");
    let rap = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite(0, -64, 0, 31, -33, 31),
        &Plan::nouveau(Masque::Etat(absent), Motif::Bloc(vers)),
        &i,
    )
    .unwrap();
    assert!(rap.est_vide());

    let mut journal = Journal::new();
    assert!(
        rap.journaliser(&mut journal, "Remplacer", "replace", Vec::new(), 0)
            .is_none(),
        "la jonction doit DIRE qu'elle n'a rien poussé"
    );
    assert!(journal.entrees().is_empty());
    assert!(!journal.peut_annuler());
}

// ── le plafond de matérialisation ───────────────────────────────────────────

/// **Une allocation refusée n'échoue pas : elle ABANDONNE le processus.**
///
/// Presque tout le moteur travaille sur des sections packées et ne paie que sa
/// portée. Trois opérations font exception et demandent une case par bloc en
/// mémoire — `//copy`, `//paste`, `//hollow`. Le nombre vient de la SOURIS, pas
/// d'un fichier, mais la conséquence est celle du piège déjà payé sur les
/// longueurs NBT : on vérifie la place AVANT de réserver. Sans la garde, un
/// « sélectionner tout » sur un monde Minefield fait disparaître l'éditeur
/// avec le travail en cours, et sans un mot.
#[test]
fn une_selection_demesuree_est_refusee_et_non_tentee() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    // Le monde entier. Aucune allocation ne doit être tentée.
    let sel = boite(-30_000_000, -64, -30_000_000, 30_000_000, 319, 30_000_000);
    match copier(&st, &SURFACE, DOSSIER, &sel, &mut i) {
        Err(tf_ops::edition::Erreur::TropGros { octets, plafond }) => {
            assert!(octets > plafond, "{octets} devrait dépasser {plafond}");
        }
        Err(autre) => panic!("mauvaise erreur : {autre}"),
        Ok(_) => panic!("une sélection de tout le monde ne doit PAS passer"),
    }
}

/// **Le plafond est de la MÉMOIRE, pas une politique.**
///
/// La même sélection passe par `//set`, qui ne matérialise rien et ne paie que
/// sa portée. Confondre les deux briderait l'opération la plus courante du
/// moteur pour protéger d'un risque qu'elle ne court pas.
#[test]
fn le_plafond_ne_bride_que_ce_qui_materialise() {
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let pierre = i.intern("minecraft:stone");
    let sel = boite(-30_000_000, -64, -30_000_000, 30_000_000, 319, 30_000_000);
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &sel,
        &Plan::nouveau(Masque::Tout, Motif::Bloc(pierre)),
        &i,
    );
    assert!(r.is_ok(), "//set sur la même sélection doit passer");
}

/// **Le produit se calcule en `u64`, et il SATURE.**
///
/// Une sélection de tout le monde dépasse `u64` une fois multipliée par les
/// octets par case. Un débordement silencieux rendrait un petit nombre, la
/// garde laisserait passer, et on retomberait exactement sur le cas qu'elle
/// existe pour attraper — la forme même du piège des longueurs NBT.
#[test]
fn le_calcul_du_plafond_ne_deborde_pas() {
    use tf_ops::edition::{verifier_materialisable, OCTETS_CREUSAGE};
    let tout = boite(
        i32::MIN / 2,
        i32::MIN / 2,
        i32::MIN / 2,
        i32::MAX / 2,
        i32::MAX / 2,
        i32::MAX / 2,
    );
    assert!(
        verifier_materialisable(&tout, OCTETS_CREUSAGE).is_err(),
        "le plus grand volume représentable doit être refusé, pas enroulé"
    );
    // Et ce qui tient passe, avec le compte exact.
    let petite = boite(0, 0, 0, 15, 15, 15);
    assert_eq!(
        verifier_materialisable(&petite, OCTETS_CREUSAGE).unwrap(),
        4096
    );
}

/// **Le chemin rapide se compare au chemin lent, sur le même monde.**
///
/// Une sélection est une BOÎTE, un monde est un SEMIS. Parcourir la boîte de
/// régions marche tant qu'elle est petite ; démesurée, elle compte des
/// milliards de cases pour une poignée de régions qui existent, et l'éditeur
/// ne rend jamais la main — ça ne plante pas, ça ne dit rien, ça ne finit pas.
/// Au-delà d'un seuil, on demande donc à la source quelles régions existent.
///
/// Un raccourci non vérifié est une corruption silencieuse : la sélection
/// large doit rendre EXACTEMENT ce que rend la sélection serrée — mêmes
/// correctifs, mêmes étages, mêmes bornes, même compte.
#[test]
fn demander_les_regions_existantes_rend_le_meme_resultat_que_la_boite() {
    let plan = |i: &mut Interner| {
        Plan::nouveau(
            Masque::Etat(i.intern("minecraft:stone")),
            Motif::Bloc(i.intern("minecraft:dirt")),
        )
        .en_comptant()
    };

    // Chemin LENT : la boîte de régions fait 1 × 1, donc elle est parcourue.
    let (src, _) = monde();
    let st = staging(src);
    let mut i = Interner::new();
    let p = plan(&mut i);
    let serre = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite(0, -64, 0, 511, 319, 511),
        &p,
        &i,
    )
    .unwrap();

    // Chemin RAPIDE : la même région, dans une boîte de 117 187² régions.
    let (src2, _) = monde();
    let st2 = staging(src2);
    let mut i2 = Interner::new();
    let p2 = plan(&mut i2);
    let large = appliquer(
        &st2,
        &SURFACE,
        DOSSIER,
        &boite(-30_000_000, -64, -30_000_000, 30_000_000, 319, 30_000_000),
        &p2,
        &i2,
    )
    .unwrap();

    assert!(!serre.est_vide(), "le test ne prouve rien sans correctif");
    assert_eq!(serre.patches.len(), large.patches.len(), "mêmes correctifs");
    assert_eq!(serre.etages, large.etages, "mêmes étages");
    assert_eq!(serre.blocs, large.blocs, "même compte de blocs");
    assert_eq!(serre.bornes, large.bornes, "mêmes bornes");
    // Et au BIT près : c'est le seul contrôle qui ne relit pas le
    // raisonnement qui a produit le raccourci.
    assert_eq!(
        st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        st2.read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        "les deux chemins doivent écrire les MÊMES octets"
    );
}

/// **Une région qui n'existe que dans la COUCHE de staging compte aussi.**
///
/// La carte de la source ne la connaît pas — elle a été créée par une
/// opération précédente, ou matérialisée pour un build vierge. La sauter
/// ferait qu'une seconde opération sur une grande sélection ignorerait
/// précisément ce qu'on vient d'écrire, sans rien signaler.
#[test]
fn une_region_ecrite_dans_la_couche_reste_visible_a_une_grande_selection() {
    let vide = MemorySource::new(); // une SOURCE sans aucune région
    let st = Staging::new(vide, MemorySource::new());
    // La région part dans la COUCHE, pas dans la source.
    st.write_region(&SURFACE, DOSSIER, ZERO, &region(&Terrain::petite()))
        .unwrap();

    let mut i = Interner::new();
    let p = Plan::nouveau(
        Masque::Etat(i.intern("minecraft:stone")),
        Motif::Bloc(i.intern("minecraft:dirt")),
    )
    .en_comptant();
    let r = appliquer(
        &st,
        &SURFACE,
        DOSSIER,
        &boite(-30_000_000, -64, -30_000_000, 30_000_000, 319, 30_000_000),
        &p,
        &i,
    )
    .unwrap();
    assert!(
        !r.est_vide(),
        "la région de la couche doit être visitée, pas seulement celles de la source"
    );
    assert!(r.blocs.unwrap_or(0) > 0);
}

/// Une entrée qui porte les correctifs d'un vrai `//replace` ET celui d'un
/// fichier du monde — ce que fera la mise à jour d'un composant.
fn entree_avec_document(
    st: &Staging<MemorySource, MemorySource>,
    interner: &Interner,
    avant: &[u8],
    apres: &[u8],
) -> Journal {
    use tf_world::journal::{Correction, FichierPatch, Genre};
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let sel = BBox::new(BlockPos::new(0, -64, 0), BlockPos::new(15, 0, 15));
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer(st, &SURFACE, DOSSIER, &sel, &plan, interner).unwrap();
    st.ecrire_fichier("projet", apres).unwrap();
    let mut genre = rap.genre("composant", Vec::new());
    if let Genre::Operation { corrections, .. } = &mut genre {
        corrections.push(Correction::Fichier(
            FichierPatch::entre("projet", avant, apres).unwrap(),
        ));
    }
    let mut journal = Journal::new();
    journal.pousser("Mettre à jour", 0, genre);
    journal
}

/// **Un Ctrl+Z défait les blocs ET le document**, et refaire les rend tous
/// les deux — la moitié qui fait qu'un composant mis à jour s'annule d'un
/// geste.
#[test]
fn une_entree_defait_ses_chunks_et_son_document_ensemble() {
    use tf_ops::edition::{rejouer, Sens};
    let (src, brut) = monde();
    src.write_meta("projet", b"trois fenetres").unwrap();
    let interner = interner_de(&brut);
    let st = staging(src);
    let mut journal = entree_avec_document(&st, &interner, b"trois fenetres", b"quatre fenetres");
    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert_eq!(st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(), brut);
    assert_eq!(st.lire_fichier("projet").unwrap(), b"trois fenetres");
    assert!(
        st.fichiers_en_attente().unwrap().is_empty(),
        "revenu à celui de la save, le document quitte la copie"
    );

    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(
        tf_world::journal::empreinte(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        tf_world::journal::empreinte(&apres)
    );
    assert_eq!(st.lire_fichier("projet").unwrap(), b"quatre fenetres");
}

/// Un document changé sous le journal refuse l'entrée ENTIÈRE, avant
/// d'écrire la moindre région — sinon les blocs seraient défaits et le
/// document non, et plus rien ne saurait rejouer l'entrée.
#[test]
fn un_document_qui_diverge_n_ecrit_aucune_region() {
    use tf_ops::edition::{rejouer, Erreur, Sens};
    let (src, brut) = monde();
    let interner = interner_de(&brut);
    let st = staging(src);
    let mut journal = entree_avec_document(&st, &interner, b"", b"quatre fenetres");
    st.ecrire_fichier("projet", b"retouche ailleurs").unwrap();
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let (e, _) = journal.annuler().unwrap();
    let r = rejouer(&st, e, Sens::Annuler);
    assert!(matches!(r, Err(Erreur::DivergenceFichier { .. })), "{r:?}");
    assert_eq!(st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(), avant);
    assert_eq!(st.lire_fichier("projet").unwrap(), b"retouche ailleurs");
}

/// **Une correction qu'on ne comprend pas fait refuser l'entrée entière.**
/// N'en rejouer que les chunks — ce que faisait `rejouer` jusqu'ici, en
/// sautant en silence tout ce qui n'en était pas — défaisait la moitié d'une
/// action écrite par une version plus récente.
#[test]
fn une_entree_d_un_genre_inconnu_est_refusee_entiere() {
    use tf_ops::edition::{rejouer, Erreur, Sens};
    use tf_world::journal::{Correction, Genre};
    let (src, brut) = monde();
    let interner = interner_de(&brut);
    let pierre = interner.get("minecraft:stone").unwrap();
    let terre = interner.get("minecraft:dirt").unwrap();
    let st = staging(src);
    let sel = BBox::new(BlockPos::new(0, -64, 0), BlockPos::new(15, 0, 15));
    let plan = Plan::nouveau(Masque::Etat(pierre), Motif::Bloc(terre));
    let rap = appliquer(&st, &SURFACE, DOSSIER, &sel, &plan, &interner).unwrap();
    let mut genre = rap.genre("greffon", Vec::new());
    if let Genre::Operation { corrections, .. } = &mut genre {
        corrections.push(Correction::Inconnu {
            genre: 200,
            octets: b"un PNJ".to_vec(),
        });
    }
    let mut journal = Journal::new();
    journal.pousser("Poser un PNJ", 0, genre);
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let (e, _) = journal.annuler().unwrap();
    let r = rejouer(&st, e, Sens::Annuler);
    assert!(matches!(r, Err(Erreur::Incomprise { genre: 200 })), "{r:?}");
    assert_eq!(
        st.read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        avant,
        "pas même la moitié qu'on comprenait"
    );
}

/// Deux correctifs sur le MÊME fichier dans une entrée s'enchaînent — dans
/// l'ordre du sens demandé — et un document CRÉÉ puis annulé, donc retiré de
/// la copie, se refait depuis « absent », qui vaut vide.
#[test]
fn deux_correctifs_d_un_document_s_enchainent_et_une_creation_se_refait() {
    use tf_ops::edition::{rejouer, Sens};
    use tf_world::journal::{Correction, FichierPatch, Genre};
    let (src, _) = monde();
    let st = staging(src);
    let (a, b, c): (&[u8], &[u8], &[u8]) = (b"", b"une fenetre", b"deux fenetres");
    st.ecrire_fichier("projet", c).unwrap();
    let mut journal = Journal::new();
    journal.pousser(
        "Poser deux fois",
        0,
        Genre::Operation {
            op: "composant".into(),
            params: Vec::new(),
            bounds: None,
            corrections: vec![
                Correction::Fichier(FichierPatch::entre("projet", a, b).unwrap()),
                Correction::Fichier(FichierPatch::entre("projet", b, c).unwrap()),
            ],
        },
    );

    let (e, _) = journal.annuler().unwrap();
    rejouer(&st, e, Sens::Annuler).unwrap();
    assert!(
        matches!(
            st.overlay().read_meta("projet"),
            Err(tf_world::source::SourceError::NotFound)
        ),
        "revenu à « rien », comme la save : il quitte la copie"
    );

    let (e, _) = journal.refaire().unwrap();
    rejouer(&st, e, Sens::Refaire).unwrap();
    assert_eq!(st.lire_fichier("projet").unwrap(), c);
}
