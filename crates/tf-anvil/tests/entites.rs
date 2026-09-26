//! Les block entities — le contenu d'un coffre, qui n'est PAS dans la grille.
//!
//! Le piège que ces tests ferment est celui qu'`ExeWorldEdit` a payé : une
//! opération qui déplace des blocs et laisse les coffres derrière elle. Rien
//! dans le format ne le signale — le build sort pivoté et vide, et c'est à
//! l'ouverture du coffre qu'on l'apprend.
//!
//! Comme partout ici, le producteur (`fixture`, qui écrit les octets à la main
//! depuis la spec) et le vérificateur (`frozen`, qui reconstruit un arbre NBT
//! complet) ne partagent pas une ligne avec `src/`.

mod common;
use common::fixture::{self, t, SectionSpec};
use common::frozen;

use tf_anvil::entites::{champ_entites, nom_du_champ, Entite};
use tf_anvil::{edition_entites, scan, splice, Edit, Layout};

/// Un chunk moderne : celui de la fixture porte un coffre.
fn chunk_moderne(cx: i32, cz: i32) -> Vec<u8> {
    fixture::chunk_nbt(cx, cz, &[SectionSpec::uniform(-4, "minecraft:stone")])
}

/// Un chunk 1.13–1.17, où tout vit sous `Level`.
fn chunk_legacy(avec_liste: bool) -> Vec<u8> {
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::INT, "DataVersion").i32v(2724);
    n.field(t::COMPOUND, "Level");
    n.field(t::INT, "xPos").i32v(2);
    n.field(t::INT, "zPos").i32v(3);
    n.field(t::LIST, "Sections").list(t::COMPOUND, 1);
    n.field(t::BYTE, "Y").i8v(0);
    n.field(t::LIST, "Palette").list(t::COMPOUND, 1);
    n.field(t::STRING, "Name").strv("minecraft:stone");
    n.end();
    n.end(); // la section
    if avec_liste {
        n.field(t::LIST, "TileEntities").list(t::COMPOUND, 1);
        n.field(t::STRING, "id").strv("minecraft:furnace");
        n.field(t::INT, "x").i32v(35);
        n.field(t::INT, "y").i32v(11);
        n.field(t::INT, "z").i32v(52);
        n.end();
    }
    // Un champ inconnu APRÈS la liste : il doit rester en place quoi qu'il
    // arrive à celle-ci.
    n.field(t::STRING, "modtest:apres").strv("témoin");
    n.end(); // Level
    n.end(); // racine
    n.b
}

fn chunk_sans_liste() -> Vec<u8> {
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::INT, "DataVersion").i32v(3465);
    n.field(t::INT, "xPos").i32v(0);
    n.field(t::INT, "zPos").i32v(0);
    n.field(t::LIST, "sections").list(t::COMPOUND, 1);
    n.field(t::BYTE, "Y").i8v(0);
    n.field(t::COMPOUND, "block_states");
    n.field(t::LIST, "palette").list(t::COMPOUND, 1);
    n.field(t::STRING, "Name").strv("minecraft:stone");
    n.end();
    n.end(); // block_states
    n.end(); // la section
    n.field(t::STRING, "modtest:apres").strv("témoin");
    n.end(); // racine
    n.b
}

fn liste_de(brut: &[u8]) -> Vec<Entite> {
    let s = scan(brut).unwrap();
    s.entites
        .entrees
        .iter()
        .filter_map(|e| Entite::depuis(brut, e))
        .collect()
}

// ── le repérage ─────────────────────────────────────────────────────────────

#[test]
fn un_chunk_moderne_rend_ses_entites_avec_leur_case() {
    let brut = chunk_moderne(4, 7);
    let s = scan(&brut).unwrap();
    assert_eq!(s.layout, Layout::Flat);
    assert_eq!(s.entites.entrees.len(), 1);
    let a = s.entites.entrees[0].ancrage.expect("le coffre a une case");
    assert_eq!(a.case, [4 * 16 + 3, 64, 7 * 16 + 5]);
    // Les trois décalages pointent bien sur des charges d'entiers du tampon.
    for (k, &at) in a.champs.iter().enumerate() {
        let v = i32::from_be_bytes(brut[at..at + 4].try_into().unwrap());
        assert_eq!(v, a.case[k], "le décalage de la coordonnée {k} est faux");
    }
}

