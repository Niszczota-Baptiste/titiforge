//! Quels états portent un fluide, et ce que l'atlas doit monter pour eux.
//!
//! Le pack de test reprend la forme du vrai : `block/water` sans élément, un
//! escalier dont les variantes ne parlent pas de `waterlogged`, du varech en
//! croix. C'est ce que le jeu livre, et c'est là qu'une classification par
//! la forme se tromperait.

use std::fs;
use std::path::{Path, PathBuf};

use tf_assets::catalogue::{table_formes, Disposition};
use tf_assets::fluides::{self, fluide_de};
use tf_assets::{textures_des_etats, Atlas, Catalogue, Dossier};
use tf_mesh::{Fluide, Formes, GenreFluide, TextureFluide};

struct TempDir(PathBuf);

impl TempDir {
    fn new(nom: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "tf-assets-fluides-{nom}-{}-{:?}",
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
    fn ecrire(&self, chemin: &str, contenu: &[u8]) {
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

fn png_uni(cote: u32, c: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, cote, cote);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .unwrap()
            .write_image_data(&c.repeat((cote * cote) as usize))
            .unwrap();
    }
    out
}

fn etat(props: &[(&str, &str)]) -> Vec<(String, String)> {
    props
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn le_fluide_se_lit_dans_l_etat() {
    let eau = |niveau| {
        Some(Fluide {
            genre: GenreFluide::Eau,
            niveau,
        })
    };
    assert_eq!(
        fluide_de("minecraft:water", &etat(&[("level", "0")])),
        eau(0)
    );
    assert_eq!(
        fluide_de("minecraft:water", &etat(&[("level", "7")])),
        eau(7)
    );
    assert_eq!(
        fluide_de("minecraft:water", &etat(&[("level", "12")])),
        eau(12),
        "une chute"
    );
    assert_eq!(
        fluide_de("minecraft:water", &[]),
        eau(0),
        "sans niveau : une source"
    );
    assert_eq!(
        fluide_de("minecraft:lava", &etat(&[("level", "3")])),
        Some(Fluide {
            genre: GenreFluide::Lave,
            niveau: 3
        })
    );
    // Tout ce qui s'inonde, vanilla comme `minefield:*`.
    for nom in ["minecraft:oak_stairs", "minefield:banc_de_parc"] {
        assert_eq!(
            fluide_de(nom, &etat(&[("facing", "east"), ("waterlogged", "true")])),
            eau(0),
            "{nom}"
        );
        assert_eq!(
            fluide_de(nom, &etat(&[("waterlogged", "false")])),
            None,
            "{nom}"
        );
    }
    // Le fond de la mer : pleins d'eau sans propriété qui le dise.
    for nom in fluides::TOUJOURS_INONDES {
        assert_eq!(fluide_de(nom, &etat(&[("age", "4")])), eau(0), "{nom}");
    }
    assert_eq!(fluide_de("minecraft:stone", &[]), None);
    assert_eq!(
        fluide_de("minecraft:water_cauldron", &etat(&[("level", "3")])),
        None
    );
}

fn pack() -> TempDir {
    let d = TempDir::new("pack");
    let e = |c: &str, j: &str| d.ecrire(c, j.as_bytes());
    // L'eau et la lave comme le jeu les livre : un modèle SANS élément.
    e(
        "assets/minecraft/blockstates/water.json",
        r#"{"variants":{"":{"model":"minecraft:block/water"}}}"#,
    );
    e(
        "assets/minecraft/blockstates/lava.json",
        r#"{"variants":{"":{"model":"minecraft:block/lava"}}}"#,
    );
    e(
        "assets/minecraft/models/block/water.json",
        r#"{"textures":{"particle":"block/water_still"}}"#,
    );
    e(
        "assets/minecraft/models/block/lava.json",
        r#"{"textures":{"particle":"block/lava_still"}}"#,
    );
    // Un escalier dont les variantes ne disent rien de `waterlogged`.
    e(
        "assets/minecraft/blockstates/stairs.json",
        r#"{"variants":{"facing=east":{"model":"minecraft:block/stairs"}}}"#,
    );
    e(
        "assets/minecraft/models/block/stairs.json",
        r#"{"elements":[
            {"from":[0,0,0],"to":[16,8,16],"faces":{"up":{"texture":"block/stone"}}},
            {"from":[8,8,0],"to":[16,16,16],"faces":{"up":{"texture":"block/stone"}}}]}"#,
    );
    // Du varech : deux plans en croix.
    e(
        "assets/minecraft/blockstates/kelp.json",
        r#"{"variants":{"":{"model":"minecraft:block/kelp"}}}"#,
    );
    e(
        "assets/minecraft/models/block/kelp.json",
        r#"{"elements":[{"from":[0.8,0,8],"to":[15.2,16,8],
            "faces":{"north":{"texture":"block/kelp"}}}]}"#,
    );
    e(
        "assets/minecraft/blockstates/stone.json",
        r#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#,
    );
    e(
        "assets/minecraft/models/block/stone.json",
        r#"{"elements":[{"from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"block/stone","cullface":"up"}}}]}"#,
    );
    d
}

