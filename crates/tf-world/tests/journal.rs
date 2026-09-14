//! Le journal d'annulation.
//!
//! Ce qu'il faut prouver ici tient en trois phrases. Une annulation rend le
//! chunk d'origine OCTET POUR OCTET. Un journal coupé en plein vol se relit
//! jusqu'à la dernière action valide au lieu de tout perdre. Et une annulation
//! qui ne peut pas être juste est REFUSÉE plutôt qu'appliquée à côté.

use tf_anvil::{inverse_edits, splice, Edit};
use tf_nbt::Span;
use tf_world::journal::{
    decoder, empreinte, encoder, entete, Chemin, ChunkPatch, Cible, Correction, Genre, Journal,
    JournalError, Record,
};
use tf_world::{BBox, BlockPos, Dimension, Folder, RegionPos};

fn cible(chunk: u16) -> Cible {
    Cible {
        dim: Dimension::Overworld,
        folder: Folder::Region,
        region: RegionPos::new(0, 0),
        chunk,
    }
}

fn ed(start: usize, end: usize, bytes: &[u8]) -> Edit {
    Edit {
        span: Span { start, end },
        bytes: bytes.to_vec(),
    }
}

/// Un correctif fabriqué comme une opération le ferait vraiment.
fn patch(chunk: u16, avant: &[u8], edits: Vec<Edit>) -> (ChunkPatch, Vec<u8>) {
    let mut e = edits.clone();
    let apres = splice(avant, &mut e).unwrap();
    let p = ChunkPatch::record(cible(chunk), avant, &apres, &edits).unwrap();
    (p, apres)
}

fn operation(label: &str, corrections: Vec<Correction>) -> (String, Genre) {
    (
        label.to_string(),
        Genre::Operation {
            op: "replace".into(),
            bounds: Some(BBox::new(BlockPos::new(0, 0, 0), BlockPos::new(15, 15, 15))),
            corrections,
        },
    )
}

// ── le correctif ────────────────────────────────────────────────────────────

#[test]
fn annuler_rend_le_chunk_octet_pour_octet() {
    let avant = b"0123456789ABCDEF".to_vec();
    let (p, apres) = patch(
        0,
        &avant,
        vec![ed(2, 4, b"xx"), ed(6, 7, b"PLUS_LONG"), ed(10, 14, b"c")],
    );
    assert_ne!(apres, avant);

    assert_eq!(p.undo(&apres).unwrap(), avant);
    assert_eq!(
        p.redo(&avant).unwrap(),
        apres,
        "et refaire repose exactement les mêmes octets"
    );
}

#[test]
fn annuler_puis_refaire_dix_fois_ne_derive_pas() {
    let avant = b"la muraille de Minefield, pierre et profondardoise".to_vec();
    let (p, apres) = patch(0, &avant, vec![ed(14, 23, b"de Nostra")]);

    let mut courant = apres.clone();
    for tour in 0..10 {
        courant = p.undo(&courant).unwrap();
        assert_eq!(courant, avant, "tour {tour}");
        courant = p.redo(&courant).unwrap();
        assert_eq!(courant, apres, "tour {tour}");
    }
}

#[test]
fn un_chunk_qui_a_change_sous_le_journal_fait_refuser_l_annulation() {
    let avant = b"0123456789ABCDEF".to_vec();
    let (p, apres) = patch(7, &avant, vec![ed(2, 4, b"xx")]);

    let mut autre = apres.clone();
    autre[15] = b'?'; // quelqu'un est passé par là

    let e = p.undo(&autre).unwrap_err();
    match &e {
        JournalError::Divergence { cible: c, .. } => assert_eq!(c.chunk, 7),
        autre => panic!("attendu une divergence, reçu {autre:?}"),
    }
    assert!(
        format!("{e}").contains("a changé depuis cette action"),
        "et le message doit dire QUOI FAIRE : {e}"
    );
}

#[test]
fn refaire_sur_le_mauvais_etat_est_refuse_aussi() {
    let avant = b"0123456789ABCDEF".to_vec();
    let (p, apres) = patch(0, &avant, vec![ed(2, 4, b"xx")]);
    // Refaire attend l'état d'AVANT, pas celui d'après.
    assert!(matches!(
        p.redo(&apres),
        Err(JournalError::Divergence { .. })
    ));
}

