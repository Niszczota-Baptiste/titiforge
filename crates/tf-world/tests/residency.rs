//! Le cache de résidence.
//!
//! Une liste chaînée intrusive se corrompt en silence : les lectures
//! continuent de marcher, seules la récence et la comptabilité du budget
//! dérivent. D'où un vérificateur d'invariants appelé après chaque mutation,
//! et un test à opérations aléatoires pour couvrir les enchaînements qu'on
//! n'imagine pas.

use tf_world::{Residency, State, Weighed};

/// Une valeur dont on choisit le poids, pour éprouver le budget.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Bloc {
    poids: usize,
    marque: u32,
}

impl Bloc {
    fn new(poids: usize) -> Self {
        Bloc { poids, marque: 0 }
    }
}

impl Weighed for Bloc {
    fn bytes(&self) -> usize {
        self.poids
    }
}

fn cache(budget: usize) -> Residency<u32, Bloc> {
    Residency::new(budget)
}

// ── bases ───────────────────────────────────────────────────────────────────

#[test]
fn un_cache_neuf_est_vide_et_coherent() {
    let c = cache(1000);
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert_eq!(c.used(), 0);
    assert_eq!(c.budget(), 1000);
    assert_eq!(c.evictions(), 0);
    c.debug_check_invariants();
}

#[test]
fn le_budget_se_compte_en_octets_pas_en_elements() {
    // C'est toute la raison d'être du type. Un chunk d'air pèse quelques
    // centaines d'octets, un chunk de terrain quelques dizaines de milliers :
    // un plafond « N éléments » laisse passer un facteur cinquante, et c'est
    // le pire cas qui fait échouer l'application chez l'utilisateur.
    let mut c = cache(1000);
    c.insert(1, Bloc::new(900), State::Clean);
    assert_eq!(c.len(), 1);
    assert_eq!(c.used(), 900);

    // Deux petits tiennent à côté d'un gros, un gros non.
    c.insert(2, Bloc::new(50), State::Clean);
    assert_eq!(c.len(), 2);
    c.debug_check_invariants();

    let delogees = c.insert(3, Bloc::new(500), State::Clean);
    assert!(!delogees.is_empty(), "il a bien fallu évincer");
    assert!(c.used() <= 1000, "used = {}", c.used());
    c.debug_check_invariants();
}

#[test]
fn get_remonte_l_entree_et_peek_ne_la_touche_pas() {
    let mut c = cache(10_000);
    for k in 1..=3u32 {
        c.insert(k, Bloc::new(100), State::Clean);
    }
    assert_eq!(c.keys_mru(), vec![3, 2, 1]);

    c.get(&1);
    assert_eq!(c.keys_mru(), vec![1, 3, 2], "un accès remonte en tête");
    c.debug_check_invariants();

    // Regarder le cache ne doit pas changer ce qu'il gardera.
    c.peek(&2);
    assert_eq!(c.keys_mru(), vec![1, 3, 2], "peek ne touche pas la récence");
    c.debug_check_invariants();
}

#[test]
fn c_est_bien_le_moins_recemment_utilise_qui_part() {
    let mut c = cache(300);
    for k in 1..=3u32 {
        c.insert(k, Bloc::new(100), State::Clean);
    }
    c.get(&1); // 1 devient le plus récent, 2 le plus ancien

    let delogees = c.insert(4, Bloc::new(100), State::Clean);
    assert_eq!(delogees.items.len(), 1);
    assert_eq!(delogees.items[0].0, 2, "le plus ancien accès");
    assert!(!c.contains(&2));
    assert!(c.contains(&1) && c.contains(&3) && c.contains(&4));
    c.debug_check_invariants();
}

// ── ce qui ne s'évince pas ─────────────────────────────────────────────────

