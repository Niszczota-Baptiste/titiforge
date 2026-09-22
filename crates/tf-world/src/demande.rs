//! **Ce que la caméra demande**, et dans quel ORDRE.
//!
//! La fenêtre de résidence existe et sait évincer ; ce module dit ce qu'il
//! faut y mettre. C'est une décision PURE — ni disque, ni GPU, ni horloge —
//! parce que c'est là que les fautes se paient et que c'est le seul endroit
//! où on peut les attraper sans monter une machine.
//!
//! **Le budget que ça sert est mesuré** (`tf-app --example residence`) : une
//! région bâtie pèse 186 Mo résidents et met 867 ms à se charger, soit 108
//! images à 8 ms. Deux gigaoctets n'en tiennent que onze. Il n'existe donc
//! aucune situation où « tout charger » est une réponse : l'ordre dans lequel
//! on charge EST la fonctionnalité, parce qu'on n'ira jamais au bout de la
//! liste.
//!
//! **Un DISQUE, pas un carré.** Un carré de rayon `r` porte `(2r+1)²`
//! cellules, un disque `≈ πr²` — 289 contre 201 à `r = 8`, soit 30 % de moins
//! pour le même horizon. Et surtout un disque est invariant par ROTATION :
//! tourner sur place ne change pas d'un iota ce qui est résident, là où un
//! carré ferait entrer et sortir ses coins. Sur une fenêtre qui évince, un
//! ensemble qui bouge quand la caméra tourne est du travail jeté à chaque
//! coup d'œil.
//!
//! **Et le disque se découpe sur la grille de CELLULES, pas sur la position
//! exacte de l'œil.** Ce qui est demandé ne change donc qu'en franchissant
//! une frontière de cellule, jamais pendant qu'on marche à l'intérieur de
//! l'une d'elles — même raison que ci-dessus, appliquée à la translation au
//! lieu de la rotation. L'ordre, lui, suit la position réelle : c'est une
//! question d'urgence, pas d'appartenance.

use std::collections::HashSet;

use crate::coords::BlockPos;
use crate::decoupe::{cellules_autour, Cellule, Niveau, RAYON_MAX};

/// Ce que coûte d'être DERRIÈRE, en multiple de la distance.
///
/// Le score est `distance × (PENALITE_DOS - cos)` : droit devant vaut sa
/// distance, de côté le double, dans le dos le triple. Continu exprès — un
/// classement en deux camps « devant / derrière » ferait basculer les
/// cellules de côté au moindre mouvement de souris, et une cellule qui entre
/// et sort de la liste à chaque image est du chargement jeté.
const PENALITE_DOS: f32 = 2.0;

/// En deçà de cette longueur, la direction du regard ne veut plus rien dire.
///
/// Un regard vertical pile n'a pas de composante horizontale, et une caméra
/// mal initialisée en a une nulle. Diviser par là donnerait des `NaN`, qui ne
/// plantent pas : ils se propagent dans la comparaison, qui répond `false`
/// dans les deux sens, et le tri rend un ordre arbitraire — donc un
/// chargement qui part dans le désordre sans qu'aucune erreur ne le dise.
/// Sous ce seuil, on classe à la DISTANCE seule, ce qui est la bonne réponse :
/// quand on regarde ses pieds, aucune direction horizontale n'est privilégiée.
const REGARD_MINI: f32 = 1e-3;

/// Une cellule que la caméra demande, avec de quoi expliquer son rang.
///
/// Les deux mesures voyagent avec la cellule plutôt que d'être recalculées
/// par l'appelant : deux implémentations d'une même règle finissent par
/// diverger, et ce dépôt l'a déjà payé quatre fois.
#[derive(Debug, Clone, PartialEq)]
pub struct Voulue {
    pub cellule: Cellule,
    /// Distance du centre de la cellule à l'œil, en CELLULES.
    ///
    /// En cellules et non en blocs, parce que c'est l'unité dans laquelle on
    /// demande la chose — et parce qu'un rayon de 8 veut dire la même chose au
    /// niveau chunk et au niveau région.
    pub distance: f32,
    /// Cosinus de l'angle entre le regard horizontal et la cellule.
    ///
    /// `1` droit devant, `0` de côté, `-1` dans le dos. Vaut `0` quand le
    /// regard n'a pas de direction horizontale exploitable — pas `NaN`.
    pub devant: f32,
    /// Le score qui a décidé du rang. Plus petit = plus urgent.
    pub score: f32,
}

