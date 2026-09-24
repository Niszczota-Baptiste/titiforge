//! L'arène des blocs-MODÈLES : une POSE par bloc, la géométrie une seule fois.
//!
//! Sur la cible Minefield, deux tiers du catalogue ne sont pas des cubes :
//! escaliers, dalles, chaises, vases, lanternes. La passe gloutonne ne les
//! voit pas, et jusqu'ici rien ne les dessinait — sur la première capture
//! d'une vraie save, 3 957 blocs manquaient à l'appel sans que l'image ne le
//! dise.
//!
//! On ne peut pas les ajouter en quads : mesuré dans `tf-mesh`, 349 000
//! blocs-modèles produisent **5,8 millions de quads**, neuf dixièmes du
//! maillage. Or ces quads sont la MÊME géométrie répétée — deux dalles de
//! chêne côte à côte n'ont pas deux modèles, elles ont deux positions.
//!
//! Alors la géométrie vit UNE fois, dans `faces`, indexée par état ; et un
//! bloc posé ne pèse que sa `Pose`. Le GPU recolle les deux.
//!
//! ## Un seul appel de dessin, et comment
//!
//! Chaque pose a un nombre de faces DIFFÉRENT — une dalle en a six, un sac de
//! friandises Minefield en a jusqu'à 492. On ne peut donc pas dessiner « n
//! faces par instance ».
//!
//! Trois façons de s'en sortir, et on prend la troisième :
//!
//! 1. aplatir côté processeur, une instance par face — c'est exactement ce que
//!    la `Pose` existe pour éviter ;
//! 2. un appel par nombre de faces distinct — une quinzaine d'appels, alors
//!    que la cible du projet est **moins de cinq** ;
//! 3. une somme préfixe : la pose *i* sait à quel rang commence sa première
//!    face dans le flot global. On dessine `F` instances d'un quad, et le
//!    sommet retrouve sa pose par **recherche dichotomique**. Un appel, zéro
//!    octet par face, vingt itérations sur un million de poses.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use tf_anvil::StateId;
use tf_mesh::forme::{Cuboide, FACES};
use tf_mesh::{Adresse, Chantier, Lot};

/// Une face d'un cuboïde de modèle. Partagée par tous les blocs de cet état.
///
/// **64 octets, et ils vivent une seule fois.** Une centaine de kilo-octets
/// pour tout le catalogue d'une scène : c'est la table, pas la géométrie.
///
/// Les bornes sont en `[f32; 4]` et non `[f32; 3]`, et ce n'est pas du
/// gaspillage : **un `vec3<f32>` s'aligne sur SEIZE octets en WGSL.** Une
/// structure Rust en `[f32; 3]` mise en face décale tout ce qui suit d'un
/// champ sur deux, et le shader lit des bornes prises au hasard dans la table
/// voisine — mesuré, ça sort en traînées qui filent à l'infini. Le `vec4`
/// rend la correspondance ÉVIDENTE plutôt que de la faire reposer sur des
/// règles de bourrage qu'on relit mal. Un test compare les deux tailles.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct FaceModele {
    /// Bornes du cuboïde, en seizièmes, LOCALES au bloc. Le quatrième
    /// composant est du bourrage.
    pub min: [f32; 4],
    pub max: [f32; 4],
    /// Les uv, en seizièmes.
    pub uv: [f32; 4],
    /// 0 = −X, 1 = +X, 2 = −Y, 3 = +Y, 4 = −Z, 5 = +Z.
    pub face: u32,
    pub couche: u32,
    pub teinte: u32,
    /// 1 si la face porte `cullface` ET touche le bord du bloc de ce côté.
    ///
    /// Les deux conditions sont pesées ICI, une fois par état, plutôt que par
    /// bloc dans le shader : une face au milieu du bloc reste visible quoi
    /// qu'il y ait à côté, et c'est une propriété du modèle.
    pub cullable: u32,
}

