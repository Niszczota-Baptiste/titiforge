//! **Choisir un bloc sans connaître son identifiant.**
//!
//! Un champ de bloc qui n'accepte que `minecraft:oak_stairs[facing=east]`
//! tapé à la main n'est utilisable que par qui connaît déjà l'identifiant —
//! c'est-à-dire par personne, sur un serveur qui en déclare 1 678 de plus que
//! le jeu. Le nuancier propose, dans l'ordre où l'on s'en sert :
//!
//! 1. le bloc **visé** — la pipette : on regarde un bloc, on le prend ;
//! 2. les **récents** — on repose presque toujours ce qu'on vient de poser ;
//! 3. ce que le **monde** porte déjà, sous l'état EXACT que le jeu a écrit —
//!    ce qu'un `//replace` doit viser pour ne rien rater ;
//! 4. tout ce que le **pack** déclare.
//!
//! Pur : ce qui se décide ici — quel candidat, dans quel ordre, ce qui est
//! connu — se teste sans fenêtre. L'interface le dessine, la coque le nourrit.

use tf_ops::catalogue::{bloc_affiche, cle_de_bloc, Descripteur, Params, Saisie, Valeur};

/// Combien de blocs récents on retient.
pub const MAX_RECENTS: usize = 12;

/// L'air n'a pas de blockstate dans un pack — il n'a rien à dessiner — et
/// c'est pourtant le bloc qu'on pose le plus : vider, creuser, remplacer par
/// rien. Il est toujours proposé.
const AIR: &str = "minecraft:air";

/// D'où vient une proposition. L'ORDRE des variantes est l'ordre de
/// préférence, à rang de correspondance égal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origine {
    Vise,
    Recent,
    Monde,
    Pack,
}

impl Origine {
    /// Le mot qu'affiche la liste à côté du bloc.
    pub fn nom(self) -> &'static str {
        match self {
            Origine::Vise => "visé",
            Origine::Recent => "récent",
            Origine::Monde => "monde",
            Origine::Pack => "pack",
        }
    }
}

/// Une proposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidat {
    /// La clé canonique (`nom|k=v`).
    pub cle: String,
    /// Ce qu'on montre ET ce qu'on écrit dans le champ : la syntaxe du jeu.
    pub affiche: String,
    pub origine: Origine,
}

/// Le nuancier.
#[derive(Debug, Clone, Default)]
pub struct Nuancier {
    /// Les noms que le pack déclare, triés et uniques.
    pack: Vec<String>,
    /// Les états que la scène a rencontrés, triés et uniques.
    monde: Vec<String>,
    /// Combien d'états de la table de la scène sont déjà dans `monde` : la
    /// table ne fait que grandir, on ne relit que la fin.
    monde_vus: usize,
    /// QUELLE table on suit — la scène en refait une à chaque rechargement.
    table: u32,
    /// Le plus récent d'abord.
    recents: Vec<String>,
}

impl Nuancier {
    /// Un nuancier sur les noms d'un pack.
    pub fn new<S: AsRef<str>>(pack: impl IntoIterator<Item = S>) -> Nuancier {
        let mut pack: Vec<String> = pack
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .chain(std::iter::once(AIR.to_string()))
            .collect();
        pack.sort_unstable();
        pack.dedup();
        Nuancier {
            pack,
            ..Default::default()
        }
    }

    /// Change de pack et de monde, en gardant les récents : ils sont une
    /// habitude de l'utilisateur, pas une propriété du monde.
    pub fn repartir<S: AsRef<str>>(&mut self, pack: impl IntoIterator<Item = S>) {
        let recents = std::mem::take(&mut self.recents);
        *self = Nuancier::new(pack);
        self.recents = recents;
    }

    /// Combien d'états de la table de la scène ont déjà été lus.
    pub fn monde_vus(&self) -> usize {
        self.monde_vus
    }

