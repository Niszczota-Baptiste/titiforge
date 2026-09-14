//! Cache de résidence plafonné en **octets**.
//!
//! Il n'existe aucun état « le monde est chargé ». Une région pleine fait déjà
//! 100 millions de blocs ; un monde Minefield en fait 80 milliards. Ce qui
//! tient en mémoire est donc une fenêtre, et le plafond est une taille en
//! octets — pas un nombre d'éléments.
//!
//! Pourquoi en octets et pas en chunks : un chunk d'air pèse quelques
//! centaines d'octets, un chunk de terrain moderne quelques dizaines de
//! kilo-octets. Un plafond « 4000 chunks » laisse donc passer un facteur
//! cinquante entre le meilleur et le pire cas, et c'est le pire qui fait
//! échouer l'application chez l'utilisateur.
//!
//! Deux propriétés qui ne se négocient pas :
//!
//! 1. **Le plafond est une CIBLE, pas une limite dure.** On n'évince jamais
//!    quelque chose de modifié ni d'épinglé pour le respecter. Perdre du
//!    travail non sauvegardé pour tenir un budget serait le pire échange
//!    possible.
//! 2. **Ce qui est en cours d'édition ne bouge pas.** Évincer un chunk au
//!    milieu d'une opération le ferait relire depuis le disque, sans ses
//!    éditions — une corruption silencieuse.

use std::collections::HashMap;
use std::hash::Hash;

/// Ce que le cache sait d'une entrée sans rien connaître de son contenu.
pub trait Weighed {
    /// Octets occupés. Sert au budget — une approximation raisonnable suffit,
    /// mais elle doit être STABLE tant que la valeur ne change pas, sinon la
    /// comptabilité du budget dérive.
    fn bytes(&self) -> usize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Lu depuis la source, jamais modifié. Évinçable à volonté.
    Clean,
    /// Modifié et pas encore écrit. **Jamais évincé** : il faut le vider
    /// d'abord.
    Dirty,
}

struct Node<K, V> {
    key: K,
    value: V,
    bytes: usize,
    state: State,
    pins: u32,
    prev: Option<usize>,
    next: Option<usize>,
}

/// Cache à éviction LRU, plafonné en octets.
///
/// La liste de récence est intrusive, sur un tableau d'emplacements réutilisés
/// — pas une `VecDeque` de clés. Chercher une clé dans une file pour la
/// remonter coûterait O(n) à chaque accès, et un accès a lieu par bloc lu.
pub struct Residency<K: Eq + Hash + Clone, V: Weighed> {
    nodes: Vec<Option<Node<K, V>>>,
    libres: Vec<usize>,
    index: HashMap<K, usize>,
    /// Le plus récemment utilisé.
    tete: Option<usize>,
    /// Le moins récemment utilisé : le prochain candidat à l'éviction.
    queue: Option<usize>,
    utilise: usize,
    budget: usize,
    evictions: u64,
}

/// Accès en écriture à une entrée.
///
/// À sa libération, le poids de la valeur est relu et la comptabilité du
/// budget mise à jour. C'est ce qui rend impossible d'oublier de recompter —
/// et ce qui empêche le budget de devenir une fiction.
pub struct Editing<'a, K: Eq + Hash + Clone, V: Weighed> {
    cache: &'a mut Residency<K, V>,
    slot: usize,
}

impl<K: Eq + Hash + Clone, V: Weighed> std::ops::Deref for Editing<'_, K, V> {
    type Target = V;
    fn deref(&self) -> &V {
        &self.cache.nodes[self.slot]
            .as_ref()
            .expect("un garde tient un emplacement vivant")
            .value
    }
}

impl<K: Eq + Hash + Clone, V: Weighed> std::ops::DerefMut for Editing<'_, K, V> {
    fn deref_mut(&mut self) -> &mut V {
        &mut self.cache.nodes[self.slot]
            .as_mut()
            .expect("un garde tient un emplacement vivant")
            .value
    }
}

impl<K: Eq + Hash + Clone, V: Weighed> Drop for Editing<'_, K, V> {
    fn drop(&mut self) {
        let n = self.cache.nodes[self.slot]
            .as_mut()
            .expect("un garde tient un emplacement vivant");
        let neuf = n.value.bytes();
        let ancien = n.bytes;
        n.bytes = neuf;
        self.cache.utilise = self.cache.utilise + neuf - ancien;
    }
}

