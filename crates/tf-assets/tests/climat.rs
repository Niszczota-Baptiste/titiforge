//! La couleur d'un biome — dérivée du jeu, jamais écrite à la main.
//!
//! Les fichiers de test sont ÉCRITS à la volée : un PNG commité est opaque en
//! revue, et une table de couleurs de 256 × 256 encore plus. Ce que ces tests
//! figent n'est donc pas une couleur de Mojang — qu'on ne peut pas
//! redistribuer — mais la FORMULE : quel pixel de la table répond à quel
//! couple (température, humidité).
//!
//! C'est le seul endroit où elle peut se tromper sans qu'on le voie : une
//! table lue à l'envers donne un désert vert et une jungle jaune, deux images
//! parfaitement plausibles.

use std::fs;
use std::path::{Path, PathBuf};

use tf_assets::climat::{echantillon, Climat, Modificateur, COTE_TABLE, EAU_PAR_DEFAUT};
use tf_assets::Dossier;

struct Faux(PathBuf);

impl Faux {
    fn neuf(nom: &str) -> Faux {
        let p = std::env::temp_dir().join(format!("tf-climat-{}-{nom}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Faux(p)
    }

    fn ecrire(&self, chemin: &str, octets: &[u8]) {
        let p = self.0.join(chemin);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, octets).unwrap();
    }

    fn biome(&self, ns: &str, nom: &str, json: &str) {
        self.ecrire(
            &format!("data/{ns}/worldgen/biome/{nom}.json"),
            json.as_bytes(),
        );
    }

    /// Une table 256 × 256 où chaque pixel ENCODE SA POSITION : rouge = x,
    /// vert = y. Lire la table à l'envers devient alors impossible à rater.
    fn table(&self, nom: &str) {
        let mut px = Vec::with_capacity(COTE_TABLE * COTE_TABLE * 4);
        for y in 0..COTE_TABLE {
            for x in 0..COTE_TABLE {
                px.extend_from_slice(&[x as u8, y as u8, 0x2A, 255]);
            }
        }
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, COTE_TABLE as u32, COTE_TABLE as u32);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&px).unwrap();
        }
        self.ecrire(
            &format!("assets/minecraft/textures/colormap/{nom}.png"),
            &out,
        );
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Faux {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const PLAINES: &str = r#"{"temperature":0.8,"downfall":0.4,
  "effects":{"sky_color":7907327,"water_color":4159204}}"#;

// ── la formule, seule ───────────────────────────────────────────────────────

#[test]
fn l_echantillon_vise_le_pixel_du_jeu() {
    // La table encode sa position : rouge = x, vert = y.
    let mut table = vec![0u8; COTE_TABLE * COTE_TABLE * 4];
    for y in 0..COTE_TABLE {
        for x in 0..COTE_TABLE {
            let k = (y * COTE_TABLE + x) * 4;
            table[k] = x as u8;
            table[k + 1] = y as u8;
        }
    }
    // Plaines : t = 0,8 · d = 0,4. L'humidité est multipliée par la
    // température AVANT l'inversion — 0,4 × 0,8 = 0,32 — et c'est le détail
    // qui se perd le plus facilement.
    let c = echantillon(&table, 0.8, 0.4);
    // **50, pas 51.** `0,8` n'est pas représentable : en flottant il vaut
    // 0,800000011920929, donc `(1 − t) × 255` vaut 50,99999… et la troncature
    // rend 50. Java fait exactement la même chose — `(int)` tronque aussi —
    // et c'est pourquoi on garde la formule à la lettre plutôt que de
    // l'arrondir « proprement » : arrondir décalerait la table d'un pixel sur
    // la moitié des biomes.
    assert_eq!((c >> 16) & 0xFF, 50, "x = (1 − 0,8) × 255, TRONQUÉ");
    assert_eq!((c >> 8) & 0xFF, 173, "y = (1 − 0,32) × 255");

    // Désert : t = 2,0 (serré à 1), d = 0. Le coin chaud et sec.
    let c = echantillon(&table, 2.0, 0.0);
    assert_eq!((c >> 16) & 0xFF, 0);
    assert_eq!((c >> 8) & 0xFF, 255);

    // Et le coin froid et humide.
    let c = echantillon(&table, 0.0, 1.0);
    assert_eq!((c >> 16) & 0xFF, 255);
    assert_eq!((c >> 8) & 0xFF, 255, "d × t = 0 quand t = 0");
}

