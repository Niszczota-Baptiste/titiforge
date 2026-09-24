//! L'arène GPU : un seul tampon pour tous les quads.
//!
//! Un tampon par section donnerait un appel de dessin par section — 24 576 sur
//! une région. La référence à battre est 1 281 appels, mesurée sur
//! `ExeWorldEdit` ; la cible est **moins de cinq**.
//!
//! Tous les quads vivent donc dans UN tampon, et chaque section y occupe une
//! PLACE qui ne bouge pas tant qu'elle existe.
//!
//! **Des places stables, pas un tableau recopié.** Première écriture : chaque
//! remaillage rebâtissait le tableau en recopiant les tranches inchangées.
//! Juste, et en O(scène) — mesuré en vol sur du bâti (`tf-app --example vol`),
//! 19 ms par image pour la seule arène des quads, et le chiffre GRANDIT à
//! mesure que la scène se remplit. Une section remplacée réécrit maintenant sa
//! place, et rien d'autre. Ce qu'elle libère devient un TROU : des instances
//! vides (`InstanceQuad::VIDE`) que le shader rend dégénérées, donc un seul
//! appel de dessin comme avant. Les trous se réemploient au plus juste, et
//! l'arène se tasse quand ils pèsent plus que ce qu'elle dessine.
//!
//! L'origine d'une section vit dans un EMPLACEMENT stable (`Emplacements`),
//! partagé avec la passe de modèles : le champ `section` d'une instance ne
//! change donc plus quand une autre section apparaît ou disparaît — c'est ce
//! décalage qui obligeait à tout recopier.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bytemuck::{Pod, Zeroable};
use tf_mesh::{Adresse, Chantier, Lot};

/// Ce qu'une instance porte au GPU. **16 octets**, et pas un de plus.
///
/// Elle en faisait 32, et c'est la mesure qui a désigné ce champ : sur une
/// région bâtie, l'arène gloutonne pèse 132 Mo — **trois fois** la passe de
/// modèles, qu'on venait pourtant d'optimiser. Le raisonnement est celui de la
/// pose : un quad tient dans sa SECTION, il n'a aucun besoin d'une position en
/// flottants monde.
///
/// La mémoire n'est pas un confort ici : la fenêtre de résidence est plafonnée
/// en OCTETS (invariant n° 7), donc diviser l'arène par deux double ce qu'on
/// peut tenir résident.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct InstanceQuad {
    /// `x | y<<5 | z<<10 | (l−1)<<15 | (h−1)<<19 | face<<23`.
    ///
    /// Positions et tailles en **BLOCS**, locales à la section : 0..16 tient
    /// sur cinq bits, une taille de 1..16 sur quatre, la face sur trois — 26
    /// bits en tout. Un quad glouton tombe toujours sur des bords de bloc,
    /// c'est ce qui rend l'entier possible ; la passe de modèles, elle, garde
    /// ses flottants parce que 17 % des coordonnées du pack ne sont pas
    /// entières.
    pub geo: u32,
    pub couche: u32,
    /// Ce par quoi multiplier le texel, en RGBA8. **Pas une couleur** : un
    /// FACTEUR, qui vaut `0xFFFFFFFF` sur une face non teintée.
    ///
    /// Les textures teintées du jeu sont GRISES — `grass_block_top.png` vaut
    /// (147, 147, 147) — et c'est le jeu qui les multiplie par une couleur de
    /// biome. Sans ce champ, le sol de tout terrain sort blanchâtre : la
    /// texture s'affiche, simplement pas de la bonne couleur.
    pub teinte: u32,
    /// Quel EMPLACEMENT — donc quelle origine. Stable tant que la section
    /// existe, et partagé avec la passe de modèles (`Emplacements`).
    pub section: u32,
}

