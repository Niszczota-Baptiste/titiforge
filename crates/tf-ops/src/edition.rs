//! De l'opération au FICHIER : staging, splice, journal.
//!
//! C'est la jonction, et c'est l'endroit le plus dangereux du dépôt. Chaque
//! pièce est testée de son côté — le plan, le splice, le journal, la copie de
//! travail — et `ExeWorldEdit` a payé cher la leçon que ça ne suffit pas :
//! deux moitiés justes dont la JONCTION ne l'est pas produisent un résultat
//! parfaitement plausible et faux (là-bas, des hauteurs en blocs passées à une
//! fonction qui attendait un rapport 0..1 ; toute cellule non nulle devenait 1,
//! et le relief sortait plat).
//!
//! Les invariants que cette fonction tient :
//!
//! 1. **On ne touche jamais au fichier source.** Tout passe par le staging.
//! 2. **Un chunk non modifié est réémis octet pour octet** — on ne le
//!    ré-encode pas, on ne le décompresse même pas si la sélection ne le
//!    touche pas.
//! 3. **Un chunk modifié n'est pas ré-encodé non plus** : on remplace les
//!    seules PLAGES d'octets des sections qu'on a touchées. Heightmaps,
//!    structures, données de mods : le lecteur n'y touche pas, donc il ne peut
//!    pas les abîmer.
//! 4. **Les deux sens de l'annulation sont enregistrés**, parce que le sens
//!    « refaire » ne se déduit pas du sens « annuler » une fois l'annulation
//!    faite.
//! 5. **Une charge déportée (`.mcc`) est résolue avant lecture.** Un chunk
//!    déporté dont on oublierait la charge serait vu comme un chunk VIDE, et
//!    l'opération l'écraserait.

use std::borrow::Cow;

use tf_anvil::chunk::{
    biome_edits, decode_biomes, decode_section, scan, section_edits, splice, EncodeError,
};
use tf_anvil::codec::{deflate_level, inflate, CodecError};
use tf_anvil::entites::Entite;
use tf_anvil::region::{
    external_file_name, read, write, Compression, RawChunk, ReadError, Region, WriteError,
};
use tf_anvil::{edition_entites, Interner, StateId};
use tf_world::coords::{BBox, BlockPos, ChunkPos, RegionPos, SectionPos};
use tf_world::journal::{ChunkPatch, Cible, Correction, Genre, Journal};
use tf_world::source::{Dimension, Folder, RegionSource, SourceError};
use tf_world::staging::{RegionStore, Staging};

use crate::colonnes::{Colonnes, Portee};
use crate::plan::{Etage, Operation};
use crate::presse::Presse;

/// Ce qu'une opération a fait à une région.
#[derive(Debug, Default)]
pub struct RapportRegion {
    /// Les correctifs à pousser dans le journal, un par chunk modifié.
    pub patches: Vec<ChunkPatch>,
    /// Combien de sections sont passées par chaque étage, dans l'ordre
    /// `rien`, `section`, `palette`, `bloc`.
    ///
    /// **Une opération de portée `Colonne` compte des CHUNKS, pas des
    /// sections** : elle décide pour le chunk entier d'un seul mouvement, et
    /// lui inventer un verdict par section dirait quelque chose qu'elle n'a
    /// pas calculé. Le total renseigne donc sur des unités différentes selon
    /// la portée — mieux vaut le dire que de faire croire à une comparaison.
    ///
    /// Public et rendu d'office : le prototype a annoncé une fois un chemin
    /// rapide que la mesure a démenti. Un rapport qui ne dit pas par où c'est
    /// passé ne permet pas de le vérifier.
    pub etages: [usize; 4],
    /// Blocs modifiés, si le plan comptait.
    pub blocs: Option<u64>,
    /// Ce que l'opération a écrit, en coordonnées monde.
    pub bornes: Option<BBox>,
    /// Block entities posées, et retirées parce que leur bloc a disparu.
    ///
    /// Rendues d'office, contrairement au compte de blocs : elles sont
    /// quelques dizaines par chunk, pas cent millions, et c'est le seul
    /// endroit où l'on peut voir qu'un coffre a été effacé. Le taire ferait
    /// de la perte de contenu un événement silencieux — exactement ce que
    /// le piège d'`ExeWorldEdit` reproche au format.
    pub entites_posees: u64,
    pub entites_retirees: u64,
    /// Entités (celles du dossier `entities/` — cadres, tableaux, bêtes)
    /// posées, et retirées de leur ancienne place par un déplacement.
    pub mobiles_poses: u64,
    pub mobiles_retires: u64,
    /// Entités qu'on n'a PAS pu poser, et pourquoi : pas de terrain à
    /// l'arrivée (un collage n'engendre pas de chunk), ou un chunk d'une autre
    /// version du jeu. Rendues d'office, comme les block entities : une
    /// entité laissée derrière ne doit pas être un événement silencieux.
    pub mobiles_sans_terrain: u64,
    pub mobiles_autre_version: u64,
    /// Sections dont les BIOMES ont changé.
    ///
    /// Compté en sections et pas en blocs, parce qu'un biome ne se pose pas
    /// au bloc : sa grille est de 4 × 4 × 4, et annoncer des blocs laisserait
    /// croire à une précision que le format n'a pas.
    pub biomes: u64,
}

impl RapportRegion {
    pub fn est_vide(&self) -> bool {
        self.patches.is_empty()
    }

    /// Le genre d'entrée de journal qui correspond à ce rapport.
    ///
    /// **La jonction, écrite UNE fois.** Sans elle, chaque hôte — la ligne de
    /// commande, la coque, un greffon — recompose à la main les correctifs,
    /// le nom de l'opération et les bornes ; trois occasions de se tromper, et
    /// l'une d'elles est un piège que ce dépôt a déjà payé (l'ordre des
    /// correctifs, qui ne se voit que sur une opération à plusieurs passes).
    /// Une jonction qu'on laisse à l'appelant est une jonction que personne ne
    /// teste.
    ///
    /// `params` porte de quoi REJOUER l'opération, pas seulement la défaire.
    /// Le journal ne les interprète pas ; c'est la couture des composants, et
    /// `Vec::new()` reste licite pour une opération qui ne se déclare pas
    /// rejouable.
    pub fn genre(&self, op: &str, params: Vec<u8>) -> Genre {
        Genre::Operation {
            op: op.to_string(),
            params,
            // Ce que l'opération a VRAIMENT écrit — pas la sélection. C'est
            // l'invariant n° 8, et c'est ce dont l'invalidation d'un document
            // et le remaillage incrémental se serviront.
            bounds: self.bornes,
            corrections: self
                .patches
                .iter()
                .cloned()
                .map(Correction::Chunk)
                .collect(),
        }
    }

    /// Pousse ce rapport dans un journal, en UNE entrée.
    ///
    /// Une seule, quel que soit le nombre de chunks touchés et le nombre de
    /// passes qu'une opération composée a faites : un `Ctrl+Z` défait le
    /// déplacement entier, pas son dernier tiers.
    ///
    /// Un rapport vide ne pousse RIEN et rend `false`. Une entrée sans
    /// correctif serait une case de plus dans la pile d'annulation qui ne
    /// défait rien — et l'utilisateur appuierait deux fois sur Ctrl+Z sans
    /// voir quoi que ce soit bouger.
    pub fn journaliser(
        &self,
        journal: &mut Journal,
        label: &str,
        op: &str,
        params: Vec<u8>,
        horodatage: i64,
    ) -> bool {
        if self.est_vide() {
            return false;
        }
        journal.pousser(label, horodatage, self.genre(op, params));
        true
    }

