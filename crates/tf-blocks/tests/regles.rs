//! Les règles de transformation, dérivées d'un pack écrit à la main.
//!
//! Le pack de test reproduit les cas DIFFICILES du serveur : une bûche
//! symétrique à 180°, une trappe dont l'orientation est invisible quand elle
//! est fermée, un bloc dont une rotation n'existe pas. Un pack facile ferait
//! passer tous les tests et ne prouverait rien.

use std::fs;
use std::path::{Path, PathBuf};

use tf_assets::catalogue::Disposition;
use tf_assets::{Catalogue, Dossier};
use tf_blocks::{Table, Transfo, TOUTES};

struct TempDir(PathBuf);

impl TempDir {
    fn new(nom: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "tf-blocks-{nom}-{}-{:?}",
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

/// Un cuboïde plein, pour un modèle sans forme particulière.
const CUBE: &str =
    r#"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"a"}}}]}"#;
/// Une marche CHIRALE : sa main gauche et sa main droite ne se confondent pas
/// avec elle-même tournée. Un modèle symétrique à 180° les confondrait, et la
/// géométrie ne pourrait plus trancher — c'est une limite réelle, pas un défaut
/// du test.
const MARCHE_G: &str = r##"{"elements":[
    {"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"a"}}},
    {"from":[0,8,0],"to":[8,16,12],"faces":{"up":{"texture":"a"}}}]}"##;

fn pack() -> TempDir {
    let d = TempDir::new("regles");
    d.ecrire(
        "blockstates.json",
        r#"{
        "t:echelle": {"variants": {
            "facing=north": {"model": "t:block/echelle"},
            "facing=east":  {"model": "t:block/echelle", "y": 90},
            "facing=south": {"model": "t:block/echelle", "y": 180},
            "facing=west":  {"model": "t:block/echelle", "y": 270}}},
        "t:buche": {"variants": {
            "axis=y": {"model": "t:block/buche"},
            "axis=z": {"model": "t:block/buche_couchee", "x": 90},
            "axis=x": {"model": "t:block/buche_couchee", "x": 90, "y": 90}}},
        "t:trappe": {"variants": {
            "facing=north,open=false": {"model": "t:block/trappe_fermee"},
            "facing=east,open=false":  {"model": "t:block/trappe_fermee"},
            "facing=south,open=false": {"model": "t:block/trappe_fermee"},
            "facing=west,open=false":  {"model": "t:block/trappe_fermee"},
            "facing=north,open=true": {"model": "t:block/trappe_ouverte"},
            "facing=east,open=true":  {"model": "t:block/trappe_ouverte", "y": 90},
            "facing=south,open=true": {"model": "t:block/trappe_ouverte", "y": 180},
            "facing=west,open=true":  {"model": "t:block/trappe_ouverte", "y": 270}}},
        "t:demi_tour": {"variants": {
            "face=north": {"model": "t:block/demi"},
            "face=south": {"model": "t:block/demi", "y": 180}}},
        "t:marche": {"variants": {
            "facing=north,main=gauche": {"model": "t:block/marche"},
            "facing=east,main=gauche":  {"model": "t:block/marche", "y": 90},
            "facing=south,main=gauche": {"model": "t:block/marche", "y": 180},
            "facing=west,main=gauche":  {"model": "t:block/marche", "y": 270},
            "facing=north,main=droite": {"model": "t:block/marche_d"},
            "facing=east,main=droite":  {"model": "t:block/marche_d", "y": 90},
            "facing=south,main=droite": {"model": "t:block/marche_d", "y": 180},
            "facing=west,main=droite":  {"model": "t:block/marche_d", "y": 270}}},
        "t:pierre": {"variants": {"": {"model": "t:block/pierre"}}},
        "t:coin": {"variants": {
            "facing=north,forme=droite": {"model": "t:block/marche"},
            "facing=east,forme=droite":  {"model": "t:block/marche", "y": 90},
            "facing=south,forme=droite": {"model": "t:block/marche", "y": 180},
            "facing=west,forme=droite":  {"model": "t:block/marche", "y": 270},
            "facing=north,forme=coin": {"model": "t:block/quart"},
            "facing=east,forme=coin":  {"model": "t:block/quart", "y": 90},
            "facing=south,forme=coin": {"model": "t:block/quart", "y": 180},
            "facing=west,forme=coin":  {"model": "t:block/quart", "y": 270}}},
        "t:bougie": {"variants": {
            "nombre=1": {"model": "t:block/bougie1"},
            "nombre=2": {"model": "t:block/bougie2"},
            "nombre=3": {"model": "t:block/bougie3"}}},
        "t:decoupe": {"variants": {
            "pose=entier": {"model": "t:block/entier"},
            "pose=morceaux": {"model": "t:block/morceaux"}}}
    }"#,
    );
    for m in [
        "echelle",
        "buche",
        "buche_couchee",
        "trappe_fermee",
        "pierre",
        "demi",
    ] {
        d.ecrire(&format!("models/block_{m}.json"), CUBE);
    }
    d.ecrire("models/block_trappe_ouverte.json", MARCHE_G);
    // Un QUART de bloc : sa forme réfléchie est aussi sa forme tournée, et il
    // n'a qu'une écriture par coin. Aucune permutation de `facing` ne peut
    // donc servir à la fois la marche (est ↔ ouest) et lui (est → sud).
    d.ecrire(
        "models/block_quart.json",
        r#"{"elements":[{"from":[8,0,0],"to":[16,16,8],"faces":{"up":{"texture":"a"}}}]}"#,
    );
    // Trois bougies : aucune orientation nulle part, et des formes qu'aucune
    // rotation ne rend.
    for (m, e) in [
        (
            "bougie1",
            r#"[{"from":[7,0,7],"to":[9,10,9],"faces":{"up":{"texture":"a"}}}]"#,
        ),
        (
            "bougie2",
            r#"[{"from":[5,0,7],"to":[7,10,9],"faces":{"up":{"texture":"a"}}},
                        {"from":[9,0,7],"to":[11,8,9],"faces":{"up":{"texture":"a"}}}]"#,
        ),
        (
            "bougie3",
            r#"[{"from":[5,0,5],"to":[7,10,7],"faces":{"up":{"texture":"a"}}},
                        {"from":[9,0,7],"to":[11,8,9],"faces":{"up":{"texture":"a"}}},
                        {"from":[6,0,10],"to":[8,6,12],"faces":{"up":{"texture":"a"}}}]"#,
        ),
    ] {
        d.ecrire(
            &format!("models/block_{m}.json"),
            &format!(r#"{{"elements":{e}}}"#),
        );
    }
    // Le MÊME solide, écrit d'un bloc puis en quatre morceaux.
    d.ecrire("models/block_entier.json", CUBE);
    d.ecrire(
        "models/block_morceaux.json",
        r#"{"elements":[
        {"from":[0,0,0],"to":[8,16,8],"faces":{"up":{"texture":"a"}}},
        {"from":[8,0,0],"to":[16,16,8],"faces":{"up":{"texture":"a"}}},
        {"from":[0,0,8],"to":[8,16,16],"faces":{"up":{"texture":"a"}}},
        {"from":[8,0,8],"to":[16,16,16],"faces":{"up":{"texture":"a"}}}]}"#,
    );
    d.ecrire("models/block_marche.json", MARCHE_G);
    // La marche DROITE est la marche gauche réfléchie : son cuboïde haut est
    // de l'autre côté.
    d.ecrire(
        "models/block_marche_d.json",
        r##"{"elements":[
        {"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"a"}}},
        {"from":[8,8,0],"to":[16,16,12],"faces":{"up":{"texture":"a"}}}]}"##,
    );
    d
}

