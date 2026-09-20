//! La répartition à TROIS ÉTAGES — le cœur de la promesse de performance.
//!
//! Une section Anvil porte une palette et 4 096 indices. Presque toutes les
//! opérations WorldEdit s'expriment sur la palette seule. Le chemin rapide
//! n'est donc pas « itérer plus vite », c'est **ne pas itérer** :
//!
//! | Étage | Quand | Coût |
//! |---|---|---|
//! | **Rien** | le masque n'accepte aucune entrée de la palette | O(palette) |
//! | **Section** | section couverte, et le résultat est UNIFORME | O(1) |
//! | **Palette** | section couverte, correspondance état → état | O(palette) |
//! | **Bloc** | bordure de sélection, ou motif qui dépend de la position | O(blocs) |
//!
//! Mesuré dans le prototype sur 100 663 296 blocs : **0,26 ms par palette
//! contre 26,8 ms par bloc**, et 11 718 ms pour le moteur JS.
//!
//! ## Le piège qui a failli coûter tout le gain
//!
//! L'étage palette ne **dédoublonne pas**. Sur du vrai terrain, une section qui
//! contient de la pierre contient presque toujours de la terre : remplacer
//! l'une par l'autre fusionnerait deux entrées, ce qui obligerait à remapper
//! les 4 096 indices. Mesuré, le chemin rapide ne se déclenchait sur AUCUNE
//! section et l'étage palette retombait au niveau de l'étage bloc.
//!
//! Or Anvil n'interdit pas deux entrées identiques : le jeu lit
//! `palette[indice]` et obtient un état valide dans les deux cas. En laissant
//! le doublon, la longueur ne bouge pas, donc `bits` non plus, donc aucun
//! indice n'est touché. Le compactage se fait à l'écriture — jamais ici.
//!
//! **Corollaire dangereux :** la palette peut donc contenir le même état deux
//! fois. Tout ce qui y cherche un état doit chercher TOUTES les occurrences,
//! jamais la première.

use tf_anvil::entites::Entite;
use tf_anvil::section::VOL;
use tf_anvil::{Section, StateId};
use tf_world::coords::{BBox, ChunkPos, LocalBox, SectionPos};

use crate::forme::{Couverture, Forme};
use crate::masque::Masque;
use crate::motif::Motif;

/// Par où une opération est passée sur une section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etage {
    /// Rien à faire : le masque n'accepte aucune entrée de la palette.
    Rien,
    /// O(1) : la section devient homogène.
    Section,
    /// O(palette) : on réécrit des entrées, pas des indices.
    Palette,
    /// O(blocs) : il faut lire chaque case.
    Bloc,
}

/// Ce qu'une opération a fait à une section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rapport {
    pub etage: Etage,
    /// Combien de blocs ont VRAIMENT changé d'état.
    ///
    /// `None` quand on ne l'a pas compté — compter à l'étage palette coûte
    /// exactement le parcours qu'on vient d'éviter, et c'est un choix de
    /// l'appelant, pas une fatalité. Voir `Plan::compter`.
    pub blocs: Option<u64>,
    /// Ce que l'opération a pu écrire, en coordonnées MONDE.
    ///
    /// C'est une borne SUPÉRIEURE honnête : l'instantané d'annulation et le
    /// remaillage s'y fient, et `tf-anvil` compare les octets, donc une
    /// opération qui n'a rien changé n'écrit rien même si ses bornes sont
    /// larges. Trop étroites, en revanche, et on perdrait de quoi annuler.
    pub bornes: Option<BBox>,
}

impl Rapport {
    pub const RIEN: Rapport = Rapport {
        etage: Etage::Rien,
        blocs: Some(0),
        bornes: None,
    };
}

/// Une opération compilée : un masque, un motif, une graine.
///
/// C'est une DONNÉE, pas une fermeture — c'est la couture qui permettra à un
/// greffon de décrire une opération sans que le cœur perde le droit de choisir
/// son étage.
#[derive(Debug, Clone)]
pub struct Plan {
    pub masque: Masque,
    pub motif: Motif,
    pub seed: u64,
    /// Compter les blocs modifiés exactement.
    ///
    /// **Coûteux à l'étage palette**, où c'est le parcours qu'on vient
    /// d'économiser. Laissé au choix de l'appelant : une interface qui affiche
    /// « 12 345 blocs » le veut, un script qui enchaîne vingt opérations non.
    pub compter: bool,
    /// Le VOLUME visé à l'intérieur de la sélection.
    ///
    /// `Forme::Boite` — le défaut — ne coûte rien : le test par bloc n'est
    /// même pas branché, et une section entièrement couverte garde son étage
    /// palette. Une sphère, elle, répond par SECTION avant de répondre par
    /// case : c'est le même chemin rapide que le masque, sur l'autre axe.
    pub forme: Forme,
}