#[test]
fn un_chunk_1_17_les_trouve_sous_level() {
    let brut = chunk_legacy(true);
    let s = scan(&brut).unwrap();
    assert_eq!(s.layout, Layout::Legacy);
    assert_eq!(s.entites.entrees.len(), 1);
    assert_eq!(s.entites.entrees[0].ancrage.unwrap().case, [35, 11, 52]);
    assert_eq!(nom_du_champ(s.layout), "TileEntities");
}

#[test]
fn une_entite_sans_coordonnees_est_reperee_mais_pas_ancree() {
    // Elle existe dans la nature (entrées abîmées, générateurs bogués). On ne
    // sait pas où elle est, donc on ne sait pas où la reposer : elle est
    // recopiée telle quelle plutôt que devinée.
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::LIST, "block_entities").list(t::COMPOUND, 1);
    n.field(t::STRING, "id").strv("minecraft:chest");
    n.field(t::INT, "x").i32v(1);
    n.field(t::INT, "z").i32v(3); // pas de `y`
    n.end();
    n.end();
    let s = scan(&n.b).unwrap();
    assert_eq!(s.entites.entrees.len(), 1);
    assert!(s.entites.entrees[0].ancrage.is_none());
    assert!(Entite::depuis(&n.b, &s.entites.entrees[0]).is_none());
}

#[test]
fn un_chunk_sans_liste_n_en_invente_pas() {
    let brut = chunk_sans_liste();
    let s = scan(&brut).unwrap();
    assert!(s.entites.champ.is_none());
    assert!(s.entites.est_vide());
    // Le point d'insertion est le `TAG_End` de la racine.
    assert_eq!(s.entites.inserer_a, brut.len() - 1);
}

// ── le déplacement : douze octets, et pas un de plus ─────────────────────────

#[test]
fn deplacer_une_entite_ne_touche_qu_a_ses_coordonnees() {
    let brut = chunk_moderne(0, 0);
    let e = liste_de(&brut).pop().unwrap();
    let avant = e.octets();
    assert_eq!(avant, e.nbt, "sans déplacement, les octets d'origine");

    let apres = e.deplacee([100, -8, 3]).octets();
    assert_eq!(
        apres.len(),
        avant.len(),
        "un déplacement ne change pas la taille"
    );
    let differents: Vec<usize> = (0..avant.len()).filter(|&i| avant[i] != apres[i]).collect();
    assert!(
        differents.len() <= 12,
        "au plus trois entiers réécrits, {} octets ont bougé",
        differents.len()
    );
    // Et ce sont bien ceux des trois coordonnées.
    for &at in &e.champs {
        assert!(differents.iter().any(|&i| (at..at + 4).contains(&i)));
    }
    assert_eq!(e.id(), Some("minecraft:chest"));
}

#[test]
fn le_contenu_d_un_coffre_traverse_un_deplacement_intact() {
    // Le vrai enjeu : ce n'est pas la case, c'est la pile d'objets. Elle doit
    // ressortir du splice telle quelle, sans jamais avoir été ré-encodée.
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::LIST, "block_entities").list(t::COMPOUND, 1);
    n.field(t::STRING, "id").strv("minecraft:chest");
    n.field(t::INT, "x").i32v(5);
    n.field(t::INT, "y").i32v(70);
    n.field(t::INT, "z").i32v(9);
    n.field(t::STRING, "CustomName").strv("Réserve 한국어");
    n.field(t::LIST, "Items").list(t::COMPOUND, 1);
    n.field(t::BYTE, "Slot").i8v(3);
    n.field(t::STRING, "id").strv("minecraft:diamond");
    n.field(t::BYTE, "Count").i8v(42);
    n.end(); // l'objet — une LISTE, elle, n'a pas de TAG_End
    n.end(); // le coffre
    n.field(t::STRING, "modtest:apres").strv("témoin");
    n.end();
    let brut = n.b;

    let e = liste_de(&brut).pop().unwrap().deplacee([-1000, 5, 64]);
    let s = scan(&brut).unwrap();
    let mut edits = vec![edition_entites(&brut, &s.entites, s.layout, &[e]).unwrap()];
    let apres = splice(&brut, &mut edits).unwrap();

    let (_, arbre) = frozen::parse_nbt(&apres);
    let liste = arbre.get("block_entities").unwrap().as_list().unwrap();
    assert_eq!(liste.len(), 1);
    let c = &liste[0];
    assert_eq!(c.get("x").unwrap().as_i32(), Some(-995));
    assert_eq!(c.get("y").unwrap().as_i32(), Some(75));
    assert_eq!(c.get("z").unwrap().as_i32(), Some(73));
    assert_eq!(
        c.get("CustomName").unwrap().as_str(),
        Some("Réserve 한국어")
    );
    let items = c.get("Items").unwrap().as_list().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("Count").unwrap().as_i8(), Some(42));
    assert_eq!(
        items[0].get("id").unwrap().as_str(),
        Some("minecraft:diamond")
    );
    // Et le champ qui suivait la liste n'a pas bougé.
    assert_eq!(arbre.get("modtest:apres").unwrap().as_str(), Some("témoin"));
}

