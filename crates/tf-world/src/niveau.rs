//! **Ce que `level.dat` dit d'un monde** — son nom, où l'on apparaît, où se
//! tient le joueur.
//!
//! Lu pour une seule raison : ouvrir un monde LÀ où l'on joue. Un éditeur qui
//! ouvre tous les mondes au bloc (0, 0) montre, sur une vieille save, un
//! désert que personne n'a jamais visité — pendant que le build est à quatre
//! mille blocs de là.
//!
//! `level.dat` est un NBT compressé en gzip :
//! `{"": {Data: {LevelName, SpawnX, SpawnY, SpawnZ, Player: {Pos, Dimension}}}}`.
//! Le lecteur est ciblé comme le reste du dépôt : il ne descend que dans
//! `Data` et `Player`, et saute tout le reste sans le matérialiser — les
//! réglages de génération d'un monde 1.18 pèsent bien plus que ce qu'on y
//! cherche.
//!
//! **Rien ici n'écrit `level.dat`.** Le jeu y garde l'inventaire du joueur en
//! solo, ses coordonnées, l'heure, la météo : une réécriture approximative
//! coûterait bien plus que ce qu'elle rapporterait.
//!
//! On y lit aussi le GÉNÉRATEUR de la surface, pour une raison : savoir si le
//! monde est VIDE — plat, et toutes ses couches d'air. Dans un tel monde, et
//! seulement là, une opération peut créer les chunks qui manquent : le jeu y
//! générerait du vide, donc un chunk vide est exactement ce qu'il aurait
//! écrit (voir [`MondeVide`]).

use std::path::Path;

use tf_nbt::{tag, Cur};

/// Ce qu'on retient d'un `level.dat`. Tout est facultatif : un champ absent ou
/// de la mauvaise forme est laissé à `None`, jamais deviné.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Niveau {
    /// Le nom affiché dans la liste des mondes du jeu — qui n'est pas
    /// toujours celui du dossier (« Nouveau monde (3) »).
    pub nom: Option<String>,
    /// Le point d'apparition du monde.
    pub apparition: Option<[i32; 3]>,
    /// Où se tient le joueur en solo, et dans quelle dimension.
    pub joueur: Option<[f64; 3]>,
    pub dimension_joueur: Option<String>,
    /// Le `DataVersion` du monde : la version du jeu qui l'a écrit en
    /// dernier — donc la FORME des octets de ses coffres et de ses entités.
    /// Un fichier d'échange exporté l'emporte ; un fichier importé plus récent
    /// peut porter des blocs que ce monde ne connaît pas.
    pub data_version: Option<i32>,
    /// Le générateur de la SURFACE (`WorldGenSettings`, 1.16+), s'il se lit.
    pub generation: Option<Generation>,
}

/// Ce que le générateur de la surface dit de lui-même.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Generation {
    /// `minecraft:flat`, `minecraft:noise`, ou celui d'un mod.
    pub genre: String,
    /// Les couches d'un monde plat, de bas en haut : bloc et épaisseur.
    pub couches: Vec<(String, i32)>,
    /// Le biome d'un monde plat, quand il le dit.
    pub biome: Option<String>,
}

/// **Un monde dont la surface est VIDE** : plat, et toutes ses couches d'air
/// — le préréglage « The Void » du jeu, ou un plat sans couche.
///
/// Ce qu'il porte est ce qu'il faut pour écrire le chunk que le jeu y
/// générerait : la version, pour la forme des octets, et le biome, dont le
/// jeu remplirait ses sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MondeVide {
    pub data_version: i32,
    pub biome: String,
}

impl Niveau {
    /// **Ce monde est-il vide ?** Plat, et chaque couche est de l'air.
    ///
    /// Un plat « Classique » (herbe, terre, bedrock) ne l'est pas : y créer un
    /// chunk vide y creuserait un trou jusqu'au fond du monde. Sans
    /// `DataVersion`, on ne sait pas quelle forme d'octets écrire : pas vide
    /// non plus. Un monde plat qui ne nomme pas son biome reçoit celui que le
    /// jeu lui donnerait, les plaines.
    pub fn monde_vide(&self) -> Option<MondeVide> {
        let g = self.generation.as_ref()?;
        let plat = matches!(
            g.genre.strip_prefix("minecraft:").unwrap_or(&g.genre),
            "flat"
        );
        let air = |b: &str| {
            matches!(
                b.strip_prefix("minecraft:").unwrap_or(b),
                "air" | "cave_air" | "void_air"
            )
        };
        if !plat || !g.couches.iter().all(|(b, _)| air(b)) {
            return None;
        }
        Some(MondeVide {
            data_version: self.data_version?,
            biome: g
                .biome
                .clone()
                .unwrap_or_else(|| "minecraft:plains".to_string()),
        })
    }
}