    /// Absorbe le rapport d'une autre passe.
    ///
    /// Une opération composée (`//move`, `//stack`) en fait plusieurs mais ne
    /// doit produire qu'UNE entrée de journal : un seul `Ctrl+Z` défait le
    /// déplacement entier, pas son dernier tiers. L'ordre des correctifs est
    /// celui des passes — et c'est pour ça que l'annulation les rejoue à
    /// l'envers (`Entree::a_annuler`).
    pub fn absorber(&mut self, autre: RapportRegion) {
        self.patches.extend(autre.patches);
        for (a, b) in self.etages.iter_mut().zip(autre.etages) {
            *a += b;
        }
        self.blocs = match (self.blocs, autre.blocs) {
            (Some(a), Some(b)) => Some(a + b),
            (x, None) | (None, x) => x,
        };
        self.bornes = unir(self.bornes, autre.bornes);
        self.entites_posees += autre.entites_posees;
        self.entites_retirees += autre.entites_retirees;
        self.mobiles_poses += autre.mobiles_poses;
        self.mobiles_retires += autre.mobiles_retires;
        self.mobiles_sans_terrain += autre.mobiles_sans_terrain;
        self.mobiles_autre_version += autre.mobiles_autre_version;
        self.biomes += autre.biomes;
    }
}

#[derive(Debug)]
pub enum Erreur {
    Source(SourceError),
    Lecture(ReadError),
    Ecriture(WriteError),
    Codec(CodecError),
    /// Le NBT du chunk est tronqué ou mal formé.
    Nbt(tf_nbt::Trunc),
    Encode(EncodeError),
    Splice(tf_anvil::chunk::SpliceError),
    /// La sélection est trop grosse pour être MATÉRIALISÉE.
    ///
    /// Presque tout le moteur travaille sur des sections packées et ne paie
    /// que sa portée ; trois opérations font exception et demandent une case
    /// par bloc en mémoire — `//copy`, `//paste` et `//hollow`. Sur une
    /// sélection d'utilisateur, « tout le build » se compte vite en centaines
    /// de millions de cases, et `vec![]` n'échoue pas gentiment : une
    /// allocation refusée **abandonne le processus**, sans message, sur la
    /// sauvegarde de quelqu'un.
    ///
    /// C'est la même règle qu'au chargement d'un `.mca` — « on vérifie la
    /// place AVANT de réserver » — appliquée à un nombre qui vient de la
    /// souris au lieu d'un fichier. La conséquence est identique.
    TropGros {
        octets: u64,
        plafond: u64,
    },
    /// Un correctif de journal ne s'applique pas : le chunk a changé sous lui.
    ///
    /// **Garde, pas accident.** Chaque correctif porte l'empreinte de l'état
    /// qu'il attend ; sans elle, rejouer une annulation sur un chunk modifié
    /// depuis produirait un mélange des deux, parfaitement plausible et faux.
    /// On refuse plutôt que d'écrire à peu près.
    Divergence {
        region: RegionPos,
        chunk: u16,
    },
}

macro_rules! de {
    ($($src:ty => $var:ident),* $(,)?) => {$(
        impl From<$src> for Erreur {
            fn from(e: $src) -> Erreur {
                Erreur::$var(e)
            }
        }
    )*};
}
de! {
    SourceError => Source,
    ReadError => Lecture,
    WriteError => Ecriture,
    CodecError => Codec,
    tf_nbt::Trunc => Nbt,
    EncodeError => Encode,
    tf_anvil::chunk::SpliceError => Splice,
}

impl std::fmt::Display for Erreur {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Erreur::Source(e) => write!(f, "source : {e:?}"),
            Erreur::Lecture(e) => write!(f, "lecture de région : {e:?}"),
            Erreur::Ecriture(e) => write!(f, "écriture de région : {e:?}"),
            Erreur::Codec(e) => write!(f, "compression : {e:?}"),
            Erreur::Nbt(e) => write!(f, "balayage de chunk : {e:?}"),
            Erreur::Encode(e) => write!(f, "encodage de section : {e:?}"),
            Erreur::Splice(e) => write!(f, "recollement : {e:?}"),
            Erreur::Divergence { region, chunk } => write!(
                f,
                "le chunk {chunk} de la région r.{}.{} a changé depuis : \
                 l'annulation ne s'applique plus. Rien n'a été écrit",
                region.x, region.z
            ),
            Erreur::TropGros { octets, plafond } => write!(
                f,
                "sélection trop grande à matérialiser : {:.1} Go demandés pour \
                 un plafond de {:.1}. //copy, //paste et //hollow demandent une \
                 case par bloc en mémoire — réduire la sélection, ou passer par \
                 une opération qui ne paie que sa portée (//set, //replace, \
                 //naturalize…)",
                *octets as f64 / 1e9,
                *plafond as f64 / 1e9
            ),
        }
    }
}

/// Ce qu'une opération a le droit de MATÉRIALISER, en **octets**.
///
/// En octets et pas en cases, pour la même raison que la fenêtre de résidence
/// (invariant n° 7) : les opérations qui matérialisent n'ont pas le même
/// appétit par case, et un plafond en cases mentirait à l'une des deux.
/// `//copy` demande quatre octets par case ; `//hollow` en demande douze — la
/// grille copiée, ses quatre tampons de diffusion, et l'extrait creusé.
///
/// Deux gigaoctets : assez pour cinq régions pleine hauteur en copie, une et
/// demie en creusage. Le plafond ne protège pas d'un excès de zèle mais d'un
/// geste accidentel — sur un monde Minefield, « sélectionner tout » se compte
/// en milliards de cases, et `vec![]` n'échoue pas gentiment : une allocation
/// refusée **abandonne le processus**, sans message, sur la sauvegarde de
/// quelqu'un. Mieux vaut une erreur qui nomme le chiffre.
///
/// C'est un plafond de MÉMOIRE, pas une politique : `//set` sur la même
/// sélection ne paie que sa portée et n'est pas bridé.
pub const MAX_OCTETS_MATERIALISES: u64 = 2_000_000_000;

/// Octets par case que coûte `//copy` — la grille d'états de l'extrait.
pub const OCTETS_COPIE: u64 = 4;

/// Octets par case que coûte `//hollow` : la copie, les quatre tampons de la
/// diffusion (`dehors`, `garde`, sa copie de travail, `interieur`), et
/// l'extrait creusé.
///
/// Compté ici et non deviné sur place : c'est un chiffre qui change quand
/// l'algorithme change, et un plafond calé sur l'ancien laisserait passer
/// exactement ce qu'il existe pour refuser.
pub const OCTETS_CREUSAGE: u64 = 12;

/// Refuse une sélection qu'on ne pourrait pas matérialiser.
///
/// `octets_par_case` est ce que l'APPELANT sait et que la garde ne peut pas
/// deviner. Rend le nombre de cases quand ça passe.
///
/// Tout en `u64` et jamais en `usize` : sur une cible 32 bits le produit
/// déborderait AVANT d'être comparé, et la garde laisserait passer exactement
/// le cas qu'elle existe pour attraper. `saturating_mul` pour la même raison
/// un cran plus haut — une sélection de tout le monde dépasse `u64` en
/// octets.
pub fn verifier_materialisable(sel: &BBox, octets_par_case: u64) -> Result<u64, Erreur> {
    let (sx, sy, sz) = sel.size();
    let cases = (sx as u64)
        .saturating_mul(sy as u64)
        .saturating_mul(sz as u64);
    let octets = cases.saturating_mul(octets_par_case);
    if octets > MAX_OCTETS_MATERIALISES {
        return Err(Erreur::TropGros {
            octets,
            plafond: MAX_OCTETS_MATERIALISES,
        });
    }
    Ok(cases)
}