#[test]
fn le_poids_d_un_correctif_est_celui_des_plages_pas_du_chunk() {
    // 64 ko de chunk, trois octets changés.
    let avant = vec![b'x'; 65_536];
    let (p, _) = patch(0, &avant, vec![ed(30_000, 30_003, b"abc")]);
    assert!(
        p.poids() < 100,
        "un correctif pèse ce que l'opération a écrit, pas ce qu'elle a \
         survolé : {} octets",
        p.poids()
    );
}

// ── la pile ─────────────────────────────────────────────────────────────────

fn journal_de_trois() -> Journal {
    let mut j = Journal::new();
    for (i, nom) in ["poser le sol", "monter le mur", "creuser la porte"]
        .iter()
        .enumerate()
    {
        let (label, genre) = operation(nom, Vec::new());
        j.pousser(&label, 1_000 + i as i64, genre);
    }
    j
}

#[test]
fn annuler_et_refaire_deplacent_le_curseur() {
    let mut j = journal_de_trois();
    assert_eq!(j.curseur(), 3);
    assert!(j.peut_annuler() && !j.peut_refaire());

    let (e, _) = j.annuler().unwrap();
    assert_eq!(e.label, "creuser la porte");
    assert_eq!(j.curseur(), 2);
    assert!(j.peut_refaire());

    let (e, _) = j.refaire().unwrap();
    assert_eq!(e.label, "creuser la porte");
    assert_eq!(j.curseur(), 3);
    assert!(j.refaire().is_none());
}

#[test]
fn agir_apres_une_annulation_abandonne_la_branche_defaite() {
    let mut j = journal_de_trois();
    j.annuler();
    j.annuler();
    assert_eq!(j.curseur(), 1);

    let (label, genre) = operation("poser un toit", Vec::new());
    let records = j.pousser(&label, 2_000, genre);

    assert_eq!(j.entrees().len(), 2);
    assert_eq!(j.entrees()[1].label, "poser un toit");
    assert!(!j.peut_refaire(), "il n'y a plus de branche à refaire");
    assert!(
        matches!(records[0], Record::Troncature(1)),
        "et le fichier doit le savoir, sans être réécrit : {records:?}"
    );
}

#[test]
fn les_identifiants_ne_sont_jamais_reutilises() {
    let mut j = journal_de_trois();
    j.annuler();
    j.annuler();
    let (label, genre) = operation("autre chose", Vec::new());
    j.pousser(&label, 2_000, genre);

    let ids: Vec<u64> = j.entrees().iter().map(|e| e.id).collect();
    assert_eq!(
        ids,
        vec![0, 3],
        "réutiliser un identifiant ferait pointer un point de reprise sur une \
         autre action que celle qu'il nommait"
    );
}

// ── points de reprise ───────────────────────────────────────────────────────

#[test]
fn un_point_de_reprise_se_traverse_sans_rien_defaire() {
    let mut j = Journal::new();
    let (l, g) = operation("poser le sol", Vec::new());
    j.pousser(&l, 1, g);
    j.reprise("avant la muraille", 2);
    let (l, g) = operation("monter le mur", Vec::new());
    j.pousser(&l, 3, g);

    let (e, _) = j.annuler().unwrap();
    assert_eq!(e.label, "monter le mur");
    let (e, _) = j.annuler().unwrap();
    assert_eq!(
        e.label, "poser le sol",
        "le repère ne défait rien : s'arrêter dessus obligerait à appuyer deux \
         fois sans que rien ne bouge"
    );
    assert!(j.annuler().is_none());
}

#[test]
fn les_points_de_reprise_se_listent_du_plus_recent_au_plus_ancien() {
    let mut j = Journal::new();
    j.reprise("départ", 1);
    let (l, g) = operation("travail", Vec::new());
    j.pousser(&l, 2, g);
    j.reprise("après le sol", 3);

    let noms: Vec<&str> = j.reprises().iter().map(|e| e.label.as_str()).collect();
    assert_eq!(noms, vec!["après le sol", "départ"]);
}

