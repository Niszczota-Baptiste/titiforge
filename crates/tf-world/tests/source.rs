//! La frontière entre `tf-world` et son hôte.
//!
//! Le cœur de ce fichier est une **suite de contrat** : une seule fonction
//! d'assertions, branchée sur deux implémentations qui n'ont rien en commun —
//! une carte en mémoire et un vrai dossier sur disque. Des tests écrits contre
//! une seule implémentation ne décrivent pas un contrat, ils décrivent cette
//! implémentation ; et le jour où on ajoute une source d'archive, ils ne
//! diraient rien.

use std::fs;
use std::path::{Path, PathBuf};

use tf_world::{
    Dimension, Folder, FsSource, LockProbe, MemorySource, Overview, RegionInfo, RegionPos,
    RegionSink, RegionSource, SourceError,
};

// ── le jeu de données du contrat ────────────────────────────────────────────

fn octets(n: usize, marque: u8) -> Vec<u8> {
    let mut v = vec![marque; n];
    // Un en-tête plausible, pour que `is_empty` ait un sens.
    if n >= 4 {
        v[..4].copy_from_slice(&(marque as u32).to_be_bytes());
    }
    v
}

/// Ce que toute source doit contenir pour passer le contrat.
fn jeu() -> Vec<(Dimension, Folder, RegionPos, Vec<u8>)> {
    vec![
        (
            Dimension::Overworld,
            Folder::Region,
            RegionPos::new(0, 0),
            octets(20_000, 1),
        ),
        (
            Dimension::Overworld,
            Folder::Region,
            RegionPos::new(-1, 2),
            octets(50_000, 2),
        ),
        // Un fichier qui n'a que son en-tête : aucun chunk dedans.
        (
            Dimension::Overworld,
            Folder::Region,
            RegionPos::new(5, 5),
            octets(8192, 3),
        ),
        (
            Dimension::Overworld,
            Folder::Entities,
            RegionPos::new(0, 0),
            octets(9_000, 4),
        ),
        (
            Dimension::Nether,
            Folder::Region,
            RegionPos::new(0, 0),
            octets(30_000, 5),
        ),
        (
            Dimension::Custom {
                namespace: "aether".into(),
                path: "the_aether".into(),
            },
            Folder::Region,
            RegionPos::new(3, -4),
            octets(12_000, 6),
        ),
    ]
}

const MCC: &str = "c.-29.68.mcc";