/// Le niveau de compression des écritures de STAGING.
///
/// La recompression pèse **68 %** d'une opération complète : c'est là, et nulle
/// part ailleurs, que se décide la réactivité de l'éditeur. Mesuré sur une
/// région pleine (1 024 chunks, 32,8 Mo décompressés) :
///
/// | niveau | temps | taille | |
/// |---:|---:|---:|---|
/// | 0 | 37 ms | 32,8 Mo | × 6,5 la taille — non |
/// | 1 | 146 ms | 7,6 Mo | +51 % |
/// | **2** | **230 ms** | **5,6 Mo** | **× 2,4 plus rapide pour +11,8 %** |
/// | 3 | 298 ms | 5,4 Mo | +7,8 % |
/// | 6 | 550 ms | 5,0 Mo | la référence, et celui de `deflate` |
/// | 9 | 4 371 ms | 4,7 Mo | × 8 plus lent pour −6 % — jamais |
///
/// Le niveau 2 est le point d'équilibre, et le raisonnement est le même que
/// pour le cache d'aperçu d'`ExeWorldEdit` : **la copie de travail se réécrit à
/// chaque opération**, pendant que l'utilisateur attend, alors que la
/// sauvegarde finale ne s'écrit qu'une fois. Un cache s'optimise pour le temps.
///
/// La contrepartie est réelle et assumée : la copie de travail pèse 11,8 % de
/// plus, et si elle est validée telle quelle, la save aussi — jusqu'à ce que le
/// jeu réécrive ces chunks à son propre niveau. C'est **le seul endroit** à
/// changer pour en décider autrement.
pub(crate) const NIVEAU_STAGING: u32 = 2;

/// Ce qu'il y a à faire sur UN chunk : sa charge, telle qu'elle est sur disque.
struct Travail {
    cpos: ChunkPos,
    index: u16,
    compression: tf_anvil::Compression,
    charge: Vec<u8>,
}

/// Ce qu'on en a fait.
struct Fait {
    index: u16,
    /// Absent quand l'opération n'a rien écrit dans ce chunk.
    ecrit: Option<(ChunkPatch, Vec<u8>)>,
    etages: [usize; 4],
    blocs: Option<u64>,
    bornes: Option<BBox>,
    entites_posees: u64,
    entites_retirees: u64,
    biomes: u64,
}

/// La chaîne complète sur un chunk : décompresser, balayer, appliquer,
/// encoder, recoller, recompresser.
///
/// **Pure et sans état partagé** — c'est ce qui la rend parallélisable, et ce
/// n'est pas un heureux hasard : le tirage aléatoire se hache sur la POSITION
/// précisément pour qu'aucune opération ne dépende de l'ordre de parcours.
///
/// L'interner est une COPIE de travail. `decode_section` a besoin d'une table
/// mutable, et la partager derrière un verrou sérialiserait exactement ce qu'on
/// cherche à paralléliser — c'est la mesure de `we-engine` relue correctement :
/// là-bas le coût était le `structuredClone` de l'arbre NBT, pas le calcul.
/// Les états qu'un chunk fait découvrir ne quittent pas le fil : le fichier
/// porte des NOMS, pas des identifiants.
fn un_chunk(
    t: &Travail,
    sel: &BBox,
    op: &dyn Operation,
    cible: Cible,
    interner: &mut Interner,
) -> Result<Fait, Erreur> {
    let avant = inflate(&t.charge, t.compression)?;
    let balayage = scan(&avant)?;
    let mut fait = Fait {
        index: t.index,
        ecrit: None,
        etages: [0; 4],
        blocs: op.compte().then_some(0),
        bornes: None,
        entites_posees: 0,
        entites_retirees: 0,
        biomes: 0,
    };
    let mut edits = Vec::new();

    // ── Les block entities, avant toute chose.
    //
    // Elles ne sont PAS dans la grille de blocs : un coffre est une entrée à
    // part du chunk, avec ses propres coordonnées. Rien ne les fait suivre les
    // blocs tout seules, et le format ne signale pas l'incohérence — un build
    // pivoté sort vide, et on l'apprend en ouvrant un coffre.
    //
    // Ce qu'une opération APPORTE, elle seule le sait. Ce qu'elle EFFACE se
    // déduit ici : une entité dont la case a changé d'état part avec son bloc.
    // Le critère est exact et vaut pour toute opération présente ou future —
    // là où la boîte `bornes` serait une approximation qui détruirait le
    // coffre qu'un `//replace` n'a pas touché.
    let posees = op.entites_posees(t.cpos);
    let habitees = !balayage.entites.est_vide();
    let mut orphelines = vec![false; balayage.entites.entrees.len()];

    // ── La PORTÉE décide de la forme du travail.
    //
    // `Section` — le défaut — traite une section à la fois et garde les trois
    // étages. `Colonne` décode le chunk d'un coup, parce que « où est la
    // surface » est une propriété de la colonne et qu'une section ne peut pas
    // y répondre seule : la supposer poserait une bande d'herbe tous les
    // seize blocs, régulièrement, au milieu de chaque falaise.
    let portee = op.portee();
    if portee == Portee::Colonne {
        let mut prises: Vec<(usize, tf_anvil::Section)> = Vec::new();
        for (rang, sc) in balayage.sections.iter().enumerate() {
            let spos = SectionPos::new(t.cpos.x, sc.y as i32, t.cpos.z);
            if sel.clip_to_section(spos).is_none() {
                continue;
            }
            if let Some(s) = decode_section(&avant, &balayage, sc, interner)? {
                prises.push((rang, s));
            }
        }
        let mut colonnes = Colonnes::depuis(prises);
        // Le témoin des coffres, pris sur la vue AVANT l'opération : même
        // critère que le chemin par section, une case qui change d'état
        // emporte l'entité qui l'habitait.
        let temoins = if habitees {
            temoins_colonnes(&balayage.entites.entrees, &colonnes, t.cpos)
        } else {
            Vec::new()
        };
        let r = op.appliquer_colonnes(&mut colonnes, sel, t.cpos);
        for t in &temoins {
            if colonnes.get(t.lx, t.wy, t.lz) != t.etat {
                orphelines[t.rang] = true;
            }
        }
        fait.etages[rang_etage(r.etage)] += 1;
        if let (Some(c), Some(n)) = (fait.blocs.as_mut(), r.blocs) {
            *c += n;
        }
        fait.bornes = unir(fait.bornes, r.bornes);
        for (rang, section) in colonnes.finir() {
            edits.extend(section_edits(
                &avant,
                &section,
                &balayage.sections[rang],
                interner,
            )?);
        }
    }

    // Le chemin ordinaire : une section à la fois, et les trois étages.
    if portee == Portee::Section {
        for sc in &balayage.sections {
            let spos = SectionPos {
                x: t.cpos.x,
                y: sc.y as i32,
                z: t.cpos.z,
            };
            if sel.clip_to_section(spos).is_none() {
                continue;
            }
            let Some(mut section) = decode_section(&avant, &balayage, sc, interner)? else {
                continue;
            };
            // Les cases habitées de CETTE section, telles qu'elles sont avant.
            // Rien n'est alloué pour un chunk sans coffre, c'est-à-dire presque
            // tous.
            let temoins = if habitees {
                temoins_de(&balayage.entites.entrees, &section, spos)
            } else {
                Vec::new()
            };
            let r = op.appliquer(&mut section, sel, spos);
            for (i, [lx, ly, lz], etat) in temoins {
                if section.get(lx, ly, lz) != etat {
                    orphelines[i] = true;
                }
            }
            fait.etages[rang_etage(r.etage)] += 1;
            if let (Some(c), Some(n)) = (fait.blocs.as_mut(), r.blocs) {
                *c += n;
            }
            fait.bornes = unir(fait.bornes, r.bornes);
            if r.etage == Etage::Rien {
                continue;
            }
            // `section_edits` compare ce qu'il va écrire à ce qui est DÉJÀ là : une
            // section que l'opération n'a pas vraiment changée ne produit aucune
            // édition, donc aucune entrée de journal vide.
            edits.extend(section_edits(&avant, &section, sc, interner)?);
        }
    }

    // Les BLOCS ont-ils changé ? Seuls eux changent la lumière et les cartes
    // de hauteur — un biome ou une entité ne jettent pas d'ombre.
    let blocs_changes = !edits.is_empty();

    // ── Les biomes, quand l'opération les touche.
    //
    // Indépendant du chemin des blocs, et volontairement : un biome vit dans
    // sa PROPRE palette, sur sa propre grille de 4 × 4 × 4 cellules. Le
    // greffer sur la boucle des blocs l'aurait rendu solidaire de la portée,
    // alors que les deux n'ont rien à voir.
    if op.touche_biomes() {
        for sc in &balayage.sections {
            let spos = SectionPos::new(t.cpos.x, sc.y as i32, t.cpos.z);
            let Some(coupe) = sel.clip_to_section(spos) else {
                continue;
            };
            // `None` veut dire « cette section ne porte pas de biome qu'on
            // sache lire » — 1.13–1.17, ou une palette d'un type inattendu.
            // On passe : ne rien faire vaut mieux qu'écrire au jugé la carte
            // des biomes de quelqu'un.
            let Some(mut b) = decode_biomes(&avant, sc, interner)? else {
                continue;
            };
            if !op.appliquer_biomes(&mut b, sel, spos) {
                continue;
            }
            fait.biomes += 1;
            let o = spos.min_block();
            fait.bornes = unir(
                fait.bornes,
                Some(BBox::new(
                    BlockPos {
                        x: o.x + coupe.x0 as i32,
                        y: o.y + coupe.y0 as i32,
                        z: o.z + coupe.z0 as i32,
                    },
                    BlockPos {
                        x: o.x + coupe.x1 as i32,
                        y: o.y + coupe.y1 as i32,
                        z: o.z + coupe.z1 as i32,
                    },
                )),
            );
            edits.extend(biome_edits(&avant, &b, sc, interner)?);
        }
    }

    if !posees.is_empty() || orphelines.contains(&true) {
        // Toute entité posée finit dans la liste : soit elle remplace celle
        // de sa case, soit elle s'ajoute. Le compte se prend donc AVANT.
        fait.entites_posees = posees.len() as u64;
        let (voulues, retirees) =
            liste_voulue(&avant, &balayage.entites.entrees, &orphelines, posees);
        fait.entites_retirees = retirees as u64;
        if let Some(e) = edition_entites(&avant, &balayage.entites, balayage.layout, &voulues) {
            edits.push(e);
        }
    }

    // ── L'éclairage et les cartes de hauteur, que le JEU recalculera.
    //
    // Ils décrivent les blocs d'AVANT : laissés tels quels, une salle creusée
    // sortait noire en jeu et la pluie traversait un toit neuf. Le journal
    // enregistre ces éditions avec les autres, donc annuler les rend aussi.
    if blocs_changes {
        edits.extend(tf_anvil::chunk::faire_recalculer(&balayage));
    }

    if !edits.is_empty() {
        let apres = splice(&avant, &mut edits)?;
        let patch = ChunkPatch::record(cible, &avant, &apres, &edits)?;
        fait.ecrit = Some((patch, deflate_level(&apres, t.compression, NIVEAU_STAGING)?));
    }
    Ok(fait)
}

