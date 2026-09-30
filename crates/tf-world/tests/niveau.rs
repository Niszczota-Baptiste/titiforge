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
    w.field(tag::INT, "DataVersion").i32_payload(2975);
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
    assert_eq!(
        n.data_version,
        Some(2975),
        "la version du jeu qui l'a écrit"
    );
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

// ── le générateur : ce monde est-il VIDE ? ──────────────────────────────────

/// Un `level.dat` 1.18 dont la surface est générée par `generateur` ; le
/// Nether, lui, est toujours un monde plat vide — pour prouver qu'on lit la
/// SURFACE et pas la première dimension venue.
fn level_dat_genere(generateur: impl Fn(&mut Writer), data_version: bool) -> Vec<u8> {
    let mut w = Writer::new();
    w.field(tag::COMPOUND, "");
    w.field(tag::COMPOUND, "Data");
    w.field(tag::COMPOUND, "WorldGenSettings");
    w.field(tag::BYTE, "bonus_chest").i8_payload(0);
    w.field(tag::LONG, "seed").raw(&42i64.to_be_bytes());
    w.field(tag::COMPOUND, "dimensions");
    w.field(tag::COMPOUND, "minecraft:the_nether");
    w.field(tag::STRING, "type").raw_str("minecraft:the_nether");
    w.field(tag::COMPOUND, "generator");
    w.field(tag::STRING, "type").raw_str("minecraft:flat");
    w.field(tag::COMPOUND, "settings");
    w.field(tag::LIST, "layers").list_header(tag::COMPOUND, 0);
    w.end().end().end();
    w.field(tag::COMPOUND, "minecraft:overworld");
    w.field(tag::STRING, "type").raw_str("minecraft:overworld");
    w.field(tag::COMPOUND, "generator");
    generateur(&mut w);
    w.end().end();
    w.end().end();
    if data_version {
        w.field(tag::INT, "DataVersion").i32_payload(2975);
    }
    w.field(tag::STRING, "LevelName").raw_str("Bac à sable");
    w.end().end();
    deflate(&w.into_bytes(), Compression::Gzip).unwrap()
}

/// Un générateur plat : ses couches `(bloc, épaisseur)` et son biome.
fn plat(
    couches: &'static [(&'static str, i32)],
    biome: Option<&'static str>,
) -> impl Fn(&mut Writer) {
    move |w: &mut Writer| {
        w.field(tag::STRING, "type").raw_str("minecraft:flat");
        w.field(tag::COMPOUND, "settings");
        w.field(tag::BYTE, "features").i8_payload(1);
        w.field(tag::LIST, "layers")
            .list_header(tag::COMPOUND, couches.len());
        for (bloc, h) in couches {
            w.field(tag::STRING, "block").raw_str(bloc);
            w.field(tag::INT, "height").i32_payload(*h);
            w.end();
        }
        if let Some(b) = biome {
            w.field(tag::STRING, "biome").raw_str(b);
        }
        w.field(tag::BYTE, "lakes").i8_payload(0);
        w.end();
    }
}

/// **Le préréglage « The Void »** : une couche d'air, le biome du vide. C'est
/// le monde où l'on peut créer des chunks, et il porte ce qu'il faut pour les
/// écrire comme le jeu les écrirait.
#[test]
fn le_preregle_du_vide_est_un_monde_vide() {
    let n = lire(&level_dat_genere(
        plat(&[("minecraft:air", 1)], Some("minecraft:the_void")),
        true,
    ))
    .unwrap();
    let g = n.generation.as_ref().unwrap();
    assert_eq!(g.genre, "minecraft:flat");
    assert_eq!(g.couches, vec![("minecraft:air".to_string(), 1)]);
    assert_eq!(
        n.monde_vide(),
        Some(tf_world::niveau::MondeVide {
            data_version: 2975,
            biome: "minecraft:the_void".into()
        })
    );
    assert_eq!(
        n.nom.as_deref(),
        Some("Bac à sable"),
        "le reste se lit toujours"
    );

    // Les autres airs du jeu sont de l'air aussi — préfixés ou non, comme un
    // préréglage tapé à la main les écrit.
    let n = lire(&level_dat_genere(
        plat(&[("minecraft:cave_air", 2), ("void_air", 1)], None),
        true,
    ))
    .unwrap();
    assert!(n.monde_vide().is_some(), "{:?}", n.generation);
}

/// Un plat SANS couche est vide aussi — et, sans biome nommé, il reçoit celui
/// que le jeu lui donnerait : les plaines.
#[test]
fn un_plat_sans_couche_est_vide_et_prend_les_plaines() {
    let n = lire(&level_dat_genere(plat(&[], None), true)).unwrap();
    assert_eq!(
        n.monde_vide().map(|v| v.biome),
        Some("minecraft:plains".into())
    );
}

/// **Ce qui n'est PAS vide ne l'est pas.** Un plat classique — y créer un
/// chunk vide creuserait un trou jusqu'au fond du monde —, un monde de bruit,
/// un monde sans `DataVersion` (on ne saurait pas quelle forme d'octets
/// écrire), un vieux monde sans `WorldGenSettings`.
#[test]
fn ce_qui_n_est_pas_vide_ne_l_est_pas() {
    let classique = plat(
        &[
            ("minecraft:bedrock", 1),
            ("minecraft:dirt", 2),
            ("minecraft:grass_block", 1),
        ],
        Some("minecraft:plains"),
    );
    assert_eq!(
        lire(&level_dat_genere(classique, true))
            .unwrap()
            .monde_vide(),
        None
    );

    // Un seul bloc qui n'est pas de l'air suffit.
    let presque = plat(&[("minecraft:air", 60), ("minecraft:stone", 1)], None);
    assert_eq!(
        lire(&level_dat_genere(presque, true)).unwrap().monde_vide(),
        None
    );

    let bruit = |w: &mut Writer| {
        w.field(tag::STRING, "type").raw_str("minecraft:noise");
        w.field(tag::STRING, "settings")
            .raw_str("minecraft:overworld");
    };
    let n = lire(&level_dat_genere(bruit, true)).unwrap();
    assert_eq!(
        n.generation.as_ref().map(|g| g.genre.as_str()),
        Some("minecraft:noise")
    );
    assert_eq!(n.monde_vide(), None, "le Nether plat vide n'y change rien");

    let sans_version = plat(&[("minecraft:air", 1)], Some("minecraft:the_void"));
    assert_eq!(
        lire(&level_dat_genere(sans_version, false))
            .unwrap()
            .monde_vide(),
        None
    );

    let ancien = lire(&level_dat(None, true)).unwrap();
    assert_eq!(ancien.monde_vide(), None);
}

/// La copie de travail porte le monde vide pour la SURFACE seulement : le
/// Nether et l'End d'un monde plat se génèrent comme partout.
#[test]
fn seule_la_surface_d_un_monde_vide_est_vide() {
    use tf_world::source::{Dimension, MemorySource};
    let v = tf_world::niveau::MondeVide {
        data_version: 2975,
        biome: "minecraft:the_void".into(),
    };
    let st = tf_world::Staging::new(MemorySource::new(), MemorySource::new())
        .avec_monde_vide(Some(v.clone()));
    assert_eq!(st.monde_vide(&Dimension::Overworld), Some(&v));
    assert_eq!(st.monde_vide(&Dimension::Nether), None);
    assert_eq!(st.monde_vide(&Dimension::End), None);
    let normal = tf_world::Staging::new(MemorySource::new(), MemorySource::new());
    assert_eq!(normal.monde_vide(&Dimension::Overworld), None);
}