    /// **Suit la table d'états de la scène**, à appeler à chaque image : rien
    /// n'est lu tant qu'elle n'a pas bougé. `etats` la parcourt du début ;
    /// seuls ceux d'après `monde_vus` sont lus.
    ///
    /// `table` dit LAQUELLE : la scène en refait une à chaque rechargement
    /// (`Ouvert::rechargements`), et une table neuve n'est pas la suite de
    /// l'ancienne. Deviner le changement à la longueur — « plus courte, donc
    /// une autre » — ratait une table rechargée plus LONGUE : on en aurait
    /// sauté le début, en gardant les blocs de l'ancienne. Une table plus
    /// courte que ce qu'on a vu repart aussi de zéro, qui qu'en dise l'appelant.
    pub fn suivre_monde<'a>(
        &mut self,
        table: u32,
        etats: impl Iterator<Item = &'a str>,
        total: usize,
    ) {
        if table == self.table && total == self.monde_vus {
            return;
        }
        if table != self.table || total < self.monde_vus {
            self.monde.clear();
            self.monde_vus = 0;
            self.table = table;
        }
        for e in etats.skip(self.monde_vus) {
            if let Err(i) = self.monde.binary_search_by(|x| x.as_str().cmp(e)) {
                self.monde.insert(i, e.to_string());
            }
        }
        self.monde_vus = total;
    }

    /// Les récents, le plus récent d'abord.
    pub fn recents(&self) -> &[String] {
        &self.recents
    }

    /// **Note qu'un bloc a servi.** Il passe en tête des récents — une seule
    /// fois. L'air n'y entre pas (il est toujours proposé), ni ce qui ne se
    /// lit pas comme un bloc : un récent qu'on ne pourrait pas reposer n'en
    /// est pas un. Rend `true` si la liste a changé.
    pub fn utiliser(&mut self, texte: &str) -> bool {
        let Ok(cle) = cle_de_bloc(texte) else {
            return false;
        };
        if cle == AIR || self.recents.first() == Some(&cle) {
            return false;
        }
        self.recents.retain(|r| *r != cle);
        self.recents.insert(0, cle);
        self.recents.truncate(MAX_RECENTS);
        true
    }

    /// Note tous les blocs qu'une opération va poser ou viser — ses
    /// paramètres de bloc et chaque entrée de ses mélanges. Rend `true` si
    /// les récents ont changé.
    pub fn utiliser_params(&mut self, d: &Descripteur, p: &Params) -> bool {
        let mut change = false;
        for decl in d.params {
            match (decl.saisie, p.get(decl.nom)) {
                (Saisie::Bloc, Some(Valeur::Texte(t))) => change |= self.utiliser(t),
                (Saisie::Melange, Some(Valeur::Melange(v))) => {
                    for (_, b) in v {
                        change |= self.utiliser(b);
                    }
                }
                _ => {}
            }
        }
        change
    }

    /// **Ce bloc est-il connu ?** Du pack par son nom, ou du monde par son
    /// état exact. Un bloc inconnu des deux est très probablement une faute
    /// de frappe — et le jeu remplace par de l'air ce qu'il ne connaît pas.
    ///
    /// Sans pack (aucun nom chargé), rien n'est jugé : `true`.
    pub fn connu(&self, cle: &str) -> bool {
        if self.pack.len() <= 1 {
            return true;
        }
        let nom = cle.split_once('|').map_or(cle, |(n, _)| n);
        self.pack.binary_search_by(|x| x.as_str().cmp(nom)).is_ok()
            || self.monde.binary_search_by(|x| x.as_str().cmp(cle)).is_ok()
    }

    /// **Les propositions pour ce qui est tapé**, au plus `n`.
    ///
    /// Sans rien de tapé : le visé, les récents, puis ce que le monde porte —
    /// pas le pack entier, deux mille noms ne sont pas une proposition.
    ///
    /// Sinon, classées par correspondance (exacte, préfixe, début de mot,
    /// contenu), puis par origine, puis les plus courtes d'abord — à
    /// correspondance égale, la plus courte est la moins spécialisée — puis
    /// par ordre alphabétique, pour un ordre TOTAL : une liste qui change
    /// d'une image à l'autre se lit « l'outil est instable ».
    pub fn chercher(&self, requete: &str, vise: Option<&str>, n: usize) -> Vec<Candidat> {
        let q = Requete::new(requete);
        let mut vus: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut trouves: Vec<(u32, Origine, &str)> = Vec::new();
        let sources = vise
            .into_iter()
            .map(|v| (Origine::Vise, v))
            .chain(self.recents.iter().map(|r| (Origine::Recent, r.as_str())))
            .chain(self.monde.iter().map(|m| (Origine::Monde, m.as_str())))
            .chain(
                self.pack
                    .iter()
                    .filter(|_| !q.vide())
                    .map(|p| (Origine::Pack, p.as_str())),
            );
        for (origine, cle) in sources {
            if !vus.insert(cle) {
                continue;
            }
            if q.vide() {
                trouves.push((0, origine, cle));
            } else if let Some(r) = q.rang(cle) {
                trouves.push((r, origine, cle));
            }
        }
        if !q.vide() {
            trouves.sort_unstable_by(|a, b| {
                (a.0, a.1, a.2.len(), a.2).cmp(&(b.0, b.1, b.2.len(), b.2))
            });
        }
        trouves
            .into_iter()
            .take(n)
            .map(|(_, origine, cle)| Candidat {
                cle: cle.to_string(),
                affiche: bloc_affiche(cle),
                origine,
            })
            .collect()
    }

    /// Les récents, une clé par ligne — pour les retrouver au prochain
    /// lancement.
    pub fn recents_en_texte(&self) -> String {
        let mut t = self.recents.join("\n");
        t.push('\n');
        t
    }

    /// Relit des récents écrits par [`recents_en_texte`](Self::recents_en_texte).
    /// Une ligne qui ne se lit pas comme un bloc est sautée : un fichier
    /// retouché à la main ne doit pas empêcher d'ouvrir.
    pub fn relire_recents(&mut self, texte: &str) {
        let mut v: Vec<String> = Vec::new();
        for l in texte.lines() {
            if let Ok(c) = cle_de_bloc(l) {
                if c != AIR && !v.contains(&c) {
                    v.push(c);
                }
            }
        }
        v.truncate(MAX_RECENTS);
        self.recents = v;
    }
}