/// Un bloc-modèle posé. **16 octets.**
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Pose {
    /// `x | y << 8 | z << 16 | voisins_opaques << 24`, le tout local à la
    /// section.
    pub local: u32,
    /// Quel EMPLACEMENT — donc quelle origine de section. `Pose::VIDE` porte
    /// `u32::MAX` : une pose qui ne dessine rien.
    pub section: u32,
    /// Rang de sa PREMIÈRE face dans le flot global. C'est la somme préfixe,
    /// et c'est ce que le shader dichotomise.
    pub debut_face: u32,
    /// Où commencent les faces de son modèle dans `faces`.
    pub debut_modele: u32,
}

impl Pose {
    /// Le marqueur d'une pose qui ne dessine RIEN. Le shader le lit AVANT de
    /// toucher à `faces` : une pose vide n'a pas de modèle.
    pub const SANS_SECTION: u32 = u32::MAX;

    /// **Une pose qui ne dessine rien**, au rang de face donné.
    ///
    /// Elles servent de deux façons, et les deux sont nécessaires :
    ///
    /// - **les TROUS** d'une place libérée, pour que le tableau garde sa
    ///   forme sans recopie ;
    /// - **le TERMINAL** de chaque place occupée. Une place réserve une
    ///   fenêtre de faces qui peut être plus large que ce que ses poses
    ///   dessinent ; le shader attribue chaque face à la DERNIÈRE pose dont le
    ///   début la précède, donc sans terminal les faces en trop tomberaient
    ///   sur la dernière vraie pose — et dessineraient les faces du modèle
    ///   SUIVANT dans la table, un morceau de chaise collé à un escalier.
    pub fn vide(debut_face: u32) -> Pose {
        Pose {
            local: 0,
            section: Pose::SANS_SECTION,
            debut_face,
            debut_modele: 0,
        }
    }

    pub fn est_vide(&self) -> bool {
        self.section == Pose::SANS_SECTION
    }
}

/// Une PLACE de la passe de modèles : une plage de poses et la fenêtre de
/// faces qu'elles couvrent dans l'appel de dessin, attribuées ENSEMBLE.
///
/// Ensemble, parce que la somme préfixe doit rester croissante d'un bout à
/// l'autre du tableau — c'est ce que le shader dichotomise. Deux places
/// rangées dans le même ordre dans les deux espaces la gardent croissante ;
/// une plage de poses réemployée avec la fenêtre d'une autre la casserait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bloc {
    /// Première pose, et combien de poses la place réserve — terminal compris.
    pd: u32,
    pcap: u32,
    /// Première face de la fenêtre, et sa largeur.
    fd: u32,
    fcap: u32,
    /// La section qui l'occupe ; `None` pour un trou.
    qui: Option<Adresse>,
}

/// Les origines de section, une par tranche de poses.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Origine {
    /// Coin de la section, en seizièmes, en monde. Quatrième composant :
    /// bourrage, pour la même raison d'alignement que `FaceModele`.
    pub position: [f32; 4],
}