// ── l'édition : rien à dire quand rien ne change ────────────────────────────

#[test]
fn reposer_la_liste_a_l_identique_ne_produit_aucune_edition() {
    let brut = chunk_moderne(1, 1);
    let s = scan(&brut).unwrap();
    let memes = liste_de(&brut);
    assert!(edition_entites(&brut, &s.entites, s.layout, &memes).is_none());
}

#[test]
fn une_liste_vide_qui_le_reste_ne_produit_rien() {
    // La fixture de bench écrit une liste vide sous la forme
    // `TAG_Compound` × 0 ; nous écririons `TAG_End` × 0. Les deux sont vides,
    // et normaliser au passage réécrirait un chunk que personne n'a touché.
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::LIST, "block_entities").list(t::COMPOUND, 0);
    n.end();
    let s = scan(&n.b).unwrap();
    assert!(s.entites.champ.is_some());
    assert!(edition_entites(&n.b, &s.entites, s.layout, &[]).is_none());
}

#[test]
fn retirer_la_derniere_entite_laisse_une_liste_vide_lisible() {
    let brut = chunk_moderne(0, 0);
    let s = scan(&brut).unwrap();
    let mut edits = vec![edition_entites(&brut, &s.entites, s.layout, &[]).unwrap()];
    let apres = splice(&brut, &mut edits).unwrap();
    let (_, arbre) = frozen::parse_nbt(&apres);
    assert_eq!(
        arbre
            .get("block_entities")
            .unwrap()
            .as_list()
            .unwrap()
            .len(),
        0
    );
    // Le témoin de données de mod, qui vit APRÈS la liste, est intact.
    let m = arbre.get("neoforge:attachments").unwrap();
    assert_eq!(
        m.get("ae2:channel").unwrap().as_str(),
        Some("témoin — ne doit jamais bouger 한국어")
    );
}

#[test]
fn une_entite_posee_dans_un_chunk_qui_n_en_avait_pas_cree_le_champ() {
    let brut = chunk_sans_liste();
    let s = scan(&brut).unwrap();
    // L'entité est RELEVÉE d'un chunk témoin, pas fabriquée à la main : des
    // décalages calculés de tête écriraient par-dessus le nom du champ
    // suivant, et le chunk sortirait plausible et faux.
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.field(t::LIST, "block_entities").list(t::COMPOUND, 1);
    n.field(t::STRING, "id").strv("minecraft:barrel");
    n.field(t::INT, "x").i32v(0);
    n.field(t::INT, "y").i32v(0);
    n.field(t::INT, "z").i32v(0);
    n.end();
    n.end();
    let mut e = liste_de(&n.b).pop().unwrap();
    e.case = [8, 12, 4];
    let mut edits = vec![edition_entites(&brut, &s.entites, s.layout, &[e]).unwrap()];
    let apres = splice(&brut, &mut edits).unwrap();

    let (_, arbre) = frozen::parse_nbt(&apres);
    let liste = arbre.get("block_entities").unwrap().as_list().unwrap();
    assert_eq!(liste.len(), 1);
    assert_eq!(
        liste[0].get("id").unwrap().as_str(),
        Some("minecraft:barrel")
    );
    assert_eq!(liste[0].get("x").unwrap().as_i32(), Some(8));
    assert_eq!(liste[0].get("y").unwrap().as_i32(), Some(12));
    assert_eq!(liste[0].get("z").unwrap().as_i32(), Some(4));
    assert_eq!(arbre.get("modtest:apres").unwrap().as_str(), Some("témoin"));
}