fn catalogue(d: &TempDir) -> Catalogue {
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Pack);
    for b in ["water", "lava", "stairs", "kelp", "stone"] {
        cat.charger_bloc(&src, &format!("minecraft:{b}")).unwrap();
    }
    cat.resoudre_modeles(&src);
    cat
}

#[test]
fn la_table_porte_le_fluide_et_la_forme_ensemble() {
    let d = pack();
    let cat = catalogue(&d);
    let cles = [
        "minecraft:water|level=0",
        "minecraft:water|level=3",
        "minecraft:lava|level=8",
        "minecraft:stairs|facing=east,waterlogged=true",
        "minecraft:stairs|facing=east,waterlogged=false",
        "minecraft:kelp|age=5",
        "minecraft:stone",
    ];
    let t = table_formes(&cat, cles.iter().map(|c| c.to_string()), &|_| false);
    let eau = |niveau| {
        Some(Fluide {
            genre: GenreFluide::Eau,
            niveau,
        })
    };
    // L'eau reste de l'AIR pour les passes de blocs : ni opaque, ni cuboïde.
    assert!(t.est_air(0) && !t.opaque(0) && t.cuboides(0).is_empty());
    assert_eq!(t.fluide(0), eau(0));
    assert_eq!(t.fluide(1), eau(3));
    assert_eq!(
        t.fluide(2),
        Some(Fluide {
            genre: GenreFluide::Lave,
            niveau: 8
        })
    );
    // L'escalier inondé est un modèle ET de l'eau.
    assert_eq!(t.cuboides(3).len(), 2, "l'escalier garde ses deux cuboïdes");
    assert_eq!(t.fluide(3), eau(0));
    assert_eq!(t.cuboides(4).len(), 2);
    assert_eq!(t.fluide(4), None, "à sec");
    // Le varech : une croix, de l'eau, et rien qui arrête un fluide.
    assert_eq!(t.fluide(5), eau(0));
    assert!(
        !t.solide(5),
        "deux plans sans épaisseur n'arrêtent pas l'eau"
    );
    assert!(t.solide(3), "un escalier, si");
    assert!(t.solide(6) && t.fluide(6).is_none());
}

#[test]
fn l_atlas_monte_les_textures_des_fluides_presents() {
    let d = pack();
    let cat = catalogue(&d);
    let voulues = |cles: &[&str]| textures_des_etats(&cat, cles.iter().map(|c| c.to_string()));
    let eau = voulues(&["minecraft:water|level=0"]);
    for t in [
        fluides::EAU_IMMOBILE,
        fluides::EAU_COURANTE,
        fluides::EAU_VOILE,
    ] {
        assert!(eau.iter().any(|v| v == t), "{t} dans {eau:?}");
    }
    assert!(!eau.iter().any(|v| v.contains("lava")));
    let inonde = voulues(&["minecraft:stairs|facing=east,waterlogged=true"]);
    assert!(
        inonde.iter().any(|v| v == fluides::EAU_IMMOBILE),
        "{inonde:?}"
    );
    let lave = voulues(&["minecraft:lava|level=0"]);
    assert!(lave.iter().any(|v| v == fluides::LAVE_COURANTE), "{lave:?}");
    let sec = voulues(&["minecraft:stairs|facing=east,waterlogged=false"]);
    assert!(!sec.iter().any(|v| v.contains("water")), "{sec:?}");
}

#[test]
fn chaque_face_de_fluide_trouve_sa_couche_et_se_replie_sans_voile() {
    // Un pack SANS `water_overlay` — c'est le cas du codex du site, où aucun
    // modèle ne le cite. Le voile se replie sur le courant.
    let d = TempDir::new("atlas");
    for (nom, c) in [
        ("water_still", [150, 150, 150, 180]),
        ("water_flow", [160, 160, 160, 180]),
        ("lava_still", [220, 100, 20, 255]),
        ("lava_flow", [230, 110, 30, 255]),
    ] {
        d.ecrire(
            &format!("assets/minecraft/textures/block/{nom}.png"),
            &png_uni(16, c),
        );
    }
    let src = Dossier::ouvrir(d.path()).unwrap();
    let noms: Vec<String> = fluides::textures(GenreFluide::Eau)
        .iter()
        .chain(fluides::textures(GenreFluide::Lave))
        .map(|t| t.to_string())
        .collect();
    let atlas = Atlas::batir(&src, noms, &|n| Disposition::Pack.chemins_texture(n));
    let couche = |g, t| fluides::couche(&atlas, g, t);
    let nommee = |n: &str| atlas.couche(n).expect(n);
    assert_eq!(
        couche(GenreFluide::Eau, TextureFluide::Immobile),
        nommee(fluides::EAU_IMMOBILE)
    );
    assert_eq!(
        couche(GenreFluide::Eau, TextureFluide::Courant),
        nommee(fluides::EAU_COURANTE)
    );
    assert_eq!(
        couche(GenreFluide::Eau, TextureFluide::Voile),
        nommee(fluides::EAU_COURANTE),
        "sans voile, le courant"
    );
    assert_eq!(
        couche(GenreFluide::Lave, TextureFluide::Immobile),
        nommee(fluides::LAVE_IMMOBILE)
    );
    assert_eq!(
        couche(GenreFluide::Lave, TextureFluide::Voile),
        nommee(fluides::LAVE_COURANTE),
        "la lave n'a pas de voile"
    );
}