#[test]
fn revenir_a_un_point_de_reprise_se_dit_en_indices() {
    let mut j = Journal::new();
    let (l, g) = operation("a", Vec::new());
    j.pousser(&l, 1, g);
    let repere = j.reprise("repère", 2);
    let id_repere = match &repere[0] {
        Record::Entree(e) => e.id,
        autre => panic!("{autre:?}"),
    };
    for nom in ["b", "c"] {
        let (l, g) = operation(nom, Vec::new());
        j.pousser(&l, 3, g);
    }

    // Revenir en arrière : défaire « c » puis « b ».
    match j.chemin_vers(id_repere).unwrap() {
        Chemin::Annuler(v) => assert_eq!(v, vec![3, 2]),
        autre => panic!("{autre:?}"),
    }

    // Une fois revenu, y aller à nouveau ne demande rien.
    j.poser_curseur(2);
    assert_eq!(j.chemin_vers(id_repere).unwrap(), Chemin::Rien);

    // Et repartir en avant : refaire « b » puis « c ».
    let dernier = j.entrees().last().unwrap().id;
    match j.chemin_vers(dernier).unwrap() {
        Chemin::Refaire(v) => assert_eq!(v, vec![2, 3]),
        autre => panic!("{autre:?}"),
    }
}

#[test]
fn un_identifiant_inconnu_n_a_pas_de_chemin() {
    assert!(journal_de_trois().chemin_vers(999).is_none());
}

// ── le fichier ──────────────────────────────────────────────────────────────

/// Écrit un journal complet comme l'hôte le ferait : en-tête puis ajouts.
fn fichier(records: &[Record]) -> Vec<u8> {
    let mut f = entete();
    for r in records {
        f.extend_from_slice(&encoder(r));
    }
    f
}

#[test]
fn un_journal_se_relit_a_l_identique() {
    let avant = vec![7u8; 4096];
    let (p, _) = patch(42, &avant, vec![ed(100, 200, &[9u8; 60])]);

    let mut j = Journal::new();
    let mut records = Vec::new();
    let (l, g) = operation("remplacer la pierre", vec![Correction::Chunk(p)]);
    records.extend(j.pousser(&l, 1_700_000_000_000, g));
    records.extend(j.reprise("avant la muraille", 1_700_000_001_000));
    let (l, g) = operation("monter le mur", Vec::new());
    records.extend(j.pousser(&l, 1_700_000_002_000, g));
    if let Some((_, r)) = j.annuler() {
        records.push(r);
    }

    let (relu, lus) = decoder(&fichier(&records)).unwrap();
    assert_eq!(lus, fichier(&records).len(), "tout le fichier est valide");
    assert_eq!(relu.entrees(), j.entrees());
    assert_eq!(
        relu.curseur(),
        j.curseur(),
        "le curseur aussi : sans lui, rouvrir un projet REFERAIT ce qu'on \
         venait d'annuler"
    );
}

#[test]
fn une_troncature_se_rejoue_a_la_relecture() {
    let mut j = Journal::new();
    let mut recs = Vec::new();
    for nom in ["poser le sol", "monter le mur", "creuser la porte"] {
        let (l, g) = operation(nom, Vec::new());
        recs.extend(j.pousser(&l, 1, g));
    }
    if let Some((_, r)) = j.annuler() {
        recs.push(r);
    }
    if let Some((_, r)) = j.annuler() {
        recs.push(r);
    }
    let (l, g) = operation("poser un toit", Vec::new());
    recs.extend(j.pousser(&l, 2, g));

    let (relu, _) = decoder(&fichier(&recs)).unwrap();
    assert_eq!(
        relu.entrees().len(),
        2,
        "le fichier n'est jamais réécrit : c'est la TRONCATURE ajoutée à la \
         fin qui dit que la branche défaite a été abandonnée"
    );
    assert_eq!(relu.entrees()[1].label, "poser un toit");
    assert_eq!(relu.curseur(), 2);
    assert_eq!(relu.entrees(), j.entrees());
}