/// Tout ce que la passe de modèles envoie au GPU.
///
/// **Chaque section a sa PLACE, et un remplacement n'écrit que la sienne.**
/// Première écriture : chaque remaillage reconstruisait le tableau des poses
/// en recopiant les tranches inchangées et en recalculant la somme préfixe de
/// bout en bout. Juste, et en O(scène) — mesuré en vol sur du bâti, **16 ms
/// par image** pour cette seule passe, qui GRANDISSAIENT avec la scène.
///
/// Ce qui rend la chose possible sans casser la dichotomie du shader tient en
/// trois règles, et un test rejoue le shader pour les vérifier :
///
/// 1. la somme préfixe `debut_face` reste croissante sur TOUT le tableau ;
/// 2. un trou ne contient que des poses vides ;
/// 3. chaque place occupée finit par un terminal vide (`Pose::vide`).
///
/// Avec elles, une face attribuée à une vraie pose tombe forcément dans les
/// faces de CETTE pose, et toute autre face tombe sur une pose vide, donc ne
/// dessine rien. Recoller deux trous ne demande alors aucune réécriture ; en
/// couper un n'en demande qu'une, courte, là où la croissance serait rompue.
#[derive(Debug, Default)]
pub struct AreneModeles {
    pub faces: Vec<FaceModele>,
    pub poses: Vec<Pose>,
    /// Toutes les places, occupées et trous, par première pose. Elles pavent
    /// les deux espaces sans jour ni recouvrement, dans le même ordre.
    blocs: BTreeMap<u32, Bloc>,
    /// Où est la place de chaque section.
    occupes: HashMap<Adresse, u32>,
    /// Les trous, par `(largeur de fenêtre, poses, première pose)` : le plus
    /// juste d'abord.
    trous: BTreeSet<(u32, u32, u32)>,
    /// Total des faces à dessiner, trous compris. C'est le nombre
    /// d'INSTANCES de l'appel de dessin.
    pub faces_a_dessiner: u32,
    /// Plages de poses réécrites depuis le dernier envoi au GPU.
    sales: Vec<(u32, u32)>,
    /// Combien de faces de `faces` le GPU a déjà : la table ne fait que
    /// grandir, donc ce qui manque est toujours une QUEUE.
    faces_envoyees: usize,
    /// Poses et faces dans les trous, tenues à jour plutôt que resommées :
    /// les resommer à chaque remplacement serait un parcours de tous les
    /// trous par image.
    trous_poses: u64,
    trous_faces: u64,
    /// Poses écrites depuis la création : le coût réel des remplacements.
    ecrites: u64,
    tassements: u32,
    /// La géométrie déjà construite, par `(état, biome)` → `(début, nombre)`
    /// dans `faces`.
    ///
    /// **Gardée entre deux appels**, et c'est ce qui rend [`remplacer`]
    /// possible : refaire une tranche sans elle rappellerait `modele` et
    /// ferait grossir `faces` d'une copie à chaque édition, sans fin et sans
    /// que rien ne le dise.
    ///
    /// [`remplacer`]: AreneModeles::remplacer
    connus: HashMap<(StateId, StateId), (u32, u32)>,
}

impl AreneModeles {
    pub fn is_empty(&self) -> bool {
        self.occupes.is_empty()
    }

    pub fn octets(&self) -> usize {
        self.faces.len() * std::mem::size_of::<FaceModele>()
            + self.poses.len() * std::mem::size_of::<Pose>()
    }

    /// Empile un chantier. Les emplacements sont ceux d'`Emplacements::depuis`
    /// — la MÊME fonction que la passe des quads appelle, donc les deux passes
    /// désignent la même origine par le même numéro.
    pub fn depuis(
        chantier: &Chantier,
        modele: &dyn Fn(StateId, StateId) -> Vec<FaceModele>,
    ) -> AreneModeles {
        let emplacements = crate::arene::Emplacements::depuis(chantier);
        let mut a = AreneModeles::default();
        for lot in &chantier.lots {
            let slot = emplacements
                .de(&lot.adresse)
                .expect("chaque lot a un emplacement");
            a.poser(lot, slot, modele);
        }
        a
    }

    /// **Refait les sections VISÉES, et ne touche à rien d'autre.**
    ///
    /// À appeler APRÈS `Arene::remplacer`, qui attribue les emplacements des
    /// deux passes : une section qui arrive y reçoit le sien, et c'est lui
    /// qu'on lit ici.
    pub fn remplacer(
        &mut self,
        emplacements: &crate::arene::Emplacements,
        visees: &[Adresse],
        neufs: &[Lot],
        modele: &dyn Fn(StateId, StateId) -> Vec<FaceModele>,
    ) {
        let reviennent: HashSet<Adresse> = neufs.iter().map(|l| l.adresse).collect();
        for a in visees {
            if !reviennent.contains(a) {
                self.enlever(a);
            }
        }
        for lot in neufs {
            let slot = emplacements
                .de(&lot.adresse)
                .expect("la passe des quads attribue l'emplacement d'abord");
            self.poser(lot, slot, modele);
        }
        let (tp, tf) = self.trous_totaux();
        if tf > crate::arene::TASSER_AU_DELA && tf > self.faces_a_dessiner as u64 / 2
            || tp > crate::arene::TASSER_AU_DELA && tp > self.poses.len() as u64 / 2
        {
            self.tasser();
        }
    }