fn table(d: &TempDir) -> Table {
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).unwrap();
    cat.resoudre_modeles(&src);
    Table::deriver(&cat)
}

fn norm(cle: &str) -> String {
    match cle.split_once('|') {
        None => cle.to_string(),
        Some((n, p)) => {
            let mut v: Vec<&str> = p.split(',').collect();
            v.sort_unstable();
            format!("{n}|{}", v.join(","))
        }
    }
}

// ── la dérivation de base ───────────────────────────────────────────────────

#[test]
fn une_rotation_se_lit_dans_le_pack_sans_regarder_le_nom_de_la_propriete() {
    // Le pack DÉCLARE déjà la réponse : même modèle, `y` décalé de 90°. C'est
    // ce qui permet de tourner `vertical`, `offset` ou `position` — quatre
    // propriétés que le vanilla ne connaît pas et qu'aucune table écrite à la
    // main ne couvrirait.
    let d = pack();
    let t = table(&d);
    assert_eq!(
        t.transformer("t:echelle|facing=north", Transfo::Rot90)
            .as_deref(),
        Some("t:echelle|facing=east")
    );
    assert_eq!(
        t.transformer("t:echelle|facing=west", Transfo::Rot90)
            .as_deref(),
        Some("t:echelle|facing=north"),
        "la rotation boucle"
    );
}

