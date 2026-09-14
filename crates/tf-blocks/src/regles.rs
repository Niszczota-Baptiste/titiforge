//! Les règles de transformation d'état, **dérivées du pack**.
//!
//! ## Pourquoi pas une table écrite à la main
//!
//! WorldEdit en tient une, et elle marche pour le vanilla. Sur Minefield elle
//! ne peut pas : 910 blocs portent un état, avec quatre propriétés que le
//! vanilla ne connaît pas — `vertical` (16 920 occurrences), `offset`, `model`,
//! `position`. Écrire et maintenir ça à la main, pour chaque mise à jour du
//! serveur, n'est pas une option.
//!
//! ## Comment la dérivation marche
//!
//! Le pack DÉCLARE déjà la réponse. Un `blockstates.json` dit, pour chaque
//! état, quel modèle afficher et sous quelle rotation. Donc :
//!
//! > deux états sont des rotations l'un de l'autre s'ils rendent **le même
//! > modèle**, avec la même inclinaison `x`, et des `y` qui diffèrent de 90°.
//!
//! Ça ne regarde JAMAIS le nom d'une propriété. `vertical` est traité comme
//! `facing`, et un bloc inventé demain le sera aussi.
//!
//! ## Ce qui a fallu corriger, mesuré sur le pack du serveur
//!
//! La formulation naïve — « cherche l'état dont la géométrie est celle-ci
//! tournée » — laisse **4 988 collisions** : une trappe FERMÉE a la même
//! géométrie quelle que soit son orientation, donc plusieurs états lui
//! correspondent. La géométrie seule ne peut pas trancher.
//!
//! On dérive donc une permutation par **(propriété, valeur)**, depuis les seuls
//! états dont la géométrie est unique, puis on l'applique à tous. La trappe
//! fermée tourne alors comme la trappe ouverte, ce qui est ce qu'on veut : son
//! `facing` doit pointer au bon endroit quand on l'ouvrira.
//!
//! Mesuré : **910 blocs Minefield entièrement dérivables, 27 047 états couverts
//! sur 27 047, zéro incohérence.**

use std::collections::{BTreeMap, BTreeSet};

use tf_assets::blockstates::Blockstate;
use tf_assets::{Catalogue, Id};

use crate::transfo::{Transfo, TOUTES};

/// `valeur → valeur`, pour une propriété d'un bloc.
pub type Permutation = BTreeMap<String, String>;

/// Ce qu'une transformation fait aux propriétés d'UN bloc.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReglesBloc {
    /// `propriété → permutation de ses valeurs`.
    pub par_propriete: BTreeMap<String, Permutation>,
}

impl ReglesBloc {
    pub fn est_vide(&self) -> bool {
        self.par_propriete
            .values()
            .all(|p| p.iter().all(|(a, b)| a == b))
    }

    /// Applique les règles à un état, propriété par propriété.
    ///
    /// Une propriété que les règles ne nomment pas passe INCHANGÉE. C'est le
    /// bon défaut : `waterlogged` ne tourne pas.
    pub fn appliquer(&self, props: &mut [(String, String)]) {
        for (k, v) in props.iter_mut() {
            if let Some(p) = self.par_propriete.get(k) {
                if let Some(neuf) = p.get(v.as_str()) {
                    v.clone_from(neuf);
                }
            }
        }
    }
}

/// Pourquoi un bloc n'a pas pu être dérivé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manque {
    /// Deux états non ambigus se contredisent sur l'image d'une valeur. Le pack
    /// dit deux choses ; on ne choisit pas à sa place.
    Incoherent {
        bloc: String,
        propriete: String,
        valeur: String,
        transfo: Transfo,
        images: Vec<String>,
    },
    /// La transformation mènerait à un état que le pack ne déclare pas.
    NonDeclare {
        bloc: String,
        etat: String,
        vise: String,
        transfo: Transfo,
    },
    /// Deux états différents tomberaient sur le même. Une transformation qui
    /// n'est pas une bijection DÉTRUIT de l'information, et le fait en
    /// silence : le bloc revient différent après quatre rotations.
    NonBijectif {
        bloc: String,
        etat: String,
        vise: String,
        transfo: Transfo,
    },
    /// Le pack ne déclare pas où iraient certains états. La transformation
    /// n'est pas représentable pour ce bloc — et l'absence de preuve ne veut
    /// PAS dire que rien ne change.
    NonRepresentable {
        bloc: String,
        transfo: Transfo,
        etats: usize,
    },
    /// La règle EXACTE existe, mais une propriété ne se laisse pas résumer en
    /// une permutation de ses valeurs. Ce n'est pas un échec : seul le repli
    /// pour les états non déclarés s'en trouve appauvri.
    NonDecomposable {
        bloc: String,
        propriete: String,
        transfo: Transfo,
    },
    /// La forme transformée n'est déclarée NULLE PART dans le pack : l'état
    /// rendu ne suivra pas exactement la transformation.
    ///
    /// Deux causes, une seule conséquence. Une bougie n'a aucun champ qui
    /// oriente, donc la règle est l'identité et la bougie ne tournera pas. Un
    /// bloc CHIRAL sans jumeau — une marche gauche dont le pack ne déclare pas
    /// la droite — rendra sa version tournée au lieu de sa réfléchie.
    ///
    /// Ce n'est pas un échec de dérivation, c'est une limite du pack. Mais
    /// elle doit se VOIR : c'est la seule chose qui distingue « ce bloc ne peut
    /// pas tourner » de « ce bloc tourne de travers ».
    FormeApprochee {
        bloc: String,
        transfo: Transfo,
        etats: usize,
    },
    /// La règle dérivée viole une loi du groupe. On la JETTE : une
    /// transformation qui ne se défait pas est pire qu'une transformation
    /// absente.
    LoiViolee {
        bloc: String,
        transfo: Transfo,
        loi: &'static str,
    },
}