#[test]
fn un_journal_coupe_en_plein_vol_se_relit_jusqu_a_la_derniere_action_valide() {
    let mut j = Journal::new();
    let mut recs = Vec::new();
    for nom in ["a", "b", "c"] {
        let (l, g) = operation(nom, Vec::new());
        recs.extend(j.pousser(&l, 1, g));
    }
    let complet = fichier(&recs);
    let deux = fichier(&recs[..2]);

    // Coupé au milieu du troisième enregistrement.
    let coupe = &complet[..deux.len() + 5];
    let (relu, valides) = decoder(coupe).unwrap();
    assert_eq!(
        relu.entrees().len(),
        2,
        "perdre la dernière action vaut mieux que perdre l'historique"
    );
    assert_eq!(
        valides,
        deux.len(),
        "et l'hôte sait où tronquer le fichier avant de reprendre ses ajouts"
    );
}

#[test]
fn un_enregistrement_abime_arrete_la_lecture_sans_la_faire_echouer() {
    let mut j = Journal::new();
    let mut recs = Vec::new();
    for nom in ["a", "b"] {
        let (l, g) = operation(nom, Vec::new());
        recs.extend(j.pousser(&l, 1, g));
    }
    let mut f = fichier(&recs);
    let un = fichier(&recs[..1]).len();
    // Un octet retourné dans le corps du second : l'empreinte ne colle plus.
    let dernier = f.len() - 1;
    f[dernier] ^= 0xff;

    let (relu, valides) = decoder(&f).unwrap();
    assert_eq!(relu.entrees().len(), 1);
    assert_eq!(valides, un);
}

#[test]
fn ce_qui_n_est_pas_un_journal_est_refuse_des_le_premier_octet() {
    assert_eq!(decoder(b"").unwrap_err(), JournalError::PasUnJournal);
    assert_eq!(decoder(b"TFJ").unwrap_err(), JournalError::PasUnJournal);
    assert_eq!(
        decoder(b"PK\x03\x04pas un journal").unwrap_err(),
        JournalError::PasUnJournal,
        "un format binaire se reconnaît à ses OCTETS, jamais à son nom"
    );
    // Un en-tête seul est un journal vide, pas une erreur.
    let (j, n) = decoder(&entete()).unwrap();
    assert_eq!(j.entrees().len(), 0);
    assert_eq!(n, 4);
}

#[test]
fn une_longueur_delirante_ne_fait_pas_allouer() {
    let mut f = entete();
    f.extend_from_slice(&u32::MAX.to_le_bytes()); // longueur
    f.extend_from_slice(&0u64.to_le_bytes()); // empreinte
    f.push(0); // codec
    f.extend_from_slice(b"quatre");

    let (j, n) = decoder(&f).unwrap();
    assert_eq!(j.entrees().len(), 0);
    assert_eq!(n, 4, "on s'arrête, on n'essaie pas de réserver 4 Go");
}

#[test]
fn un_correctif_d_un_genre_inconnu_est_conserve_tel_quel() {
    // Ce qu'écrirait une version qui en sait plus : on ne comprend pas, mais on
    // ne perd ni l'entrée ni celles d'à côté.
    let mut j = Journal::new();
    let (l, g) = operation(
        "poser un PNJ",
        vec![Correction::Inconnu {
            genre: 200,
            octets: b"des donnees de greffon".to_vec(),
        }],
    );
    let recs = j.pousser(&l, 1, g);

    let (relu, _) = decoder(&fichier(&recs)).unwrap();
    assert_eq!(relu.entrees().len(), 1);
    match &relu.entrees()[0].genre {
        Genre::Operation { corrections, .. } => assert_eq!(
            corrections[0],
            Correction::Inconnu {
                genre: 200,
                octets: b"des donnees de greffon".to_vec()
            }
        ),
        autre => panic!("{autre:?}"),
    }
}