/// Les assertions que TOUTE source doit tenir.
fn contrat(nom: &str, s: &dyn RegionSource) {
    // ── dimensions ──────────────────────────────────────────────────────────
    let dims = s.dimensions().unwrap();
    assert!(dims.contains(&Dimension::Overworld), "{nom} : surface");
    assert!(dims.contains(&Dimension::Nether), "{nom} : nether");
    assert!(
        dims.iter()
            .any(|d| matches!(d, Dimension::Custom { namespace, path }
            if namespace == "aether" && path == "the_aether")),
        "{nom} : une dimension ajoutée doit être découverte, pas codée en dur — {dims:?}"
    );
    assert!(
        !dims.contains(&Dimension::End),
        "{nom} : l'end n'est pas dans le jeu de données"
    );

    // ── carte, sans rien lire ───────────────────────────────────────────────
    let ov = s.overview(&Dimension::Overworld, Folder::Region).unwrap();
    assert_eq!(ov.regions.len(), 3, "{nom}");
    assert_eq!(ov.total_bytes(), 20_000 + 50_000 + 8192, "{nom}");

    let vide = ov
        .regions
        .iter()
        .find(|r| r.pos == RegionPos::new(5, 5))
        .unwrap();
    assert!(
        vide.is_empty(),
        "{nom} : 8192 octets, c'est l'en-tête et rien d'autre"
    );
    let plein = ov
        .regions
        .iter()
        .find(|r| r.pos == RegionPos::new(0, 0))
        .unwrap();
    assert!(!plein.is_empty(), "{nom}");

    // Les bornes ignorent les régions vides : les inclure ferait s'ouvrir un
    // monde sur une zone où il n'y a rien.
    let (min, max) = ov.bounds().unwrap();
    assert_eq!(min, RegionPos::new(-1, 0), "{nom}");
    assert_eq!(max, RegionPos::new(0, 2), "{nom}");

    // Les non vides, des plus grosses aux plus petites.
    let tri = ov.non_empty();
    assert_eq!(tri.len(), 2, "{nom}");
    assert_eq!(
        tri[0].pos,
        RegionPos::new(-1, 2),
        "{nom} : la plus grosse d'abord"
    );

    // ── les dossiers sont indépendants ──────────────────────────────────────
    let ent = s.overview(&Dimension::Overworld, Folder::Entities).unwrap();
    assert_eq!(
        ent.regions.len(),
        1,
        "{nom} : entities est un autre dossier"
    );
    assert_eq!(
        s.overview(&Dimension::Overworld, Folder::Poi).unwrap(),
        Overview::default(),
        "{nom} : un dossier absent rend une carte vide, PAS une erreur"
    );

    // ── lecture ─────────────────────────────────────────────────────────────
    let b = s
        .read_region(&Dimension::Overworld, Folder::Region, RegionPos::new(-1, 2))
        .unwrap();
    assert_eq!(b.len(), 50_000, "{nom}");
    assert_eq!(b[4], 2, "{nom} : ce sont bien les octets de CETTE région");

    // Une région absente est `NotFound` et non une erreur d'entrée-sortie :
    // c'est le cas NORMAL aux bords d'un monde.
    assert_eq!(
        s.read_region(
            &Dimension::Overworld,
            Folder::Region,
            RegionPos::new(99, 99)
        ),
        Err(SourceError::NotFound),
        "{nom}"
    );
    assert_eq!(
        s.read_region(&Dimension::End, Folder::Region, RegionPos::new(0, 0)),
        Err(SourceError::NotFound),
        "{nom} : une dimension absente non plus"
    );

    // ── charges déportées ───────────────────────────────────────────────────
    let mcc = s
        .read_external(&Dimension::Overworld, Folder::Region, MCC)
        .unwrap();
    assert_eq!(mcc, b"charge deportee".to_vec(), "{nom}");
    assert_eq!(
        s.read_external(&Dimension::Overworld, Folder::Region, "c.1.1.mcc"),
        Err(SourceError::NotFound),
        "{nom}"
    );
}

// ── les deux implémentations ────────────────────────────────────────────────

#[test]
fn la_source_en_memoire_tient_le_contrat() {
    let mut m = MemorySource::new();
    for (d, f, p, b) in jeu() {
        m.put_region(d, f, p, b);
    }
    m.put_external(
        Dimension::Overworld,
        Folder::Region,
        MCC,
        b"charge deportee".to_vec(),
    );
    contrat("mémoire", &m);
}

#[test]
fn la_source_fichiers_tient_le_meme_contrat() {
    let d = TempDir::new("contrat");
    fs::write(d.path().join("level.dat"), b"faux").unwrap();
    for (dim, folder, pos, bytes) in jeu() {
        let dir = d.path().join(dim.dir(folder));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(pos.file_name()), bytes).unwrap();
    }
    fs::write(
        d.path()
            .join(Dimension::Overworld.dir(Folder::Region))
            .join(MCC),
        b"charge deportee",
    )
    .unwrap();

    contrat("fichiers", &FsSource::open(d.path()).unwrap());
}

// ── ce qui est propre au système de fichiers ────────────────────────────────

#[test]
fn ouvrir_un_dossier_inexistant_rend_not_found() {
    assert_eq!(
        FsSource::open("/n/existe/vraiment/pas").err(),
        Some(SourceError::NotFound)
    );
}

#[test]
fn une_save_se_reconnait_a_son_level_dat_pas_a_son_dossier_region() {
    // Une save NEUVE n'a pas encore de `region/`, et un dossier `region/`
    // isolé n'est pas une save. Se tromper de critère refuserait d'ouvrir les
    // premières et accepterait les seconds.
    let d = TempDir::new("critere");
    assert!(!FsSource::looks_like_world(d.path()));
    fs::create_dir_all(d.path().join("region")).unwrap();
    assert!(
        !FsSource::looks_like_world(d.path()),
        "un dossier region seul ne suffit pas"
    );
    fs::write(d.path().join("level.dat"), b"x").unwrap();
    assert!(FsSource::looks_like_world(d.path()));
}