/// Le rang d'un étage dans `RapportRegion::etages`.
fn rang_etage(e: Etage) -> usize {
    match e {
        Etage::Rien => 0,
        Etage::Section => 1,
        Etage::Palette => 2,
        Etage::Bloc => 3,
    }
}

/// Une case habitée qu'on surveille, dans le repère de `Colonnes`.
///
/// Un `struct` plutôt qu'un `[usize; 3]` : **x et z sont LOCAUX, y est
/// MONDE**, et trois nombres du même type dans un tableau invitent à les
/// échanger. Le compilateur a d'ailleurs attrapé l'échange à la première
/// écriture.
struct Temoin {
    /// Rang de l'entrée dans la liste du chunk.
    rang: usize,
    lx: usize,
    wy: i32,
    lz: usize,
    etat: Option<StateId>,
}

/// Le même témoin que `temoins_de`, pris sur une vue par colonne.
fn temoins_colonnes(
    entrees: &[tf_anvil::EntiteReperee],
    c: &Colonnes,
    cpos: ChunkPos,
) -> Vec<Temoin> {
    let mut out = Vec::new();
    for (rang, e) in entrees.iter().enumerate() {
        let Some(a) = e.ancrage else { continue };
        // Division PLANCHER, comme partout : le bloc −1 est dans le chunk −1.
        if [a.case[0] >> 4, a.case[2] >> 4] != [cpos.x, cpos.z] {
            continue;
        }
        let lx = a.case[0].rem_euclid(16) as usize;
        let lz = a.case[2].rem_euclid(16) as usize;
        out.push(Temoin {
            rang,
            lx,
            wy: a.case[1],
            lz,
            etat: c.get(lx, a.case[1], lz),
        });
    }
    out
}

/// L'état des cases habitées d'une section, avec de quoi les retrouver.
///
/// Le rang dans la liste du chunk voyage avec : c'est lui qui désigne l'entrée
/// à retirer, et retrouver une entrée par sa case obligerait à supposer qu'il
/// n'y en a qu'une par case — ce que rien ne garantit dans un fichier réel.
fn temoins_de(
    entrees: &[tf_anvil::EntiteReperee],
    section: &tf_anvil::Section,
    pos: SectionPos,
) -> Vec<(usize, [usize; 3], Option<StateId>)> {
    let mut out = Vec::new();
    for (i, e) in entrees.iter().enumerate() {
        let Some(a) = e.ancrage else { continue };
        // Division PLANCHER : le bloc −1 est dans la section −1. Un décalage
        // arithmétique la fait, une division entière non.
        if [a.case[0] >> 4, a.case[1] >> 4, a.case[2] >> 4] != [pos.x, pos.y, pos.z] {
            continue;
        }
        let l = [
            a.case[0].rem_euclid(16) as usize,
            a.case[1].rem_euclid(16) as usize,
            a.case[2].rem_euclid(16) as usize,
        ];
        out.push((i, l, section.get(l[0], l[1], l[2])));
    }
    out
}

/// La liste de block entities voulue pour ce chunk, et combien sont retirées.
///
/// **Une entité posée prend la place EXACTE de celle qu'elle remplace.** Si
/// elle était simplement ajoutée à la fin, reposer un extrait à sa propre
/// place réordonnerait la liste : mêmes entrées, autres octets, donc un
/// correctif de journal pour zéro changement. C'est la faute que le collage a
/// déjà payée deux fois sur un vrai monde, sous deux formes différentes.
fn liste_voulue(
    inflated: &[u8],
    entrees: &[tf_anvil::EntiteReperee],
    orphelines: &[bool],
    posees: Vec<Entite>,
) -> (Vec<Entite>, usize) {
    let mut par_case: std::collections::HashMap<[i32; 3], Entite> =
        posees.into_iter().map(|e| (e.case, e)).collect();
    let mut voulues = Vec::with_capacity(entrees.len() + par_case.len());
    let mut retirees = 0usize;

    for (i, e) in entrees.iter().enumerate() {
        match e.ancrage {
            // Sans case, on ne sait ni la suivre ni la juger : elle reste.
            None => voulues.push(Entite::intouchable(e.span.slice(inflated).to_vec())),
            Some(a) => match par_case.remove(&a.case) {
                Some(neuve) => voulues.push(neuve),
                None if orphelines[i] => retirees += 1,
                None => match Entite::depuis(inflated, e) {
                    Some(gardee) => voulues.push(gardee),
                    None => voulues.push(Entite::intouchable(e.span.slice(inflated).to_vec())),
                },
            },
        }
    }

    // Celles qui atterrissent sur une case vide, dans l'ordre YZX du dépôt :
    // une liste dont l'ordre dépendrait du parcours d'une table de hachage
    // ferait deux fichiers différents pour la même opération.
    let mut neuves: Vec<Entite> = par_case.into_values().collect();
    neuves.sort_by_key(|e| (e.case[1], e.case[2], e.case[0]));
    voulues.extend(neuves);
    (voulues, retirees)
}

