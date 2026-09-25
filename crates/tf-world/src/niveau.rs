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
            (tag::COMPOUND, "Player") => lire_joueur(c, n)?,
            _ => c.skip_payload(t).ok()?,
        }
    }
    if let [Some(x), Some(y), Some(z)] = spawn {
        n.apparition = Some([x, y, z]);
    }
    Some(())
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