/// Ce qu'une opération sait faire d'une section.
///
/// Un `Plan` en est une, et c'est la seule qui sache choisir un étage : masque
/// et motif sont des données, donc le cœur peut décider de NE PAS itérer. Mais
/// toutes les opérations ne s'expriment pas ainsi — un collage lit un extrait,
/// une déformation lit ses voisins — et celles-là ont quand même besoin du
/// staging, du journal, du recollement par plages et de la parallélisation par
/// chunk.
///
/// D'où ce trait : `tf-ops/src/edition.rs` est la jonction, et elle doit être
/// écrite une fois. Le coût de la répartition dynamique est UNE indirection
/// par section — 24 576 sur une région pleine, contre 1,38 ms de travail.
///
/// `Sync` parce que la chaîne tourne sur tous les cœurs, et que c'est cette
/// parallélisation qui vaut × 3,65.
pub trait Operation: Sync {
    /// Applique l'opération à une section, et dit par quel étage elle est
    /// passée. `sel` et `pos` sont en coordonnées MONDE.
    fn appliquer(&self, section: &mut Section, sel: &BBox, pos: SectionPos) -> Rapport;

    /// L'opération compte-t-elle les blocs modifiés ?
    ///
    /// Compter coûte × 21 à l'étage palette : c'est un choix de l'appelant,
    /// jamais un service rendu d'office.
    fn compte(&self) -> bool;

    /// Les block entities que l'opération POSE dans ce chunk, en coordonnées
    /// MONDE. Chacune prend la place de celle qui occupait sa case.
    ///
    /// **Le RETRAIT n'est pas ici**, et c'est délibéré. Une entité dont la
    /// case a changé d'état part avec son bloc : c'est vrai de toute opération
    /// qui écrit, donc `edition.rs` le fait une fois pour toutes plutôt que de
    /// le confier à chaque implémentation — où il finirait par manquer dans la
    /// seule qui compte. Ce qu'une opération sait, et qu'elle seule sait,
    /// c'est ce qu'elle APPORTE.
    ///
    /// Le défaut — rien — est le bon pour tout ce qui ne déplace pas de blocs :
    /// `//set`, `//replace`, un lissage. Ces opérations écrasent des coffres,
    /// elles n'en posent pas.
    fn entites_posees(&self, chunk: ChunkPos) -> Vec<Entite> {
        let _ = chunk;
        Vec::new()
    }
}

impl Operation for Plan {
    fn appliquer(&self, section: &mut Section, sel: &BBox, pos: SectionPos) -> Rapport {
        Plan::appliquer(self, section, sel, pos)
    }

    fn compte(&self) -> bool {
        self.compter
    }
}

impl Plan {
    pub fn nouveau(masque: Masque, motif: Motif) -> Plan {
        Plan {
            masque,
            motif,
            seed: 0,
            compter: false,
            forme: Forme::Boite,
        }
    }

    /// Restreint le plan à une forme.
    pub fn dans(mut self, forme: Forme) -> Plan {
        self.forme = forme;
        self
    }

    /// La sélection à passer à `appliquer` : celle qu'on demande, resserrée
    /// sur la forme.
    ///
    /// **Une opération ne paie que sa PORTÉE.** Une sphère de rayon 20 posée
    /// dans une sélection « tout le build » ne doit pas faire parcourir le
    /// build — c'est le piège `warmup(extent)` d'`ExeWorldEdit`, qui faisait
    /// payer 5,2 s pour soixante-deux blocs.
    pub fn portee(&self, sel: &BBox) -> BBox {
        match self.forme.bornes() {
            Some(b) => sel.intersection(&b).unwrap_or(*sel),
            None => *sel,
        }
    }

    pub fn avec_seed(mut self, seed: u64) -> Plan {
        self.seed = seed;
        self
    }

    pub fn en_comptant(mut self) -> Plan {
        self.compter = true;
        self
    }

    /// L'étage que cette opération prendra sur cette section — sans rien
    /// modifier.
    ///
    /// Publique parce qu'un plan doit pouvoir s'EXPLIQUER : c'est ce qui permet
    /// à un bench de vérifier que le chemin rapide s'est vraiment déclenché, au
    /// lieu de le supposer. C'est exactement l'erreur que le prototype a faite
    /// une fois, et la mesure a dit « 0 sections rapides sur 9 216 ».
    pub fn etage(&self, section: &Section, sel: &BBox, pos: SectionPos) -> Etage {
        self.etage_avec(section, sel, pos, self.forme.couverture(pos))
    }