#[test]
fn ce_qui_n_est_pas_un_fichier_de_region_est_ignore_pas_refuse() {
    // Un dossier de save réel contient des `.DS_Store`, des `.mcc`, des
    // sauvegardes d'outils tiers. Échouer sur le premier intrus rendrait
    // l'application inutilisable sur une vraie machine.
    let d = TempDir::new("intrus");
    let dir = d.path().join("region");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("r.0.0.mca"), octets(20_000, 1)).unwrap();
    for intrus in [
        "r.0.0 (16).mca",
        ".DS_Store",
        "notes.txt",
        "c.0.0.mcc",
        "r.0.0.mca.bak",
    ] {
        fs::write(dir.join(intrus), b"x").unwrap();
    }
    fs::create_dir_all(dir.join("un_dossier")).unwrap();

    let s = FsSource::open(d.path()).unwrap();
    let ov = s.overview(&Dimension::Overworld, Folder::Region).unwrap();
    assert_eq!(
        ov.regions.len(),
        1,
        "un seul vrai fichier de région : {:?}",
        ov.regions
    );
    assert_eq!(ov.regions[0].pos, RegionPos::new(0, 0));
}

#[test]
fn l_ecriture_est_atomique_et_ne_laisse_pas_de_temporaire() {
    let d = TempDir::new("atomique");
    let s = FsSource::open(d.path()).unwrap();
    let pos = RegionPos::new(-2, 3);

    s.write_region(&Dimension::Nether, Folder::Region, pos, &octets(5_000, 7))
        .unwrap();
    let relu = s
        .read_region(&Dimension::Nether, Folder::Region, pos)
        .unwrap();
    assert_eq!(relu.len(), 5_000);
    assert_eq!(relu[4], 7);

    // Le dossier ne doit contenir QUE le fichier voulu : un temporaire laissé
    // derrière ferait grossir la save à chaque sauvegarde.
    let dir = d.path().join(Dimension::Nether.dir(Folder::Region));
    let noms: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(noms, vec!["r.-2.3.mca".to_string()], "{noms:?}");

    // Réécrire remplace, sans doubler.
    s.write_region(&Dimension::Nether, Folder::Region, pos, &octets(60, 8))
        .unwrap();
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(
        s.read_region(&Dimension::Nether, Folder::Region, pos)
            .unwrap()
            .len(),
        60
    );
}

#[test]
fn l_ecriture_cree_les_dossiers_manquants() {
    // Une save neuve n'a pas de `DIM1/region/`. Exiger qu'il existe forcerait
    // l'appelant à connaître la disposition du jeu — exactement ce que ce
    // module existe pour lui épargner.
    let d = TempDir::new("mkdir");
    let s = FsSource::open(d.path()).unwrap();
    let dim = Dimension::Custom {
        namespace: "mod".into(),
        path: "a/b".into(),
    };
    s.write_region(&dim, Folder::Region, RegionPos::new(0, 0), b"x")
        .unwrap();
    assert!(d
        .path()
        .join("dimensions/mod/a/b/region/r.0.0.mca")
        .is_file());
}

#[test]
fn une_dimension_ajoutee_se_decouvre_meme_a_plusieurs_segments() {
    let d = TempDir::new("dims");
    for chemin in [
        "dimensions/aether/the_aether/region",
        "dimensions/twilight/forest/deep/region",
        "region",
    ] {
        fs::create_dir_all(d.path().join(chemin)).unwrap();
    }
    let s = FsSource::open(d.path()).unwrap();
    let dims = s.dimensions().unwrap();
    assert!(dims.contains(&Dimension::Overworld));
    assert!(dims
        .iter()
        .any(|x| matches!(x, Dimension::Custom { path, .. } if path == "the_aether")));
    assert!(
        dims.iter()
            .any(|x| matches!(x, Dimension::Custom { path, .. } if path == "forest/deep")),
        "un chemin de dimension peut avoir plusieurs segments : {dims:?}"
    );
}

#[test]
fn un_nom_de_charge_deportee_ne_peut_pas_sortir_du_dossier() {
    // Ces noms descendent de coordonnées lues dans un fichier, et un fichier
    // vient du disque d'un utilisateur. « On contrôle l'appelant » est la
    // phrase qu'on se dit juste avant de ne plus le contrôler.
    let d = TempDir::new("traversee");
    let s = FsSource::open(d.path()).unwrap();
    for mauvais in ["../evade.mcc", "..", ".", "", "a/b.mcc", "a\\b.mcc"] {
        assert!(
            matches!(
                s.read_external(&Dimension::Overworld, Folder::Region, mauvais),
                Err(SourceError::BadName(_))
            ),
            "« {mauvais} » aurait dû être refusé en lecture"
        );
        assert!(
            matches!(
                s.write_external(&Dimension::Overworld, Folder::Region, mauvais, b"x"),
                Err(SourceError::BadName(_))
            ),
            "« {mauvais} » aurait dû être refusé en écriture"
        );
    }
    // Et un nom légitime passe.
    s.write_external(&Dimension::Overworld, Folder::Region, "c.-29.68.mcc", b"ok")
        .unwrap();
    assert_eq!(
        s.read_external(&Dimension::Overworld, Folder::Region, "c.-29.68.mcc")
            .unwrap(),
        b"ok".to_vec()
    );
}