impl InstanceQuad {
    /// **Une instance qui ne dessine RIEN** : le trou d'une place libérée.
    ///
    /// Face 7 : aucune face n'a ce rang (elles vont de 0 à 5), et le shader
    /// rend un quad dégénéré pour tout rang au-delà de 5. Un trou coûte donc
    /// six invocations de sommet et aucun fragment, et l'arène garde UN appel
    /// de dessin — la couper en morceaux pour sauter les trous coûterait des
    /// appels, ce qui est justement ce que l'arène existe pour éviter.
    pub const VIDE: InstanceQuad = InstanceQuad {
        geo: 7 << 23,
        couche: 0,
        teinte: 0,
        section: 0,
    };

    /// Vrai pour un trou. Le critère est la FACE, comme dans le shader : deux
    /// critères différents finiraient par ne plus dire la même chose.
    #[inline]
    pub fn est_vide(&self) -> bool {
        (self.geo >> 23) & 7 > 5
    }
}

/// Empaquette la géométrie d'un quad glouton.
///
/// Les seizièmes du mailleur redeviennent des blocs. Un quad qui ne tomberait
/// pas sur un bord de bloc serait TRONQUÉ ici, silencieusement — d'où
/// l'assertion : c'est une propriété de la passe gloutonne, pas une chance.
pub fn empaqueter(min: [f32; 3], taille: [f32; 2], face: u32) -> u32 {
    let bloc = |v: f32| {
        debug_assert!(
            (0.0..=256.0).contains(&v) && (v / 16.0).fract() == 0.0,
            "un quad glouton tombe sur un bord de bloc, pas sur {v} seizièmes"
        );
        (v / 16.0) as u32
    };
    let (x, y, z) = (bloc(min[0]), bloc(min[1]), bloc(min[2]));
    let (l, h) = (bloc(taille[0]), bloc(taille[1]));
    debug_assert!(l >= 1 && h >= 1 && l <= 16 && h <= 16, "taille {l} × {h}");
    x | (y << 5) | (z << 10) | ((l - 1) << 15) | ((h - 1) << 19) | (face << 23)
}

/// L'inverse, pour les bornes et les tests. Rend
/// `(position en blocs, taille en blocs, face)`.
pub fn depaqueter(geo: u32) -> ([u32; 3], [u32; 2], u32) {
    (
        [geo & 31, (geo >> 5) & 31, (geo >> 10) & 31],
        [((geo >> 15) & 15) + 1, ((geo >> 19) & 15) + 1],
        (geo >> 23) & 7,
    )
}

/// Un facteur `0..1` par canal, empaqueté en RGBA8.
///
/// Huit bits suffisent : c'est la précision de la texture qu'il multiplie.
pub(crate) fn en_rgba8(t: [f32; 3]) -> u32 {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    c(t[0]) | (c(t[1]) << 8) | (c(t[2]) << 16) | (0xFF << 24)
}

/// Une tranche de l'arène : ce qu'une section occupe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tranche {
    pub adresse: Adresse,
    pub debut: u32,
    pub nombre: u32,
}

impl Tranche {
    pub fn est_vide(&self) -> bool {
        self.nombre == 0
    }
}

/// **Où vit l'origine de chaque section — et elle n'en bouge plus.**
///
/// C'est la pièce qui rend le remplacement local. Tant que le champ `section`
/// d'une instance était l'INDICE de son lot, une section qui apparaissait
/// décalait tous les lots suivants, et il fallait réécrire chaque instance
/// derrière elle : du O(scène) pour un geste en O(édition). Un emplacement est
/// donné à une section quand elle arrive, rendu quand elle part, et réemployé
/// par la suivante.
///
/// Partagé par les deux passes, et construit par UNE fonction
/// (`Emplacements::depuis`) : deux tables se décaleraient le jour où l'une
/// saute une section que l'autre garde, et tout un pan du build se dessinerait
/// ailleurs, sans la moindre erreur.
#[derive(Debug, Default, Clone)]
pub struct Emplacements {
    index: HashMap<Adresse, u32>,
    origines: Vec<crate::modeles::Origine>,
    libres: Vec<u32>,
    /// Emplacements réécrits depuis le dernier envoi au GPU.
    sales: Vec<u32>,
}

