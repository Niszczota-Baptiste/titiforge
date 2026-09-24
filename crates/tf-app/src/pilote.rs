//! **La boucle qui fait venir le monde** : caméra → demande → fil → scène.
//!
//! Les trois pièces existaient chacune de son côté — `tf_world::demande` dit
//! ce que la caméra veut, `chargeur` lit, `Ouvert::integrer` pose — et rien ne
//! les reliait. Ce dépôt a déjà payé deux fois « une jonction que personne
//! n'écrit est une jonction que chaque hôte réécrira » : la coque l'aurait
//! écrite dans son gestionnaire d'image, donc derrière un serveur graphique,
//! donc jamais vérifiée.
//!
//! Elle est ici, dans la bibliothèque, et elle ne connaît pas la fenêtre : on
//! lui donne un œil et un regard, pas une caméra. C'est ce qui permet de faire
//! VOLER une caméra dans un test et de vérifier que le monde arrive.
//!
//! **Ce qui ne se voit pas ailleurs** : un hôte qui redemande à chaque image
//! remplace la file du chargeur soixante fois par seconde, donc annule en
//! boucle la région qu'il est en train de lire. Le monde ne se charge jamais
//! et la machine a l'air occupée — le pire des symptômes. La règle qui
//! l'évite est dans `Suivi`, pure et testée ; celle-ci ne fait que
//! l'employer, et un test compte les LECTURES pour que ça reste vrai.

use std::sync::Arc;

use tf_world::coords::BlockPos;
use tf_world::{Cellule, Dimension, Niveau, RegionSource, Suivi};

use crate::chargeur::{Chargeur, Reponse};
use crate::scene::{Arrivee, Ouvert};

/// **Combien de cellules l'hôte intègre par image.**
///
/// Ni « tout ce qui est prêt » ni « une ». Le chargeur répond par RÉGION,
/// donc un millier de cellules arrive d'un coup quand un `.mca` finit de se
/// décoder : les intégrer dans le même appel figerait la fenêtre plusieurs
/// secondes. Mais la recopie des deux arènes est en O(scène) et se paie une
/// fois par APPEL, pas une fois par cellule — mesuré (`--test chargement`),
/// 197 cellules intégrées une par une coûtent 4 577 ms contre 826 par lot,
/// soit **× 5,5**. Une par image serait donc le pire des deux mondes.
///
/// À 4,2 ms par cellule amorti sur du bâti, deux cellules tiennent dans une
/// image de 8 ms. **Provisoire, et ça se dit** : le plafond ne disparaîtra
/// qu'avec des tampons GPU par SECTION, qui supprimeraient la recopie en
/// O(scène).
pub const CELLULES_PAR_IMAGE: usize = 2;

/// Ce qu'une image de streaming a fait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fait {
    /// Sections posées dans la scène.
    pub posees: usize,
    /// Cellules arrivées du fil.
    pub arrivees: usize,
    /// Une demande est-elle partie au fil ?
    pub demande: bool,
    /// Cellules évincées RETIRÉES de la scène pendant cette image.
    pub degagees: usize,
    /// Travaux de maillage APPLIQUÉS pendant cette image : c'est là, et
    /// seulement là, que les arènes changent.
    pub appliques: u64,
}

impl Fait {
    /// **Faut-il regarnir le GPU ?**
    ///
    /// Quand un maillage a été APPLIQUÉ — pas quand une cellule arrive. Le
    /// maillage part hors du fil principal (`Ouvert::integrer`) : une arrivée
    /// ne change les arènes qu'une ou deux images plus tard, et un dégagement
    /// de même. Regarnir sur les arrivées enverrait au GPU des arènes qui
    /// n'ont pas encore bougé, et ne le ferait plus au moment où elles
    /// bougent.
    ///
    /// C'est aussi ce qui couvre les deux pièges d'avant : une cellule qui
    /// revient vide efface ce qu'elle portait, et un dégagement retire des
    /// sections — dans les deux cas un travail part, revient, et s'applique.
    pub fn a_change(&self) -> bool {
        self.appliques > 0
    }
}

/// Le pilote : un fil de chargement, et la règle qui décide quand le
/// déranger.
pub struct Pilote {
    /// `None` pour la fixture, qui n'a pas de save derrière elle.
    chargeur: Option<Chargeur>,
    suivi: Suivi,
}

impl Pilote {
    /// **Aucune source, aucun fil.** La fixture n'a pas de monde derrière
    /// elle : lancer un fil pour ne rien lire mangerait un cœur et
    /// compliquerait l'arrêt.
    pub fn sans_source(niveau: Niveau, rayon: u32, y: (i32, i32)) -> Pilote {
        Pilote {
            chargeur: None,
            suivi: Suivi::neuf(niveau, rayon, y),
        }
    }