#[test]
fn une_entree_modifiee_n_est_jamais_evincee() {
    // Perdre du travail non sauvegardé pour tenir un budget serait le pire
    // échange possible. Le plafond est une CIBLE.
    let mut c = cache(200);
    c.insert(1, Bloc::new(100), State::Dirty);
    c.insert(2, Bloc::new(100), State::Clean);

    let delogees = c.insert(3, Bloc::new(100), State::Clean);
    assert!(c.contains(&1), "le modifié doit rester");
    assert!(!c.contains(&2), "c'est le propre qui part");
    assert_eq!(delogees.items.len(), 1);
    c.debug_check_invariants();
}

#[test]
fn une_entree_epinglee_n_est_jamais_evincee() {
    // Évincer un chunk au milieu d'une opération le ferait relire depuis le
    // disque sans ses éditions — une corruption silencieuse.
    let mut c = cache(200);
    c.insert(1, Bloc::new(100), State::Clean);
    c.insert(2, Bloc::new(100), State::Clean);
    assert!(c.pin(&1));

    c.insert(3, Bloc::new(100), State::Clean);
    assert!(c.contains(&1), "l'épinglé doit rester");
    assert!(!c.contains(&2));
    c.debug_check_invariants();
}

#[test]
fn les_epingles_se_comptent() {
    // Deux opérations peuvent tenir le même chunk : une seule qui relâche ne
    // doit pas le rendre évinçable.
    let mut c = cache(200);
    c.insert(1, Bloc::new(100), State::Clean);
    c.pin(&1);
    c.pin(&1);
    assert_eq!(c.pins(&1), 2);

    c.unpin(&1);
    assert_eq!(c.pins(&1), 1);
    c.insert(2, Bloc::new(150), State::Clean);
    assert!(c.contains(&1), "encore une épingle");

    c.unpin(&1);
    assert_eq!(c.pins(&1), 0);
    c.insert(3, Bloc::new(150), State::Clean);
    assert!(!c.contains(&1), "plus d'épingle : évinçable");
    c.debug_check_invariants();
}

#[test]
fn depasser_le_budget_vaut_mieux_que_perdre_du_travail() {
    let mut c = cache(100);
    for k in 1..=3u32 {
        c.insert(k, Bloc::new(100), State::Dirty);
    }
    let r = c.insert(4, Bloc::new(100), State::Clean);
    assert!(r.over_budget, "le cache doit DIRE qu'il dépasse");
    assert!(r.is_empty(), "et n'avoir rien évincé");
    assert!(c.contains(&4), "surtout pas ce qu'on vient d'insérer");
    assert_eq!(c.len(), 4);
    assert!(c.used() > c.budget());
    c.debug_check_invariants();
}

#[test]
fn l_eviction_saute_les_non_evincables_au_lieu_de_s_arreter_dessus() {
    // Un seul chunk modifié en queue bloquerait TOUTE éviction si on
    // s'arrêtait au premier candidat refusé.
    let mut c = cache(1000);
    c.insert(1, Bloc::new(200), State::Dirty); // le plus ancien, non évinçable
    for k in 2..=5u32 {
        c.insert(k, Bloc::new(200), State::Clean);
    }
    assert_eq!(c.used(), 1000);

    let r = c.insert(6, Bloc::new(400), State::Clean);
    assert!(
        !r.over_budget,
        "il y avait de quoi évincer derrière le modifié"
    );
    assert_eq!(r.items.len(), 2, "deux propres délogés");
    assert!(c.contains(&1), "le modifié est toujours là");
    assert!(c.used() <= 1000);
    c.debug_check_invariants();
}

#[test]
fn mark_clean_rend_l_entree_evincable() {
    let mut c = cache(200);
    c.insert(1, Bloc::new(100), State::Dirty);
    c.insert(2, Bloc::new(100), State::Clean);
    assert_eq!(c.state(&1), Some(State::Dirty));

    assert!(c.mark_clean(&1));
    assert_eq!(c.state(&1), Some(State::Clean));
    c.get(&2); // 1 redevient le plus ancien
    let r = c.insert(3, Bloc::new(100), State::Clean);
    assert_eq!(r.items[0].0, 1, "vidé, donc évinçable");
    c.debug_check_invariants();
}

// ── écriture ────────────────────────────────────────────────────────────────