impl Manque {
    /// Le bloc qui n'a pas pu être dérivé.
    pub fn bloc(&self) -> &str {
        match self {
            Manque::Incoherent { bloc, .. }
            | Manque::NonDeclare { bloc, .. }
            | Manque::NonBijectif { bloc, .. }
            | Manque::NonRepresentable { bloc, .. }
            | Manque::NonDecomposable { bloc, .. }
            | Manque::FormeApprochee { bloc, .. }
            | Manque::LoiViolee { bloc, .. } => bloc,
        }
    }

    /// La transformation concernée.
    pub fn transfo(&self) -> Transfo {
        match self {
            Manque::Incoherent { transfo, .. }
            | Manque::NonDeclare { transfo, .. }
            | Manque::NonBijectif { transfo, .. }
            | Manque::NonRepresentable { transfo, .. }
            | Manque::NonDecomposable { transfo, .. }
            | Manque::FormeApprochee { transfo, .. }
            | Manque::LoiViolee { transfo, .. } => *transfo,
        }
    }

    /// Un mot pour la famille du manque, pour compter et regrouper.
    pub fn genre(&self) -> &'static str {
        match self {
            Manque::Incoherent { .. } => "incohérent",
            Manque::NonDeclare { .. } => "image non déclarée",
            Manque::NonBijectif { .. } => "non bijectif",
            Manque::NonRepresentable { .. } => "non représentable",
            Manque::NonDecomposable { .. } => "non décomposable",
            Manque::FormeApprochee { .. } => "forme approchée",
            Manque::LoiViolee { .. } => "loi du groupe",
        }
    }
}

impl std::fmt::Display for Manque {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Manque::Incoherent {
                bloc,
                propriete,
                valeur,
                transfo,
                images,
            } => write!(
                f,
                "{bloc} · {} : le pack envoie {propriete}={valeur} sur {} à la fois",
                transfo.nom(),
                images.join(" et ")
            ),
            Manque::NonDeclare {
                bloc,
                etat,
                vise,
                transfo,
            } => write!(
                f,
                "{bloc}[{etat}] · {} mènerait à [{vise}], que le pack ne déclare pas",
                transfo.nom()
            ),
            Manque::NonRepresentable {
                bloc,
                transfo,
                etats,
            } => write!(
                f,
                "{bloc} · {} : {etats} états n'ont aucune image déclarée",
                transfo.nom()
            ),
            Manque::NonDecomposable {
                bloc,
                propriete,
                transfo,
            } => write!(
                f,
                "{bloc} · {} : {propriete} ne se résume pas à une permutation — \
                 la règle exacte reste juste, le repli l'ignore",
                transfo.nom()
            ),
            Manque::FormeApprochee {
                bloc,
                transfo,
                etats,
            } => write!(
                f,
                "{bloc} · {} : la forme transformée n'est pas déclarée — {etats} \
                 états rendront une apparence approchée",
                transfo.nom()
            ),
            Manque::LoiViolee { bloc, transfo, loi } => {
                write!(f, "{bloc} · {} : {loi}", transfo.nom())
            }
            Manque::NonBijectif {
                bloc,
                etat,
                vise,
                transfo,
            } => write!(
                f,
                "{bloc}[{etat}] · {} tombe sur [{vise}], déjà atteint : la \
                 transformation détruirait de l'information",
                transfo.nom()
            ),
        }
    }
}

/// Les règles de tout un catalogue.
///
/// **Une transformation à la fois.** Un bloc peut avoir des rotations
/// parfaitement dérivables et un miroir qui ne l'est pas : jeter le bloc
/// entier pour ça perdrait 775 rotations justes sur 910 — mesuré, c'est
/// exactement ce que faisait la première version.
#[derive(Debug, Default)]
pub struct Table {
    blocs: BTreeMap<String, Bloc>,
    /// Ce qu'on n'a pas su dériver. **Vide n'est pas garanti**, et c'est
    /// pourquoi c'est public : un bloc qu'on ne sait pas tourner doit se VOIR
    /// plutôt que tourner de travers.
    pub manques: Vec<Manque>,
}

/// Ce qu'on sait d'UN bloc.
///
/// Deux représentations de la même vérité, et la seconde n'est pas une
/// redondance :
///
///  · `exacts` est la permutation d'états, **celle qui décide**. Une
///    indexation de tableau à l'exécution, et elle sait exprimer ce qu'aucune
///    permutation par propriété ne peut — mesuré sur les escaliers du serveur ;
///  · `regles` est la forme compacte, le repli pour un état que le pack ne
///    déclare PAS. Un monde plus récent que le pack en contient, et le laisser
///    intact serait tourner un build à moitié.
#[derive(Debug)]
struct Bloc {
    /// Les états déclarés, clé triée, dans l'ordre de `exacts`.
    etats: Vec<String>,
    index: BTreeMap<String, u32>,
    exacts: [Option<Vec<u32>>; 5],
    regles: [Option<ReglesBloc>; 5],
}

/// Un état déclaré : sa clé triée, ses propriétés, sa géométrie.
type Etat = (String, Vec<(String, String)>, Geo);

/// La géométrie que rend un état : modèle, inclinaison, rotation.
type Geo = (Id, u16, u16);

fn geo_de(v: &tf_assets::Variante) -> Geo {
    (v.modele.clone(), v.x % 360, v.y % 360)
}

fn est_air(g: &Geo) -> bool {
    g.0.chemin.ends_with("/air") || g.0.chemin.is_empty()
}

