//! La couleur d'un biome — **dérivée du jeu, pas écrite à la main.**
//!
//! Les textures teintées de Minecraft sont grises : `grass_block_top.png`
//! vaut (147, 147, 147). C'est le jeu qui les multiplie par une couleur, et
//! cette couleur dépend du BIOME. Jusqu'ici titiforge en posait une seule,
//! « plaines », pour tout le monde : un réglage, pas une mesure.
//!
//! ## D'où vient vraiment la couleur
//!
//! De deux endroits, et il faut les deux :
//!
//! - **`assets/minecraft/textures/colormap/grass.png`** et `foliage.png`, deux
//!   images de 256 × 256 qui sont des TABLES : une couleur par couple
//!   (température, humidité) ;
//! - **`data/<ns>/worldgen/biome/<nom>.json`**, qui donne la température et
//!   l'humidité de chaque biome — et parfois une couleur explicite qui court-
//!   circuite la table (le marais, la forêt sombre).
//!
//! Les premières vivent du côté RESSOURCES, les secondes du côté DONNÉES. Un
//! resource pack seul ne suffit donc pas : il faut une installation, ou un
//! datapack. C'est exactement pourquoi `jeu.rs` lit l'installation de
//! l'utilisateur plutôt qu'un pack embarqué — et c'est ici que ça paie une
//! seconde fois.
//!
//! ## Ce qui n'est pas dérivable, et qui est ANNONCÉ
//!
//! Le modificateur `swamp` tire entre deux verts d'après un bruit de POSITION.
//! Sans le générateur de monde, on ne peut pas le rejouer : on prend la valeur
//! dominante et on le dit (`Approche`). C'est la distinction qui compte —
//! « approché » et « faux » ne se lisent pas pareil sur une capture d'écran,
//! et on ne doit pas pouvoir les confondre dans le code non plus.

use std::collections::BTreeMap;

use crate::source::Source;

/// Ce que le jeu fait à la couleur d'herbe d'un biome, après la table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modificateur {
    #[default]
    Aucun,
    /// `(c & 0xFEFEFE) + 0x28340A >> 1` — la forêt sombre assombrit tout.
    ForetSombre,
    /// Le marais tire entre deux verts selon un bruit de POSITION. On prend le
    /// dominant, et `Climat::approche` le signale.
    Marais,
}

/// Un biome, tel que son fichier le décrit.
#[derive(Debug, Clone, PartialEq)]
pub struct Biome {
    pub temperature: f32,
    pub downfall: f32,
    /// Couleurs explicites, quand le biome en impose une.
    pub herbe: Option<u32>,
    pub feuillage: Option<u32>,
    pub eau: Option<u32>,
    pub modificateur: Modificateur,
}

impl Default for Biome {
    fn default() -> Self {
        // Les défauts de `BiomeSpecialEffects` : l'eau vaut 0x3F76E4 partout
        // où le fichier ne dit rien.
        Biome {
            temperature: 0.5,
            downfall: 0.5,
            herbe: None,
            feuillage: None,
            eau: None,
            modificateur: Modificateur::Aucun,
        }
    }
}

/// Le côté d'une table de couleurs : 256 × 256.
pub const COTE_TABLE: usize = 256;

/// Les couleurs de biome d'un jeu, telles qu'on a pu les lire.
#[derive(Debug, Default)]
pub struct Climat {
    biomes: BTreeMap<String, Biome>,
    /// RGBA8, 256 × 256. `None` quand le pack ne porte pas la table.
    herbe: Option<Vec<u8>>,
    feuillage: Option<Vec<u8>>,
    /// Ce qu'on n'a pas trouvé, nommé. Un trou tu vaut moins qu'un trou dit.
    pub manques: Vec<String>,
}

/// L'eau par défaut de `BiomeSpecialEffects`, quand rien ne la dit.
pub const EAU_PAR_DEFAUT: u32 = 0x3F_76E4;