/// **Les cellules à rendre résidentes, les plus urgentes d'abord.**
///
/// `regard` est pris dans le repère du monde et n'a pas besoin d'être
/// normalisé ; seules ses composantes X et Z comptent, une cellule étant une
/// COLONNE (16 × 16 sur toute la hauteur au niveau chunk). Un regard vertical
/// ne privilégie donc aucune direction, ce qui est exact.
///
/// `y` borne la hauteur des boîtes rendues, comme [`cellules_autour`] : elle
/// n'est pas alignée, parce qu'un utilisateur qui demande « mon chunk » ne
/// demande pas la tranche de seize blocs où il se trouve.
///
/// Le rayon est plafonné à [`RAYON_MAX`], donc la sortie est bornée par
/// construction : il n'existe aucune entrée qui puisse la faire exploser.
/// L'ordre est TOTAL et déterministe — à score égal, `(z, x)` départage — ce
/// qui est indispensable : un ordre qui dépendrait du hasard d'une table
/// donnerait deux chargements différents de la même scène, et un test
/// impossible à écrire.
pub fn voulues(
    oeil: BlockPos,
    regard: [f32; 3],
    rayon: u32,
    niveau: Niveau,
    y: (i32, i32),
) -> Vec<Voulue> {
    let rayon = rayon.min(RAYON_MAX);
    let cote = niveau.cote() as f32;

    // Le regard, réduit à l'horizontale et normalisé — ou rien du tout.
    let (rx, rz) = (regard[0], regard[2]);
    let n2 = rx * rx + rz * rz;
    let cap = if n2.is_finite() && n2 > REGARD_MINI * REGARD_MINI {
        let n = n2.sqrt();
        Some((rx / n, rz / n))
    } else {
        None
    };

    // Le centre de l'œil dans le repère des cellules : c'est de là qu'on
    // mesure, et c'est ce qui rend la cellule de l'œil toujours première.
    let ox = oeil.x as f32 / cote;
    let oz = oeil.z as f32 / cote;

    let mut out: Vec<Voulue> = cellules_autour(oeil, rayon, niveau, y)
        .into_iter()
        .filter_map(|cellule| {
            // **L'APPARTENANCE se décide en cellules ENTIÈRES, l'urgence en
            // position réelle.** Les mélanger a coûté un bug tout de suite :
            // avec un rayon de 0 et un œil près d'un coin, la cellule où l'on
            // SE TIENT était à 0,707 de son centre, donc jetée de sa propre
            // demande. Et à tout rayon, découper sur la position exacte fait
            // glisser l'ensemble demandé pendant qu'on MARCHE à l'intérieur
            // d'une cellule : on chargerait et jetterait en continu, ce qui
            // est très exactement le va-et-vient que le disque existe pour
            // empêcher. Sur la grille de cellules, l'ensemble ne change qu'en
            // FRANCHISSANT une frontière.
            let (ex, ez) = niveau.cellule_de(oeil);
            let (ix, iz) = ((cellule.x - ex) as i64, (cellule.z - ez) as i64);
            if ix * ix + iz * iz > (rayon as i64) * (rayon as i64) {
                return None;
            }
            // Le centre de la cellule, pas son coin : sur une cellule de 512
            // blocs, viser le coin déplace la mesure d'un tiers de cellule et
            // le classement s'en ressent jusqu'à deux rangs.
            let dx = cellule.x as f32 + 0.5 - ox;
            let dz = cellule.z as f32 + 0.5 - oz;
            let distance = (dx * dx + dz * dz).sqrt();
            let devant = match cap {
                // À distance nulle il n'y a pas de direction : la cellule de
                // l'œil est devant par convention, et son score vaut zéro de
                // toute façon.
                _ if distance <= f32::EPSILON => 1.0,
                Some((cx, cz)) => (dx * cx + dz * cz) / distance,
                None => 0.0,
            };
            let score = distance * (PENALITE_DOS - if cap.is_some() { devant } else { 0.0 });
            Some(Voulue {
                cellule,
                distance,
                devant: if cap.is_some() { devant } else { 0.0 },
                score,
            })
        })
        .collect();

    // `total_cmp` et non `partial_cmp().unwrap()` : un `NaN` qui aurait
    // échappé aux gardes ferait paniquer le second, en plein vol et sans rien
    // dire de sa cause. `total_cmp` ordonne tout, y compris ce qu'on n'attend
    // pas — et le départage sur `(z, x)` rend l'ordre TOTAL.
    //
    // **Tri INSTABLE, et c'est ce qui rend le départage porteur.** Avec un tri
    // stable, les ex æquo gardaient l'ordre d'émission de `cellules_autour`
    // (z puis x) — qui est justement celui du départage : la ligne ne
    // décidait donc rien, et la mutation qui la retirait survivait. Un ordre
    // qui a l'air garanti mais repose sur un détail d'un AUTRE module est
    // exactement ce qui se casse le jour où ce module change, sans que rien
    // ne le dise. Ici c'est le comparateur seul qui décide.
    out.sort_unstable_by(|a, b| {
        a.score
            .total_cmp(&b.score)
            .then(a.cellule.z.cmp(&b.cellule.z))
            .then(a.cellule.x.cmp(&b.cellule.x))
    });
    out
}