    pub fn neuf<S>(
        source: Arc<S>,
        dim: Dimension,
        niveau: Niveau,
        rayon: u32,
        y: (i32, i32),
    ) -> Pilote
    where
        S: RegionSource + Send + Sync + 'static,
    {
        Pilote {
            chargeur: Some(Chargeur::lancer(source, dim)),
            suivi: Suivi::neuf(niveau, rayon, y),
        }
    }

    /// **Le pilote d'un monde ouvert**, branché sur la MÊME copie de travail
    /// que le moteur : ce qu'une opération vient d'écrire doit arriver avec le
    /// reste, sinon une cellule streamée après une édition rendrait le monde
    /// d'AVANT.
    pub fn pour(o: &Ouvert, dim: Dimension, niveau: Niveau, rayon: u32, y: (i32, i32)) -> Pilote {
        match o.staging.clone() {
            Some(st) => Pilote::neuf(st, dim, niveau, rayon, y),
            None => Pilote::sans_source(niveau, rayon, y),
        }
    }

    pub fn rayon(&self) -> u32 {
        self.suivi.rayon()
    }

    /// Change la distance d'affichage ; prend effet à l'image suivante.
    pub fn rayon_voulu(&mut self, r: u32) {
        self.suivi.rayon_voulu(r);
    }

    /// Le fil a-t-il du travail ?
    pub fn occupe(&self) -> bool {
        self.chargeur.as_ref().is_some_and(|c| c.occupe())
    }

    /// **Une image** : demander ce qui manque, poser ce qui est prêt.
    ///
    /// `budget` borne ce qu'on intègre — voir [`CELLULES_PAR_IMAGE`].
    ///
    /// Une région qui fait paniquer le décodage revient en erreur mais
    /// n'interrompt RIEN : les cellules déjà prêtes sont posées d'abord. Un
    /// `.mca` corrompu ne doit pas arrêter l'éditeur de quelqu'un qui en a
    /// huit cents.
    pub fn image(
        &mut self,
        o: &mut Ouvert,
        oeil: BlockPos,
        regard: [f32; 3],
        budget: usize,
    ) -> Result<Fait, String> {
        let mut fait = Fait::default();
        let Some(c) = self.chargeur.as_mut() else {
            return Ok(fait);
        };
        let t = std::time::Instant::now();
        let residentes: Vec<Cellule> = o.cellules_residentes();
        crate::scene::phase("résidentes", t);
        let t = std::time::Instant::now();
        // **Ce que la caméra regarde est protégé de l'éviction**, et c'est
        // calculé à CHAQUE image, pas seulement quand on redemande : le champ
        // suit la caméra même quand rien ne manque, et une traînée qui reste
        // épinglée serait une traînée qu'on ne lâche jamais.
        //
        // Sans ça, un budget plus petit que le champ fait tourner la machine à
        // vide sans fin — mesuré, 180 évictions en 100 images sur une scène
        // qui n'avance pas.
        let champ: Vec<Cellule> = tf_world::demande::voulues(
            oeil,
            regard,
            self.suivi.rayon(),
            self.suivi.niveau(),
            self.suivi.hauteur(),
        )
        .into_iter()
        .map(|v| v.cellule)
        .collect();
        o.proteger(&champ);
        if let Some(lots) = self.suivi.suivre(oeil, regard, c.occupe(), &residentes) {
            fait.demande = c.demander(lots).is_some();
        }
        crate::scene::phase("champ+suivi", t);
        let t = std::time::Instant::now();
        let mut arrivees = Vec::new();
        let mut echec = None;
        for r in c.recevoir(budget) {
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
                Reponse::Echec(e) => echec = Some(e),
            }
        }
        fait.arrivees = arrivees.len();
        crate::scene::phase("recevoir", t);
        // **On appelle `integrer` même sans arrivée**, et c'est ce qui rend
        // une caméra IMMOBILE bornée : c'est lui qui retire de la scène ce que
        // la fenêtre de résidence a évincé à l'appel précédent. Sans cet
        // appel, on resterait au-dessus du budget dès que le vol s'arrête —
        // c'est-à-dire pendant qu'on regarde son build, donc presque tout le
        // temps.
        //
        // **Un compteur de part et d'autre, pas l'attente d'avant.**
        // `en_attente` lu avant l'appel raterait le cas où le même appel
        // évince ET dégage — ce qui arrive dès qu'on resserre le budget : la
        // scène perdrait des sections en annonçant qu'elle n'a pas changé, et
        // leur maillage resterait à l'écran.
        let (avant, appliques) = (o.degagees(), o.maillages_appliques());
        fait.posees = o.integrer(arrivees)?;
        fait.degagees = o.degagees() - avant;
        fait.appliques = o.maillages_appliques() - appliques;
        match echec {
            Some(e) => Err(e),
            None => Ok(fait),
        }
    }

    /// Arrête le fil et attend qu'il ait fini. Le `Drop` du chargeur le fait
    /// aussi ; cette méthode existe pour qu'un test puisse le faire au moment
    /// qu'il choisit.
    pub fn arreter(&mut self) {
        if let Some(c) = self.chargeur.as_mut() {
            c.arreter();
        }
    }
}