/// La plus petite boîte qui contient les deux.
fn unir(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(mut d), Some(b)) => {
            d.extend(b.min);
            d.extend(b.max);
            Some(d)
        }
    }
}

/// Les chunks d'UNE région que la sélection touche.
///
/// **Coupé avant d'itérer, jamais filtré après.** Parcourir tous les chunks de
/// la sélection puis jeter ceux des autres régions rend le tout quadratique :
/// une sélection de dix régions sur dix en contient 102 400, et les filtrer
/// cent fois fait dix millions d'itérations pour cent mille chunks utiles. Sur
/// un monde Minefield la sélection peut faire des milliers de régions ; c'est
/// le genre de coût qui ne se voit pas sur une fixture et qui rend l'outil
/// inutilisable chez l'utilisateur.
pub(crate) fn chunks_de(sel: &BBox, pos: RegionPos) -> impl Iterator<Item = ChunkPos> {
    let (a, b) = (sel.min.chunk(), sel.max.chunk());
    let x0 = a.x.max(pos.x * 32);
    let x1 = b.x.min(pos.x * 32 + 31);
    let z0 = a.z.max(pos.z * 32);
    let z1 = b.z.min(pos.z * 32 + 31);
    (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| ChunkPos::new(x, z)))
}

/// Copie une sélection dans un presse-papiers — blocs, block entities ET
/// entités.
///
/// C'est `//copy`, et c'est la seule opération du crate qui n'écrit RIEN :
/// elle lit la source à travers le staging et rend un extrait détaché. Le
/// monde n'est pas touché, donc aucun correctif de journal, donc rien à
/// annuler.
///
/// **Ce qui n'existe pas vaut de l'air, et pas une erreur.** Une sélection
/// déborde presque toujours de ce qui est généré — c'est même le cas normal
/// quand on copie un bâtiment avec sa marge. Une région absente, un chunk
/// jamais visité, une section hors du monde : l'extrait porte de l'air à ces
/// places. Refuser rendrait `//copy` inutilisable au bord d'un build.
///
/// **Une charge illisible, en revanche, est une ERREUR.** La confondre avec de
/// l'air ferait coller un trou à la place d'un mur, en silence — c'est le
/// piège `cold_read` de l'étage bloc, sous une autre forme.
///
/// **Les entités du dossier `entities/` viennent avec** — cadres, tableaux,
/// porte-armures, bêtes — quand on copie des BLOCS (`Folder::Region`) : c'est
/// là qu'elles sont ancrées. Voir `mobiles.rs`.
pub fn copier<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    interner: &mut Interner,
) -> Result<Presse, Erreur> {
    let mut presse = copier_blocs(staging, dim, folder, sel, interner)?;
    if folder == Folder::Region {
        presse.mobiles = crate::mobiles::copier_mobiles(staging, dim, sel)?;
    }
    Ok(presse)
}

/// La grille et les block entities seules, SANS les entités.
///
/// Pour ce qui repose un extrait à sa propre place (`//hollow`) ou déplace les
/// entités par son propre chemin (`//move`, qui doit les RETIRER de la source
/// et garder leur `UUID`) : leur faire suivre le presse-papiers les doublerait.
pub(crate) fn copier_blocs<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    interner: &mut Interner,
) -> Result<Presse, Erreur> {
    let (sx, sy, sz) = sel.size();
    verifier_materialisable(sel, OCTETS_COPIE)?;
    let air = interner.intern("minecraft:air");
    let mut presse = Presse::uniforme([sx, sy, sz], air);

    for pos in sel.regions() {
        let octets = match staging.read_region(dim, folder, pos) {
            Ok(b) => b,
            // Région absente : de l'air, et c'est le cas normal.
            Err(SourceError::NotFound) => continue,
            Err(e) => return Err(e.into()),
        };
        let mut region = read(&octets, pos.x, pos.z)?;
        for cpos in chunks_de(sel, pos) {
            let (lx, lz) = (cpos.x.rem_euclid(32), cpos.z.rem_euclid(32));
            let Some(brut) = region.get_mut(lx, lz) else {
                continue; // chunk jamais généré
            };
            // Une charge déportée arrive VIDE — le crate Anvil ne lit pas de
            // fichiers. L'oublier ferait copier de l'air à la place du plus
            // gros chunk de la sélection, en silence.
            if brut.needs_external() {
                let nom = external_file_name(cpos.x, cpos.z);
                let charge = staging.read_external(dim, folder, &nom)?;
                brut.resolve_external(charge);
            }
            if brut.payload.is_empty() {
                continue;
            }
            let avant = inflate(&brut.payload, brut.compression)?;
            let balayage = scan(&avant)?;

            // Les coffres de ce chunk qui tombent DANS la sélection. Sans eux,
            // un build copié puis reposé ailleurs arrive vide — le piège
            // qu'`ExeWorldEdit` a payé, et que rien dans le format ne signale.
            for e in &balayage.entites.entrees {
                let Some(a) = e.ancrage else { continue };
                let p = BlockPos {
                    x: a.case[0],
                    y: a.case[1],
                    z: a.case[2],
                };
                if !sel.contains(p) {
                    continue;
                }
                let Some(mut ent) = Entite::depuis(&avant, e) else {
                    continue;
                };
                // MONDE → LOCAL, le même repère que `blocs`. Une unité qui
                // traverse une frontière se vérifie EN TRAVERSANT : c'est le
                // scénario copier → tourner → coller qui le fait.
                ent.case = [
                    a.case[0] - sel.min.x,
                    a.case[1] - sel.min.y,
                    a.case[2] - sel.min.z,
                ];
                presse.entites.push(ent);
            }

            for sc in &balayage.sections {
                let spos = SectionPos::new(cpos.x, sc.y as i32, cpos.z);
                let Some(coupe) = sel.clip_to_section(spos) else {
                    continue;
                };
                let Some(section) = decode_section(&avant, &balayage, sc, interner)? else {
                    continue;
                };
                let coin = spos.min_block();
                for ly in coupe.y0..=coupe.y1 {
                    for lz in coupe.z0..=coupe.z1 {
                        for lx in coupe.x0..=coupe.x1 {
                            let Some(id) = section.get(lx, ly, lz) else {
                                continue;
                            };
                            // Coordonnées MONDE, puis relatives au coin de la
                            // sélection. Passer par le monde évite d'avoir à
                            // raisonner sur deux origines à la fois.
                            let (mx, my, mz) =
                                (coin.x + lx as i32, coin.y + ly as i32, coin.z + lz as i32);
                            let i = presse.index(
                                (mx - sel.min.x) as u32,
                                (my - sel.min.y) as u32,
                                (mz - sel.min.z) as u32,
                            );
                            if let Some(i) = i {
                                presse.blocs[i] = id;
                            }
                        }
                    }
                }
            }
        }
    }
    // L'ordre du ramassage dépend du parcours des régions ; celui de l'extrait
    // ne doit dépendre de rien. YZX, comme les cases.
    presse
        .entites
        .sort_by_key(|e| (e.case[1], e.case[2], e.case[0]));
    Ok(presse)
}

/// Dans quel sens on rejoue une entrée de journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    Annuler,
    Refaire,
}