/// **Ce qu'il faut charger, et ce qu'il faut jeter**, en croisant la demande
/// avec ce qui est déjà là.
///
/// `resident` répond « cette cellule est-elle déjà résidente ? ». Rendre les
/// deux listes d'un coup plutôt que deux fonctions est délibéré : elles se
/// déduisent du MÊME parcours, et deux fonctions finiraient par ne plus être
/// d'accord sur ce qu'est « voulu » — la cellule serait alors chargée puis
/// jetée aussitôt, en boucle, sans que rien ne le signale.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// À rendre résidentes, les plus urgentes d'abord.
    pub charger: Vec<Voulue>,
    /// Résidentes et plus voulues. L'appelant décide s'il les jette
    /// maintenant ou les laisse au LRU — la fenêtre est plafonnée en octets,
    /// pas en cellules, et garder ce qui tient encore évite de recharger ce
    /// qu'un demi-tour redemande aussitôt.
    pub jetables: Vec<Cellule>,
}

/// L'identité d'une cellule : son niveau et sa place. La boîte et la région
/// s'en DÉDUISENT, donc les comparer serait payer trois fois le même test.
fn cle(c: &Cellule) -> (Niveau, i32, i32) {
    (c.niveau, c.x, c.z)
}

/// Croise la demande de la caméra avec ce qui est résident.
///
/// `residentes` est la liste de ce qu'on tient — l'appelant la donne parce que
/// lui seul sait ce qu'il tient, et que ce module n'a pas à connaître la forme
/// de son cache.
///
/// **Par ENSEMBLES, jamais par balayage.** Écrite en `contains` sur deux
/// tranches, la fonction était quadratique — et mesurée, pas supposée : 0,04 ms
/// à rayon 8, mais **11,4 ms à rayon 40**, c'est-à-dire tout le budget d'image
/// dépassé pour décider 81 chargements. Un rayon de 40 chunks est une distance
/// d'affichage ordinaire, pas un cas limite. C'est le piège « filtrer APRÈS
/// avoir itéré rend quadratique », déjà payé sur `count_of` et sur
/// `appliquer_region`.
///
/// Les ensembles ne servent qu'à l'APPARTENANCE : l'ordre des deux listes vient
/// des tranches d'entrée, jamais d'une table de hachage. Un ordre qui
/// dépendrait du hasard d'un `HashSet` donnerait deux chargements différents de
/// la même scène.
pub fn planifier(voulues: Vec<Voulue>, residentes: &[Cellule]) -> Plan {
    let deja: HashSet<(Niveau, i32, i32)> = residentes.iter().map(cle).collect();
    let demandees: HashSet<(Niveau, i32, i32)> = voulues.iter().map(|v| cle(&v.cellule)).collect();
    let charger = voulues
        .into_iter()
        .filter(|v| !deja.contains(&cle(&v.cellule)))
        .collect();
    let jetables = residentes
        .iter()
        .filter(|c| !demandees.contains(&cle(c)))
        .cloned()
        .collect();
    Plan { charger, jetables }
}