#[test]
fn un_bloc_symetrique_a_180_degres_tourne_quand_meme() {
    // Une bûche ne déclare que deux orientations horizontales : tourner
    // `axis=x` vise un angle que le pack ne réécrit pas, parce qu'une bûche est
    // la même vue de face et de dos. Sans canoniser l'angle modulo cette
    // symétrie, rien n'envoyait sur `axis=x` — et quatre rotations rendaient
    // `axis=z → axis=x`.
    let d = pack();
    let t = table(&d);
    assert_eq!(
        t.transformer("t:buche|axis=x", Transfo::Rot90).as_deref(),
        Some("t:buche|axis=z")
    );
    assert_eq!(
        t.transformer("t:buche|axis=z", Transfo::Rot90).as_deref(),
        Some("t:buche|axis=x")
    );
    assert_eq!(
        t.transformer("t:buche|axis=y", Transfo::Rot90).as_deref(),
        Some("t:buche|axis=y"),
        "l'axe vertical ne bouge pas sous une rotation autour de Y"
    );
}

#[test]
fn une_trappe_fermee_tourne_comme_une_trappe_ouverte() {
    // Fermée, son orientation est INVISIBLE : quatre états rendent la même
    // géométrie. La géométrie seule ne peut pas trancher — c'est la permutation
    // dérivée des états ouverts qui s'applique à tous. Et il le faut : son
    // `facing` doit pointer au bon endroit quand on l'ouvrira.
    let d = pack();
    let t = table(&d);
    assert_eq!(
        t.transformer("t:trappe|facing=north,open=false", Transfo::Rot90)
            .map(|s| norm(&s))
            .as_deref(),
        Some("t:trappe|facing=east,open=false")
    );
    assert_eq!(
        t.transformer("t:trappe|facing=north,open=true", Transfo::Rot90)
            .map(|s| norm(&s))
            .as_deref(),
        Some("t:trappe|facing=east,open=true")
    );
}

#[test]
fn un_bloc_sans_etat_traverse_sans_rien_changer() {
    let d = pack();
    let t = table(&d);
    // `t:pierre` n'a qu'une variante : rien à dériver, donc la table ne le
    // connaît pas…
    assert!(!t.connait("t:pierre", Transfo::Rot90));
    // … et pourtant sa transformation est CONNUE : un état sans propriété est
    // le même dans toutes les orientations. Rendre `None` le faisait compter
    // parmi les états « laissés tels quels » — la pierre et l'air de tout
    // build noyaient les vrais trous du compte rendu. Vrai aussi d'un bloc que
    // le pack ne déclare même pas.
    for t_ in tf_blocks::TOUTES {
        for cle in ["t:pierre", "minecraft:air", "mod:inconnu"] {
            assert_eq!(t.transformer(cle, t_).as_deref(), Some(cle), "{cle} {t_:?}");
        }
    }
    // Un état À propriétés que la table ne connaît pas reste, lui, un vrai
    // trou — c'est celui-là que le compte rendu doit nommer.
    assert_eq!(
        t.transformer("mod:inconnu|facing=north", Transfo::Rot90),
        None
    );
}

// ── les miroirs ─────────────────────────────────────────────────────────────

