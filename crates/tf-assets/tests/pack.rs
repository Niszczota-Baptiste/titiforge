//! Lire un pack : chaînes de parents, variables de texture, formes.
//!
//! Le pack de test est écrit à la main, fichier par fichier, et il reproduit
//! les cas DIFFICILES plutôt que de les contourner : une texture de
//! démonstration déjà simple masque exactement le défaut qu'on veut voir.

use std::fs;
use std::path::{Path, PathBuf};

use tf_assets::blockstates::Blockstate;
use tf_assets::catalogue::{classer, indice_cube_plein, Classement, Disposition};
use tf_assets::modele::{resoudre, uv_de};
use tf_assets::{cuboides, Catalogue, Dossier, Id, Pile, Source, SourceError};
use tf_mesh::forme::Face;

// ── un dossier temporaire, sans dépendance ─────────────────────────────────

struct TempDir(PathBuf);

impl TempDir {
    fn new(nom: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "tf-assets-{nom}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn ecrire(&self, chemin: &str, contenu: &str) {
        let p = self.0.join(chemin);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contenu).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn chemin_pack(id: &Id) -> String {
    id.modele()
}

// ── les identifiants ────────────────────────────────────────────────────────

#[test]
fn un_identifiant_sans_namespace_est_du_vanilla() {
    assert_eq!(Id::parse("stone").namespace, "minecraft");
    assert_eq!(Id::parse("minefield:chaise").namespace, "minefield");
    assert_eq!(Id::parse("minefield:block/chaise").chemin, "block/chaise");
    assert_eq!(
        Id::parse("minefield:block/chaise").modele(),
        "assets/minefield/models/block/chaise.json"
    );
}

#[test]
fn un_nom_de_parent_ne_peut_pas_sortir_du_pack() {
    // Un pack vient du disque d'un utilisateur, et un modèle NOMME son parent.
    // `../../../etc/passwd` est un nom parfaitement bien formé.
    let d = TempDir::new("evasion");
    d.ecrire("dedans.json", "{}");
    let src = Dossier::ouvrir(d.path()).unwrap();
    assert!(src.lire("dedans.json").is_ok());
    for mechant in [
        "../secret",
        "a/../../secret",
        "/etc/passwd",
        "a\\b",
        "",
        "./a",
    ] {
        assert_eq!(
            src.lire(mechant),
            Err(SourceError::Absent(mechant.to_string())),
            "{mechant} ne doit pas sortir"
        );
    }
}

// ── la chaîne de parents ────────────────────────────────────────────────────

#[test]
fn un_modele_herite_des_elements_et_des_textures_de_son_parent() {
    let d = TempDir::new("parents");
    d.ecrire(
        "assets/minecraft/models/block/cube.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#top"},"down":{"texture":"#bottom"}}}]}"##,
    );
    d.ecrire(
        "assets/minecraft/models/block/cube_all.json",
        r##"{"parent":"block/cube","textures":{"top":"#all","bottom":"#all"}}"##,
    );
    d.ecrire(
        "assets/minecraft/models/block/stone.json",
        r##"{"parent":"block/cube_all","textures":{"all":"block/stone"}}"##,
    );

    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/stone"), &chemin_pack).unwrap();

    assert_eq!(m.chaine.len(), 3, "trois maillons : {:?}", m.chaine);
    assert_eq!(m.elements.len(), 1, "les éléments viennent du grand-parent");
    let f = &m.elements[0].faces[&Face::PlusY];
    assert_eq!(
        f.texture, "block/stone",
        "`#top` → `#all` → `block/stone` : une variable qui renvoie à une \
         variable doit être SUIVIE, sinon la face porte le nom d'une autre \
         variable et aucune texture n'est trouvée"
    );
}