/// La géométrie visée par une transformation.
///
/// Une rotation ajoute son angle. Un MIROIR conjugue : réfléchir une rotation
/// `θ` donne `−θ`, plus le décalage du plan choisi. C'est la seule
/// transformation qu'on ne peut pas lire directement dans le pack, parce qu'un
/// modèle réfléchi n'y est pas déclaré — mais elle est exacte dès que le modèle
/// est symétrique par rapport à ce plan, ce qui est le cas de tout ce que
/// Minecraft appelle un bloc orientable.
/// La géométrie visée, en canonisant `y` modulo la **symétrie du modèle**.
///
/// Une bûche ne déclare que `y = 0` et `y = 90` : tourner l'état `y = 90` vise
/// `y = 180`, que le pack ne déclare pas — non parce que c'est impossible, mais
/// parce qu'une bûche est SYMÉTRIQUE à 180° et que le pack ne réécrit pas ce
/// qu'il a déjà. Sans cette réduction, la permutation dérivée n'envoyait rien
/// sur `axis=x`, et quatre rotations de 90° rendaient `axis=z → axis=x` :
/// mesuré, 638 violations de la loi de groupe.
fn geo_visee_avec(g: &Geo, t: Transfo, periode: u16) -> Geo {
    let y = match t {
        Transfo::Rot90 | Transfo::Rot180 | Transfo::Rot270 => {
            (g.2 as i32 + t.degres()) as u16 % 360
        }
        // `x → −x` : le plan contient l'axe nord-sud, donc laisse `y = 0` et
        // `y = 180` en place et échange 90 et 270.
        Transfo::MiroirX => ((360 - g.2 as i32) % 360) as u16,
        // `z → −z` : le plan contient l'axe est-ouest, donc échange 0 et 180.
        Transfo::MiroirZ => ((540 - g.2 as i32) % 360) as u16,
    };
    (g.0.clone(), g.1, y % periode.max(1))
}

/// `a` puis `b`, en une seule règle.
fn composer(a: &ReglesBloc, b: &ReglesBloc) -> ReglesBloc {
    let mut out = ReglesBloc::default();
    for (k, pa) in &a.par_propriete {
        let mut perm = Permutation::new();
        for (de, vers) in pa {
            let apres = b
                .par_propriete
                .get(k)
                .and_then(|pb| pb.get(vers))
                .unwrap_or(vers);
            perm.insert(de.clone(), apres.clone());
        }
        out.par_propriete.insert(k.clone(), perm);
    }
    // Ce que seul `b` nomme compte aussi.
    for (k, pb) in &b.par_propriete {
        out.par_propriete
            .entry(k.clone())
            .or_insert_with(|| pb.clone());
    }
    out
}

/// La période de symétrie d'un modèle, déduite des angles qu'il DÉCLARE.
///
/// Les angles d'un modèle pavent le cercle : `période = écart × nombre`. Une
/// bûche déclare `{0, 90}` — écart 90, deux angles, période **180**, parce
/// qu'une bûche est la même vue de face et de dos. Un bloc à quatre
/// orientations déclare `{0, 90, 180, 270}` — écart 90, quatre angles, période
/// **360**, et il n'a aucune symétrie.
///
/// « L'ensemble est invariant par +90° » serait le mauvais critère, et je l'ai
/// écrit : `{0, 90, 180, 270}` l'est, ce qui donnait une période de 90 et
/// faisait s'effondrer les quatre orientations sur une seule. Mesuré, 219 blocs
/// Minefield perdaient leur rotation de 90°.
fn periode_de(ys: &BTreeSet<u16>) -> u16 {
    let v: Vec<u16> = ys.iter().copied().collect();
    if v.len() < 2 {
        return 360;
    }
    let ecart = v[1] - v[0];
    if ecart == 0 || v.windows(2).any(|w| w[1] - w[0] != ecart) {
        // Des angles irréguliers ne pavent rien : on ne suppose aucune
        // symétrie plutôt que d'en inventer une.
        return 360;
    }
    let p = ecart as u32 * v.len() as u32;
    if p == 90 || p == 180 || p == 360 {
        p as u16
    } else {
        360
    }
}

/// Ce que les couples (état → cible) disent de chaque propriété.
///
/// `propriété → valeur de départ → ensemble des valeurs d'arrivée`. Un
/// ensemble à plus d'un élément est une CONTRADICTION du pack, pas une
/// moyenne à faire.
type Images = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

fn relever(etats: &[Etat], cible: &[Option<usize>]) -> Images {
    let mut images: Images = BTreeMap::new();
    for (i, vise) in cible.iter().enumerate() {
        let Some(j) = *vise else { continue };
        for (k, v) in &etats[i].1 {
            if let Some((_, w)) = etats[j].1.iter().find(|(ck, _)| ck == k) {
                images
                    .entry(k.clone())
                    .or_default()
                    .entry(v.clone())
                    .or_default()
                    .insert(w.clone());
            }
        }
    }
    images
}

/// Ce dont on est SÛR : les seules valeurs qui n'ont qu'une image.
///
/// Les contradictions sont laissées de côté plutôt que tranchées — elles
/// ressortiront au relevé final, qui est celui qui décide.
fn permutation_sure(images: &Images) -> BTreeMap<String, Permutation> {
    images
        .iter()
        .map(|(k, par_valeur)| {
            let perm: Permutation = par_valeur
                .iter()
                .filter(|(_, vers)| vers.len() == 1)
                .map(|(de, vers)| (de.clone(), vers.iter().next().unwrap().clone()))
                .collect();
            (k.clone(), perm)
        })
        .filter(|(_, perm)| !perm.is_empty())
        .collect()
}

/// Sur combien de propriétés deux états diffèrent-ils ?
fn ecart(a: &[(String, String)], b: &[(String, String)]) -> usize {
    a.iter()
        .filter(|(k, v)| b.iter().any(|(ck, cv)| ck == k && cv != v))
        .count()
}

/// `cible` peut-il être l'image de `source` au vu de ce qu'on sait déjà ?
///
/// Une propriété dont on ne connaît pas l'image ne dit rien : elle ne filtre
/// pas. C'est ce qui permet à l'axe d'une bûche verticale de rester libre
/// pendant que le `facing` d'un escalier tranche.
fn compatible(
    acquis: &BTreeMap<String, Permutation>,
    source: &[(String, String)],
    cible: &[(String, String)],
) -> bool {
    source.iter().all(|(k, v)| {
        let Some(attendu) = acquis.get(k).and_then(|p| p.get(v)) else {
            return true;
        };
        cible.iter().any(|(ck, cv)| ck == k && cv == attendu)
    })
}

