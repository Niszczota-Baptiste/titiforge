#![allow(dead_code)]
// chaque test compile SA copie de ce module : ce qu'un
// fichier n'emploie pas n'est pas mort pour autant.

//! **Un codex MINIMAL, écrit à la volée** — et ce qu'il débloque.
//!
//! Neuf tests de la coque ne tournaient que si on leur donnait un vrai pack
//! (`TF_PACK`), donc jamais en intégration continue, donc jamais chez
//! quelqu'un qui n'a pas le serveur sous la main. Un test qui ne tourne pas
//! est un test qui ne dit rien, et ce sont justement ceux qui tiennent la
//! JONCTION entre la coque, le moteur et le rendu.
//!
//! Le codex du site est reconnu à un seul fichier : `blockstates.json` à la
//! racine (`tf_assets::jeu::Genre::Codex`). On en écrit donc un, avec ses
//! modèles et ses textures, tout en code — comme les régions, et pour la même
//! raison : **pas de fixture binaire dans le dépôt**, un `.png` commité étant
//! aussi opaque en revue qu'un `.mca`.
//!
//! Ce codex ne REMPLACE pas le vrai. Il ne porte que ce qu'il faut pour que
//! la chaîne complète tourne ; ce qui se mesure sur un vrai pack continue de
//! se mesurer sur un vrai pack (`TF_PACK`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tf_app::chargeur::{Chargeur, Reponse};
use tf_app::scene::{Arrivee, Ouvert};
use tf_world::source::{Dimension, Folder, Overview, RegionSource, Result as ResSource};
use tf_world::{coords::RegionPos, MemorySource};

/// Les blocs que les fixtures de `tf-bench` posent vraiment.
///
/// Écrits à la main plutôt que déduits : une liste qui se déduirait du
/// générateur suivrait ses changements en silence, et un bloc oublié
/// retomberait sur la texture manquante sans qu'aucun test ne le dise.
pub const BLOCS: &[&str] = &[
    "stone",
    "dirt",
    "coarse_dirt",
    "gravel",
    "granite",
    "diorite",
    "andesite",
    "tuff",
    "deepslate",
    "calcite",
    "smooth_basalt",
    "amethyst_block",
    "coal_ore",
    "copper_ore",
    "iron_ore",
    "gold_ore",
    "redstone_ore",
    "lapis_ore",
    "diamond_ore",
    "emerald",
    "diamond",
    "full",
    "water",
    "lava",
    "chest",
];

/// Un dossier temporaire qui s'efface même si le test PANIQUE.
///
/// Un `remove_dir_all` en fin de test ne s'exécute pas quand l'assertion
/// tombe — et c'est précisément le jour où l'on relance le plus souvent.
pub struct Jetable(PathBuf);

impl Jetable {
    pub fn neuf(etiquette: &str) -> Jetable {
        let d = std::env::temp_dir().join(format!(
            "tf-essai-{}-{}-{etiquette}",
            std::process::id(),
            suivant()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dossier créable");
        Jetable(d)
    }
    pub fn chemin(&self) -> &Path {
        &self.0
    }
    pub fn texte(&self) -> &str {
        self.0.to_str().expect("chemin lisible")
    }
}

impl Drop for Jetable {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Un compteur par PROCESSUS : deux fixtures du même test ne doivent pas
/// partager un dossier. C'est le piège des deux `Ouvert` qui écrivaient dans
/// la même copie de travail, un cran plus bas.
fn suivant() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn ecrire(racine: &Path, chemin: &str, octets: &[u8]) {
    let p = racine.join(chemin);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).expect("dossier créable");
    }
    std::fs::write(p, octets).expect("écriture");
}

/// Une tuile 16 × 16 unie, en vrai PNG.
///
/// Unie parce que ce qu'on teste ici est la CHAÎNE, pas l'échantillonnage.
/// La couleur dérive du nom, donc deux blocs ne peuvent pas se confondre si
/// jamais un test regarde les pixels.
fn tuile(nom: &str) -> Vec<u8> {
    tuile_de(nom, 16)
}

/// La même, à un côté choisi. Un pack n'est PAS uniformément en 16 × 16 —
/// mesuré, 92,7 % le sont — et le cas qui compte ici est la tuile plus GRANDE
/// que l'atlas courant : le tableau n'a qu'une taille de couche, et la règle
/// du dépôt est d'agrandir les petites plutôt que de réduire les grandes.
fn tuile_de(nom: &str, cote: u32) -> Vec<u8> {
    let h = nom.bytes().fold(0x811c9dc5u32, |a, b| {
        (a ^ b as u32).wrapping_mul(0x01000193)
    });
    let (r, g, b) = (
        (h >> 16) as u8 | 0x40,
        (h >> 8) as u8 | 0x40,
        h as u8 | 0x40,
    );
    let pixels: Vec<u8> = (0..cote * cote).flat_map(|_| [r, g, b, 255]).collect();
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, cote, cote);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().expect("en-tête");
        w.write_image_data(&pixels).expect("pixels");
    }
    out
}