    /// Les poses d'un lot : `(local, début du modèle, nombre de faces)`,
    /// modèles vides sautés.
    fn relever(
        &mut self,
        lot: &Lot,
        modele: &dyn Fn(StateId, StateId) -> Vec<FaceModele>,
    ) -> Vec<(u32, u32, u32)> {
        let mut out = Vec::with_capacity(lot.poses.poses.len());
        for p in &lot.poses.poses {
            // Mémoïsé sur `(état, biome)` : la géométrie teintée diffère d'un
            // biome à l'autre, et un état non teinté garde UNE entrée — le
            // mailleur écrit zéro pour son biome.
            let (debut_modele, nombre) = match self.connus.get(&(p.id, p.biome)) {
                Some(&v) => v,
                None => {
                    let f = modele(p.id, p.biome);
                    let debut = self.faces.len() as u32;
                    self.faces.extend(f);
                    let v = (debut, self.faces.len() as u32 - debut);
                    self.connus.insert((p.id, p.biome), v);
                    v
                }
            };
            if nombre == 0 {
                continue;
            }
            out.push((
                p.pos[0] as u32
                    | (p.pos[1] as u32) << 8
                    | (p.pos[2] as u32) << 16
                    | (p.voisins_opaques as u32) << 24,
                debut_modele,
                nombre,
            ));
        }
        out
    }

    /// Pose (ou repose) un lot à sa place.
    fn poser(
        &mut self,
        lot: &Lot,
        slot: u32,
        modele: &dyn Fn(StateId, StateId) -> Vec<FaceModele>,
    ) {
        let poses = self.relever(lot, modele);
        if poses.is_empty() {
            self.enlever(&lot.adresse);
            return;
        }
        let p = poses.len() as u32;
        let f: u32 = poses.iter().map(|x| x.2).sum();
        // Un terminal en plus des poses : voir `Pose::vide`.
        let (pcap, fcap) = (p + 1, f);

        let pd = match self.occupes.get(&lot.adresse).copied() {
            Some(pd) if self.blocs[&pd].pcap >= pcap && self.blocs[&pd].fcap >= fcap => pd,
            Some(_) => {
                self.enlever(&lot.adresse);
                self.allouer(pcap, fcap)
            }
            None => self.allouer(pcap, fcap),
        };
        let b = self.blocs[&pd];
        let mut rang = b.fd;
        for (k, &(local, debut_modele, nombre)) in poses.iter().enumerate() {
            self.poses[(pd + k as u32) as usize] = Pose {
                local,
                section: slot,
                debut_face: rang,
                debut_modele,
            };
            rang += nombre;
        }
        // Le terminal, puis le reste de la place : tout ce qui suit les vraies
        // poses est vide, au rang de fin des vraies faces.
        for k in p..b.pcap {
            self.poses[(pd + k) as usize] = Pose::vide(rang);
        }
        self.ecrites += b.pcap as u64;
        self.sales.push((pd, b.pcap));
        self.occupes.insert(lot.adresse, pd);
        self.blocs.insert(
            pd,
            Bloc {
                qui: Some(lot.adresse),
                ..b
            },
        );
        // Ce qui dépasse devient un trou — seulement s'il reste au moins une
        // pose à y mettre : une fenêtre sans pose ne se donnerait à personne,
        // elle reste donc dans la place, couverte par son terminal.
        if b.pcap > pcap {
            self.couper(pd, pcap, rang - b.fd);
        }
    }