/// **Rejoue une entrée de journal sur la copie de travail.**
///
/// La jonction symétrique de `RapportRegion::journaliser` — et elle n'existait
/// nulle part. Les tests la réécrivaient à la main sous le commentaire
/// « c'est exactement ce que l'application fera », ce qui est la définition
/// même d'une jonction qu'aucun hôte n'écrit : chaque hôte l'aurait
/// réinventée, avec trois occasions de se tromper dont ce dépôt a déjà payé
/// une — **les correctifs s'annulent À L'ENVERS**, et l'ordre ne se voit que
/// sur une opération qui repasse deux fois sur le même chunk.
///
/// Rend le nombre de chunks touchés. **Tout ou rien par région** : un
/// correctif qui diverge arrête la région avant la moindre écriture, plutôt
/// que de laisser la moitié d'une annulation appliquée.
pub fn rejouer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    entree: &tf_world::journal::Entree,
    sens: Sens,
) -> Result<usize, Erreur> {
    // Les correctifs dans le SENS demandé. `a_annuler` les rend à l'envers ;
    // un appelant qui écrirait `corrections.iter()` aurait raison jusqu'au
    // jour où il aurait tort, sans prévenir.
    // Deux itérateurs opaques de types différents : on les matérialise
    // séparément plutôt que de les boxer, la liste fait quelques dizaines
    // d'entrées.
    let corrections: Vec<&Correction> = match sens {
        Sens::Annuler => entree.a_annuler().collect(),
        Sens::Refaire => entree.a_refaire().collect(),
    };
    let patches: Vec<&ChunkPatch> = corrections
        .into_iter()
        .filter_map(|c| match c {
            Correction::Chunk(p) => Some(p),
            _ => None,
        })
        .collect();

    // Groupés par région, SANS réordonner à l'intérieur d'une région : deux
    // correctifs sur le même chunk s'enchaînent par leurs empreintes.
    let mut ordre: Vec<(Dimension, Folder, RegionPos)> = Vec::new();
    for p in &patches {
        let cle = (p.cible.dim.clone(), p.cible.folder, p.cible.region);
        if !ordre.contains(&cle) {
            ordre.push(cle);
        }
    }

    let mut touches = 0;
    for (dim, folder, pos) in ordre {
        // Une région qui n'existe pas se lit VIDE : c'est l'état d'avant d'un
        // correctif qui y crée le premier chunk — la première entité posée
        // dans un `entities/r.X.Z.mca` qui n'existait pas.
        let bytes = match staging.read_region(&dim, folder, pos) {
            Ok(b) => Some(b),
            Err(SourceError::NotFound) => None,
            Err(e) => return Err(e.into()),
        };
        let mut region = match &bytes {
            Some(b) => read(b, pos.x, pos.z)?,
            None => Region::vide(pos.x, pos.z),
        };
        let mut orphelins: Vec<String> = Vec::new();
        let mut n = 0;
        for p in patches
            .iter()
            .filter(|p| p.cible.dim == dim && p.cible.folder == folder && p.cible.region == pos)
        {
            let (lx, lz) = ((p.cible.chunk % 32) as i32, (p.cible.chunk / 32) as i32);
            let (cx, cz) = (pos.x * 32 + lx, pos.z * 32 + lz);
            // Un chunk ABSENT se lit comme vide, lui aussi : l'état d'avant
            // d'une création, et l'état d'après de son annulation.
            let courant = match region.get_mut(lx, lz) {
                Some(c) => {
                    // Une charge déportée arrive VIDE. Sans la résoudre,
                    // l'empreinte ne correspondait jamais et l'annulation d'un
                    // chunk de plus d'un mégaoctet refusait de se faire.
                    if c.needs_external() {
                        let charge =
                            staging.read_external(&dim, folder, &external_file_name(cx, cz))?;
                        c.resolve_external(charge);
                    }
                    if c.payload.is_empty() {
                        Vec::new()
                    } else {
                        inflate(&c.payload, c.compression)?
                    }
                }
                None => Vec::new(),
            };
            let attendu = match sens {
                Sens::Annuler => p.apres_hash,
                Sens::Refaire => p.avant_hash,
            };
            if tf_world::journal::empreinte(&courant) != attendu {
                return Err(Erreur::Divergence {
                    region: pos,
                    chunk: p.cible.chunk,
                });
            }
            let mut edits = match sens {
                Sens::Annuler => p.annuler.clone(),
                Sens::Refaire => p.refaire.clone(),
            };
            let neuf = splice(&courant, &mut edits)?;
            let index = p.cible.chunk as usize;
            if neuf.is_empty() {
                // Annuler une CRÉATION rend le chunk absent — pas une coquille
                // vide que le jeu n'aurait jamais écrite. Sa charge déportée
                // éventuelle part avec lui.
                if region.slots[index].as_ref().is_some_and(|c| c.external) {
                    orphelins.push(external_file_name(cx, cz));
                }
                region.slots[index] = None;
            } else {
                // Le niveau de la copie de travail, pas celui par défaut : elle
                // se réécrit à chaque action pendant que l'utilisateur attend,
                // et s'optimise donc pour le TEMPS. Mesuré : 230 ms contre 550.
                match region.get_mut(lx, lz) {
                    Some(c) => {
                        c.payload = Cow::Owned(deflate_level(&neuf, c.compression, NIVEAU_STAGING)?)
                    }
                    None => {
                        region.slots[index] = Some(RawChunk {
                            index: index as u16,
                            timestamp: 0,
                            compression: Compression::Zlib,
                            payload: Cow::Owned(deflate_level(
                                &neuf,
                                Compression::Zlib,
                                NIVEAU_STAGING,
                            )?),
                            external: false,
                        })
                    }
                }
            }
            n += 1;
        }
        if n > 0 {
            // Les charges déportées, écrites et retirées comme le fait
            // `appliquer_region` : un chunk qui repasse au-delà d'un mégaoctet
            // en rejouant part en `.mcc`, et n'écrire que la région laisserait
            // un talon qui désigne un fichier absent — le chunk perdu.
            let out = write(&region)?;
            staging.write_region(&dim, folder, pos, &out.region)?;
            for f in out.external {
                staging.write_external(&dim, folder, &f.name, &f.bytes)?;
            }
            for nom in out.removed_external.into_iter().chain(orphelins) {
                staging.remove_external(&dim, folder, &nom)?;
            }
            touches += n;
        }
    }
    Ok(touches)
}

