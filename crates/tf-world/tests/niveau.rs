//! **`level.dat` : où ouvrir un monde.**
//!
//! Les fichiers sont fabriqués ici, avec ce que le jeu y met d'encombrant —
//! des réglages de génération, des listes, des tableaux — pour que le lecteur
//! ciblé prouve qu'il SAUTE ce qu'il ne cherche pas.

use tf_anvil::{deflate, Compression};
use tf_nbt::{tag, Writer};
use tf_world::niveau::{lire, lire_fichier, Niveau};

/// Un `level.dat` tel que 1.18 l'écrit, à quelques champs près.
fn level_dat(joueur: Option<([f64; 3], &str)>, gzip: bool) -> Vec<u8> {
    let mut w = Writer::new();
    w.field(tag::COMPOUND, "");
    w.field(tag::COMPOUND, "Data");
    // De l'encombrant d'abord : ce qu'il faut savoir sauter.
    w.field(tag::COMPOUND, "WorldGenSettings");
    w.field(tag::LONG, "seed").raw(&42i64.to_be_bytes());
    w.field(tag::COMPOUND, "dimensions");
    w.field(tag::STRING, "type").raw_str("minecraft:overworld");
    w.end();
    w.end();
    w.field(tag::LIST, "ServerBrands")
        .list_header(tag::STRING, 2)
        .raw_str("vanilla")
        .raw_str("fabric");
    w.field(tag::LONG_ARRAY, "Big")
        .long_array_payload(&[1, 2, 3, 4]);
    w.field(tag::STRING, "LevelName")
        .raw_str("Minefield — ville");
    w.field(tag::INT, "SpawnX").i32_payload(-120);
    w.field(tag::INT, "SpawnY").i32_payload(64);
    w.field(tag::INT, "SpawnZ").i32_payload(300);
    if let Some((pos, dim)) = joueur {
        w.field(tag::COMPOUND, "Player");
        w.field(tag::LIST, "Inventory")
            .list_header(tag::COMPOUND, 1)
            .field(tag::STRING, "id")
            .raw_str("minecraft:diamond_pickaxe")
            .end();
        w.field(tag::LIST, "Pos").list_header(tag::DOUBLE, 3);
        for v in pos {
            w.raw(&v.to_bits().to_be_bytes());
        }
        w.field(tag::STRING, "Dimension").raw_str(dim);
        w.end();
    }
    w.end();
    w.end();
    let brut = w.into_bytes();
    if gzip {
        deflate(&brut, Compression::Gzip).unwrap()
    } else {
        brut
    }
}

#[test]
fn le_nom_l_apparition_et_le_joueur_se_lisent_a_travers_ce_qu_on_saute() {
    let n = lire(&level_dat(
        Some(([4012.7, 70.0, -3999.2], "minecraft:overworld")),
        true,
    ))
    .unwrap();
    assert_eq!(n.nom.as_deref(), Some("Minefield — ville"));
    assert_eq!(n.apparition, Some([-120, 64, 300]));
    assert_eq!(n.joueur, Some([4012.7, 70.0, -3999.2]));
    assert_eq!(
        n.ou_regarder(),
        Some([4012, 70, -4000]),
        "le joueur d'abord — et en division PLANCHER : −3 999,2 est dans le \
         bloc −4 000"
    );
}

#[test]
fn un_joueur_dans_le_nether_fait_ouvrir_au_point_d_apparition() {
    // Ses coordonnées désignent un autre endroit : la coque n'ouvre que la
    // surface.
    let n = lire(&level_dat(
        Some(([500.0, 40.0, 500.0], "minecraft:the_nether")),
        true,
    ))
    .unwrap();
    assert_eq!(n.dimension_joueur.as_deref(), Some("minecraft:the_nether"));
    assert_eq!(n.ou_regarder(), Some([-120, 64, 300]));
}

#[test]
fn sans_joueur_on_ouvre_au_point_d_apparition() {
    // Une save de serveur : le joueur vit dans `playerdata/`, pas ici.
    let n = lire(&level_dat(None, true)).unwrap();
    assert_eq!(n.joueur, None);
    assert_eq!(n.ou_regarder(), Some([-120, 64, 300]));
}

