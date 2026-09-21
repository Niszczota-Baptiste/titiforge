//! **Un seul aiguillage, pour tous les hôtes.**
//!
//! `catalogue.rs` dit ce qu'une opération PREND ; ce fichier dit comment on la
//! FAIT. Les deux ensemble ferment la boucle : un hôte nomme une opération,
//! remplit ses paramètres, appelle `executer`, et n'a plus rien à savoir.
//!
//! Sans lui, le catalogue n'aurait décrit qu'à moitié. L'exemple `editer`
//! portait deux cent cinquante lignes d'aiguillage — copier, transformer,
//! creuser, coller, déplacer, empiler, lisser, naturaliser, poser un biome —
//! et la coque en aurait écrit une seconde copie. C'est le piège des tables
//! qui divergent dans sa forme la plus coûteuse : pas deux constantes, deux
//! CHAÎNES D'APPELS, dont l'une finirait par oublier que `//hollow` pose de
//! l'air et ne changerait donc rien du tout.
//!
//! ## Il ne parle à personne
//!
//! Le moteur ne s'adresse pas à l'utilisateur — invariant hérité
//! d'`ExeWorldEdit`. `executer` rend donc un `CompteRendu` en DONNÉES : ce
//! qu'il a copié, ce qu'une rotation n'a pas su réécrire, combien de colonnes
//! il a relevées. La ligne de commande l'imprime, la coque l'affiche. Ni l'une
//! ni l'autre ne le reformule, et aucune des deux ne peut se tromper sur un
//! chiffre que l'autre a juste.

use crate::catalogue::{Composee, Travail};
use crate::creuser::{creuser, extrait_creuse};
use crate::edition::{
    appliquer, copier, deplacer, empiler, Erreur, Pas, RapportRegion, OCTETS_CREUSAGE,
};
use crate::forme::Forme;
use crate::masque::Masque;
use crate::naturaliser::Naturaliser;
use crate::plan::Plan;
use crate::presse::Collage;
use crate::relief::{relever, Lissage};
use crate::PoserBiome;
use tf_anvil::{Interner, StateId};
use tf_blocks::Transfo;
use tf_world::coords::{BBox, BlockPos};
use tf_world::source::{Dimension, Folder, RegionSource};
use tf_world::staging::{RegionStore, Staging};

/// La règle de réécriture d'un état sous transformation.
///
/// Un type nommé plutôt qu'écrit en place : il apparaît dans `Options`, dans
/// `Presse::transformer` et chez chaque hôte qui en fournit une, et un
/// `Option<&dyn Fn(&str, Transfo) -> Option<String>>` recopié trois fois est
/// trois occasions de se tromper d'un `&`.
pub type Regle<'a> = &'a dyn Fn(&str, Transfo) -> Option<String>;

/// Ce que l'hôte choisit, et que le catalogue ne décrit pas.
///
/// **Ce ne sont pas des paramètres d'opération** : une forme vient des gestes
/// de sélection, une graine du projet, un pack des réglages. Les mettre dans
/// le descripteur les ferait apparaître comme des champs de formulaire pour
/// chaque opération, ce qu'ils ne sont pas.
pub struct Options<'a> {
    /// Compter les blocs modifiés exactement. **× 21 à l'étage palette** :
    /// c'est le parcours qu'on vient d'éviter. Jamais rendu d'office.
    pub compter: bool,
    pub seed: u64,
    /// L'air d'un extrait écrase-t-il la destination ? `false` est le défaut
    /// de WorldEdit, et c'est le bon : on colle un bâtiment sur un terrain.
    pub avec_air: bool,
    /// Le volume visé DANS la sélection. `Boite` ne coûte rien.
    pub forme: Forme,
    /// La règle de réécriture des états sous rotation, INJECTÉE.
    ///
    /// `tf-ops` ne dépend pas de `tf-assets` : les règles se dérivent d'un
    /// pack, et une opération doit rester testable sans pack. Absente, les
    /// cases bougent et les orientations non — ce que le compte rendu DIT.
    pub regle: Option<Regle<'a>>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            compter: false,
            seed: 0,
            avec_air: false,
            forme: Forme::Boite,
            regle: None,
        }
    }
}