    /// Coupe la place `pd` à `pcap` poses et `fcap` faces ; le reste devient
    /// un trou. Les poses du reste sont DÉJÀ vides, au rang de fin des
    /// faces gardées : la croissance tient.
    fn couper(&mut self, pd: u32, pcap: u32, fcap: u32) {
        let b = self.blocs[&pd];
        debug_assert!(pcap < b.pcap && fcap <= b.fcap);
        self.blocs.insert(pd, Bloc { pcap, fcap, ..b });
        self.ajouter_trou(Bloc {
            pd: pd + pcap,
            pcap: b.pcap - pcap,
            fd: b.fd + fcap,
            fcap: b.fcap - fcap,
            qui: None,
        });
    }

    /// Une place d'au moins `pcap` poses et `fcap` faces : un trou qui suffit,
    /// ou une place neuve au bout.
    fn allouer(&mut self, pcap: u32, fcap: u32) -> u32 {
        let trouve = self
            .trous
            .range((fcap, 0, 0)..)
            .find(|&&(_, p, _)| p >= pcap)
            .copied();
        let Some((_, _, pd)) = trouve else {
            let pd = self.poses.len() as u32;
            let fd = self.faces_a_dessiner;
            self.poses
                .resize(self.poses.len() + pcap as usize, Pose::vide(fd));
            self.faces_a_dessiner += fcap;
            self.blocs.insert(
                pd,
                Bloc {
                    pd,
                    pcap,
                    fd,
                    fcap,
                    qui: None,
                },
            );
            return pd;
        };
        let b = self.blocs[&pd];
        self.oter_trou(&b);
        if b.pcap > pcap {
            // La tête est prise ; le reste redevient un trou. Ses poses vides
            // peuvent porter un rang PLUS BAS que la fin de la tête — elles
            // venaient d'anciens trous recollés — et la somme préfixe
            // décroîtrait. On relève le PRÉFIXE fautif, et seulement lui : la
            // suite est croissante, donc on s'arrête au premier rang juste.
            let fin = b.fd + fcap;
            for i in (pd + pcap)..(pd + b.pcap) {
                let q = &mut self.poses[i as usize];
                if q.debut_face >= fin {
                    break;
                }
                q.debut_face = fin;
                self.sales.push((i, 1));
            }
            self.blocs.insert(pd, Bloc { pcap, fcap, ..b });
            // **Rangé tel quel, sans recoller.** La tête qu'on vient de prendre
            // n'est pas encore marquée occupée — `poser` le fait juste après —
            // donc le recollement la reprendrait aussitôt dans le trou. Et il
            // n'y a rien à recoller de toute façon : le trou d'origine l'était
            // déjà avec ses voisins, et un trou ne finit jamais le tableau.
            self.ranger_trou(Bloc {
                pd: pd + pcap,
                pcap: b.pcap - pcap,
                fd: fin,
                fcap: b.fcap - fcap,
                qui: None,
            });
        }
        pd
    }

    /// Retire une section : sa place devient un trou.
    fn enlever(&mut self, a: &Adresse) {
        let Some(pd) = self.occupes.remove(a) else {
            return;
        };
        let b = self.blocs[&pd];
        // Les poses deviennent VIDES en gardant leur rang : la croissance tient
        // sans rien recalculer.
        for q in &mut self.poses[pd as usize..(pd + b.pcap) as usize] {
            q.section = Pose::SANS_SECTION;
        }
        self.sales.push((pd, b.pcap));
        self.ajouter_trou(Bloc { qui: None, ..b });
    }

