//! Les block entities et les entités, entre le presse-papiers et un fichier.
//!
//! **Rien n'est ré-encodé.** Le contenu d'un coffre, l'inventaire d'un
//! porte-armure, les données d'un mod voyagent par leurs OCTETS : on retire
//! les champs que le format range ailleurs (`id`, `x`, `y`, `z`), on en ajoute
//! à des places connues, et le reste est recopié. Ce qu'un lecteur ne comprend
//! pas ne peut pas être abîmé — la propriété du splice, ici aussi.

use tf_anvil::entites::Entite;
use tf_anvil::mobiles::Mobile;
use tf_nbt::{tag, Writer};
use tf_ops::mobiles::tuile_depuis_pos;

use crate::commun::{champ_position, Compound};
use crate::Erreur;

// ── block entities ──────────────────────────────────────────────────────────

/// Une block entity de presse-papiers, faite de ses champs (sans `id`, `x`,
/// `y`, `z`, et sans `TAG_End`), de son `id` et de sa case LOCALE.
///
/// Les trois coordonnées s'ajoutent EN FIN de compound, à des places qu'on
/// connaît : le collage les réécrit en place, exactement comme pour une block
/// entity lue dans une save.
pub(crate) fn block_entity(champs: &[u8], id: Option<&str>, case: [i32; 3]) -> Entite {
    let mut w = Writer::with_capacity(champs.len() + 48);
    w.raw(champs);
    if let Some(id) = id {
        w.field(tag::STRING, "id").raw_str(id);
    }
    let mut at = [0usize; 3];
    for (k, nom) in ["x", "y", "z"].into_iter().enumerate() {
        w.field(tag::INT, nom);
        at[k] = w.len();
        w.i32_payload(case[k]);
    }
    w.end();
    Entite {
        case,
        nbt: w.into_bytes(),
        champs: at,
    }
}

/// Une block entity telle qu'un fichier la range : son `id`, et ses autres
/// champs — sans `id`, `x`, `y`, `z`, et sans `TAG_End`.
pub(crate) fn decomposer(e: &Entite) -> Result<(Option<String>, Vec<u8>), Erreur> {
    let octets = e.octets();
    let c = Compound::lire(&octets, 0)?;
    Ok((
        c.chaine("id").map(str::to_string),
        c.champs_sauf(&["id", "x", "y", "z"]),
    ))
}

// ── entités ─────────────────────────────────────────────────────────────────

/// Dans quel repère un fichier écrit ce qui situe une entité AU-DELÀ de sa
/// position : sa case d'accroche et les cases dont elle se souvient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repere {
    /// Celui du fichier, décalé de `d` : local = fichier − d.
    Connu([i32; 3]),
    /// Celui d'un monde qu'on ne connaît pas. Litematica et le `.nbt` du jeu
    /// réécrivent la POSITION d'une entité dans leur repère, mais laissent le
    /// reste dans celui du monde d'où elle vient.
    Inconnu,
}

/// Une entité importée, et ce qui n'a pas pu la suivre exactement.
pub(crate) struct Importee {
    pub mobile: Mobile,
    /// Cases retenues (lit, ruche…) qu'on ne sait pas situer : laissées
    /// telles quelles dans ses octets, donc jamais réécrites au collage.
    pub souvenirs_laisses: usize,
    /// L'`id` d'une entité accrochée qu'on ne sait pas raccrocher.
    pub accroche_inconnue: Option<String>,
}

/// Au-delà, un passager n'est pas dans le même repère que sa monture.
const PASSAGER_PROCHE: f64 = 16.0;

/// La dimension qu'on suppose à une position globale d'un fichier.
const SURFACE: &str = "minecraft:overworld";