#[test]
fn un_miroir_echange_les_orientations_du_bon_axe() {
    let d = pack();
    let t = table(&d);
    // `x → −x` laisse le nord et le sud en place, échange l'est et l'ouest.
    assert_eq!(
        t.transformer("t:echelle|facing=north", Transfo::MiroirX)
            .as_deref(),
        Some("t:echelle|facing=north")
    );
    assert_eq!(
        t.transformer("t:echelle|facing=east", Transfo::MiroirX)
            .as_deref(),
        Some("t:echelle|facing=west")
    );
    // `z → −z` fait l'inverse.
    assert_eq!(
        t.transformer("t:echelle|facing=north", Transfo::MiroirZ)
            .as_deref(),
        Some("t:echelle|facing=south")
    );
    assert_eq!(
        t.transformer("t:echelle|facing=east", Transfo::MiroirZ)
            .as_deref(),
        Some("t:echelle|facing=east")
    );
}

#[test]
fn un_miroir_echange_la_main_gauche_et_la_main_droite() {
    // Un escalier réfléchi n'est PAS un escalier tourné : son `shape` passe de
    // `inner_left` à `inner_right`, et le pack déclare ça comme deux modèles
    // différents. La dérivation par les angles trouve un état — le mauvais.
    // Celle-ci compare les CUBOÏDES.
    let d = pack();
    let t = table(&d);
    let r = t
        .transformer("t:marche|facing=north,main=gauche", Transfo::MiroirX)
        .map(|s| norm(&s));
    assert_eq!(
        r.as_deref(),
        Some("t:marche|facing=north,main=droite"),
        "le miroir doit changer de main"
    );
}

// ── les lois du groupe ──────────────────────────────────────────────────────

/// Tous les états déclarés du pack de test.
fn tous_les_etats(d: &TempDir) -> Vec<String> {
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).unwrap();
    cat.resoudre_modeles(&src);
    let mut out = Vec::new();
    for (nom, bs) in cat.blocs() {
        if let tf_assets::Blockstate::Variants(v) = bs {
            for (cle, _) in v {
                if !cle.is_empty() {
                    out.push(format!("{nom}|{cle}"));
                }
            }
        }
    }
    out
}

#[test]
fn quatre_rotations_de_90_degres_rendent_l_etat_de_depart() {
    // C'est ce qui rend `//rotate 90` puis `//rotate -90` sûr. Et c'est vrai
    // par CONSTRUCTION : `rot180` et `rot270` sont `rot90` composée, jamais
    // trouvées séparément. Trouvées séparément, elles se contredisaient —
    // mesuré sur le pack du serveur, seize états violaient la loi.
    let d = pack();
    let t = table(&d);
    for etat in tous_les_etats(&d) {
        if !t.connait(etat.split('|').next().unwrap(), Transfo::Rot90) {
            continue;
        }
        let mut c = etat.clone();
        for _ in 0..4 {
            c = t.transformer(&c, Transfo::Rot90).expect("la règle existe");
        }
        assert_eq!(norm(&c), norm(&etat), "{etat} après quatre rotations");
    }
}

#[test]
fn deux_rotations_de_90_font_exactement_une_rotation_de_180() {
    let d = pack();
    let t = table(&d);
    for etat in tous_les_etats(&d) {
        let bloc = etat.split('|').next().unwrap();
        if !t.connait(bloc, Transfo::Rot90) || !t.connait(bloc, Transfo::Rot180) {
            continue;
        }
        let deux = t
            .transformer(&etat, Transfo::Rot90)
            .and_then(|x| t.transformer(&x, Transfo::Rot90))
            .unwrap();
        let direct = t.transformer(&etat, Transfo::Rot180).unwrap();
        assert_eq!(norm(&deux), norm(&direct), "{etat}");
    }
}

#[test]
fn un_miroir_applique_deux_fois_ne_fait_rien() {
    let d = pack();
    let t = table(&d);
    for etat in tous_les_etats(&d) {
        let bloc = etat.split('|').next().unwrap();
        for m in [Transfo::MiroirX, Transfo::MiroirZ] {
            if !t.connait(bloc, m) {
                continue;
            }
            let aller = t.transformer(&etat, m).unwrap();
            let retour = t.transformer(&aller, m).unwrap();
            assert_eq!(norm(&retour), norm(&etat), "{etat} · {}", m.nom());
        }
    }
}