fn cle_de(props: &[(String, String)]) -> String {
    props
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn props_de(cle: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = cle
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    v.sort();
    v
}

impl Table {
    /// Dérive les règles de tout un catalogue.
    pub fn deriver(cat: &Catalogue) -> Table {
        let mut table = Table::default();
        for (nom, bs) in cat.blocs() {
            let Blockstate::Variants(variants) = bs else {
                // Les `multipart` (293 blocs Minefield) demandent un autre
                // raisonnement : leurs propriétés désignent des CÔTÉS, pas une
                // orientation. Les traiter comme des variants dériverait des
                // règles fausses — on préfère ne rien dire.
                continue;
            };
            if variants.len() <= 1 {
                continue;
            }
            let etats: Vec<Etat> = variants
                .iter()
                .filter_map(|(cle, vars)| {
                    vars.first()
                        .map(|v| (cle.clone(), props_de(cle), geo_de(v)))
                })
                .collect();
            if etats.iter().all(|(_, p, _)| p.is_empty()) {
                continue;
            }

            // Géométrie → les états qui la produisent. Seuls les états dont la
            // géométrie est UNIQUE servent à dériver : ceux qui la partagent
            // (une trappe fermée) ne disent rien sur l'orientation.
            // La période de symétrie se relève par (modèle, inclinaison) : deux
            // modèles différents du même bloc n'ont pas la même symétrie.
            let mut ys_par_modele: BTreeMap<(Id, u16), BTreeSet<u16>> = BTreeMap::new();
            for (_, _, g) in etats.iter() {
                if !est_air(g) {
                    ys_par_modele
                        .entry((g.0.clone(), g.1))
                        .or_default()
                        .insert(g.2);
                }
            }
            let periodes: BTreeMap<(Id, u16), u16> = ys_par_modele
                .iter()
                .map(|(k, ys)| (k.clone(), periode_de(ys)))
                .collect();
            let canon = |g: &Geo| -> Geo {
                let p = periodes
                    .get(&(g.0.clone(), g.1))
                    .copied()
                    .unwrap_or(360)
                    .max(1);
                (g.0.clone(), g.1, g.2 % p)
            };

            let mut par_geo: BTreeMap<Geo, Vec<usize>> = BTreeMap::new();
            for (i, (_, _, g)) in etats.iter().enumerate() {
                if !est_air(g) {
                    par_geo.entry(canon(g)).or_default().push(i);
                }
            }

            // Les empreintes NON réfléchies, calculées une fois par état. Les
            // recalculer pour chaque source rendait la dérivation quadratique
            // en lectures de modèle : 1 988 ms au lieu de 319 sur le pack du
            // serveur, pour le même résultat.
            let droites: Vec<Option<Vec<crate::geometrie::Boite>>> = etats
                .iter()
                .map(|(_, _, g)| {
                    if est_air(g) {
                        None
                    } else {
                        crate::geometrie::empreinte(cat, &g.0, g.1, g.2, None)
                    }
                })
                .collect();

            // La permutation exacte d'un côté, la forme compacte de l'autre.
            //
            // Les manques s'accumulent ICI et pas dans la table : une
            // transformation refusée par sa propre dérivation peut être reprise
            // par COMPOSITION juste après (`mirroir z = miroir x ∘ demi-tour`),
            // et annoncer un trou qui n'existe plus serait aussi trompeur que
            // de taire un vrai.
            let mut manques: Vec<Manque> = Vec::new();
            let mut exacts: [Option<Vec<u32>>; 5] = Default::default();
            let mut regles: [Option<ReglesBloc>; 5] = Default::default();
            for t in TOUTES {
                // Les empreintes de chaque état TRANSFORMÉ, une fois par état.
                let images_geo: Vec<Option<Vec<crate::geometrie::Boite>>> = etats
                    .iter()
                    .map(|(_, _, g)| {
                        if est_air(g) {
                            None
                        } else {
                            crate::geometrie::empreinte(cat, &g.0, g.1, g.2, Some(t))
                        }
                    })
                    .collect();

                // Tous les états dont la géométrie est celle de `i` transformée.
                // On garde l'ENSEMBLE : une réponse unique se lit tout de suite, et
                // une réponse multiple n'est pas un échec — c'est une question que
                // la passe 2 saura poser autrement.
                let candidats: Vec<Vec<usize>> = images_geo
                    .iter()
                    .map(|im| match im {
                        None => Vec::new(),
                        Some(e) => droites
                            .iter()
                            .enumerate()
                            .filter(|(_, d)| d.as_ref() == Some(e))
                            .map(|(j, _)| j)
                            .collect(),
                    })
                    .collect();

                // Ce bloc a-t-il seulement quelque chose à faire tourner ?
                //
                // Deux preuves possibles, et il en faut UNE :
                //  · le pack déclare un même modèle à plusieurs angles — alors `y`
                //    porte du sens, même si la forme est un cube (une bûche) ;
                //  · la forme d'un état, transformée, est celle d'un AUTRE état —
                //    alors la transformation déplace vraiment quelque chose.
                //
                // Sans aucune des deux, aucun champ de l'état ne peut encoder une
                // orientation : une bougie a `candles` et `lit`, rien d'autre. La
                // règle est alors l'IDENTITÉ, et ce n'est pas un aveu d'ignorance —
                // c'est la seule fonction totale sur cet espace d'états, et c'est
                // aussi ce que fait le jeu. Refuser ici rendrait `//rotate`
                // inutilisable sur tout un mur décoré.
                let angles_parlent = ys_par_modele.values().any(|ys| ys.len() > 1);
                let geometrie_parle = (0..etats.len()).any(|i| {
                    images_geo[i].is_some()
                        && images_geo[i] != droites[i]
                        && candidats[i].iter().any(|&j| j != i)
                });
                if !angles_parlent && !geometrie_parle {
                    // L'identité est la seule fonction totale ici. Si une
                    // forme AURAIT bougé, le contrôle de forme commun, plus
                    // bas, le dira — il n'y a pas deux endroits qui comptent
                    // la même chose.
                    let approchees = (0..etats.len())
                        .filter(|&i| images_geo[i].is_some() && images_geo[i] != droites[i])
                        .count();
                    if approchees > 0 {
                        manques.push(Manque::FormeApprochee {
                            bloc: nom.clone(),
                            transfo: t,
                            etats: approchees,
                        });
                    }
                    exacts[t.indice()] = Some((0..etats.len() as u32).collect());
                    regles[t.indice()] = Some(ReglesBloc::default());
                    continue;
                }

                let angles_de = |i: usize| -> usize {
                    let g = &etats[i].2;
                    ys_par_modele
                        .get(&(g.0.clone(), g.1))
                        .map(|s| s.len())
                        .unwrap_or(1)
                };

                // Les états qui rendent le MÊME modèle à l'angle visé.
                //
                // **Sans exiger que la source soit seule sur sa géométrie.** C'était
                // la garde qui coûtait le plus cher : sur le pack du serveur, une
                // trappe OUVERTE partage son (modèle, angle) entre `half=bottom` et
                // `half=top`, un escalier entre trois états. La garde coupait donc
                // la route des angles — la seule exacte — sur tous les escaliers,
                // toutes les trappes, toutes les portes et tous les portillons :
                // 35 blocs Minefield, qui retombaient sur une règle IDENTITÉ
                // silencieuse. Partager un angle ne rend pas l'angle faux ; ça veut
                // seulement dire que plusieurs états ont cette orientation, et
                // c'est au départage de choisir lequel.
                let cand_ang: Vec<Vec<usize>> = etats
                    .iter()
                    .map(|(_, _, g)| {
                        if est_air(g) {
                            return Vec::new();
                        }
                        let p = periodes
                            .get(&(g.0.clone(), g.1))
                            .copied()
                            .unwrap_or(360)
                            .max(1);
                        par_geo
                            .get(&geo_visee_avec(g, t, p))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect();

                // **La géométrie ARBITRE, les angles départagent.**
                //
                // L'arithmétique d'angles suppose que la transformation du
                // monde se ramène à un décalage de `y`. C'est vrai pour une
                // rotation ; pour un MIROIR, ça ne l'est que si le modèle est
                // lui-même symétrique. Sur un escalier en coin, elle désigne
                // l'escalier TOURNÉ au lieu du réfléchi — et comme la géométrie
                // d'un coin est ambiguë (deux écritures pour le même dessin),
                // le repli sur les angles avait le dernier mot. Mesuré par le
                // contrôle indépendant : 3 431 couples sur 132 380 rendaient
                // une forme fausse, soit tous les escaliers en coin et toutes
                // les portes ouvertes du pack.
                //
                // On ne garde donc des angles que ce qui a la BONNE forme.
                // Quand aucune géométrie n'est calculable, les angles restent
                // seuls juges — c'est leur domaine, pas un repli.
                let cand_ang: Vec<Vec<usize>> = cand_ang
                    .into_iter()
                    .enumerate()
                    .map(|(i, ang)| {
                        if images_geo[i].is_none() || candidats[i].is_empty() {
                            // Aucune géométrie calculable, ou la forme
                            // transformée n'est DÉCLARÉE nulle part : la
                            // géométrie ne départage pas, elle se tait. Les
                            // angles restent seuls juges — et le contrôle de
                            // forme, plus bas, dira que le résultat est
                            // approché.
                            ang
                        } else {
                            ang.into_iter()
                                .filter(|j| candidats[i].contains(j))
                                .collect()
                        }
                    })
                    .collect();

                // Les angles DÉCLARÉS doivent être CLOS par la transformation.
                //
                // C'est le seul contrôle qui sait dire « cette rotation n'existe
                // pas » — et il le sait sans rien deviner : si le pack se sert de
                // `y` pour orienter un modèle, alors l'angle visé doit être un des
                // angles qu'il déclare. `minecraft:snow` n'a de formes qu'au nord et
                // au sud ; sa rotation de 90° mènerait à un `y` que rien ne
                // déclare, et « aucune preuve » ne veut PAS dire « rien ne change ».
                // Sans ce contrôle, la règle sortait identité et tournait un mur
                // sans le tourner.
                //
                // Il ne regarde QUE les modèles déclarés à plusieurs angles : là où
                // `y` ne porte rien, il n'y a rien à fermer.
                let sans_image = (0..etats.len())
                    .filter(|&i| {
                        let g = &etats[i].2;
                        if est_air(g) || angles_de(i) < 2 {
                            return false;
                        }
                        let p = periodes
                            .get(&(g.0.clone(), g.1))
                            .copied()
                            .unwrap_or(360)
                            .max(1);
                        let vise = geo_visee_avec(g, t, p).2;
                        !ys_par_modele
                            .get(&(g.0.clone(), g.1))
                            .is_some_and(|ys| ys.iter().any(|&d| d % p == vise))
                    })
                    .count();
                if sans_image > 0 {
                    manques.push(Manque::NonRepresentable {
                        bloc: nom.clone(),
                        transfo: t,
                        etats: sans_image,
                    });
                    continue;
                }

                // Le candidat le plus PROCHE, s'il est seul à l'être.
                //
                // Départage : **ce que la géométrie ne voit pas reste en place.**
                // Une porte a `powered`, qui ne change aucun cuboïde : chaque forme
                // est donc déclarée deux fois, et toute recherche rend deux
                // candidats jumeaux. Celui qui diffère de la source sur le moins de
                // propriétés est celui qui ne bouge que ce que la transformation
                // bouge.
                let choisir = |ens: &[usize], i: usize| -> Option<usize> {
                    let mut meilleur: Option<(usize, usize)> = None;
                    let mut seul = true;
                    for &j in ens {
                        let d = ecart(&etats[i].1, &etats[j].1);
                        match meilleur {
                            Some((md, _)) if d > md => {}
                            Some((md, _)) if d == md => seul = false,
                            _ => {
                                meilleur = Some((d, j));
                                seul = true;
                            }
                        }
                    }
                    meilleur.filter(|_| seul).map(|(_, j)| j)
                };

                // Pour un MIROIR, la géométrie passe d'abord : la route des angles
                // trouve bien un état, mais le mauvais — l'escalier TOURNÉ, pas le
                // réfléchi. Pour une rotation, l'inverse : les angles sont exacts.
                let ordre = |i: usize| -> [&Vec<usize>; 2] {
                    if t.est_miroir() {
                        [&candidats[i], &cand_ang[i]]
                    } else {
                        [&cand_ang[i], &candidats[i]]
                    }
                };

                // ── Trois passes, de la preuve la plus forte à la plus faible.
                //
                // L'ordre n'est pas un détail de mise en œuvre : **tous les
                // candidats géométriques rendent le MÊME solide**, par construction.
                // Départager au plus proche avant d'avoir la moindre certitude,
                // c'est donc trancher au hasard — mesuré, `facing=east` d'un
                // escalier en coin partait sur `south` pendant que l'escalier DROIT,
                // lui, disait `west` : 371 incohérences, toute la famille des
                // escaliers du pack.
                let mut cible: Vec<Option<usize>> = vec![None; etats.len()];

                // Passe 1 — un seul candidat, donc aucune question.
                let seul = |ens: &Vec<usize>| -> Option<usize> {
                    match ens.as_slice() {
                        [j] => Some(*j),
                        _ => None,
                    }
                };
                for i in 0..etats.len() {
                    if est_air(&etats[i].2) {
                        continue;
                    }
                    let [premier, second] = ordre(i);
                    cible[i] = seul(premier).or_else(|| seul(second));
                }

                if std::env::var("TF_TRACE").is_ok_and(|v| v == *nom) {
                    eprintln!("── {nom} · {} · passe 1", t.nom());
                    for i in 0..etats.len() {
                        if let Some(j) = cible[i] {
                            eprintln!("  {:58} → {}", etats[i].0, etats[j].0);
                        }
                    }
                }
                // Passe 2 — ce que l'acquis permet de trancher, jusqu'au POINT FIXE.
                //
                // Un escalier en coin a deux encodages du même dessin
                // (`facing=east,shape=inner_right` et `facing=south,shape=inner_left`
                // sont le même bloc) : sa géométrie réfléchie en désigne donc
                // plusieurs, et aucune mesure ne les départage. Mais la passe 1 a lu
                // `facing: east → west` sur les escaliers DROITS, qui eux n'ont
                // qu'un candidat. Filtrer par ce qu'on sait n'en laisse qu'un — et
                // c'est lui qui apprend `shape: inner_right → inner_left`.
                //
                // Le point fixe est nécessaire et pas un luxe : une trappe FERMÉE se
                // résout par le `facing` des trappes ouvertes, qui n'est lui-même
                // acquis qu'au tour d'avant.
                loop {
                    let acquis = permutation_sure(&relever(&etats, &cible));
                    let mut bouge = false;
                    for i in 0..etats.len() {
                        if cible[i].is_some() || est_air(&etats[i].2) {
                            continue;
                        }
                        let filtrer = |ens: &Vec<usize>| -> Vec<usize> {
                            ens.iter()
                                .copied()
                                .filter(|&j| compatible(&acquis, &etats[i].1, &etats[j].1))
                                .collect()
                        };
                        let [premier, second] = ordre(i);
                        let choix = choisir(&filtrer(premier), i)
                            .or_else(|| choisir(&filtrer(second), i))
                            .filter(|&j| j != i);
                        if let Some(j) = choix {
                            cible[i] = Some(j);
                            bouge = true;
                        }
                    }
                    if !bouge {
                        break;
                    }
                }

                // Passe 3 — l'IMMOBILITÉ, et seulement en dernier.
                //
                // Un état ne peut conclure qu'il ne bouge pas que sur une preuve,
                // jamais faute de mieux — c'est toute la différence entre l'échelle
                // face au nord, qu'un miroir est-ouest laisse vraiment en place, et
                // le bloc à deux faces dont la rotation de 90° n'existe pas. Et ça
                // ne se juge qu'APRÈS les déplacements : une trappe fermée est
                // géométriquement immobile sous toute rotation, et conclure avant
                // contredisait le `facing` que la trappe ouverte venait de donner.
                //
                //  · l'arithmétique d'angles du pack retombe sur lui ;
                //  · sa forme est invariante ET son modèle n'est déclaré qu'à UN
                //    angle — la bûche verticale, un cube sans autre état qui lui
                //    ressemble.
                let acquis = permutation_sure(&relever(&etats, &cible));
                for i in 0..etats.len() {
                    if cible[i].is_some() || est_air(&etats[i].2) {
                        continue;
                    }
                    let immobile = (cand_ang[i] == [i]
                        || (angles_de(i) == 1
                            && images_geo[i].is_some()
                            && images_geo[i] == droites[i]))
                        && compatible(&acquis, &etats[i].1, &etats[i].1);
                    if immobile {
                        cible[i] = Some(i);
                    }
                }

                // ── De la preuve à la RÈGLE.
                //
                // Deux formes, et ce n'est pas une redondance :
                //
                //  · la permutation EXACTE, état par état. C'est elle qui décide, et
                //    elle coûte une indexation de tableau à l'exécution ;
                //  · la forme COMPACTE, une permutation par propriété. C'est le
                //    repli pour un état que le pack ne déclare pas — un monde plus
                //    récent que le pack, un bloc d'un mod absent.
                //
                // La compacte ne peut pas toujours tout porter, et c'est la mesure
                // qui l'a dit : sur les escaliers du serveur, `shape=outer` (un
                // ajout Minefield, une seule écriture par coin) exige
                // `facing: east → south` sous miroir est-ouest, pendant que
                // l'escalier DROIT exige `east → west`. Les deux sont
                // géométriquement JUSTES ; aucune permutation de `facing` ne fait
                // les deux. La propriété contradictoire est retirée de la forme
                // compacte — et signalée — sans rien enlever à l'exacte.
                let mut r = ReglesBloc::default();
                for (k, par_valeur) in relever(&etats, &cible) {
                    let sures: Permutation = par_valeur
                        .iter()
                        .filter(|(_, vers)| vers.len() == 1)
                        .map(|(de, vers)| (de.clone(), vers.iter().next().unwrap().clone()))
                        .collect();
                    if sures.len() < par_valeur.len() {
                        manques.push(Manque::NonDecomposable {
                            bloc: nom.clone(),
                            propriete: k.clone(),
                            transfo: t,
                        });
                    }
                    // Une permutation NON injective détruirait de l'information à
                    // chaque état non déclaré qu'elle touche : on préfère ne rien
                    // dire de cette propriété.
                    let injective = sures.values().collect::<BTreeSet<_>>().len() == sures.len();
                    if injective && !sures.is_empty() {
                        r.par_propriete.insert(k, sures);
                    }
                }

                // ── L'assignation, classe par classe.
                //
                // Tous les candidats d'un état rendent le MÊME solide : ils sont sa
                // classe d'arrivée, et le pack ne dit rien de plus. **N'importe
                // quelle bijection entre les deux classes est donc visuellement
                // juste** — ce qui se choisit ici, c'est laquelle, pas si.
                //
                // On honore d'abord les preuves, puis on complète sur ce qui reste
                // LIBRE. L'injectivité est ainsi vraie par construction, et ce
                // n'est pas un détail : sans elle, deux escaliers en coin tombaient
                // sur le même état et la moitié du mur disparaissait à
                // l'annulation.
                let mut pris = vec![false; etats.len()];
                let mut exact: Vec<Option<usize>> = vec![None; etats.len()];
                let poser =
                    |i: usize, j: usize, exact: &mut Vec<Option<usize>>, pris: &mut Vec<bool>| {
                        exact[i] = Some(j);
                        pris[j] = true;
                        // Un miroir est sa propre réciproque : poser i → j pose j → i,
                        // et l'involution devient vraie par construction au lieu d'être
                        // espérée puis vérifiée.
                        if t.est_miroir() && exact[j].is_none() {
                            exact[j] = Some(i);
                            pris[i] = true;
                        }
                    };
                for i in 0..etats.len() {
                    if let (None, Some(j)) = (exact[i], cible[i]) {
                        if !pris[j] {
                            poser(i, j, &mut exact, &mut pris);
                        }
                    }
                }
                for i in 0..etats.len() {
                    if exact[i].is_some() || est_air(&etats[i].2) {
                        continue;
                    }
                    let libres: Vec<usize> =
                        candidats[i].iter().copied().filter(|&j| !pris[j]).collect();
                    let compatibles: Vec<usize> = libres
                        .iter()
                        .copied()
                        .filter(|&j| compatible(&acquis, &etats[i].1, &etats[j].1))
                        .collect();
                    let choix = [&compatibles, &libres]
                        .into_iter()
                        .find(|v| !v.is_empty())
                        .and_then(|v| {
                            v.iter()
                                .copied()
                                .min_by_key(|&j| (ecart(&etats[i].1, &etats[j].1), j))
                        });
                    if let Some(j) = choix {
                        poser(i, j, &mut exact, &mut pris);
                    }
                }
                // Un état d'AIR n'a pas de forme : il ne bouge pas.
                for i in 0..etats.len() {
                    if exact[i].is_none() && est_air(&etats[i].2) && !pris[i] {
                        exact[i] = Some(i);
                        pris[i] = true;
                    }
                }

                // Chaque état doit avoir une image. Sans elle, la transformation
                // n'est pas représentable pour ce bloc — et « aucune preuve » ne
                // veut pas dire « rien ne change ».
                let orphelins = exact.iter().filter(|c| c.is_none()).count();
                let Some(exact) = exact.into_iter().collect::<Option<Vec<usize>>>() else {
                    manques.push(Manque::NonRepresentable {
                        bloc: nom.clone(),
                        transfo: t,
                        etats: orphelins,
                    });
                    continue;
                };

                // ── Le contrôle de FORME, sur le résultat.
                //
                // Les lois du groupe disent qu'une règle est cohérente avec
                // elle-même ; elles ne disent pas qu'elle est juste. Une règle qui
                // tournerait tout d'un quart de trop les passerait toutes. Ici on
                // compare le solide de l'état d'arrivée à celui de la source
                // TRANSFORMÉE — la seule question qui compte pour l'utilisateur.
                //
                // Un écart n'est pas forcément une faute : il l'est quand le pack
                // déclarait la bonne forme et qu'on ne l'a pas prise, il ne l'est
                // pas quand le pack ne la déclare nulle part. Le premier cas est un
                // bug et se voit ici en développement ; le second est une limite du
                // pack, et se dit à l'appelant.
                let approchees = (0..etats.len())
                    .filter(|&i| images_geo[i].is_some() && droites[exact[i]] != images_geo[i])
                    .count();
                if approchees > 0 {
                    debug_assert!(
                        (0..etats.len()).all(|i| {
                            images_geo[i].is_none()
                                || droites[exact[i]] == images_geo[i]
                                || candidats[i].is_empty()
                        }),
                        "{nom} · {} : la bonne forme était déclarée et on ne l'a pas prise",
                        t.nom()
                    );
                    manques.push(Manque::FormeApprochee {
                        bloc: nom.clone(),
                        transfo: t,
                        etats: approchees,
                    });
                }

                exacts[t.indice()] = Some(exact.iter().map(|&j| j as u32).collect());
                regles[t.indice()] = Some(r);
            }

            // Les lois du groupe ne se VÉRIFIENT pas après coup : elles se
            // construisent. `rot180` et `rot270` sont `rot90` composée, donc
            // `rot90 ∘ rot90 = rot180` est vrai par construction.
            //
            // Trouvées séparément, elles pouvaient se contredire : mesuré,
            // `minecraft:snow` n'a de formes qu'en nord et sud, la rotation de
            // 90° n'y est pas représentable, et la dérivation concluait
            // « identité » faute de preuve — pendant que la rotation de 180°
            // échangeait bien nord et sud. Seize états du pack violaient la
            // loi.
            if let Some(p90) = exacts[Transfo::Rot90.indice()].clone() {
                let quatre = |i: usize| -> usize {
                    let mut j = i;
                    for _ in 0..4 {
                        j = p90[j] as usize;
                    }
                    j
                };
                if (0..p90.len()).all(|i| quatre(i) == i) {
                    let p180: Vec<u32> = p90.iter().map(|&i| p90[i as usize]).collect();
                    let p270: Vec<u32> = p180.iter().map(|&i| p90[i as usize]).collect();
                    exacts[Transfo::Rot180.indice()] = Some(p180);
                    exacts[Transfo::Rot270.indice()] = Some(p270);
                    if let Some(r90) = regles[Transfo::Rot90.indice()].clone() {
                        let r180 = composer(&r90, &r90);
                        regles[Transfo::Rot270.indice()] = Some(composer(&r180, &r90));
                        regles[Transfo::Rot180.indice()] = Some(r180);
                    }
                } else {
                    manques.push(Manque::LoiViolee {
                        bloc: nom.clone(),
                        transfo: Transfo::Rot90,
                        loi: "quatre rotations de 90° doivent rendre l'état de départ",
                    });
                    exacts[Transfo::Rot90.indice()] = None;
                    exacts[Transfo::Rot270.indice()] = None;
                }
            }
            // Les deux miroirs ne sont pas indépendants non plus :
            // `z → −z` est `x → −x` suivi d'un demi-tour. Les dériver
            // séparément les faisait diverger là où le pack est asymétrique —
            // mesuré, 97,0 % de miroirs est-ouest contre 84,5 % de nord-sud sur
            // exactement les mêmes blocs. On en dérive UN et on compose
            // l'autre : la relation devient vraie par construction, et le
            // meilleur des deux tire le second.
            let (ix, iz, i180) = (
                Transfo::MiroirX.indice(),
                Transfo::MiroirZ.indice(),
                Transfo::Rot180.indice(),
            );
            if let Some(demi) = exacts[i180].clone() {
                for (source, cible_) in [(ix, iz), (iz, ix)] {
                    if exacts[source].is_some() {
                        let m = exacts[source].clone().unwrap();
                        exacts[cible_] = Some(m.iter().map(|&i| demi[i as usize]).collect());
                        regles[cible_] = regles[source]
                            .as_ref()
                            .zip(regles[i180].as_ref())
                            .map(|(m, d)| composer(m, d));
                        break;
                    }
                }
            }

            for t in [Transfo::MiroirX, Transfo::MiroirZ, Transfo::Rot180] {
                let tient = exacts[t.indice()]
                    .as_ref()
                    .is_none_or(|p| (0..p.len()).all(|i| p[p[i] as usize] as usize == i));
                if !tient {
                    manques.push(Manque::LoiViolee {
                        bloc: nom.clone(),
                        transfo: t,
                        loi: "appliquée deux fois, elle doit rendre l'état de départ",
                    });
                    exacts[t.indice()] = None;
                }
            }

            table.manques.extend(manques.into_iter().filter(|m| {
                // `NonDecomposable` et `FormeApprochee` restent vrais même quand
                // l'exacte existe : c'est justement ce qu'ils disent.
                matches!(
                    m,
                    Manque::NonDecomposable { .. } | Manque::FormeApprochee { .. }
                ) || exacts[m.transfo().indice()].is_none()
            }));

            if exacts.iter().any(|p| p.is_some()) {
                table.blocs.insert(
                    nom.clone(),
                    Bloc {
                        index: etats
                            .iter()
                            .enumerate()
                            .map(|(i, (cle, _, _))| (cle.clone(), i as u32))
                            .collect(),
                        etats: etats.into_iter().map(|(cle, _, _)| cle).collect(),
                        exacts,
                        regles,
                    },
                );
            }
        }
        table
    }

    pub fn len(&self) -> usize {
        self.blocs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocs.is_empty()
    }

    /// La forme COMPACTE des règles — le repli pour un état non déclaré.
    ///
    /// Elle peut être partielle là où la permutation exacte, elle, est
    /// complète : voir `Manque::NonDecomposable`.
    pub fn regles(&self, bloc: &str, t: Transfo) -> Option<&ReglesBloc> {
        let b = self.blocs.get(bloc)?;
        b.exacts[t.indice()].as_ref()?;
        b.regles[t.indice()].as_ref()
    }

    /// Combien de blocs savent subir cette transformation.
    pub fn couverture(&self, t: Transfo) -> usize {
        self.blocs
            .values()
            .filter(|b| b.exacts[t.indice()].is_some())
            .count()
    }

    /// Transforme un état. Rend `None` si le bloc n'est pas dérivable — et
    /// c'est à l'appelant de décider quoi en faire, PAS à nous de tourner de
    /// travers en silence.
    pub fn transformer(&self, cle: &str, t: Transfo) -> Option<String> {
        let (nom, reste) = match cle.split_once('|') {
            Some((n, r)) => (n, r),
            None => (cle, ""),
        };
        let bloc = self.blocs.get(nom)?;
        let exact = bloc.exacts[t.indice()].as_ref()?;
        if reste.is_empty() {
            return Some(cle.to_string());
        }
        let mut props = props_de(reste);
        props.sort();
        let triee = cle_de(&props);
        // La permutation exacte d'abord — c'est elle qui décide.
        if let Some(&i) = bloc.index.get(&triee) {
            return Some(format!("{nom}|{}", bloc.etats[exact[i as usize] as usize]));
        }
        // Le pack ne déclare pas cet état : le repli compact, faute de mieux.
        let regles = bloc.regles[t.indice()].as_ref()?;
        regles.appliquer(&mut props);
        props.sort();
        Some(format!("{nom}|{}", cle_de(&props)))
    }

    /// Vraie si ce bloc sait subir CETTE transformation.
    pub fn connait(&self, bloc: &str, t: Transfo) -> bool {
        self.blocs
            .get(bloc)
            .is_some_and(|b| b.exacts[t.indice()].is_some())
    }
}