#[test]
fn supprimer_une_charge_deja_absente_reussit() {
    // C'est le résultat voulu. Échouer obligerait chaque appelant à vérifier
    // d'abord, et un `.mcc` qu'on croit présent peut avoir été effacé à la
    // main entre-temps.
    let d = TempDir::new("suppr");
    let s = FsSource::open(d.path()).unwrap();
    assert!(s
        .remove_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc")
        .is_ok());

    s.write_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc", b"x")
        .unwrap();
    assert!(s
        .remove_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc")
        .is_ok());
    assert_eq!(
        s.read_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc"),
        Err(SourceError::NotFound)
    );
}

#[test]
fn une_source_en_lecture_seule_refuse_toute_ecriture() {
    let d = TempDir::new("lecture_seule");
    let s = FsSource::open(d.path()).unwrap().read_only();
    assert_eq!(
        s.write_region(
            &Dimension::Overworld,
            Folder::Region,
            RegionPos::new(0, 0),
            b"x"
        ),
        Err(SourceError::ReadOnly)
    );
    assert_eq!(
        s.write_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc", b"x"),
        Err(SourceError::ReadOnly)
    );
    assert_eq!(
        s.remove_external(&Dimension::Overworld, Folder::Region, "c.0.0.mcc"),
        Err(SourceError::ReadOnly)
    );
}

// ── le verrou ───────────────────────────────────────────────────────────────

#[test]
fn le_verrou_ne_se_reduit_jamais_a_un_booleen() {
    // Sous Windows, le jeu tient `session.lock` ouvert et l'écriture échoue :
    // la réponse est fiable. Ailleurs le verrou est CONSULTATIF — l'ouvrir
    // réussit même quand le jeu tourne. Affirmer « le monde est libre » sur
    // cette base ferait écrire pendant que le jeu tourne, et perdre les deux
    // côtés.
    let d = TempDir::new("verrou");
    let s = FsSource::open(d.path()).unwrap();

    // Pas de verrou du tout : le jeu n'a jamais ouvert cette save. Fiable
    // partout.
    let p = s.probe_lock();
    assert_eq!(p, LockProbe::LIBRE_SUR);
    assert!(p.surement_libre());

    fs::write(d.path().join("session.lock"), b"\xe2\x98\x83").unwrap();
    let p = s.probe_lock();
    if cfg!(windows) {
        assert!(p.reliable, "sous Windows la sonde conclut");
    } else {
        assert!(!p.reliable, "ailleurs elle ne peut PAS conclure");
        assert!(
            !p.surement_libre(),
            "et ne doit donc pas dire que c'est libre"
        );
    }

    // La propriété qui compte : `surement_libre` n'est vraie que si on SAIT.
    assert!(!LockProbe::INDECIDABLE.surement_libre());
    assert!(!LockProbe::TENU.surement_libre());
    assert!(LockProbe::LIBRE_SUR.surement_libre());
}

// ── carte ───────────────────────────────────────────────────────────────────

#[test]
fn une_carte_de_dimension_entierement_vide_n_a_pas_de_bornes() {
    let ov = Overview {
        regions: vec![RegionInfo {
            pos: RegionPos::new(0, 0),
            bytes: 8192,
        }],
    };
    assert_eq!(ov.bounds(), None, "que des régions vides : aucune borne");
    assert!(ov.non_empty().is_empty());
    assert_eq!(Overview::default().bounds(), None);
}

// ── un dossier temporaire, sans dépendance ──────────────────────────────────

struct TempDir(PathBuf);

impl TempDir {
    fn new(etiquette: &str) -> Self {
        // Identifiant unique sans dépendance : l'horloge et l'identifiant du
        // fil. Deux tests parallèles ne doivent pas se marcher dessus.
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "tf-{etiquette}-{n}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