#[test]
fn une_table_trop_courte_rend_un_gris_pas_un_plantage() {
    // Un pack peut livrer n'importe quoi. Le jeu rend un magenta criard ; on
    // préfère un gris neutre, qui ne se lit pas comme « ce bloc est bizarre ».
    assert_eq!(echantillon(&[], 0.5, 0.5), 0x80_8080);
    assert_eq!(echantillon(&[0; 16], 0.5, 0.5), 0x80_8080);
}

// ── la lecture d'une installation ───────────────────────────────────────────

#[test]
fn un_biome_sans_couleur_explicite_passe_par_la_table() {
    let f = Faux::neuf("table");
    f.table("grass");
    f.table("foliage");
    f.biome("minecraft", "plains", PLAINES);
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());

    assert_eq!(c.nb_biomes(), 1);
    assert!(!c.est_vide());
    let h = c.herbe("minecraft:plains").expect("la table répond");
    assert_eq!([h[0], h[1]], [50, 173], "le pixel du jeu, pas un autre");
    assert!(!c.approche("minecraft:plains"));
}

#[test]
fn une_couleur_explicite_court_circuite_la_table() {
    let f = Faux::neuf("explicite");
    f.table("grass");
    f.table("foliage");
    f.biome(
        "minecraft",
        "cherry_grove",
        r#"{"temperature":0.5,"downfall":0.8,
            "effects":{"grass_color":11983713,"foliage_color":11983713,"water_color":6141935}}"#,
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.herbe("minecraft:cherry_grove"), Some([0xB6, 0xDB, 0x61]));
    assert_eq!(
        c.feuillage("minecraft:cherry_grove"),
        Some([0xB6, 0xDB, 0x61])
    );
    assert_eq!(c.eau("minecraft:cherry_grove"), Some([0x5D, 0xB7, 0xEF]));
}

#[test]
fn le_modificateur_de_la_foret_sombre_assombrit_l_herbe_et_pas_le_feuillage() {
    let f = Faux::neuf("foret");
    f.table("grass");
    f.table("foliage");
    f.biome(
        "minecraft",
        "dark_forest",
        r#"{"temperature":0.7,"downfall":0.8,
            "effects":{"grass_color_modifier":"dark_forest","water_color":4159204}}"#,
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    let brut = {
        let mut table = vec![0u8; COTE_TABLE * COTE_TABLE * 4];
        for y in 0..COTE_TABLE {
            for x in 0..COTE_TABLE {
                let k = (y * COTE_TABLE + x) * 4;
                table[k] = x as u8;
                table[k + 1] = y as u8;
                table[k + 2] = 0x2A;
            }
        }
        echantillon(&table, 0.7, 0.8)
    };
    let attendu = ((brut & 0x00FE_FEFE) + 0x0028_340A) >> 1;
    let h = c.herbe("minecraft:dark_forest").unwrap();
    assert_eq!(
        h,
        [(attendu >> 16) as u8, (attendu >> 8) as u8, attendu as u8],
        "la moyenne avec le vert sombre du jeu"
    );
    // Le FEUILLAGE, lui, ne subit pas le modificateur : l'étendre
    // assombrirait les arbres d'un biome sur deux.
    let fol = c.feuillage("minecraft:dark_forest").unwrap();
    assert_eq!(
        fol,
        [(brut >> 16) as u8, (brut >> 8) as u8, brut as u8],
        "le feuillage passe par la table, sans modificateur"
    );
}

#[test]
fn le_marais_est_annonce_comme_approche() {
    // Le jeu tire entre deux verts d'après un bruit de POSITION. Sans le
    // générateur de monde on ne peut pas le rejouer : on prend le dominant,
    // et on le DIT. « Approché » et « faux » ne se lisent pas pareil.
    let f = Faux::neuf("marais");
    f.table("grass");
    f.table("foliage");
    f.biome(
        "minecraft",
        "swamp",
        r#"{"temperature":0.8,"downfall":0.9,
            "effects":{"grass_color_modifier":"swamp","water_color":6388580}}"#,
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.herbe("minecraft:swamp"), Some([0x6A, 0x70, 0x39]));
    assert!(c.approche("minecraft:swamp"), "et il faut que ça se sache");
    assert_eq!(
        c.biome("minecraft:swamp").unwrap().modificateur,
        Modificateur::Marais
    );
}