impl Niveau {
    /// **Où regarder en ouvrant**, dans la SURFACE : le joueur s'il s'y
    /// trouve, sinon le point d'apparition. Le joueur d'abord parce que c'est
    /// là qu'il construisait en quittant ; la surface seulement parce que
    /// c'est la seule dimension que la coque ouvre aujourd'hui, et que des
    /// coordonnées du Nether désigneraient un tout autre endroit.
    pub fn ou_regarder(&self) -> Option<[i32; 3]> {
        let en_surface = self
            .dimension_joueur
            .as_deref()
            .is_none_or(|d| d == "minecraft:overworld");
        match self.joueur {
            Some([x, y, z]) if en_surface => {
                Some([x.floor() as i32, y.floor() as i32, z.floor() as i32])
            }
            _ => self.apparition,
        }
    }
}

/// Lit le `level.dat` d'un dossier de save. `None` s'il manque ou ne se lit
/// pas — une save sans `level.dat` lisible s'ouvre quand même, là où elle
/// s'ouvrait avant.
pub fn lire_fichier(monde: &Path) -> Option<Niveau> {
    lire(&std::fs::read(monde.join("level.dat")).ok()?)
}

/// Décode un `level.dat` : gzip, ou NBT nu (certains outils l'écrivent sans
/// compression, et le jeu le relit quand même).
pub fn lire(octets: &[u8]) -> Option<Niveau> {
    let brut = match tf_anvil::inflate(octets, tf_anvil::Compression::Gzip) {
        Ok(b) => b,
        Err(_) if octets.first() == Some(&tag::COMPOUND) => octets.to_vec(),
        Err(_) => return None,
    };
    let mut c = Cur::new(&brut);
    c.enter_root().ok()?;
    let mut n = Niveau::default();
    while let Some((t, k)) = c.next_field().ok()? {
        if t == tag::COMPOUND && k == "Data" {
            lire_data(&mut c, &mut n)?;
        } else {
            c.skip_payload(t).ok()?;
        }
    }
    Some(n)
}

fn lire_data(c: &mut Cur<'_>, n: &mut Niveau) -> Option<()> {
    let mut spawn: [Option<i32>; 3] = [None; 3];
    while let Some((t, k)) = c.next_field().ok()? {
        match (t, k) {
            (tag::STRING, "LevelName") => n.nom = Some(c.str().ok()?.to_string()),
            (tag::INT, "SpawnX") => spawn[0] = Some(c.i32().ok()?),
            (tag::INT, "SpawnY") => spawn[1] = Some(c.i32().ok()?),
            (tag::INT, "SpawnZ") => spawn[2] = Some(c.i32().ok()?),
            (tag::INT, "DataVersion") => n.data_version = Some(c.i32().ok()?),
            (tag::COMPOUND, "Player") => lire_joueur(c, n)?,
            (tag::COMPOUND, "WorldGenSettings") => lire_generation(c, n)?,
            _ => c.skip_payload(t).ok()?,
        }
    }
    if let [Some(x), Some(y), Some(z)] = spawn {
        n.apparition = Some([x, y, z]);
    }
    Some(())
}

/// `WorldGenSettings.dimensions."minecraft:overworld".generator` — et rien
/// d'autre : les dimensions d'un mod, le Nether et l'End sont sautés.
fn lire_generation(c: &mut Cur<'_>, n: &mut Niveau) -> Option<()> {
    while let Some((t, k)) = c.next_field().ok()? {
        if (t, k) != (tag::COMPOUND, "dimensions") {
            c.skip_payload(t).ok()?;
            continue;
        }
        while let Some((t, k)) = c.next_field().ok()? {
            if (t, k) != (tag::COMPOUND, "minecraft:overworld") {
                c.skip_payload(t).ok()?;
                continue;
            }
            while let Some((t, k)) = c.next_field().ok()? {
                if (t, k) == (tag::COMPOUND, "generator") {
                    n.generation = Some(lire_generateur(c)?);
                } else {
                    c.skip_payload(t).ok()?;
                }
            }
        }
    }
    Some(())
}