#[test]
fn get_mut_marque_modifie_et_recompte_le_poids() {
    // Une section qui grossit doit être recomptée, sinon le budget dérive et
    // finit par ne plus rien vouloir dire.
    let mut c = cache(10_000);
    c.insert(1, Bloc::new(100), State::Clean);
    assert_eq!(c.used(), 100);

    {
        let mut v = c.edit(&1).unwrap();
        v.poids = 500;
        v.marque = 7;
    } // ← le garde relit le poids ici
    assert_eq!(c.state(&1), Some(State::Dirty), "écrire marque modifié");
    assert_eq!(
        c.used(),
        500,
        "le poids doit être relu à la libération du garde"
    );
    assert_eq!(c.peek(&1).unwrap().marque, 7);
    c.debug_check_invariants();
}

#[test]
fn reinserer_une_cle_remplace_et_recompte() {
    let mut c = cache(10_000);
    c.insert(1, Bloc::new(100), State::Dirty);
    c.insert(1, Bloc::new(400), State::Clean);
    assert_eq!(c.len(), 1, "pas de doublon");
    assert_eq!(c.used(), 400);
    assert_eq!(
        c.state(&1),
        Some(State::Clean),
        "l'état repart de l'insertion"
    );
    c.debug_check_invariants();
}

#[test]
fn remove_sort_meme_ce_qui_est_epingle_ou_modifie() {
    // C'est un ordre, pas une suggestion : à l'appelant de savoir.
    let mut c = cache(10_000);
    c.insert(1, Bloc::new(100), State::Dirty);
    c.pin(&1);
    let v = c.remove(&1).expect("remove doit sortir l'entrée");
    assert_eq!(v.poids, 100);
    assert!(c.is_empty());
    assert_eq!(c.used(), 0);
    assert_eq!(c.remove(&1), None, "deux fois ne rend rien");
    c.debug_check_invariants();
}

#[test]
fn dirty_keys_rend_les_plus_froides_d_abord() {
    // C'est l'ordre dans lequel il faut les vider : les plus froides d'abord,
    // puisque ce sont celles dont on a le moins besoin.
    let mut c = cache(10_000);
    for k in 1..=4u32 {
        c.insert(
            k,
            Bloc::new(10),
            if k % 2 == 0 {
                State::Dirty
            } else {
                State::Clean
            },
        );
    }
    // Ordre d'accès : 1, 2, 3, 4 → le plus froid est 1, mais 1 est propre.
    assert_eq!(c.dirty_keys(), vec![2, 4]);

    c.get(&2); // 2 devient le plus chaud
    assert_eq!(c.dirty_keys(), vec![4, 2]);
    c.debug_check_invariants();
}

// ── réutilisation des emplacements ─────────────────────────────────────────

#[test]
fn les_emplacements_liberes_se_reutilisent_sans_corrompre_la_liste() {
    // Le piège classique d'une liste intrusive sur tableau : un emplacement
    // recyclé garde les chaînages de son ancien occupant.
    let mut c = cache(10_000);
    for k in 1..=5u32 {
        c.insert(k, Bloc::new(10), State::Clean);
    }
    c.remove(&2);
    c.remove(&4);
    c.debug_check_invariants();

    for k in 6..=9u32 {
        c.insert(k, Bloc::new(10), State::Clean);
        c.debug_check_invariants();
    }
    assert_eq!(c.len(), 7);
    let mut vues = c.keys_mru();
    vues.sort_unstable();
    assert_eq!(vues, vec![1, 3, 5, 6, 7, 8, 9]);
}

#[test]
fn vider_entierement_puis_remplir_a_nouveau() {
    let mut c = cache(10_000);
    for k in 1..=4u32 {
        c.insert(k, Bloc::new(10), State::Clean);
    }
    for k in 1..=4u32 {
        c.remove(&k);
        c.debug_check_invariants();
    }
    assert!(c.is_empty());
    assert_eq!(c.used(), 0);
    assert_eq!(c.keys_mru(), Vec::<u32>::new());

    c.insert(42, Bloc::new(10), State::Clean);
    assert_eq!(c.keys_mru(), vec![42]);
    c.debug_check_invariants();
}