impl Emplacements {
    /// Un emplacement par lot, dans l'ordre du chantier : le lot `i` reçoit
    /// l'emplacement `i`. Les deux passes appellent CETTE fonction pour une
    /// construction complète, donc s'accordent par construction.
    pub fn depuis(chantier: &Chantier) -> Emplacements {
        let mut e = Emplacements::default();
        for lot in &chantier.lots {
            e.prendre(lot.adresse);
        }
        e
    }

    /// L'emplacement d'une section, s'il y en a un.
    pub fn de(&self, a: &Adresse) -> Option<u32> {
        self.index.get(a).copied()
    }

    /// Les origines, indexées par emplacement. Un emplacement libre garde
    /// l'origine de sa dernière section : aucune instance ne le désigne plus,
    /// donc elle ne se lit pas.
    pub fn origines(&self) -> &[crate::modeles::Origine] {
        &self.origines
    }

    /// Nombre de sections qui tiennent un emplacement.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Rend les emplacements réécrits depuis le dernier appel, triés et sans
    /// doublon. C'est ce que le GPU doit renvoyer, et rien d'autre.
    pub fn prendre_sales(&mut self) -> Vec<u32> {
        let mut v = std::mem::take(&mut self.sales);
        v.sort_unstable();
        v.dedup();
        v
    }

    /// L'emplacement de la section, attribué s'il n'existait pas.
    fn prendre(&mut self, a: Adresse) -> u32 {
        if let Some(&i) = self.index.get(&a) {
            return i;
        }
        let o = origine_de(a);
        let i = match self.libres.pop() {
            Some(i) => {
                self.origines[i as usize] = o;
                i
            }
            None => {
                self.origines.push(o);
                self.origines.len() as u32 - 1
            }
        };
        self.index.insert(a, i);
        self.sales.push(i);
        i
    }

    /// Rend l'emplacement de la section, s'il y en avait un.
    fn rendre(&mut self, a: &Adresse) {
        if let Some(i) = self.index.remove(a) {
            self.libres.push(i);
        }
    }
}

/// L'origine d'une section, en seizièmes. Écrite une fois : la passe de
/// modèles et celle des quads la lisent dans la même table.
fn origine_de(a: Adresse) -> crate::modeles::Origine {
    let (cx, cz, sy) = a;
    let s = crate::lignes::SEIZIEMES_PAR_BLOC;
    crate::modeles::Origine {
        position: [
            (cx * 16) as f32 * s,
            (sy as i32 * 16) as f32 * s,
            (cz * 16) as f32 * s,
            0.0,
        ],
    }
}

/// **Les plages libres d'un tableau**, rendues au plus juste.
///
/// Deux index sur les mêmes plages : par DÉBUT, pour recoller une plage
/// rendue à ses voisines ; par TAILLE, pour trouver la plus petite qui suffit.
/// Le plus-juste plutôt que le premier-venu : une cellule qui arrive ressemble
/// en taille à celle qui vient de partir, et le trou qu'elle laisse se remplit
/// sans émietter les grands.
#[derive(Debug, Default, Clone)]
pub(crate) struct Libres {
    par_debut: BTreeMap<u32, u32>,
    par_taille: BTreeSet<(u32, u32)>,
    total: u64,
}