#[test]
fn l_enfant_l_emporte_sur_son_parent() {
    let d = TempDir::new("enfant");
    d.ecrire(
        "assets/minecraft/models/block/parent.json",
        r##"{"textures":{"all":"parent"},
            "elements":[{"from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#all"}}}]}"##,
    );
    d.ecrire(
        "assets/minecraft/models/block/enfant.json",
        r##"{"parent":"block/parent","textures":{"all":"enfant"},
            "elements":[{"from":[0,0,0],"to":[16,8,16],
            "faces":{"up":{"texture":"#all"}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/enfant"), &chemin_pack).unwrap();
    assert_eq!(m.elements.len(), 1);
    assert_eq!(
        m.elements[0].to,
        [16.0, 8.0, 16.0],
        "les éléments de l'enfant"
    );
    assert_eq!(m.elements[0].faces[&Face::PlusY].texture, "enfant");
}

#[test]
fn une_chaine_de_parents_circulaire_est_refusee_et_ne_boucle_pas() {
    // Un pack vient du disque d'un utilisateur. Une boucle ferait tourner la
    // résolution jusqu'à la mort du processus, sans message.
    let d = TempDir::new("boucle");
    d.ecrire(
        "assets/minecraft/models/a.json",
        r##"{"parent":"minecraft:b"}"##,
    );
    d.ecrire(
        "assets/minecraft/models/b.json",
        r##"{"parent":"minecraft:a"}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    assert!(resoudre(&src, &Id::parse("a"), &chemin_pack).is_err());
}

#[test]
fn une_variable_de_texture_qui_ne_se_resout_pas_reste_nommee() {
    let d = TempDir::new("variable");
    d.ecrire(
        "assets/minecraft/models/x.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#jamais_declaree"}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("x"), &chemin_pack).unwrap();
    assert_eq!(
        m.elements[0].faces[&Face::PlusY].texture,
        "#jamais_declaree",
        "la remplacer par du vide ferait chercher une texture nommée « » et le \
         message ne nommerait pas la variable fautive"
    );
}

// ── les formes ──────────────────────────────────────────────────────────────

fn pack_des_formes() -> TempDir {
    let d = TempDir::new("formes");
    // Une pierre : un cuboïde qui remplit.
    d.ecrire(
        "assets/minecraft/models/block/stone.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"a","cullface":"up"},
                     "down":{"texture":"a","cullface":"down"},
                     "north":{"texture":"a","cullface":"north"},
                     "south":{"texture":"a","cullface":"south"},
                     "east":{"texture":"a","cullface":"east"},
                     "west":{"texture":"a","cullface":"west"}}}]}"##,
    );
    // `grass_block` : DEUX cuboïdes — le cube, puis la couche d'herbe teintée.
    // Le premier déclare six faces, le second quatre.
    d.ecrire(
        "assets/minecraft/models/block/grass_block.json",
        r##"{"elements":[
            {"from":[0,0,0],"to":[16,16,16],
             "faces":{"up":{"texture":"t","cullface":"up"},
                      "down":{"texture":"d","cullface":"down"},
                      "north":{"texture":"s","cullface":"north"},
                      "south":{"texture":"s","cullface":"south"},
                      "east":{"texture":"s","cullface":"east"},
                      "west":{"texture":"s","cullface":"west"}}},
            {"from":[0,0,0],"to":[16,16,16],
             "faces":{"north":{"texture":"o","tintindex":0},
                      "south":{"texture":"o","tintindex":0},
                      "east":{"texture":"o","tintindex":0},
                      "west":{"texture":"o","tintindex":0}}}]}"##,
    );
    // Une dalle basse : ne remplit pas.
    d.ecrire(
        "assets/minecraft/models/block/slab.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,8,16],
            "faces":{"up":{"texture":"a"},
                     "down":{"texture":"a","cullface":"down"}}}]}"##,
    );
    // Une chaise qui DÉBORDE : Minecraft autorise −16 à 32.
    d.ecrire(
        "assets/minecraft/models/block/chaise.json",
        r##"{"elements":[{"from":[2,0,2],"to":[14,20,14],
            "faces":{"up":{"texture":"a"}}}]}"##,
    );
    // De la fumée : aucun élément.
    d.ecrire("assets/minecraft/models/block/smoke.json", r##"{}"##);
    d
}