    /// Le même, la couverture déjà connue.
    ///
    /// `appliquer` la calcule UNE fois et la passe ici puis à l'étage bloc :
    /// la demander deux fois doublait le seul coût que les formes ajoutent
    /// aux opérations qui n'en ont pas.
    fn etage_avec(
        &self,
        section: &Section,
        sel: &BBox,
        pos: SectionPos,
        couverture: Couverture,
    ) -> Etage {
        if self.motif.est_muet() || section.palette.is_empty() {
            return Etage::Rien;
        }
        if self.masque.n_accepte_rien(&section.palette) {
            return Etage::Rien;
        }
        // La forme répond pour les 4 096 cases d'un coup. « Dehors » est le
        // cas le plus fréquent sur une sphère — 48 % de sa boîte englobante —
        // et il évite jusqu'au parcours.
        if couverture == Couverture::Dehors {
            return Etage::Rien;
        }
        if !sel.covers_section(pos) || couverture == Couverture::Partielle {
            return Etage::Bloc;
        }
        match self.motif.uniforme() {
            None => Etage::Bloc,
            Some(_) if self.masque.accepte_toute(&section.palette) => Etage::Section,
            Some(_) => Etage::Palette,
        }
    }

    /// Applique l'opération à une section.
    pub fn appliquer(&self, section: &mut Section, sel: &BBox, pos: SectionPos) -> Rapport {
        let couverture = self.forme.couverture(pos);
        let etage = self.etage_avec(section, sel, pos, couverture);
        match etage {
            Etage::Rien => Rapport::RIEN,
            Etage::Section => {
                let cible = self.motif.uniforme().expect("l'étage section l'exige");
                let avant = self.compter.then(|| section.count_of(cible) as u64);
                section.set_uniform(cible);
                Rapport {
                    etage,
                    blocs: avant.map(|deja| VOL as u64 - deja),
                    bornes: bornes_de(sel, pos, LocalBox::PLEINE),
                }
            }
            Etage::Palette => {
                let cible = self.motif.uniforme().expect("l'étage palette l'exige");
                let table = self.masque.table(&section.palette);
                // **Sans dédoublonnage** : on écrase l'entrée sur place. La
                // longueur ne bouge pas, donc `bits` non plus, donc aucun des
                // 4 096 indices n'est touché.
                let mut touchees = false;
                for (i, e) in section.palette.iter_mut().enumerate() {
                    if table[i] && *e != cible {
                        *e = cible;
                        touchees = true;
                    }
                }
                let blocs = self.compter.then(|| {
                    if !touchees {
                        return 0;
                    }
                    // Le coût qu'on vient d'éviter, payé sciemment.
                    let idx = section.unpack();
                    idx.iter()
                        .filter(|&&v| table.get(v as usize).copied().unwrap_or(false))
                        .count() as u64
                });
                Rapport {
                    etage,
                    blocs,
                    bornes: touchees
                        .then(|| bornes_de(sel, pos, LocalBox::PLEINE))
                        .flatten(),
                }
            }
            Etage::Bloc => {
                let zone = match sel.clip_to_section(pos) {
                    Some(z) => z,
                    None => return Rapport::RIEN,
                };
                // Le test par bloc n'est branché que si la forme hésite :
                // c'est un booléen INVARIANT de la boucle, donc un branchement
                // que le processeur prédit à coup sûr, et rien du tout pour
                // `Forme::Boite`.
                let blocs =
                    self.etage_bloc(section, &zone, pos, couverture == Couverture::Partielle);
                Rapport {
                    etage,
                    blocs: Some(blocs),
                    bornes: (blocs > 0).then(|| bornes_de(sel, pos, zone)).flatten(),
                }
            }
        }
    }