/// Le fichier des blocs récents, à côté des séances et des mondes récents :
/// `<données>/titiforge/blocs.txt`.
pub fn fichier_blocs(seances: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(seances.parent()?.join("blocs.txt"))
}

/// Ce qui est tapé, ramené à la forme des clés : minuscules, et la syntaxe du
/// jeu traduite — `[` devient `|`, `]` disparaît — pour qu'un début de
/// propriétés tapé à la main trouve les états qui les portent.
struct Requete {
    mots: Vec<String>,
}

impl Requete {
    fn new(texte: &str) -> Requete {
        let t: String = texte
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| *c != ']')
            .map(|c| if c == '[' { '|' } else { c })
            .collect();
        Requete {
            mots: t.split_whitespace().map(str::to_string).collect(),
        }
    }

    fn vide(&self) -> bool {
        self.mots.is_empty()
    }

    /// Le rang de correspondance d'une clé — plus petit, meilleur — ou
    /// `None` si un mot n'y est pas. Celui d'une requête de plusieurs mots est
    /// la SOMME des rangs de ses mots : avec le seul pire, « oak st » classait
    /// `dark_oak_stool` à égalité avec `oak_stool`, parce que « st » y est
    /// dans les deux un début de mot — et perdait que « oak » COMMENCE l'un
    /// des deux noms.
    fn rang(&self, cle: &str) -> Option<u32> {
        let nom = cle.split_once('|').map_or(cle, |(n, _)| n);
        let chemin = nom.split_once(':').map_or(nom, |(_, c)| c);
        let mut somme = 0u32;
        for m in &self.mots {
            let r = if cle == m || nom == m || chemin == m {
                0
            } else if cle.starts_with(m.as_str()) || chemin.starts_with(m.as_str()) {
                1
            } else if chemin.split(['_', '/']).any(|w| w.starts_with(m.as_str())) {
                2
            } else if cle.contains(m.as_str()) {
                3
            } else {
                return None;
            };
            somme += r;
        }
        Some(somme)
    }
}