#[test]
fn un_cuboide_n_est_pas_le_critere_d_un_cube() {
    // `grass_block` déclare DEUX cuboïdes. Le compter comme « modèle » le rend
    // non opaque : sur un terrain, chaque bloc SOUS la surface redevient
    // visible. Mesuré dans ExeWorldEdit : 1 281 appels de dessin et un sol
    // méconnaissable.
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let herbe = resoudre(&src, &Id::parse("block/grass_block"), &chemin_pack).unwrap();
    assert_eq!(herbe.elements.len(), 2);
    assert_eq!(
        classer(&herbe),
        Classement::Cube,
        "le critère est un cuboïde qui REMPLIT, pas « un seul cuboïde »"
    );

    // Et c'est le VRAI cube qui gagne, pas la couche posée par-dessus.
    let c = cuboides(&herbe);
    assert_eq!(
        indice_cube_plein(&c),
        Some(0),
        "à égalité de remplissage, celui qui déclare le plus de faces"
    );
}

#[test]
fn les_formes_se_classent_comme_le_pack_les_declare() {
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    for (nom, attendu) in [
        ("block/stone", Classement::Cube),
        ("block/grass_block", Classement::Cube),
        ("block/slab", Classement::Modele),
        ("block/chaise", Classement::Modele),
        ("block/smoke", Classement::Vide),
    ] {
        let m = resoudre(&src, &Id::parse(nom), &chemin_pack).unwrap();
        assert_eq!(classer(&m), attendu, "{nom}");
    }
}

#[test]
fn un_modele_qui_deborde_du_bloc_n_est_pas_rogne() {
    // Minecraft autorise −16 à 32, et le dossier d'une chaise Minefield monte
    // à 20. Cadrer sur 0..16 le rognerait.
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/chaise"), &chemin_pack).unwrap();
    let c = cuboides(&m);
    assert_eq!(c[0].max[1], 20.0, "le dossier monte à 20, il y reste");
}

#[test]
fn le_cull_ne_porte_que_les_faces_qui_declarent_cullface() {
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/slab"), &chemin_pack).unwrap();
    let c = cuboides(&m);
    assert_eq!(
        c[0].cull,
        Face::MoinsY.bit(),
        "seule la face du bas est cullable ; le dessus est à ras mais sans \
         `cullface`, donc il reste dessiné quoi qu'il y ait dessus"
    );
    assert_eq!(c[0].faces, Face::MoinsY.bit() | Face::PlusY.bit());
}

#[test]
fn une_face_non_declaree_n_est_pas_dessinee() {
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/chaise"), &chemin_pack).unwrap();
    let c = cuboides(&m);
    assert_eq!(
        c[0].faces,
        Face::PlusY.bit(),
        "un modèle qui n'a que `up` n'est pas un cube à six faces"
    );
}

#[test]
fn from_et_to_inverses_ne_produisent_pas_un_cuboide_negatif() {
    let d = TempDir::new("inverse");
    d.ecrire(
        "assets/minecraft/models/x.json",
        r##"{"elements":[{"from":[16,16,16],"to":[0,0,0],
            "faces":{"up":{"texture":"a"}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("x"), &chemin_pack).unwrap();
    let c = cuboides(&m);
    assert_eq!(c[0].min, [0.0, 0.0, 0.0]);
    assert_eq!(c[0].max, [16.0, 16.0, 16.0]);
    assert!(c[0].remplit(), "un modèle écrit à l'envers reste un cube");
}

// ── les uv ──────────────────────────────────────────────────────────────────

#[test]
fn des_uv_absentes_se_deduisent_du_cuboide() {
    // **`uv` absent n'est PAS `[0,0,0,0]`.** C'est ce qui fait qu'une dalle
    // montre la moitié basse de sa texture au lieu de la texture entière
    // écrasée. Mesuré sur le pack du serveur : 26,4 % des faces sont dans ce
    // cas — ce n'est pas un cas limite.
    let d = pack_des_formes();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("block/slab"), &chemin_pack).unwrap();
    let e = &m.elements[0];

    let dessus = uv_de(e, Face::PlusY, &e.faces[&Face::PlusY]);
    assert_eq!(
        dessus,
        [0.0, 0.0, 16.0, 16.0],
        "vue de dessus, une dalle est pleine"
    );

    // Sur un côté, la dalle ne montre que la MOITIÉ BASSE de sa texture.
    let d2 = TempDir::new("uv-cote");
    d2.ecrire(
        "assets/minecraft/models/s.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,8,16],
            "faces":{"north":{"texture":"a"}}}]}"##,
    );
    let src2 = Dossier::ouvrir(d2.path()).unwrap();
    let m2 = resoudre(&src2, &Id::parse("s"), &chemin_pack).unwrap();
    let e2 = &m2.elements[0];
    let cote = uv_de(e2, Face::MoinsZ, &e2.faces[&Face::MoinsZ]);
    assert_eq!(
        cote,
        [0.0, 8.0, 16.0, 16.0],
        "la moitié BASSE de la texture, pas la texture entière écrasée"
    );
}

