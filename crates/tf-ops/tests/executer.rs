//! **La boucle complète : nommer, exécuter, journaliser, annuler, refaire.**
//!
//! `catalogue.rs` dit ce qu'une opération prend, `executer.rs` dit comment on
//! la fait, `rejouer` la défait. Chacun est testé de son côté ; ce fichier
//! teste ce qu'aucun des trois ne peut prouver seul — que la chaîne tient
//! d'un bout à l'autre, sur des OCTETS et pas sur des comptes.
//!
//! C'est le prolongement direct de `edition.rs`, un cran plus haut : là-bas on
//! part d'un `Plan` écrit à la main, ici on part d'un NOM et de paramètres,
//! comme un hôte le fera.

use tf_anvil::Interner;
use tf_anvil::{chunk::scan, codec::inflate, region::read};
use tf_bench::{region, Terrain};
use tf_ops::catalogue::{construire, Params, Valeur, OPS};
use tf_ops::edition::{rejouer, Erreur, Sens};
use tf_ops::executer::{executer, Options};
use tf_world::coords::{BBox, BlockPos, RegionPos};
use tf_world::journal::Journal;
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

/// Une sélection modeste et alignée, qui tient dans la région.
fn sel() -> BBox {
    BBox::new(BlockPos::new(0, -48, 0), BlockPos::new(31, -33, 31))
}