#[test]
fn une_liste_posee_dans_un_chunk_1_17_va_sous_level_pas_a_la_racine() {
    // Le nom et l'EMPLACEMENT changent avec la disposition. Insérer à la
    // racine d'un chunk 1.17 donnerait un fichier que le jeu charge sans
    // erreur — et dont les coffres ont disparu.
    let brut = chunk_legacy(false);
    let s = scan(&brut).unwrap();
    assert!(s.entites.champ.is_none());
    let e = liste_de(&chunk_legacy(true)).pop().unwrap();
    let mut edits = vec![edition_entites(&brut, &s.entites, s.layout, &[e]).unwrap()];
    let apres = splice(&brut, &mut edits).unwrap();

    let (_, arbre) = frozen::parse_nbt(&apres);
    assert!(
        arbre.get("TileEntities").is_none(),
        "rien ne doit être écrit à la racine d'un chunk 1.17"
    );
    let level = arbre.get("Level").unwrap();
    let liste = level.get("TileEntities").unwrap().as_list().unwrap();
    assert_eq!(liste.len(), 1);
    assert_eq!(
        liste[0].get("id").unwrap().as_str(),
        Some("minecraft:furnace")
    );
    assert_eq!(level.get("modtest:apres").unwrap().as_str(), Some("témoin"));
}

#[test]
fn le_champ_ecrit_est_relisible_par_notre_propre_balayage() {
    // L'aller-retour le plus court : ce qu'on écrit, on doit savoir le relire
    // avec la même case et les mêmes décalages.
    let source = chunk_moderne(0, 0);
    let voulues: Vec<Entite> = liste_de(&source)
        .into_iter()
        .map(|e| e.deplacee([7, -3, 11]))
        .collect();
    let mut n = fixture::Nbt::new();
    n.field(t::COMPOUND, "");
    n.b.extend_from_slice(&champ_entites("block_entities", &voulues));
    n.end();
    let relues = liste_de(&n.b);
    assert_eq!(relues.len(), voulues.len());
    for (a, b) in relues.iter().zip(&voulues) {
        assert_eq!(a.case, b.case);
        assert_eq!(a.id(), b.id());
    }
}

#[test]
fn une_edition_d_entites_ne_chevauche_pas_celles_des_sections() {
    // Les deux partent dans le même `splice` : si leurs plages se croisaient,
    // il refuserait — ce qui vaut mieux qu'un chunk plausible et faux.
    let brut = chunk_moderne(0, 0);
    let s = scan(&brut).unwrap();
    let ed = edition_entites(&brut, &s.entites, s.layout, &[]).unwrap();
    for sc in &s.sections {
        let Some(spans) = sc.spans else { continue };
        let p = match spans {
            tf_anvil::SectionSpans::Flat { palette, .. } => palette,
            tf_anvil::SectionSpans::Legacy { palette, .. } => palette,
        };
        assert!(
            p.end <= ed.span.start || ed.span.end <= p.start,
            "la liste d'entités chevauche une palette de section"
        );
    }
    let mut edits: Vec<Edit> = vec![ed];
    assert!(splice(&brut, &mut edits).is_ok());
}

/// **Une block entity faite de ses seuls octets** — la forme des fichiers
/// d'échange — se situe comme dans un chunk ; sans ses trois coordonnées elle
/// n'est pas une entité qu'on sait poser, et des octets en trop après le
/// compound la font refuser : ils partiraient dans la save sans que personne
/// sache ce qu'ils sont.
#[test]
fn une_block_entity_se_fait_de_ses_seuls_octets() {
    let mut w = tf_nbt::Writer::new();
    w.field(tf_nbt::tag::STRING, "id")
        .raw_str("minecraft:chest");
    w.field(tf_nbt::tag::INT, "x").i32_payload(4);
    w.field(tf_nbt::tag::INT, "y").i32_payload(-7);
    w.field(tf_nbt::tag::INT, "z").i32_payload(9);
    w.end();
    let nbt = w.into_bytes();
    let e = Entite::depuis_compound(nbt.clone()).unwrap().unwrap();
    assert_eq!(e.case, [4, -7, 9]);
    assert_eq!(e.octets(), nbt, "rien n'est ré-encodé");
    let mut plus = nbt.clone();
    plus.push(0);
    assert!(Entite::depuis_compound(plus).is_err());
    let mut sans_z = tf_nbt::Writer::new();
    sans_z.field(tf_nbt::tag::INT, "x").i32_payload(1);
    sans_z.end();
    assert_eq!(Entite::depuis_compound(sans_z.into_bytes()), Ok(None));
}