#[test]
fn des_uv_declarees_sont_prises_telles_quelles() {
    let d = TempDir::new("uv-decl");
    d.ecrire(
        "assets/minecraft/models/x.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,8,16],
            "faces":{"up":{"texture":"a","uv":[4,4,12,12]}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let m = resoudre(&src, &Id::parse("x"), &chemin_pack).unwrap();
    let e = &m.elements[0];
    assert_eq!(
        uv_de(e, Face::PlusY, &e.faces[&Face::PlusY]),
        [4.0, 4.0, 12.0, 12.0]
    );
}

// ── les blockstates ─────────────────────────────────────────────────────────

#[test]
fn un_blockstate_variants_rend_le_modele_de_l_etat() {
    let v: serde_json::Value = serde_json::from_str(
        r##"{"variants":{
            "facing=north":{"model":"minefield:block/chaise"},
            "facing=east":{"model":"minefield:block/chaise","y":90}}}"##,
    )
    .unwrap();
    let b = Blockstate::depuis_json(&v).unwrap();
    let est = b.pour(&[("facing".into(), "east".into())]);
    assert_eq!(est.len(), 1);
    assert_eq!(est[0].y, 90);
    assert_eq!(b.modeles().len(), 2);
}

#[test]
fn un_blockstate_multipart_cumule_les_regles_qui_passent() {
    // Un tiers des blocs Minefield passent par là. Les sauter laissait 353
    // blocs sur 1 678 non résolus, et la part de blocs-modèles sortait à 50 %
    // au lieu de 66,8 % — un recensement qui laisse 21 % de trous ne dit rien.
    let v: serde_json::Value = serde_json::from_str(
        r##"{"multipart":[
            {"apply":{"model":"m:post"}},
            {"when":{"north":"true"},"apply":{"model":"m:side"}},
            {"when":{"east":"true"},"apply":{"model":"m:side","y":90}}]}"##,
    )
    .unwrap();
    let b = Blockstate::depuis_json(&v).unwrap();

    let rien = b.pour(&[]);
    assert_eq!(rien.len(), 1, "seule la règle sans condition");

    let deux = b.pour(&[
        ("north".into(), "true".into()),
        ("east".into(), "true".into()),
    ]);
    assert_eq!(deux.len(), 3, "le poteau et ses deux bras");
}

#[test]
fn une_condition_multipart_accepte_plusieurs_valeurs() {
    let v: serde_json::Value = serde_json::from_str(
        r##"{"multipart":[{"when":{"type":"low|tall"},"apply":{"model":"m:x"}}]}"##,
    )
    .unwrap();
    let b = Blockstate::depuis_json(&v).unwrap();
    assert_eq!(b.pour(&[("type".into(), "low".into())]).len(), 1);
    assert_eq!(b.pour(&[("type".into(), "tall".into())]).len(), 1);
    assert_eq!(b.pour(&[("type".into(), "none".into())]).len(), 0);
}