#[test]
fn une_transformation_est_une_bijection() {
    // Deux états qui tomberaient sur un seul DÉTRUISENT de l'information, et le
    // font en silence. C'est la vérification qui manquait, et sans elle une
    // bûche revenait `axis=x` après quatre rotations partant de `axis=z`.
    let d = pack();
    let t = table(&d);
    let etats = tous_les_etats(&d);
    for tr in TOUTES {
        let mut vus = std::collections::BTreeSet::new();
        for etat in &etats {
            if !t.connait(etat.split('|').next().unwrap(), tr) {
                continue;
            }
            let image = norm(&t.transformer(etat, tr).unwrap());
            assert!(
                vus.insert(image.clone()),
                "{tr:?} : {etat} et un autre → {image}"
            );
        }
    }
}

#[test]
fn une_rotation_non_representable_est_refusee_et_non_devinee() {
    // `t:demi_tour` n'a que deux faces, nord et sud : la tourner de 90° mènerait
    // à un état que le pack ne déclare pas. On refuse plutôt que de rendre
    // l'identité — une transformation qui ne se défait pas est pire qu'une
    // transformation absente.
    let d = pack();
    let t = table(&d);
    assert!(
        !t.connait("t:demi_tour", Transfo::Rot90),
        "une rotation de 90° impossible ne doit pas être inventée"
    );
    assert!(
        t.connait("t:demi_tour", Transfo::Rot180),
        "mais la rotation de 180°, elle, est parfaitement représentable"
    );
    assert_eq!(
        t.transformer("t:demi_tour|face=north", Transfo::Rot180)
            .as_deref(),
        Some("t:demi_tour|face=south")
    );
}

// ── les positions ───────────────────────────────────────────────────────────

#[test]
fn une_rotation_de_position_suit_le_repere_minecraft() {
    // +X = Est, +Z = Sud. Une rotation de 90° va donc de l'est vers le sud.
    let o = [0, 0, 0];
    assert_eq!(Transfo::Rot90.position([1, 5, 0], o), [0, 5, 1]);
    assert_eq!(Transfo::Rot180.position([1, 5, 0], o), [-1, 5, 0]);
    assert_eq!(Transfo::Rot270.position([1, 5, 0], o), [0, 5, -1]);
    assert_eq!(
        Transfo::Rot90.position([3, 7, 2], [3, 0, 2]),
        [3, 7, 2],
        "l'origine ne bouge pas"
    );
}

#[test]
fn un_miroir_de_position_ne_touche_pas_la_hauteur() {
    let o = [0, 0, 0];
    assert_eq!(Transfo::MiroirX.position([4, 9, 7], o), [-4, 9, 7]);
    assert_eq!(Transfo::MiroirZ.position([4, 9, 7], o), [4, 9, -7]);
}

#[test]
fn les_positions_respectent_les_memes_lois_que_les_etats() {
    let o = [10, 0, -3];
    for p in [[0, 0, 0], [15, 4, -20], [-7, 200, 7]] {
        let mut c = p;
        for _ in 0..4 {
            c = Transfo::Rot90.position(c, o);
        }
        assert_eq!(c, p, "quatre rotations");
        for m in [Transfo::MiroirX, Transfo::MiroirZ] {
            assert_eq!(m.position(m.position(p, o), o), p, "{}", m.nom());
        }
        assert_eq!(
            Transfo::Rot90.position(Transfo::Rot90.position(p, o), o),
            Transfo::Rot180.position(p, o)
        );
    }
}

#[test]
fn l_inverse_d_une_transformation_l_annule() {
    let d = pack();
    let t = table(&d);
    for etat in tous_les_etats(&d) {
        let bloc = etat.split('|').next().unwrap();
        for tr in TOUTES {
            if !t.connait(bloc, tr) || !t.connait(bloc, tr.inverse()) {
                continue;
            }
            let aller = t.transformer(&etat, tr).unwrap();
            let retour = t.transformer(&aller, tr.inverse()).unwrap();
            assert_eq!(norm(&retour), norm(&etat), "{etat} · {}", tr.nom());
        }
    }
}

// ── ce qu'une permutation par propriété ne sait pas dire ────────────────────