/// Applique un plan à une sélection, sur UNE région, à travers le staging.
///
/// Rend les correctifs à pousser dans le journal. Ne les pousse pas lui-même :
/// une opération qui porte sur plusieurs régions doit faire UNE entrée de
/// journal, pas une par région — sinon `Ctrl+Z` défait un tiers du travail.
pub fn appliquer_region<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    pos: RegionPos,
    sel: &BBox,
    op: &dyn Operation,
    interner: &Interner,
) -> Result<RapportRegion, Erreur> {
    let bytes = match staging.read_region(dim, folder, pos) {
        Ok(b) => b,
        // Une région absente est le cas NORMAL au bord d'un monde. La
        // confondre avec un échec ferait refuser une save parfaitement saine.
        Err(SourceError::NotFound) => return Ok(RapportRegion::default()),
        Err(e) => return Err(e.into()),
    };
    let mut region = read(&bytes, pos.x, pos.z)?;

    // ── Le ramassage, séquentiel : c'est lui qui touche au disque.
    //
    // Une charge déportée arrive VIDE — le crate Anvil ne lit pas de fichiers.
    // L'oublier ferait lire un chunk vide et l'écraser.
    let mut travaux: Vec<Travail> = Vec::new();
    for cpos in chunks_de(sel, pos) {
        let (lx, lz) = (cpos.x.rem_euclid(32), cpos.z.rem_euclid(32));
        let Some(brut) = region.get_mut(lx, lz) else {
            continue;
        };
        if brut.needs_external() {
            let nom = external_file_name(cpos.x, cpos.z);
            let charge = staging.read_external(dim, folder, &nom)?;
            brut.resolve_external(charge);
        }
        if brut.payload.is_empty() {
            continue;
        }
        travaux.push(Travail {
            cpos,
            index: brut.index,
            compression: brut.compression,
            // On COPIE la charge compressée — quelques mégaoctets pour toute une
            // région — parce que la suite tourne sur plusieurs fils et ne peut
            // pas emprunter la région qu'on va réécrire.
            charge: brut.payload.to_vec(),
        });
    }

    // ── Le travail, parallèle : 92 % du temps d'une opération est ici.
    //
    // Mesuré sur une région pleine, la chaîne complète prend 800 ms dont
    // 541 de recompression, 89 de décompression, 65 d'application et
    // d'encodage, 37 de décodage et 5 de recollement. Optimiser le calcul
    // — les trois étages, 1,35 ms — n'aurait rien changé : c'est le piège
    // n° 1 du dépôt, et il a failli se refermer une deuxième fois.
    let cible = |index: u16| Cible {
        dim: dim.clone(),
        folder,
        region: pos,
        chunk: index,
    };
    let faits: Result<Vec<Fait>, Erreur> = {
        #[cfg(feature = "parallele")]
        {
            use rayon::prelude::*;
            travaux
                .par_iter()
                .map_init(
                    || interner.clone(),
                    |local, t| un_chunk(t, sel, op, cible(t.index), local),
                )
                .collect()
        }
        #[cfg(not(feature = "parallele"))]
        {
            let mut local = interner.clone();
            travaux
                .iter()
                .map(|t| un_chunk(t, sel, op, cible(t.index), &mut local))
                .collect()
        }
    };
    let faits = faits?;

    // ── Le recollage, séquentiel et DÉTERMINISTE.
    //
    // `par_iter().collect()` garde l'ordre d'entrée : le rapport et le journal
    // ne dépendent donc pas du nombre de cœurs. Un journal qui changerait
    // d'ordre selon la machine rendrait deux annulations différentes du même
    // travail.
    let mut rap = RapportRegion::default();
    let mut compte = op.compte().then_some(0u64);
    for f in faits {
        for (a, b) in rap.etages.iter_mut().zip(f.etages) {
            *a += b;
        }
        if let (Some(c), Some(n)) = (compte.as_mut(), f.blocs) {
            *c += n;
        }
        rap.bornes = unir(rap.bornes, f.bornes);
        rap.entites_posees += f.entites_posees;
        rap.entites_retirees += f.entites_retirees;
        rap.biomes += f.biomes;
        if let Some((patch, charge)) = f.ecrit {
            let (lx, lz) = ((f.index % 32) as i32, (f.index / 32) as i32);
            if let Some(brut) = region.get_mut(lx, lz) {
                brut.payload = Cow::Owned(charge);
            }
            rap.patches.push(patch);
        }
    }

    if !rap.patches.is_empty() {
        let out = write(&region)?;
        staging.write_region(dim, folder, pos, &out.region)?;
        for f in out.external {
            staging.write_external(dim, folder, &f.name, &f.bytes)?;
        }
        // Un `.mcc` devenu inutile qu'on laisserait occuperait le disque pour
        // toujours — et une save qui grossit sans raison finit par être
        // signalée comme un bug.
        for n in out.removed_external {
            staging.remove_external(dim, folder, &n)?;
        }
    }
    rap.blocs = compte;
    Ok(rap)
}

/// Applique un plan à une sélection, sur TOUTES les régions qu'elle touche.
///
/// Une seule entrée de journal pour l'ensemble : une opération qui déborde sur
/// quatre régions doit s'annuler d'un seul `Ctrl+Z`, pas de quatre.
pub fn appliquer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    op: &dyn Operation,
    interner: &Interner,
) -> Result<RapportRegion, Erreur> {
    let mut total = RapportRegion {
        blocs: op.compte().then_some(0),
        ..Default::default()
    };
    for pos in regions_a_visiter(staging, dim, folder, sel)? {
        total.absorber(appliquer_region(
            staging, dim, folder, pos, sel, op, interner,
        )?);
    }
    Ok(total)
}

/// Au-delà de combien de régions dans la BOÎTE on demande à la source
/// lesquelles existent vraiment.
///
/// 1 024, soit un carré de 32 × 32 régions — 16 384 blocs de côté. Aucune
/// sélection dessinée à la main n'en approche, et tout monde réel en a moins
/// en tout.
const REGIONS_AVANT_DE_DEMANDER: u64 = 1024;

/// Les régions qu'une opération doit visiter.
///
/// **Une sélection est une BOÎTE, un monde est un semis.** Parcourir la boîte
/// marche tant qu'elle est petite ; sur une sélection démesurée — « tout
/// sélectionner » sur un monde dont on ne connaît pas l'emprise — elle compte
/// des milliards de cases pour une poignée de régions qui existent, et
/// l'éditeur ne rend jamais la main. Ça ne plante pas, ça ne dit rien, ça ne
/// finit pas : le pire des trois.
///
/// L'équivalence est EXACTE et c'est ce qui rend le raccourci sûr : une région
/// absente fait rendre `RapportRegion::default()` à `appliquer_region`, et
/// `absorber` d'un rapport par défaut n'ajoute rien. Sauter ces régions-là ne
/// change donc aucun résultat — seulement le temps.
///
/// On ne demande la carte qu'au-delà d'un seuil, parce que la dresser coûte un
/// parcours de dossier : la payer sur chaque `//set` d'un mur de dix blocs
/// serait échanger un problème contre un autre.
///
/// La couche de staging compte autant que la source : une région n'existant
/// que là — un build vierge matérialisé, une région déjà écrite par une
/// opération précédente — serait invisible à la carte de la source seule, et
/// l'opération sauterait précisément ce qu'on vient de créer.
pub(crate) fn regions_a_visiter<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
) -> Result<Vec<RegionPos>, Erreur> {
    let (a, b) = sel.region_bounds();
    let largeur = (b.x as i64 - a.x as i64 + 1).max(0) as u64;
    let profondeur = (b.z as i64 - a.z as i64 + 1).max(0) as u64;
    if largeur.saturating_mul(profondeur) <= REGIONS_AVANT_DE_DEMANDER {
        return Ok(sel.regions().collect());
    }
    let mut vues: std::collections::BTreeSet<RegionPos> = staging
        .source()
        .overview(dim, folder)
        .map(|o| o.regions.into_iter().map(|r| r.pos).collect())
        .unwrap_or_default();
    for (d, f, pos) in staging.touched() {
        if &d == dim && f == folder {
            vues.insert(pos);
        }
    }
    Ok(vues
        .into_iter()
        .filter(|p| p.x >= a.x && p.x <= b.x && p.z >= a.z && p.z <= b.z)
        .collect())
}

/// Le pas et le remplissage d'un déplacement ou d'un empilement.
///
/// Un `struct` plutôt que six arguments : `deplacer(st, dim, f, sel, d, r, a,
/// true, false, i)` est une ligne où deux booléens voisins s'échangent sans
/// que rien ne le signale.
#[derive(Debug, Clone, Copy)]
pub struct Pas {
    /// De combien on décale, en blocs.
    pub d: [i32; 3],
    /// L'air de l'extrait écrase-t-il la destination ? `false` est le défaut
    /// de WorldEdit, et c'est le bon : on déplace un bâtiment sur un terrain.
    pub avec_air: bool,
    /// Ce qui compte comme air DANS l'extrait. Passé plutôt que deviné : un
    /// `StateId` n'a de sens que relativement à son interner.
    pub air: StateId,
    pub compter: bool,
}