#[test]
fn une_entree_plus_grosse_que_le_budget_entre_quand_meme() {
    // Refuser l'entrée serait pire : l'appelant en a besoin MAINTENANT pour
    // travailler, et un chunk qu'on ne peut pas charger est une opération qui
    // échoue. Le cache dépasse et le dit.
    let mut c = cache(100);
    let r = c.insert(1, Bloc::new(5000), State::Clean);
    assert!(
        c.contains(&1),
        "insert suivi de contains ne doit jamais rendre faux"
    );
    assert!(r.over_budget, "et le cache doit dire qu'il dépasse");
    assert_eq!(c.used(), 5000);
    c.debug_check_invariants();
}

#[test]
fn set_budget_n_evince_pas_immediatement() {
    let mut c = cache(10_000);
    for k in 1..=5u32 {
        c.insert(k, Bloc::new(100), State::Clean);
    }
    c.set_budget(200);
    assert_eq!(c.len(), 5, "l'éviction se fait à la prochaine insertion");

    c.insert(6, Bloc::new(100), State::Clean);
    assert!(c.used() <= 200, "used = {}", c.used());
    c.debug_check_invariants();
}

// ── au hasard ───────────────────────────────────────────────────────────────

#[test]
fn aucun_enchainement_d_operations_ne_corrompt_le_cache() {
    // Les tests ci-dessus couvrent les cas qu'on imagine. Celui-ci couvre les
    // enchaînements qu'on n'imagine pas — et il est REJOUABLE : graine fixe.
    let mut s: u32 = 0x1234_5678;
    let mut rand = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };

    let mut c = cache(4_000);
    for tour in 0..20_000u32 {
        let k = rand() % 40;
        match rand() % 8 {
            0 | 1 => {
                c.insert(k, Bloc::new((rand() % 500) as usize + 1), State::Clean);
            }
            2 => {
                c.insert(k, Bloc::new((rand() % 500) as usize + 1), State::Dirty);
            }
            3 => {
                c.get(&k);
            }
            4 => {
                if let Some(mut v) = c.edit(&k) {
                    v.poids = (rand() % 500) as usize + 1;
                }
            }
            5 => {
                c.remove(&k);
            }
            6 => {
                c.pin(&k);
            }
            _ => {
                c.unpin(&k);
                c.mark_clean(&k);
            }
        }
        if tour % 37 == 0 {
            c.debug_check_invariants();
        }
    }
    c.debug_check_invariants();

    // Et le cache a bien travaillé, sinon le test ne prouverait rien.
    assert!(
        c.evictions() > 0,
        "aucune éviction : le budget était trop large"
    );
    assert!(!c.is_empty());
}

#[test]
fn trim_ramene_sous_le_budget_apres_des_ecritures_qui_ont_grossi() {
    // Le garde ne provoque pas d'éviction : le budget est une cible, et une
    // éviction au milieu d'une série d'écritures délogerait justement ce
    // qu'on est en train d'éditer.
    let mut c = cache(1000);
    for k in 1..=5u32 {
        c.insert(k, Bloc::new(100), State::Clean);
    }
    for k in 1..=5u32 {
        c.mark_clean(&k);
        if let Some(mut v) = c.edit(&k) {
            v.poids = 300;
        }
        c.mark_clean(&k);
    }
    assert_eq!(c.used(), 1500, "le garde a bien recompté");
    c.debug_check_invariants();

    let r = c.trim();
    assert!(!r.is_empty());
    assert!(c.used() <= 1000, "used = {}", c.used());
    c.debug_check_invariants();
}

// ── correction de poids ─────────────────────────────────────────────────────