#[test]
fn une_regle_exacte_dit_ce_qu_aucune_permutation_de_propriete_ne_peut() {
    // `t:coin` mélange deux familles dans un seul bloc :
    //
    //  · une MARCHE, chirale, dont le miroir est-ouest est est ↔ ouest ;
    //  · un QUART de bloc, dont la forme réfléchie est aussi sa forme tournée,
    //    et qui n'a qu'UNE écriture par coin — son miroir est donc est → sud.
    //
    // Les deux sont géométriquement justes, et aucune permutation de `facing`
    // ne fait les deux. C'est le cas RÉEL des escaliers du serveur
    // (`shape=outer`), et c'est pour lui que la règle est une permutation
    // d'ÉTATS et pas de propriétés.
    let d = pack();
    let t = table(&d);
    assert_eq!(
        t.transformer("t:coin|facing=east,forme=droite", Transfo::MiroirX)
            .as_deref(),
        Some("t:coin|facing=west,forme=droite"),
        "la marche se réfléchit est ↔ ouest"
    );
    assert_eq!(
        t.transformer("t:coin|facing=east,forme=coin", Transfo::MiroirX)
            .as_deref(),
        Some("t:coin|facing=south,forme=coin"),
        "le quart de bloc n'a qu'une écriture par coin : son miroir est une rotation"
    );
    // Et la table le DIT, au lieu de le taire.
    assert!(
        t.manques.iter().any(|m| matches!(
            m,
            tf_blocks::Manque::NonDecomposable { bloc, propriete, transfo }
                if bloc == "t:coin" && propriete == "facing" && *transfo == Transfo::MiroirX
        )),
        "la forme compacte doit signaler ce qu'elle ne peut pas porter"
    );
}

#[test]
fn un_bloc_sans_la_moindre_orientation_ne_bouge_pas() {
    // Trois bougies, aucune propriété qui porte une orientation : `nombre`
    // compte, il n'oriente pas. Aucune rotation de leurs formes n'est
    // déclarée nulle part — et pourtant refuser serait le mauvais choix.
    // L'identité est la seule fonction TOTALE sur cet espace d'états, c'est ce
    // que fait le jeu, et refuser rendrait `//rotate` inutilisable sur tout un
    // mur décoré.
    let d = pack();
    let t = table(&d);
    for tr in TOUTES {
        assert!(t.connait("t:bougie", tr), "{}", tr.nom());
        assert_eq!(
            t.transformer("t:bougie|nombre=2", tr).as_deref(),
            Some("t:bougie|nombre=2"),
            "{}",
            tr.nom()
        );
    }
    // Ce qui ne doit surtout PAS s'étendre au bloc à deux faces : lui a un
    // modèle déclaré à deux angles, donc `y` porte du sens, donc l'angle
    // manquant est un vrai trou.
    assert!(!t.connait("t:demi_tour", Transfo::Rot90));
}

#[test]
fn le_meme_solide_ecrit_autrement_reste_le_meme_solide() {
    // Un cube d'un bloc et le même cube en quatre morceaux. Comparer les
    // listes de cuboïdes répondrait « formes différentes » — c'est ce qui
    // faisait rater aux escaliers en coin leur propre miroir, et retomber sur
    // l'escalier TOURNÉ. La comparaison porte sur le SOLIDE.
    let d = pack();
    let t = table(&d);
    for tr in TOUTES {
        assert!(t.connait("t:decoupe", tr), "{}", tr.nom());
        assert_eq!(
            t.transformer("t:decoupe|pose=entier", tr).as_deref(),
            Some("t:decoupe|pose=entier"),
            "{}",
            tr.nom()
        );
    }
}

#[test]
fn un_etat_que_le_pack_ne_declare_pas_passe_par_la_forme_compacte() {
    // Un monde peut être plus récent que le pack. La permutation exacte ne
    // connaît que les états déclarés : le repli par propriété prend la suite,
    // sinon la moitié d'un build resterait droite pendant que l'autre tourne.
    let d = pack();
    let t = table(&d);
    assert_eq!(
        t.transformer("t:echelle|facing=east,inconnu=7", Transfo::Rot90)
            .as_deref(),
        Some("t:echelle|facing=south,inconnu=7")
    );
}