/// Une entité de presse-papiers depuis le compound d'un fichier.
///
/// `nbt` est la charge d'un compound complet — `id` et `TAG_End` compris.
/// `pos` est sa position LOCALE, telle que le format la donne ; elle remplace
/// celle que le compound porte, qui peut être dans un autre repère.
///
/// Trois règles, parce que les outils ne s'accordent pas :
///
/// 1. **La case d'accroche se RECALCULE depuis la position** (cadres,
///    tableaux, nœuds de laisse). Litematica laisse `TileX/Y/Z` dans le repère
///    du monde d'origine, et c'est elle qui décide où le jeu repose le cadre.
/// 2. **Un passager garde son écart à sa monture** s'il est dans le même
///    repère qu'elle (à moins de 16 blocs) ; sinon il est posé SUR elle — le
///    jeu le replace de toute façon à chaque pas.
/// 3. **Une case retenue ne se traduit que si le repère est CONNU** et qu'elle
///    tombe dans l'extrait ; sinon elle reste telle quelle, comme dans une
///    copie dont le lit est resté dehors.
pub(crate) fn importer(
    nbt: Vec<u8>,
    pos: [f64; 3],
    repere: Repere,
    taille: [u32; 3],
    data_version: Option<i32>,
) -> Result<Importee, Erreur> {
    let mut m = Mobile::depuis_compound(nbt, data_version).map_err(|_| Erreur::Illisible)?;
    if m.corps.first().is_none_or(|k| k.pos.is_none()) {
        // Sans `Pos` lisible — absent, ou de la mauvaise forme — on en pose
        // une, sans quoi le jeu la mettrait à l'origine du monde.
        let c = Compound::lire(&m.nbt, 0)?;
        let mut w = Writer::with_capacity(m.nbt.len() + 48);
        w.raw(&c.champs_sauf(&["Pos"]));
        champ_position(&mut w, "Pos", [0.0; 3]);
        w.end();
        m = Mobile::depuis_compound(w.into_bytes(), data_version).map_err(|_| Erreur::Illisible)?;
    }
    let depart = m.corps[0].pos.map(|p| p.v).unwrap_or([0.0; 3]);

    let mut souvenirs_laisses = 0;
    let mut accroche_inconnue = None;
    for (rang, k) in m.corps.iter_mut().enumerate() {
        if let Some(p) = &mut k.pos {
            p.v = if rang == 0 {
                pos
            } else {
                let d: [f64; 3] = std::array::from_fn(|a| p.v[a] - depart[a]);
                if d.iter().all(|x| x.abs() < PASSAGER_PROCHE) {
                    std::array::from_fn(|a| pos[a] + d[a])
                } else {
                    pos
                }
            };
        }
        let ici = k.pos.map(|p| p.v);
        let id = k.id.clone().unwrap_or_default();
        let facing = k.facing.map(|f| f.v);
        if let Some(t) = &mut k.tuile {
            match (ici.and_then(|p| tuile_depuis_pos(&id, p, facing)), repere) {
                (Some(c), _) => t.v = c,
                (None, Repere::Connu(d)) => {
                    t.v = std::array::from_fn(|a| t.v[a].wrapping_sub(d[a]));
                }
                (None, Repere::Inconnu) => {
                    // Ni recalculable ni traduisible : ses octets restent ceux
                    // du fichier, et le compte rendu le nomme.
                    k.tuile = None;
                    accroche_inconnue = Some(id);
                }
            }
        }
        let avant = k.retenues.len();
        match repere {
            Repere::Connu(d) => {
                for r in &mut k.retenues {
                    r.v = std::array::from_fn(|a| r.v[a].wrapping_sub(d[a]));
                }
                // Une case retenue ne suit que si le bloc qu'elle désigne est
                // DANS l'extrait — la règle de la copie. Une position GLOBALE
                // porte sa dimension, et un fichier ne dit pas de laquelle il
                // vient : on suit celles de la surface, où vivent les
                // villageois qui en portent, et on laisse les autres.
                k.retenues.retain(|r| {
                    r.dimension.as_deref().is_none_or(|d| d == SURFACE)
                        && (0..3).all(|a| r.v[a] >= 0 && (r.v[a] as u32) < taille[a])
                });
            }
            Repere::Inconnu => k.retenues.clear(),
        }
        souvenirs_laisses += avant - k.retenues.len();
    }
    Ok(Importee {
        mobile: m,
        souvenirs_laisses,
        accroche_inconnue,
    })
}

/// Une entité telle qu'un fichier Sponge la range : son `id`, et ses autres
/// champs — sans `id`, et sans `TAG_End`. Sa position y reste, LOCALE.
pub(crate) fn decomposer_mobile(m: &Mobile) -> Result<(Option<String>, Vec<u8>), Erreur> {
    let octets = m.octets();
    let c = Compound::lire(&octets, 0)?;
    Ok((c.chaine("id").map(str::to_string), c.champs_sauf(&["id"])))
}