impl Libres {
    /// Une plage de `n`, au plus juste. `None` s'il n'y en a pas d'assez
    /// grande : l'appelant ajoute alors au bout.
    pub(crate) fn prendre(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return None;
        }
        let &(taille, debut) = self.par_taille.range((n, 0)..).next()?;
        self.oter(debut, taille);
        if taille > n {
            self.inserer(debut + n, taille - n);
        }
        Some(debut)
    }

    /// Rend une plage, recollée à ses voisines libres.
    pub(crate) fn rendre(&mut self, mut debut: u32, mut n: u32) {
        if n == 0 {
            return;
        }
        if let Some((&d, &t)) = self.par_debut.range(..debut).next_back() {
            if d + t == debut {
                self.oter(d, t);
                debut = d;
                n += t;
            }
        }
        if let Some(&t) = self.par_debut.get(&(debut + n)) {
            self.oter(debut + n, t);
            n += t;
        }
        self.inserer(debut, n);
    }

    /// Si une plage libre FINIT à `fin`, la retire et rend son début : le
    /// tableau peut alors raccourcir au lieu de garder une queue de trous.
    pub(crate) fn couper_fin(&mut self, fin: u32) -> Option<u32> {
        let (&d, &t) = self.par_debut.range(..fin).next_back()?;
        if d + t != fin {
            return None;
        }
        self.oter(d, t);
        Some(d)
    }

    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    pub(crate) fn vider(&mut self) {
        *self = Libres::default();
    }

    fn inserer(&mut self, debut: u32, n: u32) {
        self.par_debut.insert(debut, n);
        self.par_taille.insert((n, debut));
        self.total += n as u64;
    }

    fn oter(&mut self, debut: u32, n: u32) {
        self.par_debut.remove(&debut);
        self.par_taille.remove(&(n, debut));
        self.total -= n as u64;
    }
}

/// À partir de quelle taille une arène se TASSE : quand ses trous pèsent plus
/// que ce qu'elle dessine, et au-delà de ce seuil. Sous le seuil, tasser ne
/// gagnerait rien de mesurable et coûterait une recopie entière.
pub const TASSER_AU_DELA: u64 = 1 << 16;

/// Les instances de tout un chantier, chaque section à sa PLACE.
#[derive(Debug, Default)]
pub struct Arene {
    /// Le tableau que le GPU dessine d'un seul appel — trous compris.
    pub instances: Vec<InstanceQuad>,
    emplacements: Emplacements,
    places: HashMap<Adresse, Tranche>,
    libres: Libres,
    /// Plages réécrites depuis le dernier envoi au GPU, `(début, nombre)`.
    sales: Vec<(u32, u32)>,
    /// Instances écrites depuis la création. **Le compteur qui prouve qu'un
    /// remplacement paie ce qu'il change**, et pas la scène : un chronomètre
    /// dirait la même chose en dépendant de la machine.
    ecrites: u64,
    tassements: u32,
}

impl Arene {
    /// Empile un chantier. `apparence` dit, pour un état et une FACE, quelle
    /// tuile d'atlas l'habille et par quoi multiplier son texel.
    ///
    /// Par face, et pas seulement par état : un modèle déclare une texture par
    /// face, et prendre celle du dessus habille les côtés d'un bloc d'herbe
    /// avec de l'herbe.
    ///
    /// Le troisième argument est le BIOME de la case. Il vaut zéro partout où
    /// le bloc n'en prend pas la couleur — la fusion gloutonne ne coupe un
    /// quad sur une frontière de biome que pour les états teintés. Un
    /// appelant qui n'a pas de biomes passe donc la même fonction qu'avant et
    /// l'ignore.
    pub fn depuis(
        chantier: &Chantier,
        apparence: &dyn Fn(
            tf_anvil::StateId,
            tf_mesh::forme::Face,
            tf_anvil::StateId,
        ) -> (u32, [f32; 3]),
    ) -> Arene {
        let mut a = Arene {
            emplacements: Emplacements::depuis(chantier),
            ..Arene::default()
        };
        for lot in &chantier.lots {
            a.poser(lot, apparence);
        }
        a
    }