#[test]
fn un_etat_sans_correspondance_rend_quand_meme_un_modele() {
    // Un bloc sans modèle est INVISIBLE, ce qui est pire qu'un bloc dans la
    // mauvaise orientation.
    let v: serde_json::Value =
        serde_json::from_str(r##"{"variants":{"facing=north":{"model":"m:x"}}}"##).unwrap();
    let b = Blockstate::depuis_json(&v).unwrap();
    assert_eq!(b.pour(&[("facing".into(), "sud-ouest".into())]).len(), 1);
}

// ── la pile de sources ──────────────────────────────────────────────────────

#[test]
fn le_premier_pack_de_la_pile_recouvre_les_suivants() {
    let haut = TempDir::new("haut");
    let bas = TempDir::new("bas");
    haut.ecrire("a.json", "\"du haut\"");
    bas.ecrire("a.json", "\"du bas\"");
    bas.ecrire("b.json", "\"du bas\"");

    let p = Pile::new(vec![
        Box::new(Dossier::ouvrir(haut.path()).unwrap()),
        Box::new(Dossier::ouvrir(bas.path()).unwrap()),
    ]);
    assert_eq!(p.lire("a.json").unwrap(), b"\"du haut\"".to_vec());
    assert_eq!(
        p.lire("b.json").unwrap(),
        b"\"du bas\"".to_vec(),
        "ce que le pack du dessus n'a pas, on le prend en dessous"
    );
    assert!(matches!(p.lire("c.json"), Err(SourceError::Absent(_))));
}

// ── le catalogue ────────────────────────────────────────────────────────────

#[test]
fn un_catalogue_signale_les_modeles_qu_il_n_a_pas_trouves() {
    // Un trou silencieux, c'est un recensement qui ment. Il doit se VOIR.
    let d = TempDir::new("trous");
    d.ecrire(
        "blockstates.json",
        r##"{"m:present":{"variants":{"":{"model":"m:block/la"}}},
            "m:absent":{"variants":{"":{"model":"m:block/nulle_part"}}}}"##,
    );
    d.ecrire(
        "models/block_la.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"a"}}}]}"##,
    );

    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    assert_eq!(cat.charger_codex(&src).unwrap(), 2);
    cat.resoudre_modeles(&src);

    assert_eq!(cat.classement("m:present"), Some(Classement::Cube));
    assert_eq!(cat.classement("m:absent"), None);
    assert_eq!(cat.introuvables.len(), 1);
    assert_eq!(cat.introuvables[0].to_string(), "m:block/nulle_part");
}

#[test]
fn le_codex_cherche_les_modeles_dans_ses_deux_dossiers() {
    let d = TempDir::new("deux-dossiers");
    d.ecrire(
        "blockstates.json",
        r##"{"m:a":{"variants":{"":{"model":"m:block/a"}}}}"##,
    );
    // Seulement dans `render-models/` : c'est celui qui porte les modèles de
    // rendu, et il est essayé EN PREMIER.
    d.ecrire(
        "render-models/block_a.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"a"}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).unwrap();
    cat.resoudre_modeles(&src);
    assert!(cat.introuvables.is_empty());
    assert_eq!(cat.classement("m:a"), Some(Classement::Modele));
}

#[test]
fn un_modele_partage_n_est_resolu_qu_une_fois() {
    let d = TempDir::new("partage");
    d.ecrire(
        "blockstates.json",
        r##"{"m:a":{"variants":{"":{"model":"m:block/c"}}},
            "m:b":{"variants":{"":{"model":"m:block/c"}}}}"##,
    );
    d.ecrire(
        "models/block_c.json",
        r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"a"}}}]}"##,
    );
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).unwrap();
    cat.resoudre_modeles(&src);
    assert_eq!(cat.nb_blocs(), 2);
    assert_eq!(
        cat.nb_modeles(),
        1,
        "vingt escaliers renvoient au même parent : les résoudre par ÉTAT les \
         relirait des milliers de fois"
    );
}