#[test]
fn une_dimension_ajoutee_traverse_le_fichier() {
    let avant = vec![1u8; 64];
    let mut p = ChunkPatch::record(
        cible(3),
        &avant,
        &splice(&avant, &mut [ed(0, 1, b"Z")]).unwrap(),
        &[ed(0, 1, b"Z")],
    )
    .unwrap();
    p.cible.dim = Dimension::Custom {
        namespace: "aether".into(),
        path: "the_aether".into(),
    };
    p.cible.folder = Folder::Entities;
    p.cible.region = RegionPos::new(-3, 17);

    let mut j = Journal::new();
    let (l, g) = operation("x", vec![Correction::Chunk(p.clone())]);
    let recs = j.pousser(&l, 1, g);

    let (relu, _) = decoder(&fichier(&recs)).unwrap();
    match &relu.entrees()[0].genre {
        Genre::Operation { corrections, .. } => {
            assert_eq!(corrections[0], Correction::Chunk(p));
        }
        autre => panic!("{autre:?}"),
    }
}

#[test]
fn un_gros_correctif_se_compresse_et_un_petit_non() {
    // Les indices packés d'un `//set` : très compressibles.
    let avant = vec![0u8; 64 * 1024];
    let (gros, _) = patch(0, &avant, vec![ed(0, 40_000, &vec![3u8; 40_000])]);
    let mut j = Journal::new();
    let (l, g) = operation("remplir", vec![Correction::Chunk(gros)]);
    let octets = encoder(&j.pousser(&l, 1, g)[0]);
    assert!(
        octets.len() < 5_000,
        "80 ko d'éditions répétitives doivent tenir dans quelques kilooctets : \
         {} octets",
        octets.len()
    );

    let petit = encoder(&Record::Curseur(3));
    assert!(
        petit.len() < 32,
        "et un curseur ne doit pas se faire GROSSIR par un en-tête zlib : {} \
         octets",
        petit.len()
    );
}

#[test]
fn le_journal_relu_reste_utilisable_pour_annuler() {
    // Le bout à bout : opérer, écrire le journal, le relire, annuler.
    let avant = b"le mur de Nostra, en pierre taillee".to_vec();
    let edits = vec![ed(10, 16, b"Minefield")];
    let (p, apres) = patch(5, &avant, edits);

    let mut j = Journal::new();
    let (l, g) = operation("renommer", vec![Correction::Chunk(p)]);
    let recs = j.pousser(&l, 1, g);

    let (mut relu, _) = decoder(&fichier(&recs)).unwrap();
    let (entree, _) = relu.annuler().unwrap();
    let Genre::Operation { corrections, .. } = &entree.genre else {
        panic!("pas une opération");
    };
    let Correction::Chunk(p) = &corrections[0] else {
        panic!("pas un correctif de chunk");
    };
    assert_eq!(
        p.undo(&apres).unwrap(),
        avant,
        "un journal relu depuis le disque doit défaire exactement ce que le \
         journal en mémoire défaisait"
    );
}

#[test]
fn les_regions_a_relire_se_lisent_sur_l_entree() {
    let avant = vec![0u8; 32];
    let e = ed(0, 1, b"Z");
    let mk = |c: u16, r: RegionPos| {
        let mut p = ChunkPatch::record(
            cible(c),
            &avant,
            &splice(&avant, &mut [e.clone()]).unwrap(),
            std::slice::from_ref(&e),
        )
        .unwrap();
        p.cible.region = r;
        Correction::Chunk(p)
    };
    let mut j = Journal::new();
    let (l, g) = operation(
        "x",
        vec![
            mk(0, RegionPos::new(0, 0)),
            mk(1, RegionPos::new(0, 0)),
            mk(2, RegionPos::new(1, 0)),
        ],
    );
    j.pousser(&l, 1, g);

    let regions = j.entrees()[0].regions();
    assert_eq!(
        regions.len(),
        2,
        "deux régions, pas trois chunks : c'est le fichier qu'il faut relire"
    );
}

// ── l'empreinte ─────────────────────────────────────────────────────────────

#[test]
fn l_empreinte_distingue_ce_qu_il_faut() {
    assert_eq!(empreinte(b""), empreinte(b""));
    assert_ne!(empreinte(b"abc"), empreinte(b"abd"));
    assert_ne!(
        empreinte(b"ab"),
        empreinte(b"ba"),
        "l'ordre compte : deux chunks aux mêmes octets mélangés ne sont pas le \
         même chunk"
    );
    assert_ne!(empreinte(b"a"), empreinte(b"a\0"));
}