#[test]
fn une_position_qui_n_est_pas_un_nombre_est_ignoree() {
    let n = lire(&level_dat(
        Some(([f64::NAN, 64.0, 0.0], "minecraft:overworld")),
        true,
    ))
    .unwrap();
    assert_eq!(n.joueur, None, "ouvrir « nulle part » n'est pas une option");
    assert_eq!(n.ou_regarder(), Some([-120, 64, 300]));
}

#[test]
fn un_level_dat_non_compresse_se_lit_aussi() {
    let n = lire(&level_dat(None, false)).unwrap();
    assert_eq!(n.nom.as_deref(), Some("Minefield — ville"));
}

#[test]
fn l_ancienne_dimension_numerique_se_traduit() {
    // Avant 1.16, `Dimension` est un entier.
    let mut w = Writer::new();
    w.field(tag::COMPOUND, "").field(tag::COMPOUND, "Data");
    w.field(tag::COMPOUND, "Player");
    w.field(tag::INT, "Dimension").i32_payload(-1);
    w.end().end().end();
    let n = lire(&w.into_bytes()).unwrap();
    assert_eq!(n.dimension_joueur.as_deref(), Some("minecraft:the_nether"));
}

#[test]
fn un_fichier_abime_ne_fait_ni_paniquer_ni_mentir() {
    let bon = level_dat(Some(([1.0, 2.0, 3.0], "minecraft:overworld")), false);
    // Toutes les troncatures : aucune ne doit paniquer, et aucune ne doit
    // rendre un point à moitié lu.
    for n in 0..bon.len() {
        if let Some(niveau) = lire(&bon[..n]) {
            panic!("tronqué à {n} octets, et lu quand même : {niveau:?}");
        }
    }
    assert_eq!(lire(b"pas un level.dat"), None);
    assert_eq!(lire(&[]), None);
}

#[test]
fn une_save_sans_level_dat_rend_rien() {
    let d = std::env::temp_dir().join(format!(
        "tf-niveau-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    assert_eq!(lire_fichier(&d), None);
    std::fs::write(d.join("level.dat"), level_dat(None, true)).unwrap();
    assert!(matches!(
        lire_fichier(&d),
        Some(Niveau {
            apparition: Some(_),
            ..
        })
    ));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn les_saves_d_une_installation_se_listent_de_la_plus_recente_a_la_plus_ancienne() {
    use tf_world::niveau::saves_de;
    let d = std::env::temp_dir().join(format!(
        "tf-saves-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let saves = d.join("saves");
    for s in ["ancienne", "recente", "sans-nom"] {
        std::fs::create_dir_all(saves.join(s)).unwrap();
    }
    std::fs::write(saves.join("ancienne/level.dat"), level_dat(None, true)).unwrap();
    // Un level.dat illisible : la save se propose quand même, sous le nom
    // de son dossier.
    std::fs::write(saves.join("sans-nom/level.dat"), b"abime").unwrap();
    std::fs::write(saves.join("recente/level.dat"), level_dat(None, true)).unwrap();
    // Des dates POSÉES, pas attendues : une horloge de système de fichiers à
    // la seconde rendrait un `sleep` de quelques millisecondes aléatoire.
    let dater = |s: &str, secondes: u64| {
        std::fs::File::options()
            .write(true)
            .open(saves.join(s).join("level.dat"))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secondes))
            .unwrap();
    };
    dater("ancienne", 1_000_000);
    dater("sans-nom", 1_500_000);
    dater("recente", 2_000_000);
    // Ni un dossier sans level.dat, ni un fichier ne sont des saves.
    std::fs::create_dir_all(saves.join("captures")).unwrap();
    std::fs::write(saves.join("notes.txt"), b"x").unwrap();

    let trouvees = saves_de(&d);
    let dossiers: Vec<String> = trouvees
        .iter()
        .map(|s| s.chemin.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(dossiers.len(), 3, "{dossiers:?}");
    assert_eq!(
        dossiers,
        ["recente", "sans-nom", "ancienne"],
        "la dernière partie d'abord"
    );
    let nom_de = |d: &str| {
        trouvees
            .iter()
            .find(|s| s.chemin.ends_with(d))
            .unwrap()
            .nom
            .clone()
    };
    assert_eq!(nom_de("recente"), "Minefield — ville", "le nom du JEU");
    assert_eq!(nom_de("sans-nom"), "sans-nom", "à défaut, celui du dossier");
    assert!(saves_de(&d.join("nulle-part")).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}