#[test]
fn l_eau_a_un_defaut_quand_le_fichier_se_tait() {
    let f = Faux::neuf("eau");
    f.table("grass");
    f.table("foliage");
    f.biome(
        "minecraft",
        "nulle_part",
        r#"{"temperature":0.5,"downfall":0.5}"#,
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    let e = c.eau("minecraft:nulle_part").unwrap();
    assert_eq!(
        e,
        [
            (EAU_PAR_DEFAUT >> 16) as u8,
            (EAU_PAR_DEFAUT >> 8) as u8,
            EAU_PAR_DEFAUT as u8
        ]
    );
}

/// Le namespace se DÉCOUVRE : un serveur pose ses biomes sous le sien, et
/// chercher « minecraft » seul les raterait tous.
#[test]
fn les_biomes_d_un_autre_namespace_sont_lus_aussi() {
    let f = Faux::neuf("ns");
    f.table("grass");
    f.table("foliage");
    f.biome("minecraft", "plains", PLAINES);
    f.biome(
        "minefield",
        "trefonds",
        r#"{"temperature":0.1,"downfall":0.0,"effects":{"grass_color":1122867}}"#,
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.nb_biomes(), 2);
    assert_eq!(c.herbe("minefield:trefonds"), Some([0x11, 0x22, 0x33]));
}

// ── ce qui manque est NOMMÉ ─────────────────────────────────────────────────

#[test]
fn un_pack_sans_donnees_de_biome_le_dit_au_lieu_d_inventer() {
    // Un resource pack seul ne porte PAS les données de biome : elles vivent
    // du côté `data/`. C'est exactement pourquoi il faut une installation, et
    // il vaut mieux le dire que rendre une couleur inventée.
    let f = Faux::neuf("sans-data");
    f.table("grass");
    f.table("foliage");
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.nb_biomes(), 0);
    assert!(c.est_vide());
    assert!(c.herbe("minecraft:plains").is_none(), "on n'invente pas");
    assert!(
        c.manques.iter().any(|m| m.contains("worldgen/biome")),
        "le manque doit être nommé : {:?}",
        c.manques
    );
}

#[test]
fn un_jeu_sans_table_de_couleurs_le_dit_aussi() {
    let f = Faux::neuf("sans-table");
    f.biome("minecraft", "plains", PLAINES);
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.nb_biomes(), 1);
    assert!(c.est_vide(), "un biome sans table ne donne aucune couleur");
    assert!(c.herbe("minecraft:plains").is_none());
    // L'eau, elle, ne passe pas par la table : elle reste connue.
    assert!(c.eau("minecraft:plains").is_some());
    assert_eq!(c.manques.len(), 2, "les deux tables : {:?}", c.manques);
}

#[test]
fn un_json_illisible_ne_fait_pas_tomber_le_reste() {
    let f = Faux::neuf("casse");
    f.table("grass");
    f.table("foliage");
    f.biome("minecraft", "plains", PLAINES);
    f.biome("minecraft", "casse", "{ ceci n'est pas du JSON");
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.nb_biomes(), 1, "le biome valide reste lu");
    assert!(c.manques.iter().any(|m| m.contains("casse")));
}

#[test]
fn un_sous_dossier_n_est_pas_un_biome() {
    let f = Faux::neuf("sous-dossier");
    f.table("grass");
    f.table("foliage");
    f.biome("minecraft", "plains", PLAINES);
    f.ecrire(
        "data/minecraft/worldgen/biome/truc/machin.json",
        b"{\"temperature\":1}",
    );
    let c = Climat::charger(&Dossier::ouvrir(f.path()).unwrap());
    assert_eq!(c.nb_biomes(), 1, "un nom à rallonge ne désigne rien");
}