/// Ce qu'une exécution a fait, en données.
#[derive(Debug, Default)]
pub struct CompteRendu {
    pub rapport: RapportRegion,
    /// La taille de l'extrait, quand l'opération en a copié un.
    pub extrait: Option<[u32; 3]>,
    pub entites_copiees: usize,
    /// Les états qu'une rotation n'a pas su réécrire, laissés TELS QUELS.
    ///
    /// **Rendus plutôt que tus** : à moitié tourné, un build est faux d'une
    /// façon qu'aucune capture d'écran ne montre.
    pub intacts: Vec<StateId>,
    /// Vrai si aucune règle n'a été fournie. Sans elle, TOUS les états sont
    /// intacts, et les nommer un par un noierait la seule information utile :
    /// il manque un pack.
    pub sans_regle: bool,
    pub colonnes_relevees: usize,
    /// Les cases que l'opération a matérialisées en mémoire. Zéro pour tout
    /// ce qui travaille sur des sections packées.
    pub cases_materialisees: u64,
    /// Le pas effectif d'un `//stack`, calculé depuis la taille de la
    /// sélection. L'hôte ne le devine pas.
    pub pas: Option<[i32; 3]>,
}

/// **Exécute un travail. Le seul chemin.**
///
/// `sel` est la sélection FINALE — les gestes (`//expand`, `//chunk`) ont déjà
/// été appliqués par l'hôte, parce que c'est elle qui décide de la portée, des
/// formes et de ce que le rapport annoncera. Les appliquer après rendrait un
/// chiffre qui ne correspond à rien.
pub fn executer<S: RegionSource, O: RegionStore>(
    travail: &Travail,
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    interner: &mut Interner,
    opts: &Options,
) -> Result<CompteRendu, Erreur> {
    let mut cr = CompteRendu::default();
    let air = interner.intern("minecraft:air");

    // ── ce qui n'est pas « un masque et un motif »
    let composee = match travail {
        Travail::Direct { plan, .. } => {
            let plan = plan.clone().avec_seed(opts.seed);
            let plan = if opts.compter {
                plan.en_comptant()
            } else {
                plan
            };
            let plan = match &opts.forme {
                Forme::Boite => plan,
                f => plan.dans(f.clone()),
            };
            let portee = plan.portee(sel);
            cr.rapport = appliquer(staging, dim, folder, &portee, &plan, interner)?;
            return Ok(cr);
        }
        Travail::Composee(c) => c,
    };

    match composee {
        // ── celles qui s'appliquent directement
        Composee::Biome { nom } => {
            let op = PoserBiome {
                biome: interner.intern(nom),
                compter: opts.compter,
            };
            cr.rapport = appliquer(staging, dim, folder, sel, &op, interner)?;
        }
        Composee::Naturaliser {
            surface,
            sous_sol,
            roche,
            profondeur,
        } => {
            let mut n = Naturaliser::nouveau(
                interner.intern(surface),
                interner.intern(sous_sol),
                interner.intern(roche),
                air,
            );
            n.profondeur = *profondeur;
            n.compter = opts.compter;
            cr.rapport = appliquer(staging, dim, folder, sel, &n, interner)?;
        }
        Composee::Deplacer { decalage, remplir } => {
            let remplissage = interner.intern(remplir);
            cr.rapport = deplacer(
                staging,
                dim,
                folder,
                sel,
                pas(*decalage, opts, air),
                remplissage,
                interner,
            )?;
        }
        Composee::Empiler { fois, direction } => {
            // **Le pas d'un `//stack` est la TAILLE de la sélection**, pas un
            // bloc : c'est ce que fait WorldEdit, et c'est ce qu'on veut neuf
            // fois sur dix — un mur qu'on prolonge.
            let (sx, sy, sz) = sel.size();
            let d = direction.pas();
            let d = [d[0] * sx as i32, d[1] * sy as i32, d[2] * sz as i32];
            cr.pas = Some(d);
            cr.rapport = empiler(
                staging,
                dim,
                folder,
                sel,
                pas(d, opts, air),
                *fois,
                interner,
            )?;
        }
        Composee::Lisser { rayon, passes } => {
            // **La lecture DÉBORDE de la sélection**, du rayon du noyau. Sans
            // cette marge, le bord se moyennerait contre des colonnes qu'on
            // n'a pas lues — donc contre du vide — et s'effondrerait.
            let m = *rayon as i32;
            let large = BBox::new(
                BlockPos {
                    x: sel.min.x - m,
                    y: sel.min.y,
                    z: sel.min.z - m,
                },
                BlockPos {
                    x: sel.max.x + m,
                    y: sel.max.y,
                    z: sel.max.z + m,
                },
            );
            let solide = Masque::Non(Box::new(Masque::Etat(air)));
            let brute = relever(staging, dim, folder, &large, &solide, interner)?;
            cr.colonnes_relevees = brute.h.len();
            let (sx, _, sz) = sel.size();
            let voulue = brute
                .lissee(*rayon, (*passes).max(1))
                .resserree(sel.min.x, sel.min.z, sx, sz);
            let op = Lissage {
                carte: &voulue,
                vide: air,
                compter: opts.compter,
            };
            cr.rapport = appliquer(staging, dim, folder, sel, &op, interner)?;
        }

        // ── celles qui passent par un extrait
        Composee::CopierVers { decalage, transfo } => {
            let mut p = copier(staging, dim, folder, sel, interner)?;
            cr.extrait = Some(p.taille);
            cr.entites_copiees = p.entites.len();
            if let Some(t) = transfo {
                cr.sans_regle = opts.regle.is_none();
                let r = p.transformer(*t, interner, &|cle, t| opts.regle.and_then(|f| f(cle, t)));
                cr.intacts = r.intacts;
                p = r.presse;
            }
            let collage = Collage {
                presse: &p,
                coin: BlockPos {
                    x: sel.min.x + decalage[0],
                    y: sel.min.y + decalage[1],
                    z: sel.min.z + decalage[2],
                },
                avec_air: opts.avec_air,
                air,
                compter: opts.compter,
            };
            // Une opération ne paie que sa PORTÉE : un collage paie son
            // extrait, pas la sélection d'où il vient.
            let portee = collage.bornes();
            cr.rapport = appliquer(staging, dim, folder, &portee, &collage, interner)?;
        }
        Composee::Creuser { epaisseur } => {
            // Le creusage a son PROPRE appétit — la copie, les quatre tampons
            // de diffusion, et l'extrait creusé. Vérifier avec celui de
            // `//copy` laisserait passer trois fois trop, et une allocation
            // refusée ABANDONNE le processus au lieu de rendre une erreur.
            cr.cases_materialisees =
                crate::edition::verifier_materialisable(sel, OCTETS_CREUSAGE)? * OCTETS_CREUSAGE;
            let p = copier(staging, dim, folder, sel, interner)?;
            cr.extrait = Some(p.taille);
            let solide = Masque::Non(Box::new(Masque::Etat(air)));
            let c = creuser(&p, &solide, (*epaisseur).max(1));
            let vide = extrait_creuse(&p, &c, air);
            let collage = Collage {
                presse: &vide,
                coin: sel.min,
                // **Creuser POSE de l'air.** Sans `avec_air`, le collage
                // sauterait exactement les cases qu'on vient de calculer et ne
                // changerait rien — une opération qui s'exécute, se rapporte,
                // et ne fait rien.
                avec_air: true,
                air,
                compter: opts.compter,
            };
            let portee = collage.bornes();
            cr.rapport = appliquer(staging, dim, folder, &portee, &collage, interner)?;
        }
    }
    Ok(cr)
}

fn pas(d: [i32; 3], opts: &Options, air: StateId) -> Pas {
    Pas {
        d,
        avec_air: opts.avec_air,
        air,
        compter: opts.compter,
    }
}

/// Le plan nu d'un travail direct, pour un hôte qui veut l'inspecter avant de
/// l'exécuter (ce que coûtera l'étage, par exemple) sans rien écrire.
pub fn plan_de(travail: &Travail) -> Option<&Plan> {
    match travail {
        Travail::Direct { plan, .. } => Some(plan),
        Travail::Composee(_) => None,
    }
}