/// Un cube plein qui déclare ses six faces et les CULLE.
///
/// C'est ce qui rend le bloc opaque, donc ce qui donne du travail au mailleur
/// glouton. Un modèle non opaque ferait sortir des maillages vingt fois trop
/// gros et fausserait toute mesure prise dessus — c'est le piège de
/// `grass_block` classé « modèle », qui rendait visible chaque bloc SOUS la
/// surface.
fn modele_cube(ns: &str, nom: &str) -> String {
    format!(
        r##"{{"textures":{{"a":"{ns}:block/{nom}"}},
        "elements":[{{"from":[0,0,0],"to":[16,16,16],"faces":{{
          "up":{{"texture":"#a","cullface":"up"}},
          "down":{{"texture":"#a","cullface":"down"}},
          "north":{{"texture":"#a","cullface":"north"}},
          "south":{{"texture":"#a","cullface":"south"}},
          "east":{{"texture":"#a","cullface":"east"}},
          "west":{{"texture":"#a","cullface":"west"}}}}}}]}}"##
    )
}

/// Un bloc-MODÈLE : `n` cuboïdes empilés, qui ne remplissent pas la case.
///
/// Un non-cube doit rester NON opaque, sinon il efface les faces de ses
/// voisins — un escalier creuserait un trou dans le mur qu'il touche.
fn modele_cuboides(ns: &str, nom: &str, n: u8) -> String {
    let mut el = String::new();
    let t = n.max(1) as i32;
    for k in 0..t {
        if k > 0 {
            el.push(',');
        }
        let bas = k * 16 / t;
        let haut = ((k + 1) * 16 / t).max(bas + 1).min(16);
        el.push_str(&format!(
            r##"{{"from":[2,{bas},2],"to":[14,{haut},14],"faces":{{
              "up":{{"texture":"#a"}},"down":{{"texture":"#a"}},
              "north":{{"texture":"#a"}},"south":{{"texture":"#a"}},
              "east":{{"texture":"#a"}},"west":{{"texture":"#a"}}}}}}"##
        ));
    }
    format!(r##"{{"textures":{{"a":"{ns}:block/{nom}"}},"elements":[{el}]}}"##)
}

/// **Écrit un codex complet** dans `racine`, et rend le chemin à passer à
/// `Ouvert::ouvrir`.
///
/// Couvre les deux fixtures : les blocs `minecraft:*` que `Terrain` pose, ET
/// les 221 `minefield:*` du catalogue que `Build` pose — avec leur VRAIE
/// forme, celle que `tf-bench` porte et qui est engendrée par le même code
/// que l'application. Sans eux, un monde bâti se chargeait avec des blocs
/// inconnus : six quads pour une région de 256 chunks, et une mesure qui ne
/// mesurait rien. Une fixture qui ne couvre pas ce que l'autre fixture écrit
/// est un test vert qui ne dit rien.
///
/// `extras` ajoute des blocs au-delà : c'est ce qui permet à un test de poser
/// un bloc que la scène ne contenait PAS, donc d'éprouver le chemin de l'état
/// inconnu.
pub fn codex(racine: &Path, extras: &[&str]) -> String {
    let mut etats = String::from("{");
    let mut premier = true;
    let mut ligne = |etats: &mut String, ns: &str, nom: &str| {
        if !premier {
            etats.push(',');
        }
        premier = false;
        etats.push_str(&format!(
            r#""{ns}:{nom}":{{"variants":{{"":{{"model":"{ns}:block/{nom}"}}}}}}"#
        ));
    };
    // Le catalogue Minefield, avec ses formes réelles.
    for (id, forme, cuboides) in tf_bench::catalogue::BLOCS {
        let Some(nom) = id.strip_prefix("minefield:") else {
            continue;
        };
        ligne(&mut etats, "minefield", nom);
        let json = match forme {
            tf_bench::catalogue::Forme::Modele => modele_cuboides("minefield", nom, *cuboides),
            _ => modele_cube("minefield", nom),
        };
        ecrire(racine, &format!("models/block_{nom}.json"), json.as_bytes());
        ecrire(
            racine,
            &format!("render-textures/block_{nom}.png"),
            &tuile(nom),
        );
    }
    for nom in BLOCS.iter().chain(extras.iter()) {
        ligne(&mut etats, "minecraft", nom);
        ecrire(
            racine,
            &format!("models/block_{nom}.json"),
            modele_cube("minecraft", nom).as_bytes(),
        );
        ecrire(
            racine,
            &format!("render-textures/block_{nom}.png"),
            &tuile(nom),
        );
    }
    etats.push('}');
    ecrire(racine, "blockstates.json", etats.as_bytes());
    racine.to_str().expect("chemin lisible").to_string()
}