    /// Le seul chemin qui lit chaque case. Tout le reste existe pour l'éviter.
    fn etage_bloc(
        &self,
        section: &mut Section,
        zone: &LocalBox,
        pos: SectionPos,
        borde: bool,
    ) -> u64 {
        let table = self.masque.table(&section.palette);
        // **Dépacker AVANT de toucher à la palette.** `unpack` la consulte pour
        // savoir si la section est homogène : la lui retirer d'abord la faisait
        // répondre « homogène », donc rendre 4 096 zéros, donc écraser toute la
        // section avec sa première entrée. Un mur entier changé de bloc, sans
        // la moindre erreur — attrapé par le croisement avec le chemin lent, et
        // par rien d'autre.
        let mut idx = section.unpack();

        // Les états du motif entrent dans la palette AVANT la boucle : ajouter
        // une entrée en plein parcours ferait grandir `bits` au milieu, et le
        // `repack` final travaillerait sur des indices de deux largeurs.
        let mut palette = std::mem::take(&mut section.palette);
        let avant_palette = palette.len();
        let cibles: Vec<u16> = self
            .motif
            .etats()
            .into_iter()
            .map(|id| indice_ou_ajoute(&mut palette, id))
            .collect();

        // Le tirage se COMPILE une fois : hors de la boucle il ne coûte rien,
        // dedans il coûtait 1,88 s par région.
        let tirage = self.motif.tirage();
        let base = [pos.x * 16, pos.y * 16, pos.z * 16];
        let mut n = 0u64;
        for y in zone.y0..=zone.y1 {
            for z in zone.z0..=zone.z1 {
                for x in zone.x0..=zone.x1 {
                    // La forme, quand elle hésite. `borde` est invariant de la
                    // boucle : pour `Forme::Boite` c'est du code mort, et pour
                    // une section entièrement dedans aussi.
                    if borde
                        && !self.forme.contient(
                            base[0] + x as i32,
                            base[1] + y as i32,
                            base[2] + z as i32,
                        )
                    {
                        continue;
                    }
                    let i = (y << 8) | (z << 4) | x;
                    // **Un indice que la palette ne contient pas ne fait pas
                    // paniquer.** `bits` se DÉDUIT de la longueur de palette :
                    // deux entrées se lisent sur quatre bits, donc seize
                    // valeurs sont représentables pour deux valides. Un `.mca`
                    // corrompu, tronqué ou forgé en porte, et un éditeur qui
                    // meurt dessus est un éditeur qui meurt sur la sauvegarde
                    // de quelqu'un. On ne touche pas à ce qu'on ne comprend
                    // pas : la case reste telle quelle, et comme `repack_brut`
                    // réémet les indices tels qu'on les lui donne, elle ressort
                    // à l'identique.
                    let Some(&pris) = table.get(idx[i] as usize) else {
                        continue;
                    };
                    if !pris {
                        continue;
                    }
                    let Some(k) = tirage.indice(
                        base[0] + x as i32,
                        base[1] + y as i32,
                        base[2] + z as i32,
                        self.seed,
                    ) else {
                        continue;
                    };
                    let neuf = cibles[k];
                    // On compare les ÉTATS, pas les indices : la palette peut
                    // porter le même état deux fois, et compter un
                    // « changement » qui n'en est pas remplirait le journal
                    // d'entrées vides.
                    if palette[idx[i] as usize] != palette[neuf as usize] {
                        n += 1;
                    }
                    idx[i] = neuf;
                }
            }
        }
        if n == 0 {
            // **Rien n'a changé : on ne repacke pas.** Le repack coûte le même
            // parcours que le dépack, et sur une sélection bordée d'un bloc,
            // beaucoup de sections sont visitées pour rien. Il faut aussi
            // rendre la palette telle qu'on l'a trouvée : les entrées ajoutées
            // pour le motif ne sont référencées par aucun indice, et les
            // laisser ferait grandir `bits` à la prochaine écriture — une
            // section réécrite plus large sans qu'un seul bloc ait bougé.
            palette.truncate(avant_palette);
            section.palette = palette;
            return 0;
        }
        section.palette = palette;
        section.repack(&idx);
        n
    }
}

/// L'emprise MONDE d'une zone locale de section, intersectée avec la sélection.
fn bornes_de(sel: &BBox, pos: SectionPos, zone: LocalBox) -> Option<BBox> {
    let base = [pos.x * 16, pos.y * 16, pos.z * 16];
    let boite = BBox::new(
        tf_world::coords::BlockPos {
            x: base[0] + zone.x0 as i32,
            y: base[1] + zone.y0 as i32,
            z: base[2] + zone.z0 as i32,
        },
        tf_world::coords::BlockPos {
            x: base[0] + zone.x1 as i32,
            y: base[1] + zone.y1 as i32,
            z: base[2] + zone.z1 as i32,
        },
    );
    sel.intersection(&boite)
}

/// L'indice de cet état dans la palette, en l'ajoutant s'il n'y est pas.
///
/// On cherche la PREMIÈRE occurrence, et c'est correct ici : on veut un indice
/// qui désigne cet état, n'importe lequel fait l'affaire. Ce qui serait faux,
/// c'est de s'en servir pour REMPLACER — là il faut toutes les occurrences.
fn indice_ou_ajoute(palette: &mut Vec<StateId>, id: StateId) -> u16 {
    match palette.iter().position(|&e| e == id) {
        Some(i) => i as u16,
        None => {
            palette.push(id);
            (palette.len() - 1) as u16
        }
    }
}