/// Le contenu d'une région, section par section — la vérité qu'on compare.
/// Pas un résumé, pas un compte : les octets inflatés.
fn contenu(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let r = read(bytes, 0, 0).unwrap();
    let mut out = Vec::new();
    for c in r.iter() {
        let brut = inflate(&c.payload, c.compression).unwrap();
        let _ = scan(&brut);
        out.push((c.index, brut));
    }
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Des paramètres complets pour une opération : ses défauts, plus de quoi
/// satisfaire ce qui est obligatoire. Déduit de la SAISIE, donc une opération
/// ajoutée demain est couverte sans toucher à ce fichier.
fn remplir(d: &tf_ops::catalogue::Descripteur) -> Params {
    use tf_ops::catalogue::Saisie;
    let mut p = d.defauts();
    for decl in d.params {
        if decl.defaut.is_none() {
            p.poser(
                decl.nom,
                match decl.saisie {
                    Saisie::Bloc => Valeur::texte("minecraft:stone"),
                    Saisie::Biome => Valeur::texte("minecraft:plains"),
                    Saisie::Melange => Valeur::Melange(vec![(1, "minecraft:dirt".into())]),
                    Saisie::Entier { min, .. } => Valeur::Entier(min),
                    Saisie::Vecteur => Valeur::Vecteur([0, 0, 0]),
                    Saisie::Direction => Valeur::Direction(tf_world::selection::Direction::PlusX),
                    Saisie::Transformation => Valeur::Transformation(None),
                },
            );
        }
    }
    p
}

/// **Déclaré, décrit, construit — et inexécutable.** La forme la plus tardive
/// du piège : tout se compile, le formulaire s'affiche, et le bouton lève une
/// erreur. On l'exige donc au bout de la chaîne, pas au milieu.
#[test]
fn chaque_operation_du_catalogue_s_execute() {
    for d in OPS {
        let (src, _) = monde();
        let st = staging(src);
        let mut interner = Interner::new();
        let travail = construire(d.id, &remplir(d), &mut interner)
            .unwrap_or_else(|e| panic!("« {} » ne se construit pas : {e}", d.id));
        let r = executer(
            &travail,
            &st,
            &SURFACE,
            DOSSIER,
            &sel(),
            &mut interner,
            &Options::default(),
        );
        match r {
            Ok(_) => {}
            Err(e) => panic!("« {} » ne s'exécute pas : {e}", d.id),
        }
    }
}

/// **Invariant n° 1 : on ne touche jamais au fichier source.** Vérifié ici à
/// travers l'exécuteur, parce que c'est le chemin que les hôtes prendront.
#[test]
fn la_source_reste_intacte() {
    let (src, avant) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let mut p = Params::new();
    p.poser("bloc", Valeur::texte("minecraft:dirt"));
    let t = construire("poser", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert!(!cr.rapport.patches.is_empty(), "l'opération doit écrire");
    // La SOURCE, à travers le staging : `Staging` lit la copie de travail si
    // elle existe, la source sinon — c'est donc la source elle-même qu'on
    // interroge, et elle doit être octet pour octet celle du départ.
    assert_eq!(
        st.source().read_region(&SURFACE, DOSSIER, ZERO).unwrap(),
        avant,
        "la source a été touchée"
    );
}

/// **La boucle entière, sur des OCTETS.** Un round-trip qui rendrait les mêmes
/// blocs dans un fichier réécrit autrement aurait perdu tout ce qu'on n'a pas
/// compris du format.
#[test]
fn appliquer_puis_annuler_rend_le_monde_octet_pour_octet() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let mut p = Params::new();
    p.poser("de", Valeur::texte("minecraft:stone"));
    p.poser("vers", Valeur::texte("minecraft:dirt"));
    let t = construire("//replace", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert!(!cr.rapport.patches.is_empty());
    let apres = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();
    assert_ne!(contenu(&apres), contenu(&avant), "rien n'a changé");

    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Remplacer", "remplacer", Vec::new(), 0));

    let (entree, _) = journal.annuler().expect("il y a de quoi annuler");
    let n = rejouer(&st, entree, Sens::Annuler).unwrap();
    assert!(n > 0, "l'annulation doit toucher des chunks");
    assert_eq!(
        contenu(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        contenu(&avant),
        "annuler doit rendre EXACTEMENT le monde d'avant"
    );

    let (entree, _) = journal.refaire().expect("il y a de quoi refaire");
    rejouer(&st, entree, Sens::Refaire).unwrap();
    assert_eq!(
        contenu(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        contenu(&apres),
        "refaire doit rendre EXACTEMENT le monde d'après"
    );
}

/// **Un correctif est gardé par l'empreinte de l'état qu'il attend.** Sans
/// elle, rejouer une annulation sur un chunk modifié depuis produirait un
/// mélange des deux — plausible, et faux. Et le refus est TOTAL : rien n'est
/// écrit, pas une moitié d'annulation.
#[test]
fn un_correctif_qui_ne_colle_plus_est_refuse_sans_rien_ecrire() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();

    let mut p = Params::new();
    p.poser("bloc", Valeur::texte("minecraft:dirt"));
    let t = construire("poser", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Remplir", "poser", Vec::new(), 0));

    // Une SECONDE opération passe par-dessus : le chunk n'est plus celui que
    // le premier correctif attend.
    let mut p2 = Params::new();
    p2.poser("bloc", Valeur::texte("minecraft:cobblestone"));
    let t2 = construire("poser", &p2, &mut interner).unwrap();
    executer(
        &t2,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    let etat = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let (entree, _) = journal.annuler().unwrap();
    match rejouer(&st, entree, Sens::Annuler) {
        Err(Erreur::Divergence { .. }) => {}
        autre => panic!("la divergence doit être refusée : {autre:?}"),
    }
    assert_eq!(
        contenu(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        contenu(&etat),
        "un refus ne doit rien écrire du tout"
    );
}

/// **Creuser POSE de l'air.** Sans `avec_air`, le collage sauterait exactement
/// les cases qu'on vient de calculer : une opération qui s'exécute, se
/// rapporte, et ne fait rien.
#[test]
fn creuser_pose_de_l_air() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    // Un bloc de pierre plein, pour avoir quelque chose à vider.
    let mut p = Params::new();
    p.poser("bloc", Valeur::texte("minecraft:stone"));
    let t = construire("poser", &p, &mut interner).unwrap();
    executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    let plein = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let mut p = Params::new();
    p.poser("epaisseur", Valeur::Entier(1));
    let t = construire("//hollow", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert!(
        !cr.rapport.patches.is_empty(),
        "creuser un bloc plein doit écrire"
    );
    assert_ne!(
        contenu(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        contenu(&plein),
        "le creusage n'a rien changé"
    );
    assert!(cr.cases_materialisees > 0, "le coût doit être rapporté");
    let _ = avant;
}

/// **Le pas d'un `//stack` est la TAILLE de la sélection**, pas un bloc. C'est
/// ce que fait WorldEdit, et c'est ce qu'on veut neuf fois sur dix — un mur
/// qu'on prolonge.
#[test]
fn empiler_avance_de_la_taille_de_la_selection() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let mut p = Params::new();
    p.poser("fois", Valeur::Entier(2));
    p.poser(
        "direction",
        Valeur::Direction(tf_world::selection::Direction::PlusX),
    );
    let t = construire("//stack", &p, &mut interner).unwrap();
    let s = sel();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &s,
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    let (sx, _, _) = s.size();
    assert_eq!(cr.pas, Some([sx as i32, 0, 0]));
}

/// **Sans pack, les cases bougent et les orientations non.** Le taire
/// produirait un build à moitié tourné, et rien à l'écran pour le dire.
#[test]
fn une_rotation_sans_regle_le_dit() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let mut p = Params::new();
    p.poser("decalage", Valeur::Vecteur([64, 0, 0]));
    p.poser(
        "transformation",
        Valeur::Transformation(Some(tf_blocks::Transfo::Rot90)),
    );
    let t = construire("copier-vers", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert!(cr.sans_regle, "l'absence de règle doit être rapportée");
    assert!(
        !cr.intacts.is_empty(),
        "sans règle, TOUS les états sont intacts"
    );
    assert!(cr.extrait.is_some());
}

/// **Une allocation refusée ABANDONNE le processus.** Le refus doit donc
/// arriver AVANT de réserver, pas après — c'est la règle des longueurs NBT,
/// avec un nombre qui vient de la souris au lieu d'un fichier.
#[test]
fn un_creusage_demesure_est_refuse_avant_d_allouer() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let mut p = Params::new();
    p.poser("epaisseur", Valeur::Entier(1));
    let t = construire("creuser", &p, &mut interner).unwrap();
    let enorme = BBox::new(BlockPos::new(0, -64, 0), BlockPos::new(4095, 319, 4095));
    match executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &enorme,
        &mut interner,
        &Options::default(),
    ) {
        Err(Erreur::TropGros { octets, plafond }) => assert!(octets > plafond),
        autre => panic!("une sélection démesurée doit être refusée : {autre:?}"),
    }
}

/// Compter coûte × 21 à l'étage palette : c'est un choix de l'appelant, jamais
/// un service rendu d'office. Mais quand on le demande, le chiffre doit venir.
#[test]
fn compter_est_une_option_et_le_compte_arrive() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let mut p = Params::new();
    p.poser("bloc", Valeur::texte("minecraft:dirt"));
    let t = construire("poser", &p, &mut interner).unwrap();

    let sans = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(sans.rapport.blocs, None, "compter n'est pas rendu d'office");

    let (src2, _) = monde();
    let st2 = staging(src2);
    let avec = executer(
        &t,
        &st2,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options {
            compter: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(avec.rapport.blocs.is_some());
}

/// **La lecture d'un lissage DÉBORDE de la sélection, du rayon du noyau.**
/// Sans cette marge, le bord se moyennerait contre des colonnes qu'on n'a pas
/// lues — donc contre du vide — et s'effondrerait. Le compte de colonnes
/// relevées est ce qui le rend observable.
#[test]
fn lisser_lit_plus_large_que_la_selection() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let s = sel();
    let (sx, _, sz) = s.size();

    let mut p = Params::new();
    p.poser("rayon", Valeur::Entier(3));
    let t = construire("//smooth", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &s,
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    let attendu = (sx as usize + 6) * (sz as usize + 6);
    assert_eq!(
        cr.colonnes_relevees, attendu,
        "la marge doit valoir le rayon, des deux côtés de chaque axe"
    );
    assert!(cr.colonnes_relevees > (sx as usize * sz as usize));
}

/// **Une opération ne paie que sa PORTÉE**, et une forme la resserre avant
/// qu'un seul chunk ne soit lu. Les bornes rapportées doivent donc décrire ce
/// que la FORME a écrit, pas la sélection.
#[test]
fn une_forme_resserre_ce_que_l_operation_ecrit() {
    use tf_ops::Forme;
    let s = sel();
    let centre = [
        (s.min.x + s.max.x).div_euclid(2),
        (s.min.y + s.max.y).div_euclid(2),
        (s.min.z + s.max.z).div_euclid(2),
    ];

    let mut bornes = Vec::new();
    for forme in [
        Forme::Boite,
        Forme::Ellipsoide {
            centre,
            rayons: [4.0, 4.0, 4.0],
        },
    ] {
        let (src, _) = monde();
        let st = staging(src);
        let mut interner = Interner::new();
        let mut p = Params::new();
        p.poser("bloc", Valeur::texte("minecraft:dirt"));
        let t = construire("poser", &p, &mut interner).unwrap();
        let cr = executer(
            &t,
            &st,
            &SURFACE,
            DOSSIER,
            &s,
            &mut interner,
            &Options {
                forme,
                ..Options::default()
            },
        )
        .unwrap();
        bornes.push(cr.rapport.bornes.expect("l'opération a écrit"));
    }
    let (boite, sphere) = (bornes[0], bornes[1]);
    let vol = |b: BBox| {
        let (x, y, z) = b.size();
        x as u64 * y as u64 * z as u64
    };
    assert!(
        vol(sphere) < vol(boite),
        "la sphère doit écrire moins que la boîte : {} contre {}",
        vol(sphere),
        vol(boite)
    );
}

/// **Les correctifs d'une entrée s'annulent À L'ENVERS**, et rien d'autre ne
/// peut le montrer.
///
/// Tant qu'une opération ne touche chaque chunk qu'une fois, l'ordre n'a
/// aucune importance : `a_annuler` et `a_refaire` rendent le même ensemble, et
/// un test écrit avec l'un passe avec l'autre. `//move` avec une source et une
/// destination qui SE CHEVAUCHENT repasse sur les mêmes chunks — le second
/// correctif échoue alors sur `Divergence` si on les rejoue dans le sens
/// d'enregistrement. C'est le seul motif qui sépare les deux sens.
#[test]
fn deplacer_sur_lui_meme_s_annule_dans_le_bon_ordre() {
    let (src, _) = monde();
    let st = staging(src);
    let mut interner = Interner::new();
    let avant = st.read_region(&SURFACE, DOSSIER, ZERO).unwrap();

    let mut p = Params::new();
    // Un décalage PLUS PETIT que la sélection : source et destination
    // partagent des chunks, donc l'opération y repasse.
    p.poser("decalage", Valeur::Vecteur([8, 0, 8]));
    let t = construire("//move", &p, &mut interner).unwrap();
    let cr = executer(
        &t,
        &st,
        &SURFACE,
        DOSSIER,
        &sel(),
        &mut interner,
        &Options::default(),
    )
    .unwrap();
    assert!(cr.rapport.patches.len() > 1, "il faut plusieurs correctifs");

    let mut journal = Journal::new();
    assert!(cr
        .rapport
        .journaliser(&mut journal, "Déplacer", "deplacer", Vec::new(), 0));

    let (entree, _) = journal.annuler().unwrap();
    rejouer(&st, entree, Sens::Annuler)
        .expect("l'annulation d'un //move doit s'appliquer entièrement");
    assert_eq!(
        contenu(&st.read_region(&SURFACE, DOSSIER, ZERO).unwrap()),
        contenu(&avant),
        "un seul Ctrl+Z doit défaire le déplacement ENTIER"
    );
}