    /// Range un trou, recollé à ses voisins — et s'il finit le tableau, le
    /// tableau raccourcit.
    fn ajouter_trou(&mut self, mut b: Bloc) {
        // **L'ancienne entrée d'abord.** Une place libérée est encore rangée
        // sous sa première pose ; si elle se recolle au trou d'AVANT, le
        // résultat se range sous la clé de ce trou et l'ancienne entrée
        // survivrait — une place fantôme par-dessus un trou. Trouvé par le
        // vérificateur de pavage, seulement quand le voisin d'avant était
        // libre : tous les cas simples passaient.
        self.blocs.remove(&b.pd);
        // Le voisin d'avant, s'il est libre.
        if let Some((&pp, &prec)) = self.blocs.range(..b.pd).next_back() {
            if prec.qui.is_none() && prec.pd + prec.pcap == b.pd {
                self.oter_trou(&prec);
                self.blocs.remove(&pp);
                b = Bloc {
                    pd: prec.pd,
                    pcap: prec.pcap + b.pcap,
                    fd: prec.fd,
                    fcap: prec.fcap + b.fcap,
                    qui: None,
                };
            }
        }
        // Le voisin d'après, s'il est libre. Aucune pose à réécrire : ses
        // rangs sont déjà au-dessus des nôtres.
        if let Some(&suiv) = self.blocs.get(&(b.pd + b.pcap)) {
            if suiv.qui.is_none() {
                self.oter_trou(&suiv);
                self.blocs.remove(&suiv.pd);
                b.pcap += suiv.pcap;
                b.fcap += suiv.fcap;
            }
        }
        if b.pd + b.pcap == self.poses.len() as u32 {
            // Dernière place : on coupe au lieu de garder une queue de trous.
            self.blocs.remove(&b.pd);
            self.poses.truncate(b.pd as usize);
            self.faces_a_dessiner = b.fd;
            return;
        }
        self.ranger_trou(b);
    }

    /// Range un trou dans les deux index, sans rien recoller.
    fn ranger_trou(&mut self, b: Bloc) {
        debug_assert!(b.qui.is_none());
        self.blocs.insert(b.pd, b);
        self.trous.insert((b.fcap, b.pcap, b.pd));
        self.trous_poses += b.pcap as u64;
        self.trous_faces += b.fcap as u64;
    }

    /// Sort un trou de l'index des trous (pas de `blocs`).
    fn oter_trou(&mut self, b: &Bloc) {
        if self.trous.remove(&(b.fcap, b.pcap, b.pd)) {
            self.trous_poses -= b.pcap as u64;
            self.trous_faces -= b.fcap as u64;
        }
    }

    /// `(poses, faces)` dans les trous.
    fn trous_totaux(&self) -> (u64, u64) {
        (self.trous_poses, self.trous_faces)
    }

    /// **Tasse** : chaque place recopiée bout à bout, plus un seul trou. En
    /// O(scène), donc seulement quand les trous pèsent plus que ce qu'on
    /// dessine.
    fn tasser(&mut self) {
        let occupes: Vec<Bloc> = self
            .blocs
            .values()
            .filter(|b| b.qui.is_some())
            .copied()
            .collect();
        let mut poses = Vec::with_capacity(self.poses.len());
        let mut blocs = BTreeMap::new();
        let mut rang = 0u32;
        for b in occupes {
            let pd = poses.len() as u32;
            let decalage = rang as i64 - b.fd as i64;
            for q in &self.poses[b.pd as usize..(b.pd + b.pcap) as usize] {
                poses.push(Pose {
                    debut_face: (q.debut_face as i64 + decalage) as u32,
                    ..*q
                });
            }
            let nb = Bloc { pd, fd: rang, ..b };
            blocs.insert(pd, nb);
            self.occupes.insert(b.qui.expect("filtré"), pd);
            rang += b.fcap;
        }
        self.poses = poses;
        self.blocs = blocs;
        self.trous.clear();
        self.trous_poses = 0;
        self.trous_faces = 0;
        self.faces_a_dessiner = rang;
        self.sales.clear();
        self.sales.push((0, self.poses.len() as u32));
        self.tassements += 1;
    }

    /// Les places occupées : `(adresse, première pose, poses)`, dans l'ordre
    /// du tableau.
    pub fn tranches(&self) -> Vec<(Adresse, u32, u32)> {
        self.blocs
            .values()
            .filter_map(|b| b.qui.map(|a| (a, b.pd, b.pcap)))
            .collect()
    }