/// Sème un monde d'essai : `cote × cote` régions de terrain, avec biomes.
pub fn semer(dir: &Path, cote: i32, chunks: u32) {
    let region = dir.join("region");
    std::fs::create_dir_all(&region).expect("dossier créable");
    let t = tf_bench::Terrain {
        side: chunks,
        biomes: true,
        ..Default::default()
    };
    for x in 0..cote {
        for z in 0..cote {
            let brut = tf_bench::region_en(&t, x, z);
            std::fs::write(region.join(tf_anvil::region::region_file_name(x, z)), &brut)
                .expect("écriture");
        }
    }
    std::fs::write(dir.join("level.dat"), []).expect("level.dat");
}

/// Remplace la texture d'un bloc par une plus GRANDE.
///
/// Sert au cas où l'atlas doit GRANDIR : une tuile qui dépasse son côté fait
/// réécrire toutes les couches déjà montées. Il ne doit ni la réduire — ce
/// qui perd la moitié de ses pixels — ni recharger la zone, ce qu'il faisait,
/// et en boucle sous le streaming. Le pack du serveur mélange du 16 et du 32.
pub fn texture_hd(racine: &Path, nom: &str, cote: u32) {
    ecrire(
        racine,
        &format!("render-textures/block_{nom}.png"),
        &tuile_de(nom, cote),
    );
}

/// Sème un monde BÂTI — un bâtiment décoré, pas du sous-sol.
///
/// Les deux fixtures ne mesurent pas la même chose et les confondre fausse
/// tout : `Terrain` mesure Anvil, `Build` mesure le rendu. Sur du sous-sol
/// presque toutes les sections sont homogènes et se maillent en six quads ;
/// une région bâtie en porte sept MILLIONS. Ce qui coûte par ÉDITION, et qui
/// dépend de ce que la scène contient, ne se mesure que là.
pub fn semer_build(dir: &Path, chunks: u32) {
    let region = dir.join("region");
    std::fs::create_dir_all(&region).expect("dossier créable");
    let b = tf_bench::Build {
        side: chunks,
        ..Default::default()
    };
    std::fs::write(
        region.join(tf_anvil::region::region_file_name(0, 0)),
        tf_bench::build::region(&b),
    )
    .expect("écriture");
    std::fs::write(dir.join("level.dat"), []).expect("level.dat");
}

// ── Ce que trois fichiers de test partagent ────────────────────────────────
//
// Écrites une fois : `chargement`, `residence` et tout ce qui suivra comparent
// une scène streamée à une scène chargée d'un bloc, et deux copies de la
// canonisation des couches d'atlas finiraient par ne plus canoniser la même
// chose.

/// Ramène les couches d'atlas d'un côté à un dictionnaire COMMUN.
///
/// Un numéro de couche n'a de sens que relativement à son atlas, et les deux
/// chemins n'ont pas le même : celui qui charge d'un bloc bâtit son atlas par
/// nom, celui qui streame l'ÉTEND à mesure. Comparer les numéros, c'est
/// comparer deux systèmes de coordonnées.
pub fn canon(o: &Ouvert, mots: &mut Vec<String>) -> Vec<u32> {
    o.monde
        .atlas
        .couches
        .iter()
        .map(|c| match mots.iter().position(|m| *m == c.nom) {
            Some(i) => i as u32,
            None => {
                mots.push(c.nom.clone());
                mots.len() as u32 - 1
            }
        })
        .collect()
}

/// Ce que la scène MONTRE : chaque quad par sa géométrie, sa teinte, son
/// origine de section et le NOM de sa texture.
///
/// L'origine et non l'indice de lot : l'ordre des lots dépend de l'ordre
/// d'arrivée des cellules, qui est justement ce qui diffère entre les deux
/// chemins. Ce qu'on veut savoir est si les mêmes quads sont au même ENDROIT.
pub fn montre(o: &Ouvert, c: &[u32]) -> Vec<(u32, u32, [u32; 4], String)> {
    let origines = o.monde.arene.origines();
    let mut v: Vec<(u32, u32, [u32; 4], String)> = o
        .monde
        .arene
        // Les TROUS sautés : une place libérée ne dessine rien, et deux
        // chemins n'en laissent pas les mêmes.
        .visibles()
        .map(|i| {
            let nom = o
                .monde
                .atlas
                .couches
                .get(i.couche as usize)
                .map(|x| x.nom.clone())
                .unwrap_or_default();
            let org = origines
                .get(i.section as usize)
                .map(|p| p.position.map(|f| f.to_bits()))
                .unwrap_or([0; 4]);
            (
                i.geo,
                i.teinte,
                org,
                c.get(i.couche as usize).map(|_| nom).unwrap_or_default(),
            )
        })
        .collect();
    // L'ORDRE des instances suit l'ordre des lots, donc l'ordre d'arrivée.
    // Ce qui doit être identique est l'ENSEMBLE, pas la suite.
    v.sort();
    v
}