/// Ce qu'une insertion a délogé.
#[derive(Debug)]
pub struct Evicted<K, V> {
    pub items: Vec<(K, V)>,
    /// Vrai si le budget reste dépassé faute de candidat évinçable — tout ce
    /// qui reste est modifié ou épinglé. L'appelant doit alors vider des
    /// entrées modifiées, pas insister.
    pub over_budget: bool,
}

// `derive(Default)` exigerait `K: Default` et `V: Default`, alors qu'une liste
// vide n'a besoin d'aucun des deux. Une borne parasite sur un type public se
// propage à tous les appelants.
impl<K, V> Default for Evicted<K, V> {
    fn default() -> Self {
        Evicted {
            items: Vec::new(),
            over_budget: false,
        }
    }
}

impl<K, V> Evicted<K, V> {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl<K: Eq + Hash + Clone, V: Weighed> Residency<K, V> {
    pub fn new(budget_bytes: usize) -> Self {
        Residency {
            nodes: Vec::new(),
            libres: Vec::new(),
            index: HashMap::new(),
            tete: None,
            queue: None,
            utilise: 0,
            budget: budget_bytes,
            evictions: 0,
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn used(&self) -> usize {
        self.utilise
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Nombre total d'évictions depuis la création. Sert à voir, dans un
    /// bench ou un panneau de diagnostic, si le budget est trop serré : un
    /// cache qui évince en boucle relit le disque sans arrêt.
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    pub fn contains(&self, k: &K) -> bool {
        self.index.contains_key(k)
    }

    /// Change le plafond. Ne provoque pas d'éviction immédiate : elle se fait
    /// à la prochaine insertion, comme le reste.
    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// Lecture qui compte comme un ACCÈS : l'entrée remonte en tête.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        let i = *self.index.get(k)?;
        self.remonter(i);
        self.nodes[i].as_ref().map(|n| &n.value)
    }

    /// Lecture qui ne change PAS la récence.
    ///
    /// Utile pour un panneau de diagnostic ou une vérification : regarder le
    /// cache ne doit pas modifier ce qu'il gardera.
    pub fn peek(&self, k: &K) -> Option<&V> {
        self.index
            .get(k)
            .and_then(|&i| self.nodes[i].as_ref())
            .map(|n| &n.value)
    }

    /// Écriture. Rend un **garde** : l'entrée est marquée modifiée, et son
    /// poids est relu à la libération du garde.
    ///
    /// Pourquoi un garde et pas un `&mut V` : la valeur change APRÈS le retour
    /// de la fonction. Un `&mut V` ne laisse donc aucun moment où observer le
    /// nouveau poids, et la comptabilité du budget dérive en silence — une
    /// section qui double de taille continuerait d'être comptée pour son
    /// ancienne. Un garde rend l'oubli impossible plutôt qu'improbable.
    ///
    /// Le garde n'évince pas à sa libération : le budget est une cible, et
    /// l'éviction a lieu à la prochaine insertion ou sur `trim`.
    pub fn edit(&mut self, k: &K) -> Option<Editing<'_, K, V>> {
        let i = *self.index.get(k)?;
        self.remonter(i);
        self.nodes[i].as_mut()?.state = State::Dirty;
        Some(Editing {
            cache: self,
            slot: i,
        })
    }

    /// Ramène l'occupation sous le budget sans rien insérer. Utile après une
    /// série d'écritures qui ont fait grossir des entrées.
    pub fn trim(&mut self) -> Evicted<K, V> {
        self.evict_to_budget(None)
    }

    /// Insère, puis évince jusqu'à revenir sous le budget.
    ///
    /// Une clé déjà présente est REMPLACÉE — et son état repart à `Clean`.
    pub fn insert(&mut self, k: K, v: V, state: State) -> Evicted<K, V> {
        if let Some(&i) = self.index.get(&k) {
            let n = self.nodes[i]
                .as_mut()
                .expect("index et emplacements désynchronisés");
            self.utilise -= n.bytes;
            n.bytes = v.bytes();
            n.value = v;
            n.state = state;
            self.utilise += self.nodes[i].as_ref().unwrap().bytes;
            self.remonter(i);
            return self.evict_to_budget(Some(i));
        }

        let bytes = v.bytes();
        let node = Node {
            key: k.clone(),
            value: v,
            bytes,
            state,
            pins: 0,
            prev: None,
            next: self.tete,
        };
        let i = match self.libres.pop() {
            Some(i) => {
                self.nodes[i] = Some(node);
                i
            }
            None => {
                self.nodes.push(Some(node));
                self.nodes.len() - 1
            }
        };
        if let Some(t) = self.tete {
            self.nodes[t].as_mut().unwrap().prev = Some(i);
        }
        self.tete = Some(i);
        if self.queue.is_none() {
            self.queue = Some(i);
        }
        self.index.insert(k, i);
        self.utilise += bytes;
        // On protège l'entrée qu'on vient d'insérer. Sans ça, `insert` suivi de
        // `contains` pouvait rendre faux : une valeur plus grosse que le budget,
        // ou insérée alors que tout le reste est modifié, se faisait évincer
        // aussitôt. L'appelant l'a demandée — il en a besoin MAINTENANT.
        self.evict_to_budget(Some(i))
    }

    /// Retire une entrée, épinglée ou modifiée comprise. C'est un ordre, pas
    /// une suggestion — à l'appelant de savoir ce qu'il fait.
    pub fn remove(&mut self, k: &K) -> Option<V> {
        let i = self.index.remove(k)?;
        self.detacher(i);
        let n = self.nodes[i].take()?;
        self.utilise -= n.bytes;
        self.libres.push(i);
        Some(n.value)
    }

    pub fn state(&self, k: &K) -> Option<State> {
        self.index
            .get(k)
            .and_then(|&i| self.nodes[i].as_ref())
            .map(|n| n.state)
    }

    /// Déclare l'entrée écrite : elle redevient évinçable.
    pub fn mark_clean(&mut self, k: &K) -> bool {
        match self.index.get(k).copied() {
            Some(i) => {
                self.nodes[i].as_mut().unwrap().state = State::Clean;
                true
            }
            None => false,
        }
    }

    /// Toutes les entrées modifiées, du plus ancien accès au plus récent.
    /// C'est l'ordre dans lequel il faut les vider : les plus froides d'abord,
    /// puisque ce sont celles dont on a le moins besoin.
    pub fn dirty_keys(&self) -> Vec<K> {
        let mut out = Vec::new();
        let mut cur = self.queue;
        while let Some(i) = cur {
            let n = self.nodes[i].as_ref().unwrap();
            if n.state == State::Dirty {
                out.push(n.key.clone());
            }
            cur = n.prev;
        }
        out
    }

    // ── épinglage ───────────────────────────────────────────────────────────

    /// Empêche l'éviction tant que l'épingle tient. Les épingles se comptent :
    /// deux opérations peuvent tenir le même chunk.
    pub fn pin(&mut self, k: &K) -> bool {
        match self.index.get(k).copied() {
            Some(i) => {
                self.nodes[i].as_mut().unwrap().pins += 1;
                true
            }
            None => false,
        }
    }

    pub fn unpin(&mut self, k: &K) -> bool {
        match self.index.get(k).copied() {
            Some(i) => {
                let n = self.nodes[i].as_mut().unwrap();
                n.pins = n.pins.saturating_sub(1);
                true
            }
            None => false,
        }
    }

    pub fn pins(&self, k: &K) -> u32 {
        self.index
            .get(k)
            .and_then(|&i| self.nodes[i].as_ref())
            .map(|n| n.pins)
            .unwrap_or(0)
    }

    // ── interne ─────────────────────────────────────────────────────────────

    fn evincable(&self, i: usize) -> bool {
        match self.nodes[i].as_ref() {
            Some(n) => n.pins == 0 && n.state == State::Clean,
            None => false,
        }
    }

    fn evict_to_budget(&mut self, protege: Option<usize>) -> Evicted<K, V> {
        let mut out = Evicted::default();
        while self.utilise > self.budget {
            // On remonte depuis la queue jusqu'au premier candidat évinçable.
            // Sauter les non-évinçables plutôt que s'arrêter au premier :
            // sinon un seul chunk modifié en queue bloquerait toute éviction.
            let mut cur = self.queue;
            let mut choisi = None;
            while let Some(i) = cur {
                if Some(i) != protege && self.evincable(i) {
                    choisi = Some(i);
                    break;
                }
                cur = self.nodes[i].as_ref().unwrap().prev;
            }
            let Some(i) = choisi else {
                // Tout est modifié ou épinglé. On DÉPASSE le budget plutôt que
                // de perdre du travail : c'est une cible, pas une limite dure.
                out.over_budget = true;
                break;
            };
            let key = self.nodes[i].as_ref().unwrap().key.clone();
            self.index.remove(&key);
            self.detacher(i);
            let n = self.nodes[i].take().unwrap();
            self.utilise -= n.bytes;
            self.libres.push(i);
            self.evictions += 1;
            out.items.push((key, n.value));
        }
        out
    }

    /// Remonte l'emplacement en tête de la liste de récence.
    fn remonter(&mut self, i: usize) {
        if self.tete == Some(i) {
            return;
        }
        self.detacher(i);
        let n = self.nodes[i].as_mut().unwrap();
        n.prev = None;
        n.next = self.tete;
        if let Some(t) = self.tete {
            self.nodes[t].as_mut().unwrap().prev = Some(i);
        }
        self.tete = Some(i);
        if self.queue.is_none() {
            self.queue = Some(i);
        }
    }

    /// Sort l'emplacement de la liste sans toucher à son contenu.
    fn detacher(&mut self, i: usize) {
        let (prev, next) = {
            let n = self.nodes[i].as_ref().unwrap();
            (n.prev, n.next)
        };
        match prev {
            Some(p) => self.nodes[p].as_mut().unwrap().next = next,
            None => self.tete = next,
        }
        match next {
            Some(s) => self.nodes[s].as_mut().unwrap().prev = prev,
            None => self.queue = prev,
        }
        let n = self.nodes[i].as_mut().unwrap();
        n.prev = None;
        n.next = None;
    }

    /// Vérifie la cohérence interne. Une liste chaînée intrusive se corrompt
    /// en silence : les lectures continuent de marcher, seules la récence et
    /// la comptabilité du budget dérivent. Les tests l'appellent après chaque
    /// mutation.
    ///
    /// Panique en décrivant ce qui cloche, plutôt que de rendre un booléen
    /// qu'un test afficherait sans dire où.
    #[doc(hidden)]
    pub fn debug_check_invariants(&self) {
        let vivants = self.nodes.iter().filter(|n| n.is_some()).count();
        assert_eq!(
            vivants,
            self.index.len(),
            "emplacements vivants ≠ entrées indexées"
        );

        // Parcours avant : tête → queue.
        let mut avant = Vec::new();
        let mut cur = self.tete;
        let mut precedent = None;
        while let Some(i) = cur {
            let n = self.nodes[i]
                .as_ref()
                .expect("la liste pointe sur un emplacement vide");
            assert_eq!(
                n.prev, precedent,
                "chaînage arrière rompu à l'emplacement {i}"
            );
            assert_eq!(
                self.index.get(&n.key),
                Some(&i),
                "index incohérent pour {i}"
            );
            avant.push(i);
            precedent = Some(i);
            cur = n.next;
            assert!(avant.len() <= vivants + 1, "cycle dans la liste de récence");
        }
        assert_eq!(
            avant.len(),
            vivants,
            "la liste ne couvre pas toutes les entrées"
        );
        assert_eq!(
            avant.last().copied(),
            self.queue,
            "la queue ne termine pas la liste"
        );

        // Parcours arrière : il doit rendre exactement l'inverse.
        let mut arriere = Vec::new();
        let mut cur = self.queue;
        while let Some(i) = cur {
            arriere.push(i);
            cur = self.nodes[i].as_ref().unwrap().prev;
        }
        arriere.reverse();
        assert_eq!(avant, arriere, "les deux sens de parcours divergent");

        let somme: usize = self
            .nodes
            .iter()
            .filter_map(|n| n.as_ref())
            .map(|n| n.bytes)
            .sum();
        assert_eq!(somme, self.utilise, "la comptabilité du budget a dérivé");
    }

    /// Les clés, du plus récemment utilisé au moins récent. Pour les tests et
    /// les diagnostics.
    pub fn keys_mru(&self) -> Vec<K> {
        let mut out = Vec::with_capacity(self.index.len());
        let mut cur = self.tete;
        while let Some(i) = cur {
            let n = self.nodes[i].as_ref().unwrap();
            out.push(n.key.clone());
            cur = n.next;
        }
        out
    }
}