    /// **Ce que le shader dessine, rejoué sur le processeur.**
    ///
    /// Pour chaque face de l'appel de dessin : la pose que la dichotomie du
    /// shader lui attribue, et la face de modèle qu'elle lit — les faces qui
    /// tombent sur une pose vide sautées. Écrite comme le shader et pas comme
    /// l'arène : c'est ce qui en fait un contrôle, et pas une relecture du
    /// raisonnement qui a produit les places. Rend `(emplacement, local,
    /// rang dans `faces`)`.
    pub fn dessinees(&self) -> Vec<(u32, u32, u32)> {
        let mut out = Vec::new();
        for f in 0..self.faces_a_dessiner {
            // La dichotomie du shader, à la lettre (`modeles.wgsl`, `pose_de`).
            let (mut lo, mut hi) = (0usize, self.poses.len());
            while lo + 1 < hi {
                let mid = lo + (hi - lo) / 2;
                if self.poses[mid].debut_face <= f {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let Some(p) = self.poses.get(lo) else {
                continue;
            };
            if p.est_vide() {
                continue;
            }
            out.push((p.section, p.local, p.debut_modele + (f - p.debut_face)));
        }
        out
    }

    /// Ce que le GPU doit renvoyer : les plages de poses réécrites, et le
    /// début de la queue de `faces` qu'il n'a pas encore.
    pub fn prendre_sales(&mut self) -> (Vec<(u32, u32)>, usize) {
        let mut v = std::mem::take(&mut self.sales);
        v.sort_unstable();
        let mut plages: Vec<(u32, u32)> = Vec::with_capacity(v.len());
        for (d, n) in v {
            match plages.last_mut() {
                Some((pd, pn)) if *pd + *pn >= d => *pn = (*pn).max(d + n - *pd),
                _ => plages.push((d, n)),
            }
        }
        let fin = self.poses.len() as u32;
        plages.retain_mut(|(d, n)| {
            if *d >= fin {
                return false;
            }
            *n = (*n).min(fin - *d);
            true
        });
        let depuis = self.faces_envoyees;
        self.faces_envoyees = self.faces.len();
        (plages, depuis)
    }

    /// Poses écrites depuis la création : le coût RÉEL des remplacements.
    pub fn ecrites(&self) -> u64 {
        self.ecrites
    }

    /// **Vérifie les trois règles dont dépend la dichotomie du shader**, et
    /// le pavage des places. Panique en disant laquelle et où — une liste
    /// chaînée ou une somme préfixe se corrompt en silence, et un booléen
    /// qu'un test afficherait ne dirait pas OÙ.
    #[doc(hidden)]
    pub fn verifier(&self) {
        let nombre: HashMap<u32, u32> = self.connus.values().map(|&(d, n)| (d, n)).collect();
        let (mut pd, mut fd) = (0u32, 0u32);
        for (&k, b) in &self.blocs {
            assert_eq!(k, b.pd, "clé {k} pour une place qui commence en {}", b.pd);
            assert_eq!(
                (b.pd, b.fd),
                (pd, fd),
                "jour ou recouvrement avant la place {k}"
            );
            let poses = &self.poses[b.pd as usize..(b.pd + b.pcap) as usize];
            match b.qui {
                None => assert!(
                    poses.iter().all(Pose::est_vide),
                    "le trou {k} contient une vraie pose"
                ),
                Some(a) => {
                    assert_eq!(
                        self.occupes.get(&a),
                        Some(&k),
                        "index d'occupation faux pour {a:?}"
                    );
                    let p = poses.iter().take_while(|q| !q.est_vide()).count();
                    assert!(p < poses.len(), "la place {k} n'a pas de terminal");
                    let mut rang = b.fd;
                    for q in &poses[..p] {
                        assert_eq!(q.debut_face, rang, "somme préfixe fausse dans la place {k}");
                        rang += nombre.get(&q.debut_modele).copied().unwrap_or(0);
                    }
                    assert!(rang <= b.fd + b.fcap, "la place {k} déborde de sa fenêtre");
                    assert!(
                        poses[p..]
                            .iter()
                            .all(|q| q.est_vide() && q.debut_face >= rang),
                        "après les vraies poses de {k}, tout doit être vide et au-delà"
                    );
                }
            }
            pd += b.pcap;
            fd += b.fcap;
        }
        assert_eq!(
            pd as usize,
            self.poses.len(),
            "les places ne couvrent pas les poses"
        );
        assert_eq!(
            fd, self.faces_a_dessiner,
            "les fenêtres ne couvrent pas l'appel de dessin"
        );
        for w in self.poses.windows(2) {
            assert!(
                w[0].debut_face <= w[1].debut_face,
                "la somme préfixe décroît"
            );
        }
        let (tp, tf) = self
            .blocs
            .values()
            .filter(|b| b.qui.is_none())
            .fold((0u64, 0u64), |(p, f), b| {
                (p + b.pcap as u64, f + b.fcap as u64)
            });
        assert_eq!(
            (tp, tf),
            (self.trous_poses, self.trous_faces),
            "compte des trous faux"
        );
    }

    pub fn tassements(&self) -> u32 {
        self.tassements
    }

    /// Faces de l'appel de dessin qui tombent dans des trous.
    pub fn trous(&self) -> u64 {
        self.trous_totaux().1
    }

    /// La même, pour un appelant qui n'a pas de biomes.
    ///
    /// Bancs et tests de géométrie : leur catalogue n'est pas teinté, donc le
    /// mailleur leur écrit `biome = 0` partout et la table se mémoïse sur
    /// l'état seul, au bit près comme avant que les biomes existent. La porte
    /// est là pour le DIRE, pas pour offrir un second comportement.
    pub fn sans_biome(
        chantier: &Chantier,
        modele: &dyn Fn(StateId) -> Vec<FaceModele>,
    ) -> AreneModeles {
        AreneModeles::depuis(chantier, &|id, _| modele(id))
    }
}

/// L'habillage des six faces d'un cuboïde : couche d'atlas, teinte, uv.
///
/// Un tuple et non la structure de `tf-assets` : le rendu n'a pas à dépendre
/// d'un lecteur de packs pour savoir ce qu'est une couche de texture. C'est la
/// même raison qui garde `Formes` en trait plutôt qu'en table concrète.
pub type HabillageFaces = [(u32, [f32; 3], [f32; 4]); 6];

/// L'origine de chaque lot du chantier, dans l'ORDRE DES LOTS.
///
/// Une seule table pour les deux passes. Deux se décaleraient le jour où l'une
/// saute une section vide — et tout un pan du build se dessinerait ailleurs,
/// sans la moindre erreur. L'index d'une section EST son rang de lot ; c'est
/// la seule chose que les deux arènes ont à partager, et un test le fige.
pub fn origines(chantier: &Chantier) -> Vec<Origine> {
    // Une seule règle : celle des emplacements. Écrite deux fois, elle
    // finirait par ne plus ranger les lots dans le même ordre, et tout un pan
    // du build se dessinerait à l'origine d'un autre.
    crate::arene::Emplacements::depuis(chantier)
        .origines()
        .to_vec()
}

/// Les faces d'un état, prêtes pour le GPU.
///
/// `cuboides` et `habillage` viennent du même parcours (`table_rendu`), donc
/// le n-ième habillage va avec le n-ième cuboïde — structurellement, pas par
/// convention.
pub fn faces_de(cuboides: &[Cuboide], habillage: &[HabillageFaces]) -> Vec<FaceModele> {
    let mut out = Vec::new();
    for (c, hab) in cuboides.iter().zip(habillage) {
        for f in FACES {
            if c.faces & f.bit() == 0 {
                continue;
            }
            let (couche, teinte, uv) = hab[f.indice()];
            out.push(FaceModele {
                min: [c.min[0], c.min[1], c.min[2], 0.0],
                max: [c.max[0], c.max[1], c.max[2], 0.0],
                uv,
                face: f as u32,
                couche,
                teinte: crate::arene::en_rgba8(teinte),
                // `cull` ne porte que les faces qui DÉCLARENT `cullface`, et
                // seules celles à ras du bord peuvent être masquées : une face
                // au milieu du bloc reste visible quoi qu'il y ait à côté.
                cullable: u32::from(c.cull & f.bit() != 0 && c.au_bord(f)),
            });
        }
    }
    out
}