#[test]
fn les_deux_miroirs_sont_liees_par_le_demi_tour() {
    // `z → −z` est `x → −x` suivi d'un demi-tour. Dérivés séparément, les deux
    // miroirs divergeaient là où le pack est asymétrique — mesuré sur le pack
    // du serveur, 97,0 % contre 84,5 % sur exactement les mêmes blocs.
    let d = pack();
    let t = table(&d);
    for etat in tous_les_etats(&d) {
        let bloc = etat.split('|').next().unwrap();
        if !t.connait(bloc, Transfo::MiroirX) || !t.connait(bloc, Transfo::Rot180) {
            continue;
        }
        let compose = t
            .transformer(&etat, Transfo::MiroirX)
            .and_then(|x| t.transformer(&x, Transfo::Rot180))
            .unwrap();
        assert_eq!(
            norm(&compose),
            norm(&t.transformer(&etat, Transfo::MiroirZ).unwrap()),
            "{etat}"
        );
    }
}

#[test]
fn toute_regle_rend_la_bonne_forme_ou_le_dit() {
    // Le contrôle qui ne relit RIEN du raisonnement qui a produit la règle :
    // on prend l'état d'arrivée et on compare son SOLIDE à celui de la source
    // transformée. Les lois du groupe ne savent pas voir ça — une règle qui
    // tournerait tout d'un quart de trop les passerait toutes.
    //
    // Un écart est tolérable quand le pack ne déclare la bonne forme nulle
    // part (un bloc chiral sans jumeau, une bougie). Il ne l'est JAMAIS quand
    // elle est là et qu'on ne l'a pas prise : c'est le bug qui a fait rendre à
    // tous les escaliers en coin et à toutes les portes ouvertes du pack du
    // serveur leur version TOURNÉE au lieu de la réfléchie, et les lois du
    // groupe n'y voyaient rien. Alors la table doit l'ANNONCER, faute de quoi
    // l'utilisateur n'a aucun moyen de distinguer « ce bloc ne peut pas
    // tourner » de « ce bloc tourne de travers ».
    let d = pack();
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Codex);
    cat.charger_codex(&src).unwrap();
    cat.resoudre_modeles(&src);
    let t = Table::deriver(&cat);

    let mut fausses = 0usize;
    let mut annoncees = 0usize;
    for (nom, bs) in cat.blocs() {
        let tf_assets::Blockstate::Variants(variants) = bs else {
            continue;
        };
        let geo: std::collections::BTreeMap<String, (tf_assets::Id, u16, u16)> = variants
            .iter()
            .filter_map(|(cle, v)| {
                v.first()
                    .map(|v| (norm(cle), (v.modele.clone(), v.x % 360, v.y % 360)))
            })
            .collect();
        for tr in TOUTES {
            for cle in geo.keys() {
                let etat = format!("{nom}|{cle}");
                let Some(arrivee) = t.transformer(&etat, tr) else {
                    continue;
                };
                let apres = norm(arrivee.split_once('|').map_or("", |(_, p)| p));
                let (Some(depart), Some(cible)) = (geo.get(cle), geo.get(&apres)) else {
                    continue;
                };
                let attendu =
                    tf_blocks::geometrie::empreinte(&cat, &depart.0, depart.1, depart.2, Some(tr));
                let obtenu =
                    tf_blocks::geometrie::empreinte(&cat, &cible.0, cible.1, cible.2, None);
                if attendu.is_some() && attendu != obtenu {
                    fausses += 1;
                    assert!(
                        t.manques.iter().any(|m| matches!(
                            m,
                            tf_blocks::Manque::FormeApprochee { bloc, transfo, .. }
                                if bloc == nom && transfo == &tr
                        )),
                        "{etat} · {} → {arrivee} rend une autre forme, sans que la table le dise",
                        tr.nom()
                    );
                }
            }
        }
    }
    for m in &t.manques {
        if let tf_blocks::Manque::FormeApprochee { etats, .. } = m {
            annoncees += etats;
        }
    }
    // Le pack de test CONTIENT des cas approchés (la marche chirale sans son
    // jumeau, les bougies) : zéro ici voudrait dire que le contrôle ne
    // contrôle rien.
    assert!(fausses > 0, "le pack de test doit exercer le cas approché");
    assert_eq!(
        fausses, annoncees,
        "la table doit annoncer exactement les formes approchées, ni plus ni moins"
    );
}