#[test]
fn l_inverse_et_le_correctif_disent_la_meme_chose() {
    // `ChunkPatch::record` ne doit pas réinventer l'inverse dans son coin.
    let avant = b"0123456789".to_vec();
    let edits = vec![ed(2, 5, b"ZZ")];
    let (p, _) = patch(0, &avant, edits.clone());
    assert_eq!(p.annuler, inverse_edits(&avant, &edits).unwrap());
    assert_eq!(p.refaire, edits);
}

// ── la chaîne complète, sur un vrai fichier de région ───────────────────────

/// Deux moitiés testées ne prouvent rien sur leur jonction. Ici la chaîne
/// entière est bouclée : lire un `.mca`, opérer, enregistrer le correctif,
/// écrire le fichier, relire le journal depuis ses octets, annuler — et
/// comparer au fichier de départ.
#[test]
fn operer_sur_une_vraie_region_puis_annuler_la_rend_octet_pour_octet() {
    use std::borrow::Cow;
    use tf_anvil::{decode_section, deflate, inflate, read, scan, section_edits, write, Interner};
    use tf_bench::{region, Terrain};

    let t = Terrain::petite();
    let origine = region(&t);

    // ── 1. opérer : //replace pierre → profondardoise, sur toute la région
    let mut region_ecrite = read(&origine, 0, 0).unwrap();
    let mut corrections = Vec::new();
    let mut charges = Vec::new();

    for lz in 0..32i32 {
        for lx in 0..32i32 {
            let Some(brut) = region_ecrite.get(lx, lz) else {
                continue;
            };
            let compression = brut.compression;
            let avant = inflate(&brut.payload, compression).unwrap();
            let sc = scan(&avant).unwrap();
            let mut interner = Interner::new();
            let mut edits = Vec::new();
            for s in &sc.sections {
                let Some(mut sec) = decode_section(&avant, &sc, s, &mut interner).unwrap() else {
                    continue;
                };
                let Some(de) = interner.get("minecraft:stone") else {
                    continue;
                };
                let vers = interner.intern("minecraft:deepslate");
                if sec.replace_state(de, vers) == 0 {
                    continue;
                }
                edits.extend(section_edits(&avant, &sec, s, &interner).unwrap());
            }
            if edits.is_empty() {
                continue;
            }
            let mut e = edits.clone();
            let apres = tf_anvil::splice(&avant, &mut e).unwrap();
            corrections.push(Correction::Chunk(
                ChunkPatch::record(
                    Cible {
                        dim: Dimension::Overworld,
                        folder: Folder::Region,
                        region: RegionPos::new(0, 0),
                        chunk: (lz as u16 & 31) * 32 + (lx as u16 & 31),
                    },
                    &avant,
                    &apres,
                    &edits,
                )
                .unwrap(),
            ));
            charges.push((lx, lz, compression, apres));
        }
    }
    assert!(!corrections.is_empty(), "l'opération doit avoir mordu");

    for (lx, lz, compression, apres) in &charges {
        region_ecrite.get_mut(*lx, *lz).unwrap().payload =
            Cow::Owned(deflate(apres, *compression).unwrap());
    }
    let modifiee = write(&region_ecrite).unwrap().region;
    assert_ne!(modifiee, origine);

    // ── 2. le journal part sur le disque et en revient
    let mut j = Journal::new();
    let recs = j.pousser(
        "Remplacer pierre → profondardoise",
        1_700_000_000_000,
        Genre::Operation {
            op: "replace".into(),
            bounds: None,
            corrections,
        },
    );
    let sur_disque = fichier(&recs);
    let (mut relu, valides) = decoder(&sur_disque).unwrap();
    assert_eq!(valides, sur_disque.len());

    // Comparaison DURE : le `.mca` est compressé, le journal ne l'est qu'une
    // fois écrit. Face aux octets inflatés, le rapport est encore dix fois
    // meilleur.
    let poids = relu.poids();
    assert!(
        poids * 10 < origine.len(),
        "le journal d'une opération de palette doit peser une fraction de la \
         région : {poids} octets contre {} pour le `.mca` COMPRESSÉ",
        origine.len()
    );

    // ── 3. annuler, en repassant par les octets du fichier
    let (entree, _) = relu.annuler().unwrap();
    let Genre::Operation { corrections, .. } = &entree.genre else {
        panic!("pas une opération");
    };
    let mut region_annulee = read(&modifiee, 0, 0).unwrap();
    for c in corrections {
        let Correction::Chunk(p) = c else {
            panic!("pas un correctif de chunk")
        };
        let lx = (p.cible.chunk % 32) as i32;
        let lz = (p.cible.chunk / 32) as i32;
        let brut = region_annulee.get(lx, lz).unwrap();
        let compression = brut.compression;
        let courant = inflate(&brut.payload, compression).unwrap();
        let rendu = p.undo(&courant).unwrap();
        region_annulee.get_mut(lx, lz).unwrap().payload =
            Cow::Owned(deflate(&rendu, compression).unwrap());
    }
    let rendue = write(&region_annulee).unwrap().region;

    assert_eq!(
        rendue.len(),
        origine.len(),
        "la région rendue doit faire la taille de l'originale"
    );
    assert_eq!(
        rendue, origine,
        "et être la MÊME, octet pour octet — c'est la seule preuve qui compte \
         pour la save d'un utilisateur"
    );
}

