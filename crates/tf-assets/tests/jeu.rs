//! Lire l'installation d'un utilisateur : archives et détection.
//!
//! Le ZIP de test est CONSTRUIT ici, octet par octet — pas de fixture binaire
//! dans le dépôt, même pour un format qu'on lit. Un `.zip` commité serait
//! opaque en revue, et on ne saurait pas quel cas il couvre.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tf_assets::jeu::{est_une_installation, inspecter, ordre};
use tf_assets::{Archive, Source};

struct TempDir(PathBuf);

impl TempDir {
    fn new(nom: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "tf-jeu-{nom}-{}-{:?}",
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
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ── un ZIP écrit à la main ──────────────────────────────────────────────────

/// Écrit un ZIP minimal. `deflate` dit si l'entrée est dégonflée (méthode 8)
/// ou stockée telle quelle (méthode 0) — un pack réel contient les deux.
fn zip(entrees: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut nombre = 0u16;

    for (nom, contenu, deflate) in entrees {
        let entete = out.len() as u32;
        let (methode, charge) = if *deflate {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(contenu).unwrap();
            (8u16, e.finish().unwrap())
        } else {
            (0u16, contenu.to_vec())
        };
        let crc = 0u32; // aucun lecteur du dépôt ne le vérifie ; on ne ment pas dessus

        out.extend(b"PK\x03\x04");
        out.extend(20u16.to_le_bytes()); // version
        out.extend(0u16.to_le_bytes()); // drapeaux
        out.extend(methode.to_le_bytes());
        out.extend(0u16.to_le_bytes()); // heure
        out.extend(0u16.to_le_bytes()); // date
        out.extend(crc.to_le_bytes());
        out.extend((charge.len() as u32).to_le_bytes());
        out.extend((contenu.len() as u32).to_le_bytes());
        out.extend((nom.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes()); // extra
        out.extend(nom.as_bytes());
        out.extend(&charge);

        central.extend(b"PK\x01\x02");
        central.extend(20u16.to_le_bytes()); // version d'écriture
        central.extend(20u16.to_le_bytes()); // version minimale
        central.extend(0u16.to_le_bytes());
        central.extend(methode.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(crc.to_le_bytes());
        central.extend((charge.len() as u32).to_le_bytes());
        central.extend((contenu.len() as u32).to_le_bytes());
        central.extend((nom.len() as u16).to_le_bytes());
        central.extend(0u16.to_le_bytes()); // extra
        central.extend(0u16.to_le_bytes()); // commentaire
        central.extend(0u16.to_le_bytes()); // disque
        central.extend(0u16.to_le_bytes()); // attributs internes
        central.extend(0u32.to_le_bytes()); // attributs externes
        central.extend(entete.to_le_bytes());
        central.extend(nom.as_bytes());
        nombre += 1;
    }

    let debut_central = out.len() as u32;
    let taille_central = central.len() as u32;
    out.extend(&central);
    out.extend(b"PK\x05\x06");
    out.extend(0u16.to_le_bytes()); // disque
    out.extend(0u16.to_le_bytes()); // disque du central
    out.extend(nombre.to_le_bytes());
    out.extend(nombre.to_le_bytes());
    out.extend(taille_central.to_le_bytes());
    out.extend(debut_central.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // commentaire
    out
}

fn ecrire(d: &TempDir, chemin: &str, octets: &[u8]) -> PathBuf {
    let p = d.path().join(chemin);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, octets).unwrap();
    p
}

#[test]
fn une_archive_rend_ses_entrees_stockees_et_degonflees() {
    let d = TempDir::new("lire");
    // Un JSON répétitif, pour que le dégonflage fasse vraiment quelque chose.
    let gros = "{\"parent\":\"block/cube_all\"}".repeat(400);
    let p = ecrire(
        &d,
        "v.jar",
        &zip(&[
            (
                "assets/minecraft/blockstates/stone.json",
                b"{\"a\":1}",
                false,
            ),
            (
                "assets/minecraft/models/block/cube.json",
                gros.as_bytes(),
                true,
            ),
        ]),
    );
    let a = Archive::ouvrir(&p).unwrap();
    assert_eq!(a.len(), 2);
    assert_eq!(
        a.lire("assets/minecraft/blockstates/stone.json").unwrap(),
        b"{\"a\":1}"
    );
    assert_eq!(
        a.lire("assets/minecraft/models/block/cube.json").unwrap(),
        gros.as_bytes(),
        "une entrée dégonflée doit ressortir identique"
    );
    assert!(a.lire("absent.json").is_err());
}

/// Un dossier n'a pas de contenu, et l'inclure ferait croire à un fichier vide.
#[test]
fn les_dossiers_ne_sont_pas_des_entrees() {
    let d = TempDir::new("dossiers");
    let p = ecrire(
        &d,
        "v.jar",
        &zip(&[("assets/", b"", false), ("assets/a.json", b"{}", false)]),
    );
    let a = Archive::ouvrir(&p).unwrap();
    assert_eq!(a.noms(), vec!["assets/a.json"]);
}

/// Une archive tronquée ne doit ni paniquer ni rendre n'importe quoi.
#[test]
fn une_archive_tronquee_est_refusee_sans_paniquer() {
    let d = TempDir::new("tronquee");
    let complet = zip(&[("a.json", b"{}", false)]);
    for coupe in [0, 4, 10, complet.len() / 2, complet.len() - 3] {
        let p = ecrire(&d, &format!("t{coupe}.jar"), &complet[..coupe]);
        // Ouvrable ou non — mais jamais une panique, et jamais un contenu
        // inventé.
        if let Ok(a) = Archive::ouvrir(&p) {
            for n in a.noms() {
                let _ = a.lire(n);
            }
        }
    }
}

/// Un fichier qui n'est pas une archive du tout.
#[test]
fn un_fichier_quelconque_n_est_pas_une_archive() {
    let d = TempDir::new("pasunzip");
    let p = ecrire(&d, "x.jar", &[0xABu8; 4096]);
    assert!(Archive::ouvrir(&p).is_err());
}

// ── la détection d'une installation ─────────────────────────────────────────

/// **Chercher « .minecraft » ne trouve pas l'installation.** Un serveur a son
/// propre launcher — celui de l'utilisateur de référence est
/// `%APPDATA%\.minefield_1_18`. Le critère est `versions/`, jamais le nom.
#[test]
fn une_installation_se_reconnait_a_son_dossier_versions() {
    let d = TempDir::new("detect");
    let racine = d.path().join(".minefield_1_18");
    fs::create_dir_all(racine.join("versions/1.18.2")).unwrap();
    assert!(est_une_installation(&racine));

    let faux = d.path().join(".minecraft");
    fs::create_dir_all(&faux).unwrap();
    assert!(
        !est_une_installation(&faux),
        "le NOM ne fait pas une installation"
    );
}

/// **Trier des versions en TEXTE choisit la mauvaise.**
///
/// « 1.9 » passe après « 1.18 » en lexicographique, et on ouvrirait les
/// textures d'une version de 2016.
#[test]
fn les_versions_se_comparent_en_nombres_pas_en_texte() {
    use std::cmp::Ordering::*;
    assert_eq!(ordre("1.18", "1.9"), Greater, "1.18 est APRÈS 1.9");
    assert_eq!(ordre("1.21", "1.18.2"), Greater);
    assert_eq!(
        ordre("1.18.2", "1.18"),
        Greater,
        "un composant en plus = plus tard"
    );
    assert_eq!(ordre("1.18", "1.18"), Equal);
    // Une publication passe avant une capture instantanée, quels que soient
    // les nombres : `24w14a` commencerait par 24 et gagnerait sur le seul
    // texte.
    assert_eq!(ordre("1.21", "24w14a"), Greater);
    assert_eq!(ordre("1.21", "1.21-pre1"), Greater);
    assert_eq!(ordre("1.21", "1.21-rc1"), Greater);
}

/// L'ordre doit être TOTAL : un tri qui dépend de l'ordre d'entrée rendrait
/// deux réponses différentes au même disque.
#[test]
fn l_ordre_des_versions_est_total() {
    let noms = [
        "1.18",
        "1.18.2",
        "1.9",
        "1.21",
        "24w14a",
        "1.21-pre1",
        "1.7.10",
    ];
    for a in noms {
        for b in noms {
            assert_eq!(
                ordre(a, b),
                ordre(b, a).reverse(),
                "{a} contre {b} : l'ordre doit être antisymétrique"
            );
        }
    }
}

/// Une version DÉCLARÉE mais pas téléchargée n'a pas de `.jar` — la proposer
/// ferait échouer l'ouverture avec un message qui ne parle pas de ça.
#[test]
fn une_version_sans_jar_n_est_pas_proposee() {
    let d = TempDir::new("versions");
    let racine = d.path().join("launcher");
    for v in ["1.9", "1.18.2", "1.21"] {
        fs::create_dir_all(racine.join(format!("versions/{v}"))).unwrap();
    }
    // Deux seulement portent leur `.jar`.
    for v in ["1.9", "1.18.2"] {
        fs::write(
            racine.join(format!("versions/{v}/{v}.jar")),
            zip(&[("a.json", b"{}", false)]),
        )
        .unwrap();
    }
    let i = inspecter(&racine).unwrap();
    let noms: Vec<&str> = i.versions.iter().map(|v| v.nom.as_str()).collect();
    assert_eq!(noms, vec!["1.18.2", "1.9"], "la plus récente d'abord");
}

/// **Le dossier d'un launcher contient aussi les packs du SERVEUR.** C'est ce
/// qui fait qu'un bloc `minefield:*` arrive avec sa texture sans rien demander.
#[test]
fn les_packs_du_serveur_recouvrent_le_jeu() {
    let d = TempDir::new("pile");
    let racine = d.path().join("launcher");
    fs::create_dir_all(racine.join("versions/1.18.2")).unwrap();
    fs::write(
        racine.join("versions/1.18.2/1.18.2.jar"),
        zip(&[("assets/minecraft/blockstates/stone.json", b"JEU", false)]),
    )
    .unwrap();
    fs::create_dir_all(racine.join("server-resource-packs")).unwrap();
    fs::write(
        racine.join("server-resource-packs/minefield.zip"),
        zip(&[
            ("assets/minecraft/blockstates/stone.json", b"SERVEUR", false),
            ("assets/minefield/blockstates/marbre.json", b"CUSTOM", false),
        ]),
    )
    .unwrap();

    let i = inspecter(&racine).unwrap();
    assert_eq!(i.packs.len(), 1);
    let pile = i.pile(None).unwrap();
    assert_eq!(
        pile.lire("assets/minecraft/blockstates/stone.json")
            .unwrap(),
        b"SERVEUR",
        "le pack du serveur RECOUVRE le jeu"
    );
    assert_eq!(
        pile.lire("assets/minefield/blockstates/marbre.json")
            .unwrap(),
        b"CUSTOM"
    );
}

/// Un dossier qui n'est pas une installation le dit, au lieu de rendre une
/// installation vide qu'on découvrirait trois écrans plus loin.
#[test]
fn un_dossier_quelconque_est_refuse_en_le_disant() {
    let d = TempDir::new("vide");
    let e = inspecter(d.path()).unwrap_err();
    assert!(
        format!("{e}").contains("versions"),
        "le message doit nommer le critère : {e}"
    );
}
