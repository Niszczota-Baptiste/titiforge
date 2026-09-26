//! **Les règles de rotation des états, pour le fil moteur.**
//!
//! Tourner un extrait d'un quart de tour, c'est deux choses : déplacer les
//! cases — de la géométrie — et tourner les ÉTATS : un escalier qui regardait
//! l'est regarde le sud. La seconde vient de règles DÉRIVÉES du pack
//! (`tf_blocks::Table`). La coque passait `None` au moteur : dans la fenêtre,
//! un « Copier vers » tourné déplaçait les cases et laissait chaque escalier,
//! chaque porte, chaque échelle dans son orientation d'origine — sans un mot,
//! c'est-à-dire très exactement le build « à moitié tourné » que `presse.rs`
//! interdit de taire.
//!
//! La dérivation coûte 629 ms sur le codex du serveur : elle se fait dans un
//! fil à elle, dès que les assets sont là, et le moteur ne l'ATTEND que la
//! première fois qu'une opération demande une règle — jamais pour un `//set`.

use std::sync::{Arc, OnceLock};

/// Les règles, partagées entre le fil qui les dérive et le moteur qui s'en
/// sert. Cloner ne coûte qu'un compteur.
#[derive(Clone)]
pub struct Regles(Arc<OnceLock<Option<tf_blocks::Table>>>);

/// Ce qui remet les règles une fois dérivées. Un seul envoi : c'est un
/// `OnceLock` derrière.
pub struct Livraison(Arc<OnceLock<Option<tf_blocks::Table>>>);

impl Livraison {
    /// Remet les règles — ou leur ABSENCE : une dérivation qui échoue doit
    /// quand même répondre, sinon le moteur attendrait pour toujours.
    pub fn livrer(self, t: Option<tf_blocks::Table>) {
        let _ = self.0.set(t);
    }
}

impl Drop for Livraison {
    /// Une livraison abandonnée sans avoir rien remis vaut « aucune règle » :
    /// un fil de dérivation mort ne doit pas laisser le moteur attendre.
    fn drop(&mut self) {
        let _ = self.0.set(None);
    }
}

impl Default for Regles {
    /// Aucune règle — surtout pas « en attente » : un `Default` qui ferait
    /// attendre le moteur pour toujours serait un piège à retardement.
    fn default() -> Self {
        Regles::absentes()
    }
}

impl std::fmt::Debug for Regles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let etat = match self.0.get() {
            None => "en cours",
            Some(None) => "absentes",
            Some(Some(_)) => "prêtes",
        };
        write!(f, "Regles({etat})")
    }
}

impl Regles {
    /// Aucune règle : les cases tournent, les états non — et la réponse du
    /// moteur le DIT.
    pub fn absentes() -> Regles {
        let l = OnceLock::new();
        let _ = l.set(None);
        Regles(Arc::new(l))
    }

    /// Des règles déjà dérivées.
    pub fn pretes(t: tf_blocks::Table) -> Regles {
        let l = OnceLock::new();
        let _ = l.set(Some(t));
        Regles(Arc::new(l))
    }

    /// Des règles qui arriveront plus tard, par la `Livraison`.
    pub fn en_attente() -> (Regles, Livraison) {
        let l = Arc::new(OnceLock::new());
        (Regles(l.clone()), Livraison(l))
    }

    /// **Dérive en fond**, dans un fil à lui. Une dérivation qui panique vaut
    /// « aucune règle » — la `Livraison` abandonnée le dit — plutôt qu'un
    /// moteur qui attend sans fin.
    pub fn deriver_en_fond(cat: Arc<tf_assets::Catalogue>) -> Regles {
        let (r, livraison) = Regles::en_attente();
        let lance = std::thread::Builder::new()
            .name("regles".into())
            .spawn(move || {
                let t = tf_blocks::Table::deriver(&cat);
                livraison.livrer(Some(t));
            });
        // Un fil qui ne se lance pas a déjà lâché sa `Livraison` : les règles
        // valent « absentes », et rien n'attend.
        drop(lance);
        r
    }

    /// Les règles, en les ATTENDANT si la dérivation n'est pas finie. `None`
    /// si elles n'existent pas.
    pub fn table(&self) -> Option<&tf_blocks::Table> {
        self.0.wait().as_ref()
    }

    /// Sont-elles déjà connues — prêtes ou absentes ? Sans attendre.
    pub fn connues(&self) -> bool {
        self.0.get().is_some()
    }
}