fn lire_generateur(c: &mut Cur<'_>) -> Option<Generation> {
    let mut g = Generation::default();
    while let Some((t, k)) = c.next_field().ok()? {
        match (t, k) {
            (tag::STRING, "type") => g.genre = c.str().ok()?.to_string(),
            // Un générateur de BRUIT nomme ses réglages par une chaîne : seul
            // celui d'un monde plat est un compound, et c'est lui qu'on lit.
            (tag::COMPOUND, "settings") => {
                while let Some((t, k)) = c.next_field().ok()? {
                    match (t, k) {
                        (tag::STRING, "biome") => g.biome = Some(c.str().ok()?.to_string()),
                        (tag::LIST, "layers") => {
                            let (et, len) = c.list_header().ok()?;
                            if et != tag::COMPOUND {
                                c.skip_list_body(et, len).ok()?;
                                continue;
                            }
                            for _ in 0..len {
                                let (mut bloc, mut epaisseur) = (None, 0);
                                while let Some((t, k)) = c.next_field().ok()? {
                                    match (t, k) {
                                        (tag::STRING, "block") => {
                                            bloc = Some(c.str().ok()?.to_string())
                                        }
                                        (tag::INT, "height") => epaisseur = c.i32().ok()?,
                                        _ => c.skip_payload(t).ok()?,
                                    }
                                }
                                // Une couche sans bloc n'est pas de l'air :
                                // on ne la devine pas.
                                g.couches
                                    .push((bloc.unwrap_or_else(|| "?".into()), epaisseur));
                            }
                        }
                        _ => c.skip_payload(t).ok()?,
                    }
                }
            }
            _ => c.skip_payload(t).ok()?,
        }
    }
    Some(g)
}

fn lire_joueur(c: &mut Cur<'_>, n: &mut Niveau) -> Option<()> {
    while let Some((t, k)) = c.next_field().ok()? {
        match (t, k) {
            (tag::LIST, "Pos") => {
                let (et, len) = c.list_header().ok()?;
                if et == tag::DOUBLE && len == 3 {
                    let mut p = [0f64; 3];
                    for v in &mut p {
                        *v = f64::from_bits(c.u64().ok()?);
                    }
                    // Une position qui n'est pas un nombre ferait ouvrir le
                    // monde nulle part : on retombe sur le point d'apparition.
                    if p.iter().all(|v| v.is_finite()) {
                        n.joueur = Some(p);
                    }
                } else {
                    c.skip_list_body(et, len).ok()?;
                }
            }
            // Depuis 1.16 un identifiant ; avant, un numéro.
            (tag::STRING, "Dimension") => {
                n.dimension_joueur = Some(c.str().ok()?.to_string());
            }
            (tag::INT, "Dimension") => {
                n.dimension_joueur = Some(
                    match c.i32().ok()? {
                        0 => "minecraft:overworld",
                        -1 => "minecraft:the_nether",
                        1 => "minecraft:the_end",
                        _ => "?",
                    }
                    .to_string(),
                );
            }
            _ => c.skip_payload(t).ok()?,
        }
    }
    Some(())
}

/// Une save trouvée dans une installation.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveTrouvee {
    pub chemin: std::path::PathBuf,
    /// Le nom affiché : celui de `level.dat` s'il se lit, sinon celui du
    /// dossier.
    pub nom: String,
    /// La dernière fois que le jeu a écrit `level.dat` — c'est-à-dire la
    /// dernière partie.
    pub derniere_partie: Option<std::time::SystemTime>,
}

/// **Les saves d'une installation** (`saves/*/level.dat`), de la plus
/// récemment jouée à la plus ancienne.
///
/// Le critère d'une save est `level.dat`, comme partout ailleurs : un dossier
/// `saves/` sert aussi de fourre-tout, et y proposer un dossier de captures
/// ferait ouvrir un monde vide.
pub fn saves_de(installation: &Path) -> Vec<SaveTrouvee> {
    let Ok(entrees) = std::fs::read_dir(installation.join("saves")) else {
        return Vec::new();
    };
    let mut out: Vec<SaveTrouvee> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("level.dat").is_file())
        .map(|p| {
            let dossier = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let nom = lire_fichier(&p)
                .and_then(|n| n.nom)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(dossier);
            let derniere_partie = std::fs::metadata(p.join("level.dat"))
                .and_then(|m| m.modified())
                .ok();
            SaveTrouvee {
                chemin: p,
                nom,
                derniere_partie,
            }
        })
        .collect();
    // La plus récente d'abord ; à égalité, le chemin — pour un ordre TOTAL,
    // qu'une liste ne change pas d'une ouverture à l'autre.
    out.sort_by(|a, b| {
        b.derniere_partie
            .cmp(&a.derniere_partie)
            .then_with(|| a.chemin.cmp(&b.chemin))
    });
    out
}