/// **Corriger un poids ne doit pas mentir sur la récence.**
///
/// C'est la raison d'être d'`update`. L'appelant qui se sert du cache comme
/// d'un comptable voit le poids d'une entrée changer sans que personne ne
/// l'ait regardée — une cellule de monde maigrit quand sa voisine arrive et
/// masque ses faces de bord. `insert` ferait remonter l'entrée en tête, donc
/// le LRU garderait justement ce qu'il faudrait lâcher.
#[test]
fn update_corrige_le_poids_sans_toucher_a_la_recence() {
    let mut c = cache(1000);
    c.insert(1, Bloc::new(100), State::Clean);
    c.insert(2, Bloc::new(100), State::Clean);
    c.insert(3, Bloc::new(100), State::Clean);
    assert_eq!(c.keys_mru(), vec![3, 2, 1]);

    assert!(c.update(&1, Bloc::new(400)));
    c.debug_check_invariants();
    assert_eq!(
        c.keys_mru(),
        vec![3, 2, 1],
        "corriger un poids n'est pas un accès"
    );
    assert_eq!(c.used(), 600, "le budget doit suivre la correction");
    assert_eq!(c.peek(&1).map(|b| b.poids), Some(400));
}

/// Un poids corrigé reste ÉVINÇABLE. `edit` marquerait l'entrée modifiée,
/// donc protégée pour toujours — un cache dont tout est « modifié » ne peut
/// plus rien rendre.
#[test]
fn update_ne_marque_pas_l_entree_modifiee() {
    let mut c = cache(1000);
    c.insert(7, Bloc::new(10), State::Clean);
    c.update(&7, Bloc::new(20));
    assert_eq!(c.state(&7), Some(State::Clean));
    assert!(c.dirty_keys().is_empty());
}

/// **`update` n'évince pas**, même en dépassant le budget : le plafond est une
/// cible, et l'éviction a lieu à la prochaine insertion ou sur `trim`. Sinon
/// un appelant qui corrige dix poids d'affilée verrait le cache se vider au
/// milieu de sa passe, sur des poids encore faux.
#[test]
fn update_n_evince_pas_meme_au_dela_du_budget() {
    let mut c = cache(1000);
    c.insert(1, Bloc::new(100), State::Clean);
    c.insert(2, Bloc::new(100), State::Clean);

    assert!(c.update(&1, Bloc::new(5000)));
    c.debug_check_invariants();
    assert_eq!(c.len(), 2, "rien ne doit partir sur une correction");
    assert_eq!(c.used(), 5100);
    assert_eq!(c.evictions(), 0);

    // C'est `trim` qui tranche, et il évince le plus ANCIEN accès d'abord —
    // ici l'entrée corrigée, qui est aussi la plus froide. Elle suffit à
    // repasser sous le plafond, donc `2` reste : on évince ce qu'il faut, pas
    // tout ce qu'on peut.
    let sortis = c.trim();
    c.debug_check_invariants();
    assert_eq!(
        sortis.items.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(c.used(), 100);
    assert!(c.contains(&2));
}

/// **Ce n'est pas une insertion déguisée.** Une clé absente rend faux et ne
/// crée rien : le comptable corrige ce qu'il connaît, il n'invente pas
/// d'entrée dont personne ne tient le contenu.
#[test]
fn update_refuse_une_cle_absente() {
    let mut c = cache(1000);
    assert!(!c.update(&42, Bloc::new(10)));
    c.debug_check_invariants();
    assert!(c.is_empty());
    assert_eq!(c.used(), 0);
}

/// Une correction qui allège doit rendre les octets, pas seulement les
/// compter : sans la soustraction, le budget ne ferait que monter et le cache
/// finirait par tout évincer pour de la place déjà libre.
#[test]
fn update_rend_les_octets_quand_l_entree_maigrit() {
    let mut c = cache(1000);
    c.insert(1, Bloc::new(500), State::Clean);
    c.insert(2, Bloc::new(400), State::Clean);
    assert_eq!(c.used(), 900);
    c.update(&1, Bloc::new(50));
    c.debug_check_invariants();
    assert_eq!(c.used(), 450);
    // La place rendue sert : l'insertion suivante n'évince plus rien.
    let sortis = c.insert(3, Bloc::new(500), State::Clean);
    assert!(sortis.is_empty(), "il y avait la place");
}