/// **Ce que la passe de MODÈLES dessine**, rejoué comme le shader le fait.
///
/// Chaque face tirée de l'appel de dessin : l'origine réelle de sa pose, sa
/// case, et le CONTENU de la face — couche ramenée au dictionnaire commun
/// (`canon`), parce qu'un numéro de couche n'a de sens que relativement à son
/// atlas. Jamais le rang dans la table de faces, qui dépend de l'ordre de
/// remplissage, ni l'emplacement, qui dépend de l'ordre d'arrivée.
///
/// Le contenu est résumé en une empreinte de 64 bits : sur une région bâtie
/// l'appel de dessin porte des millions de faces, et en garder seize mots
/// chacune ferait peser le témoin plus que la scène.
pub fn montre_modeles(o: &Ouvert, c: &[u32]) -> Vec<([u32; 4], u32, u64)> {
    let m = &o.monde.modeles;
    let origines = o.monde.arene.origines();
    let mut v: Vec<([u32; 4], u32, u64)> = m
        .dessinees()
        .into_iter()
        .map(|(slot, local, f)| {
            let mut face = m.faces[f as usize];
            face.couche = c.get(face.couche as usize).copied().unwrap_or(u32::MAX);
            // FNV-1a : assez pour distinguer deux faces, sans dépendance.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in bytemuck::bytes_of(&face) {
                h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
            }
            (origines[slot as usize].position.map(f32::to_bits), local, h)
        })
        .collect();
    v.sort_unstable();
    v
}

/// Fait tourner le chargeur jusqu'à ce que `n` cellules soient intégrées.
pub fn streamer(o: &mut Ouvert, c: &mut Chargeur, n: usize) -> usize {
    let debut = Instant::now();
    let mut faites = 0;
    while faites < n && debut.elapsed() < Duration::from_secs(60) {
        let lot = c.recevoir(0);
        if lot.is_empty() {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        let mut arrivees = Vec::new();
        for r in lot {
            match r {
                Reponse::Prete {
                    cellule,
                    sections,
                    interner,
                } => arrivees.push(Arrivee {
                    cellule,
                    sections,
                    interner,
                }),
                Reponse::Echec(e) => panic!("le chargeur a échoué : {e}"),
            }
        }
        faites += arrivees.len();
        o.integrer(arrivees).expect("intégration");
    }
    faites
}

/// Une source qui COMPTE ses lectures.
///
/// C'est la seule façon de vérifier la conclusion qui a décidé l'unité de
/// lecture : un chunk demandé seul coûte × 10 d'un chunk amorti, parce que le
/// `.mca` est relu à chaque appel. Un test qui chronomètre dirait la même
/// chose en moins sûr — un compteur ne dépend pas de la charge de la machine.
pub struct Comptee {
    dessous: MemorySource,
    lectures: AtomicUsize,
}

impl Comptee {
    pub fn neuve() -> Comptee {
        Comptee {
            dessous: MemorySource::new(),
            lectures: AtomicUsize::new(0),
        }
    }
    pub fn lectures(&self) -> usize {
        self.lectures.load(Ordering::Relaxed)
    }
}

impl RegionSource for Comptee {
    fn dimensions(&self) -> ResSource<Vec<Dimension>> {
        self.dessous.dimensions()
    }
    fn overview(&self, d: &Dimension, f: Folder) -> ResSource<Overview> {
        self.dessous.overview(d, f)
    }
    fn read_region(&self, d: &Dimension, f: Folder, p: RegionPos) -> ResSource<Vec<u8>> {
        self.lectures.fetch_add(1, Ordering::Relaxed);
        self.dessous.read_region(d, f, p)
    }
    fn read_external(&self, d: &Dimension, f: Folder, n: &str) -> ResSource<Vec<u8>> {
        self.dessous.read_external(d, f, n)
    }
    fn external_names(&self, d: &Dimension, f: Folder) -> ResSource<Vec<String>> {
        self.dessous.external_names(d, f)
    }
}

/// Une source de `cote × cote` régions de terrain, avec biomes.
pub fn monde(cote: i32, chunks: u32) -> Arc<Comptee> {
    let s = Comptee::neuve();
    let t = tf_bench::Terrain {
        side: chunks,
        biomes: true,
        ..Default::default()
    };
    for x in 0..cote {
        for z in 0..cote {
            s.dessous.put_region(
                Dimension::Overworld,
                Folder::Region,
                RegionPos { x, z },
                tf_bench::region_en(&t, x, z),
            );
        }
    }
    Arc::new(s)
}