    /// **Refait les sections VISÉES, et ne touche à rien d'autre.**
    ///
    /// `neufs` sont les lots que le remaillage vient de produire pour ces
    /// visées ; une visée sans lot neuf est une section qui n'existe plus, et
    /// elle part — c'est le piège « un chunk qui se VIDE ne figure plus dans
    /// la liste des chunks », payé dans `ExeWorldEdit`.
    ///
    /// L'arène ne voit JAMAIS le chantier entier ici, et c'est le point : le
    /// coût est celui des sections visées et de leurs instances, quelle que
    /// soit la taille de la scène. `ecrites` le compte.
    ///
    /// **C'est aussi elle qui attribue et rend les emplacements**, pour les
    /// deux passes : la passe de modèles se remplace ENSUITE et lit ceux-ci.
    pub fn remplacer(
        &mut self,
        visees: &[Adresse],
        neufs: &[Lot],
        apparence: &dyn Fn(
            tf_anvil::StateId,
            tf_mesh::forme::Face,
            tf_anvil::StateId,
        ) -> (u32, [f32; 3]),
    ) {
        let reviennent: std::collections::HashSet<Adresse> =
            neufs.iter().map(|l| l.adresse).collect();
        for a in visees {
            if !reviennent.contains(a) {
                self.enlever(a);
            }
        }
        for lot in neufs {
            self.poser(lot, apparence);
        }
        if self.libres.total() > TASSER_AU_DELA
            && self.libres.total() > self.instances.len() as u64 / 2
        {
            self.tasser();
        }
    }

    /// Pose (ou repose) le lot à sa place.
    fn poser(
        &mut self,
        lot: &Lot,
        apparence: &dyn Fn(
            tf_anvil::StateId,
            tf_mesh::forme::Face,
            tf_anvil::StateId,
        ) -> (u32, [f32; 3]),
    ) {
        let slot = self.emplacements.prendre(lot.adresse);
        let n = lot.quads.quads.len() as u32;
        // La place d'avant : réemployée si elle suffit, sinon rendue.
        let debut = match self.places.remove(&lot.adresse) {
            Some(t) if t.nombre >= n => {
                self.liberer(t.debut + n, t.nombre - n);
                t.debut
            }
            Some(t) => {
                self.liberer(t.debut, t.nombre);
                self.allouer(n)
            }
            None => self.allouer(n),
        };
        for (k, q) in lot.quads.quads.iter().enumerate() {
            let (couche, teinte) = apparence(q.id, q.face, q.biome);
            self.instances[debut as usize + k] = InstanceQuad {
                geo: empaqueter(q.min, q.taille, q.face as u32),
                couche,
                teinte: en_rgba8(teinte),
                // Le quad est LOCAL à sa section : sans l'origine, tout le
                // monde se dessinerait empilé sur la section zéro.
                section: slot,
            };
        }
        self.ecrites += n as u64;
        if n > 0 {
            self.sales.push((debut, n));
            self.places.insert(
                lot.adresse,
                Tranche {
                    adresse: lot.adresse,
                    debut,
                    nombre: n,
                },
            );
        }
    }

    /// Retire une section : sa place devient un trou, son emplacement se rend.
    fn enlever(&mut self, a: &Adresse) {
        if let Some(t) = self.places.remove(a) {
            self.liberer(t.debut, t.nombre);
        }
        self.emplacements.rendre(a);
    }

    /// Une plage de `n` instances, prise dans les trous ou ajoutée au bout.
    fn allouer(&mut self, n: u32) -> u32 {
        if let Some(d) = self.libres.prendre(n) {
            return d;
        }
        let d = self.instances.len() as u32;
        self.instances
            .resize(self.instances.len() + n as usize, InstanceQuad::VIDE);
        d
    }

    /// Rend une plage : elle devient un trou — et si elle finit le tableau,
    /// le tableau raccourcit.
    fn liberer(&mut self, debut: u32, n: u32) {
        if n == 0 {
            return;
        }
        self.instances[debut as usize..(debut + n) as usize].fill(InstanceQuad::VIDE);
        self.sales.push((debut, n));
        self.libres.rendre(debut, n);
        if let Some(d) = self.libres.couper_fin(self.instances.len() as u32) {
            self.instances.truncate(d as usize);
        }
    }