/// `//move` : déplacer le contenu d'une sélection.
///
/// Trois passes, **une seule entrée de journal** : copier (qui n'écrit rien),
/// effacer la source, reposer l'extrait décalé. Un `Ctrl+Z` défait le
/// déplacement entier ; trois entrées en défairaient le dernier tiers.
///
/// **La source est effacée EN ENTIER, même si la destination la recouvre.**
/// L'extrait est déjà détaché à ce moment-là, donc rien n'est perdu, et le
/// collage réécrit ensuite la partie commune. Effacer seulement le complément
/// demanderait une soustraction de boîtes qui n'est pas rectangulaire — donc
/// une opération de plus, pour un résultat identique.
///
/// Les block entities suivent : le collage les repose, et l'effacement retire
/// celles dont la case a changé d'état. Un coffre déplacé garde son contenu.
///
/// Les ENTITÉS suivent par leur propre chemin (`deplacer_mobiles`) : elles
/// quittent leur chunk et gardent leur `UUID`, parce que c'est la même entité.
/// Passer par le presse-papiers les aurait COPIÉES — l'original restait, et la
/// copie changeait d'identité.
pub fn deplacer<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    pas: Pas,
    // `remplissage` : ce qui reste à la place de la source, de l'air
    // d'ordinaire. Passé plutôt que supposé — un `StateId` n'a de sens que
    // relativement à son interner, et « air » n'est pas toujours le bon choix
    // (on creuse parfois une tranchée de pierre).
    remplissage: StateId,
    interner: &mut Interner,
) -> Result<RapportRegion, Erreur> {
    let presse = copier_blocs(staging, dim, folder, sel, interner)?;
    let mut total = RapportRegion::default();

    let efface = crate::plan::Plan::nouveau(crate::Masque::Tout, crate::Motif::Bloc(remplissage));
    let efface = if pas.compter {
        efface.en_comptant()
    } else {
        efface
    };
    total.absorber(appliquer(staging, dim, folder, sel, &efface, interner)?);
    total.absorber(coller(
        staging, dim, folder, &presse, sel.min, pas, interner,
    )?);
    if folder == Folder::Region {
        total.absorber(crate::mobiles::deplacer_mobiles(staging, dim, sel, pas.d)?);
    }
    Ok(total)
}

/// `//stack` : répéter le contenu d'une sélection `fois` fois.
///
/// Le pas est donné en entier plutôt que « une direction et un nombre » :
/// c'est l'appelant qui sait s'il empile de la taille de la sélection ou d'un
/// bloc, et une direction cardinale ne saurait pas dire « en diagonale ».
///
/// **Chaque copie part du monde tel qu'il est à ce moment-là.** Avec un pas
/// plus petit que la sélection, les copies se recouvrent et la dernière
/// l'emporte — c'est ce que fait WorldEdit, et c'est ce qu'on attend d'un
/// empilement.
pub fn empiler<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    sel: &BBox,
    pas: Pas,
    fois: u32,
    interner: &mut Interner,
) -> Result<RapportRegion, Erreur> {
    let presse = copier(staging, dim, folder, sel, interner)?;
    let mut total = RapportRegion::default();
    for k in 1..=fois as i32 {
        let un = Pas {
            d: [pas.d[0] * k, pas.d[1] * k, pas.d[2] * k],
            ..pas
        };
        total.absorber(coller(
            staging, dim, folder, &presse, sel.min, un, interner,
        )?);
    }
    Ok(total)
}

/// Pose un extrait, le coin de plus petites coordonnées à `depuis + pas.d`.
///
/// Une opération ne paie que sa PORTÉE : la sélection passée à `appliquer` est
/// celle de l'extrait posé, jamais celle d'où il vient.
///
/// Les entités de l'extrait suivent, avec des `UUID` NEUFS : l'original reste
/// où il est, la copie est une autre entité (`mobiles::poser_mobiles`).
pub fn coller<S: RegionSource, O: RegionStore>(
    staging: &Staging<S, O>,
    dim: &Dimension,
    folder: Folder,
    presse: &Presse,
    depuis: tf_world::coords::BlockPos,
    pas: Pas,
    interner: &Interner,
) -> Result<RapportRegion, Erreur> {
    let c = crate::presse::Collage {
        presse,
        coin: BlockPos {
            x: depuis.x + pas.d[0],
            y: depuis.y + pas.d[1],
            z: depuis.z + pas.d[2],
        },
        avec_air: pas.avec_air,
        air: pas.air,
        compter: pas.compter,
    };
    let mut rap = appliquer(staging, dim, folder, &c.bornes(), &c, interner)?;
    if folder == Folder::Region && !presse.mobiles.is_empty() {
        rap.absorber(crate::mobiles::poser_mobiles(
            staging,
            dim,
            &presse.mobiles,
            c.coin,
        )?);
    }
    Ok(rap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_world::coords::BlockPos;

    fn boite(a: (i32, i32), b: (i32, i32)) -> BBox {
        BBox::new(
            BlockPos {
                x: a.0,
                y: 0,
                z: a.1,
            },
            BlockPos {
                x: b.0,
                y: 0,
                z: b.1,
            },
        )
    }

    #[test]
    fn les_chunks_d_une_region_sont_coupes_pas_filtres() {
        // Une sélection de 3 × 3 régions. Chaque région ne doit voir QUE ses
        // chunks, et la somme doit faire exactement ceux de la sélection : ni
        // trou, ni doublon.
        let sel = boite((-600, -600), (1100, 1100));
        let mut total = 0usize;
        let mut vus = std::collections::BTreeSet::new();
        for pos in sel.regions() {
            let mut n = 0;
            for c in chunks_de(&sel, pos) {
                assert_eq!(c.region(), pos, "{c:?} n'est pas dans {pos:?}");
                assert!(vus.insert((c.x, c.z)), "{c:?} vu deux fois");
                n += 1;
            }
            assert!(n > 0, "aucune région de la sélection n'est vide");
            total += n;
        }
        assert_eq!(
            total,
            sel.chunks().count(),
            "la somme des régions doit couvrir la sélection, exactement"
        );
    }

    #[test]
    fn une_selection_hors_region_ne_rend_aucun_chunk() {
        let sel = boite((0, 0), (15, 15));
        assert_eq!(chunks_de(&sel, RegionPos { x: 5, z: 5 }).count(), 0);
        // Et le bloc −1 est dans la région −1, pas la région 0.
        let sel = boite((-1, -1), (-1, -1));
        assert_eq!(chunks_de(&sel, RegionPos { x: 0, z: 0 }).count(), 0);
        assert_eq!(chunks_de(&sel, RegionPos { x: -1, z: -1 }).count(), 1);
    }

    #[test]
    fn les_coordonnees_negatives_tombent_dans_la_bonne_region() {
        // Le piège le mieux documenté du dépôt : une division entière naïve
        // charge la mauvaise moitié du monde sans rien signaler.
        let sel = boite((-1, -1), (0, 0));
        let mut par_region: std::collections::BTreeMap<(i32, i32), Vec<(i32, i32)>> =
            Default::default();
        for pos in sel.regions() {
            for c in chunks_de(&sel, pos) {
                par_region
                    .entry((pos.x, pos.z))
                    .or_default()
                    .push((c.x, c.z));
            }
        }
        assert_eq!(par_region[&(-1, -1)], vec![(-1, -1)]);
        assert_eq!(par_region[&(0, -1)], vec![(0, -1)]);
        assert_eq!(par_region[&(-1, 0)], vec![(-1, 0)]);
        assert_eq!(par_region[&(0, 0)], vec![(0, 0)]);
    }
}