impl Climat {
    /// Lit ce qu'une source porte. **Ne refuse jamais** : un pack sans données
    /// de biome rend un `Climat` vide qui dit ce qui lui manque, et l'appelant
    /// retombe sur son réglage.
    pub fn charger<S: Source + ?Sized>(src: &S) -> Climat {
        let mut c = Climat::default();
        for (nom, chemin) in [
            ("herbe", "assets/minecraft/textures/colormap/grass.png"),
            (
                "feuillage",
                "assets/minecraft/textures/colormap/foliage.png",
            ),
        ] {
            match crate::texture::lire(src, chemin) {
                Ok(t) if t.cote as usize == COTE_TABLE => {
                    let px = t.pixels;
                    if nom == "herbe" {
                        c.herbe = Some(px)
                    } else {
                        c.feuillage = Some(px)
                    }
                }
                Ok(t) => c.manques.push(format!(
                    "{chemin} : {} × {} au lieu de {COTE_TABLE} × {COTE_TABLE}",
                    t.cote, t.cote
                )),
                Err(_) => c.manques.push(format!("{chemin} : absent")),
            }
        }

        // `data/<ns>/worldgen/biome/<nom>.json`. Le namespace se découvre,
        // comme pour les blockstates : un serveur pose ses biomes sous le
        // sien, et chercher « minecraft » seul les raterait tous.
        let mut vus = 0usize;
        for chemin in src.lister("data/") {
            let Some(reste) = chemin.strip_suffix(".json") else {
                continue;
            };
            let Some((ns, nom)) = decouper_biome(reste) else {
                continue;
            };
            let Ok(octets) = src.lire(&chemin) else {
                continue;
            };
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&octets) else {
                c.manques.push(format!("{chemin} : JSON illisible"));
                continue;
            };
            c.biomes.insert(format!("{ns}:{nom}"), depuis_json(&v));
            vus += 1;
        }
        if vus == 0 {
            c.manques.push(
                "data/<ns>/worldgen/biome/*.json : aucun — un resource pack seul \
                 ne porte pas les données de biome, il faut une installation ou un datapack"
                    .to_string(),
            );
        }
        c
    }

    pub fn nb_biomes(&self) -> usize {
        self.biomes.len()
    }

    pub fn biome(&self, nom: &str) -> Option<&Biome> {
        self.biomes.get(nom)
    }

    pub fn est_vide(&self) -> bool {
        self.biomes.is_empty() || self.herbe.is_none()
    }

    /// La couleur de ce biome est-elle APPROCHÉE plutôt qu'exacte ?
    pub fn approche(&self, nom: &str) -> bool {
        matches!(
            self.biomes.get(nom).map(|b| b.modificateur),
            Some(Modificateur::Marais)
        )
    }

    /// La couleur d'herbe d'un biome, ou `None` si on ne sait pas.
    pub fn herbe(&self, nom: &str) -> Option<[u8; 3]> {
        let b = self.biomes.get(nom)?;
        let base = match b.herbe {
            Some(c) => c,
            None => echantillon(self.herbe.as_deref()?, b.temperature, b.downfall),
        };
        Some(octets(match b.modificateur {
            Modificateur::Aucun => base,
            // `(c & 0xFEFEFE) + 0x28340A >> 1` : la moyenne avec un vert
            // sombre, le bit de poids faible jeté pour éviter le débordement.
            Modificateur::ForetSombre => ((base & 0x00FE_FEFE) + 0x0028_340A) >> 1,
            // Le dominant des deux verts du marais.
            Modificateur::Marais => 0x6A_7039,
        }))
    }

    /// La couleur de feuillage. **Le modificateur de la forêt sombre ne s'y
    /// applique PAS** — le jeu ne l'applique qu'à l'herbe, et l'étendre
    /// assombrirait les arbres d'un biome sur deux.
    pub fn feuillage(&self, nom: &str) -> Option<[u8; 3]> {
        let b = self.biomes.get(nom)?;
        Some(octets(match b.feuillage {
            Some(c) => c,
            None => echantillon(self.feuillage.as_deref()?, b.temperature, b.downfall),
        }))
    }

    /// La couleur de l'eau. Toujours explicite dans un fichier de biome ; le
    /// défaut du jeu sinon.
    pub fn eau(&self, nom: &str) -> Option<[u8; 3]> {
        let b = self.biomes.get(nom)?;
        Some(octets(b.eau.unwrap_or(EAU_PAR_DEFAUT)))
    }
}

/// `data/<ns>/worldgen/biome/<nom>` → `(ns, nom)`.
fn decouper_biome(chemin: &str) -> Option<(&str, &str)> {
    let reste = chemin.strip_prefix("data/")?;
    let (ns, reste) = reste.split_once('/')?;
    let nom = reste.strip_prefix("worldgen/biome/")?;
    // Un sous-dossier n'est pas un biome : le jeu n'en met pas, et l'accepter
    // fabriquerait des noms qui ne correspondent à rien.
    if nom.contains('/') || ns.is_empty() || nom.is_empty() {
        return None;
    }
    Some((ns, nom))
}

fn depuis_json(v: &serde_json::Value) -> Biome {
    let f = |k: &str, d: f32| v.get(k).and_then(|x| x.as_f64()).unwrap_or(d as f64) as f32;
    let e = v.get("effects");
    let couleur = |k: &str| {
        e.and_then(|e| e.get(k))
            .and_then(|x| x.as_u64())
            .map(|n| (n & 0x00FF_FFFF) as u32)
    };
    Biome {
        temperature: f("temperature", 0.5),
        downfall: f("downfall", 0.5),
        herbe: couleur("grass_color"),
        feuillage: couleur("foliage_color"),
        eau: couleur("water_color"),
        modificateur: match e
            .and_then(|e| e.get("grass_color_modifier"))
            .and_then(|x| x.as_str())
        {
            Some("dark_forest") => Modificateur::ForetSombre,
            Some("swamp") => Modificateur::Marais,
            _ => Modificateur::Aucun,
        },
    }
}

/// L'échantillon de la table pour un couple (température, humidité).
///
/// **C'est la formule du jeu, à la lettre**, et chaque détail y compte :
/// l'humidité est multipliée par la température AVANT d'être inversée, les
/// deux sont serrées à `[0, 1]` d'abord, et l'index est `j × 256 + i` — pas
/// l'inverse. Intervertir les deux axes donne une table plausible et fausse :
/// un désert vert et une jungle jaune.
pub fn echantillon(table: &[u8], temperature: f32, downfall: f32) -> u32 {
    let t = temperature.clamp(0.0, 1.0) as f64;
    let d = (downfall.clamp(0.0, 1.0) as f64) * t;
    let i = ((1.0 - t) * 255.0) as usize;
    let j = ((1.0 - d) * 255.0) as usize;
    let k = (j * COTE_TABLE + i) * 4;
    if k + 2 >= table.len() {
        // Le jeu rend un magenta criard dans ce cas. On préfère un gris
        // neutre : une couleur qui HURLE dans un rendu ferait croire à un
        // bloc bizarre plutôt qu'à une table trop courte.
        return 0x80_8080;
    }
    ((table[k] as u32) << 16) | ((table[k + 1] as u32) << 8) | table[k + 2] as u32
}

fn octets(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}