    /// **Tasse l'arène** : chaque section recopiée bout à bout, plus un seul
    /// trou. En O(scène), et c'est pourquoi il n'a lieu que quand les trous
    /// pèsent plus que ce qu'on dessine — donc rarement, puisqu'une cellule
    /// qui arrive réemploie en général le trou de celle qui vient de partir.
    fn tasser(&mut self) {
        let mut places: Vec<Tranche> = self.places.values().copied().collect();
        places.sort_unstable_by_key(|t| t.debut);
        let mut neuves = Vec::with_capacity(self.instances.len() - self.libres.total() as usize);
        for t in &mut places {
            let d = neuves.len() as u32;
            neuves.extend_from_slice(
                &self.instances[t.debut as usize..(t.debut + t.nombre) as usize],
            );
            t.debut = d;
            self.places.insert(t.adresse, *t);
        }
        self.instances = neuves;
        self.libres.vider();
        self.sales.clear();
        self.sales.push((0, self.instances.len() as u32));
        self.tassements += 1;
    }

    /// Les emplacements — donc les origines — que les deux passes partagent.
    pub fn emplacements(&self) -> &Emplacements {
        &self.emplacements
    }

    /// Les origines, indexées par emplacement.
    pub fn origines(&self) -> &[crate::modeles::Origine] {
        self.emplacements.origines()
    }

    /// Les tranches occupées, dans l'ordre du tableau.
    pub fn tranches(&self) -> Vec<Tranche> {
        let mut v: Vec<Tranche> = self.places.values().copied().collect();
        v.sort_unstable_by_key(|t| t.debut);
        v
    }

    /// Les instances DESSINÉES — les trous sautés.
    pub fn visibles(&self) -> impl Iterator<Item = &InstanceQuad> + '_ {
        self.instances.iter().filter(|i| !i.est_vide())
    }

    /// Ce que le GPU doit renvoyer depuis le dernier appel : les plages
    /// d'instances réécrites (triées, recollées) et les emplacements
    /// réécrits.
    pub fn prendre_sales(&mut self) -> (Vec<(u32, u32)>, Vec<u32>) {
        let mut v = std::mem::take(&mut self.sales);
        v.sort_unstable();
        let mut plages: Vec<(u32, u32)> = Vec::with_capacity(v.len());
        for (d, n) in v {
            match plages.last_mut() {
                Some((pd, pn)) if *pd + *pn >= d => {
                    *pn = (*pn).max(d + n - *pd);
                }
                _ => plages.push((d, n)),
            }
        }
        // Une plage peut viser au-delà d'un tableau qui a raccourci depuis.
        let fin = self.instances.len() as u32;
        plages.retain_mut(|(d, n)| {
            if *d >= fin {
                return false;
            }
            *n = (*n).min(fin - *d);
            true
        });
        (plages, self.emplacements.prendre_sales())
    }

    /// Instances écrites depuis la création : le coût RÉEL des remplacements.
    pub fn ecrites(&self) -> u64 {
        self.ecrites
    }

    /// Combien de fois l'arène s'est tassée.
    pub fn tassements(&self) -> u32 {
        self.tassements
    }

    /// Instances au tableau, trous compris : ce que l'appel de dessin couvre.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Les instances dans les trous — ce que l'appel de dessin paie pour
    /// rien.
    pub fn trous(&self) -> u64 {
        self.libres.total()
    }

    pub fn octets(&self) -> usize {
        std::mem::size_of_val(&self.instances[..])
    }

    /// Bornes du contenu, en blocs monde. `None` si l'arène est vide.
    pub fn bornes(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let origines = self.origines();
        for i in self.visibles() {
            let (p, _, _) = depaqueter(i.geo);
            let o = match origines.get(i.section as usize) {
                Some(o) => o.position,
                None => continue,
            };
            for k in 0..3 {
                let v = o[k] / 16.0 + p[k] as f32;
                min[k] = min[k].min(v);
                max[k] = max[k].max(v);
            }
        }
        if min[0] > max[0] {
            return None;
        }
        // Un quad s'étend au-delà de son coin : sans ça, la boîte serait trop
        // petite d'un bloc sur chaque axe et la caméra couperait le bord.
        for m in max.iter_mut() {
            *m += 1.0;
        }
        Some((min, max))
    }
}