// ── tenir dans un budget ────────────────────────────────────────────────────

/// Une entrée qui pèse à peu près `n` octets.
fn lourde(j: &mut Journal, nom: &str, n: usize) {
    let avant = vec![0u8; n * 2 + 16];
    let (p, _) = patch(0, &avant, vec![ed(0, n, &vec![1u8; n])]);
    j.pousser(
        nom,
        1,
        Genre::Operation {
            op: "set".into(),
            bounds: None,
            corrections: vec![Correction::Chunk(p)],
        },
    );
}

#[test]
fn elaguer_oublie_les_plus_vieilles_entrees() {
    let mut j = Journal::new();
    for nom in ["a", "b", "c", "d"] {
        lourde(&mut j, nom, 1_000);
    }
    assert!(j.poids() > 8_000);

    let oubliees = j.elaguer(5_000);

    assert!(oubliees >= 1);
    assert!(j.poids() <= 5_000, "{} octets", j.poids());
    assert_eq!(
        j.entrees().last().unwrap().label,
        "d",
        "on élague par le VIEUX bout : oublier le récent perdrait ce que \
         l'utilisateur vient de faire"
    );
    assert_eq!(
        j.curseur(),
        j.entrees().len(),
        "et le curseur suit, sinon il désignerait une autre action"
    );
}

#[test]
fn elaguer_ne_touche_pas_a_ce_qui_attend_d_etre_refait() {
    let mut j = Journal::new();
    for nom in ["a", "b", "c"] {
        lourde(&mut j, nom, 1_000);
    }
    j.annuler();
    j.annuler();
    assert_eq!(j.curseur(), 1);

    j.elaguer(0);

    assert_eq!(
        j.entrees().len(),
        2,
        "« b » et « c » sont annulés : les oublier perdrait du travail que \
         l'utilisateur peut encore vouloir refaire"
    );
    assert_eq!(j.curseur(), 0);
    assert!(j.peut_refaire());
}

#[test]
fn elaguer_garde_les_points_de_reprise() {
    let mut j = Journal::new();
    lourde(&mut j, "a", 2_000);
    j.reprise("repère", 2);
    lourde(&mut j, "b", 2_000);

    j.elaguer(2_500);

    let noms: Vec<&str> = j.entrees().iter().map(|e| e.label.as_str()).collect();
    assert!(
        noms.contains(&"repère"),
        "un repère ne pèse rien et reste atteignable tant que ce qui le SUIT \
         est intact : {noms:?}"
    );
}

#[test]
fn un_journal_compacte_se_relit_a_l_identique() {
    let mut j = Journal::new();
    for nom in ["a", "b", "c"] {
        lourde(&mut j, nom, 200);
    }
    j.annuler();
    j.elaguer(300);

    let (relu, _) = decoder(&fichier(&j.reecrire())).unwrap();
    assert_eq!(relu.entrees(), j.entrees());
    assert_eq!(relu.curseur(), j.curseur());
}
