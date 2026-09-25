//! Le presse-papiers : un extrait de monde, détaché de sa save.
//!
//! C'est ce sur quoi reposent `//copy`, `//paste`, `//rotate` et `//flip` — et
//! c'est le premier endroit où les règles de transformation de `tf-blocks`
//! servent à autre chose qu'à être mesurées. Elles couvrent 99,3 % des
//! rotations Minefield depuis un moment ; rien ne les appelait.
//!
//! ## Une transformation se fait sur la PALETTE
//!
//! Tourner un build d'un quart de tour, c'est deux choses : déplacer les cases,
//! et transformer les ÉTATS — un escalier qui regardait l'est regarde le sud.
//!
//! La seconde ne se fait pas par bloc. Un extrait d'un million de cases porte
//! deux cents états distincts : on transforme les deux cents, on en fait une
//! table de correspondance, et le parcours des cases n'est plus qu'une
//! indirection. C'est le même raisonnement que l'étage palette des opérations,
//! appliqué au presse-papiers — et un test le fige en comptant les appels.
//!
//! ## Ce qu'on ne sait pas transformer, on n'y touche pas
//!
//! Un bloc dont la table ne connaît pas la rotation reste tel quel, et il est
//! SIGNALÉ. Le supposer symétrique produirait un build subtilement faux : une
//! moitié tournée, l'autre non, et rien à l'écran pour le dire.

use tf_anvil::entites::Entite;
use tf_anvil::mobiles::Mobile;
use tf_anvil::{Interner, StateId};

use crate::mobiles::Approche;
use tf_blocks::Transfo;

/// Un extrait de monde, en coordonnées LOCALES.
///
/// Les cases sont rangées en **YZX**, comme partout ailleurs dans le dépôt :
/// `i = (y × sz + z) × sx + x`. Une seconde convention d'ordre ici ferait
/// sortir les builds en miroir un jour sur deux.
#[derive(Debug, Clone, PartialEq)]
pub struct Presse {
    /// Dimensions en blocs, dans l'ordre X, Y, Z.
    pub taille: [u32; 3],
    /// Un état par case. Longueur = produit des dimensions.
    pub blocs: Vec<StateId>,
    /// Le point que `//paste` remettra là où l'on est, relatif au coin de plus
    /// petites coordonnées.
    ///
    /// Il suit les transformations comme le reste : sans lui, un build tourné
    /// se collerait décalé de sa propre largeur, ce qui se lit « le collage est
    /// cassé » et ne désigne pas la cause. Il peut sortir de la boîte — on
    /// copie souvent depuis l'extérieur de sa sélection.
    pub ancre: [i32; 3],
    /// Les block entities de l'extrait, **en coordonnées LOCALES** — le même
    /// repère que `blocs`, celui du coin de plus petites coordonnées.
    ///
    /// Elles ne sont pas dans la grille : un coffre est une entrée à part,
    /// avec ses propres coordonnées. Les oublier fait qu'un build pivoté
    /// abandonne ses coffres, et rien ne le signale avant qu'on en ouvre un.
    pub entites: Vec<Entite>,
    /// Les ENTITÉS de l'extrait — cadres, tableaux, porte-armures, bêtes —
    /// elles aussi en LOCAL : position continue relative au même coin.
    pub mobiles: Vec<Mobile>,
}

/// Ce qu'une transformation a produit, et ce qu'elle n'a pas su faire.
#[derive(Debug, Clone, PartialEq)]
pub struct Transforme {
    pub presse: Presse,
    /// Les états que la règle n'a pas su transformer, laissés TELS QUELS.
    ///
    /// Rendus plutôt que tus : à moitié tourné, un build est faux d'une façon
    /// qu'aucune capture d'écran ne montre.
    pub intacts: Vec<StateId>,
    /// Ce que la transformation des ENTITÉS n'a pas su porter exactement —
    /// une pose de porte-armure sous miroir, un tableau de mod. Même raison.
    pub approches: Vec<Approche>,
}

impl Presse {
    /// Un extrait vide de cette taille, tout à l'état donné.
    pub fn uniforme(taille: [u32; 3], id: StateId) -> Presse {
        Presse {
            blocs: vec![id; Presse::volume(taille)],
            taille,
            ancre: [0, 0, 0],
            entites: Vec::new(),
            mobiles: Vec::new(),
        }
    }

    pub fn volume(taille: [u32; 3]) -> usize {
        taille[0] as usize * taille[1] as usize * taille[2] as usize
    }

    /// L'index d'une case, en YZX. `None` hors de la boîte.
    pub fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        let [sx, sy, sz] = self.taille;
        if x >= sx || y >= sy || z >= sz {
            return None;
        }
        Some((y as usize * sz as usize + z as usize) * sx as usize + x as usize)
    }

    pub fn get(&self, x: u32, y: u32, z: u32) -> Option<StateId> {
        self.index(x, y, z).map(|i| self.blocs[i])
    }

    /// Les états distincts présents, triés. C'est la « palette » de l'extrait.
    pub fn palette(&self) -> Vec<StateId> {
        let mut v = self.blocs.clone();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// L'extrait transformé : les cases déplacées, les états réécrits.
    ///
    /// `regle` rend la clé d'état transformée, ou `None` si elle ne sait pas —
    /// injectée plutôt qu'importée pour que `tf-ops` se teste sans pack, et
    /// pour que le jour où une autre source de règles arrive (un mod, une table
    /// écrite à la main pour un cas tordu) elle se branche ici sans toucher à
    /// l'opération.
    pub fn transformer(
        &self,
        t: Transfo,
        interner: &mut Interner,
        regle: &dyn Fn(&str, Transfo) -> Option<String>,
    ) -> Transforme {
        // ── 1. la palette, et elle SEULE
        let palette = self.palette();
        let mut vers: std::collections::HashMap<StateId, StateId> =
            std::collections::HashMap::with_capacity(palette.len());
        let mut intacts = Vec::new();
        for id in palette {
            let Some(cle) = interner.resolve(id).map(str::to_string) else {
                // Un identifiant qu'aucun interner ne résout ne se devine pas.
                intacts.push(id);
                vers.insert(id, id);
                continue;
            };
            match regle(&cle, t) {
                Some(neuve) => {
                    let n = interner.intern(&neuve);
                    vers.insert(id, n);
                }
                None => {
                    intacts.push(id);
                    vers.insert(id, id);
                }
            }
        }

        // ── 2. la géométrie
        let [sx, sy, sz] = self.taille;
        let taille = t.taille_apres(self.taille);
        let mut blocs = vec![StateId::default(); Presse::volume(taille)];
        let [nx, _, nz] = taille;
        for y in 0..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let (ax, az) = t.case_apres((x, z), (sx, sz));
                    let src = (y as usize * sz as usize + z as usize) * sx as usize + x as usize;
                    let dst = (y as usize * nz as usize + az as usize) * nx as usize + ax as usize;
                    blocs[dst] = vers[&self.blocs[src]];
                }
            }
        }

        // ── 3. les block entities
        //
        // Leur CASE suit la même formule que celle des blocs ; leur CONTENU
        // n'est pas touché. Aucune block entity vanilla ne porte d'orientation
        // — un coffre, un escalier, une bannière la portent dans leur état de
        // bloc, donc la table de rotation s'en occupe déjà. Tourner en plus le
        // contenu le ferait deux fois.
        let entites = self
            .entites
            .iter()
            .map(|e| Entite {
                case: t.point_apres(e.case, self.taille),
                nbt: e.nbt.clone(),
                champs: e.champs,
            })
            .collect();

        // ── 4. les entités
        //
        // Elles, si : leur position est continue, leur lacet tourne, un cadre
        // change de face et un tableau de mur. Ce qui ne se transforme pas
        // exactement est NOMMÉ, jamais deviné.
        let mut approches = Vec::new();
        let mobiles = self
            .mobiles
            .iter()
            .map(|m| crate::mobiles::transformer_mobile(m, t, self.taille, &mut approches))
            .collect();

        Transforme {
            presse: Presse {
                taille,
                blocs,
                ancre: t.point_apres(self.ancre, self.taille),
                entites,
                mobiles,
            },
            intacts,
            approches,
        }
    }
}

/// Ce qu'une transformation fait à une BOÎTE — sa taille, ses cases, un point.
///
/// Écrit une fois ici et pas dans l'opération : les trois doivent s'accorder,
/// et deux d'entre elles écrites à deux endroits finiraient par diverger. La
/// convention est celle de tout le dépôt, celle que `tf-blocks` applique à la
/// géométrie d'un modèle : **un quart de tour envoie `+X` sur `+Z`**.
pub trait TransfoBoite {
    fn taille_apres(self, taille: [u32; 3]) -> [u32; 3];
    fn case_apres(self, xz: (u32, u32), taille: (u32, u32)) -> (u32, u32);
    fn point_apres(self, p: [i32; 3], taille: [u32; 3]) -> [i32; 3];
}

impl TransfoBoite for Transfo {
    fn taille_apres(self, [sx, sy, sz]: [u32; 3]) -> [u32; 3] {
        match self {
            // Un quart de tour échange la largeur et la profondeur. L'oublier
            // rendrait un extrait non carré tronqué d'un côté et vide de
            // l'autre.
            Transfo::Rot90 | Transfo::Rot270 => [sz, sy, sx],
            _ => [sx, sy, sz],
        }
    }

    fn case_apres(self, (x, z): (u32, u32), (sx, sz): (u32, u32)) -> (u32, u32) {
        match self {
            Transfo::Rot90 => (sz - 1 - z, x),
            Transfo::Rot180 => (sx - 1 - x, sz - 1 - z),
            Transfo::Rot270 => (z, sx - 1 - x),
            Transfo::MiroirX => (sx - 1 - x, z),
            Transfo::MiroirZ => (x, sz - 1 - z),
        }
    }

    /// Le même calcul, pour un point qui peut SORTIR de la boîte.
    ///
    /// L'ancre est souvent dehors — on copie depuis là où l'on se tient. La
    /// formule est celle des cases, sans la borne : `sz - 1 - z` s'écrit
    /// `sz as i32 - 1 - z` et reste juste pour un `z` négatif.
    fn point_apres(self, [x, y, z]: [i32; 3], [sx, _, sz]: [u32; 3]) -> [i32; 3] {
        let (sx, sz) = (sx as i32, sz as i32);
        let (ax, az) = match self {
            Transfo::Rot90 => (sz - 1 - z, x),
            Transfo::Rot180 => (sx - 1 - x, sz - 1 - z),
            Transfo::Rot270 => (z, sx - 1 - x),
            Transfo::MiroirX => (sx - 1 - x, z),
            Transfo::MiroirZ => (x, sz - 1 - z),
        };
        [ax, y, az]
    }
}

// ── coller ──────────────────────────────────────────────────────────────────

use tf_anvil::section::{in_section, Section, VOL};
use tf_world::coords::{BBox, BlockPos, ChunkPos, LocalBox, SectionPos};

use crate::plan::{Etage, Operation, Rapport};

/// Poser un extrait dans le monde. C'est `//paste`.
///
/// **Un collage est nécessairement à l'étage BLOC**, et c'est la seule
/// opération du crate dont on peut le dire d'avance : ce qu'elle écrit dépend
/// de la POSITION, pas de l'état qu'elle trouve. Aucun raisonnement sur la
/// palette ne peut l'éviter — là où `//replace` réécrit un nom, un collage
/// recopie un extrait.
///
/// La contrepartie est mesurée ailleurs : l'étage bloc traite 100 millions de
/// cases en 951 ms. Un collage ne paie que la taille de son extrait, jamais
/// celle de la sélection.
///
/// **Un collage n'ENGENDRE pas de chunk.** Coller là où le monde n'a jamais
/// été généré n'écrit rien — pas d'erreur, pas de chunk créé. C'est cohérent
/// avec le reste du crate, qui ne crée pas de terrain, mais ça se lit
/// « j'ai collé et il ne s'est rien passé » : le rapport le dit en ne rendant
/// aucun correctif, et un test fige le comportement. Le jour où l'on voudra
/// l'inverse, ce sera une opération à part — engendrer un chunk vide est une
/// décision, pas un effet de bord d'un collage.
pub struct Collage<'a> {
    pub presse: &'a Presse,
    /// Où atterrit le coin de plus petites coordonnées de l'extrait, en MONDE.
    pub coin: BlockPos,
    /// L'air de l'extrait écrase-t-il ce qui est là ?
    ///
    /// `false` est le défaut de WorldEdit, et c'est le bon : on colle presque
    /// toujours un bâtiment sur un terrain, pas un cube d'air. `true` sert à
    /// reposer un extrait à l'identique, trous compris.
    pub avec_air: bool,
    /// Ce qui compte comme air DANS l'extrait.
    ///
    /// Passé plutôt que deviné : un `StateId` n'a de sens que relativement à
    /// son interner, et chercher « minecraft:air » ici obligerait le collage à
    /// en porter un.
    pub air: StateId,
    pub compter: bool,
}

impl Collage<'_> {
    /// La boîte MONDE que l'extrait occupe. C'est la sélection à passer à
    /// `appliquer` — un collage ne paie que son extrait.
    pub fn bornes(&self) -> BBox {
        let [sx, sy, sz] = self.presse.taille;
        BBox::new(
            self.coin,
            BlockPos {
                x: self.coin.x + sx as i32 - 1,
                y: self.coin.y + sy as i32 - 1,
                z: self.coin.z + sz as i32 - 1,
            },
        )
    }
}

impl Operation for Collage<'_> {
    fn appliquer(&self, section: &mut Section, sel: &BBox, pos: SectionPos) -> Rapport {
        let Some(coupe) = sel.clip_to_section(pos) else {
            return Rapport::RIEN;
        };
        let origine = pos.min_block();
        let mut ecrits = 0u64;

        // Le dépack se fait UNE fois pour la section, pas par case : reposer
        // la question à chaque bloc rendrait le collage quadratique en la
        // taille de la palette.
        let mut idx = section.unpack();
        let mut palette = section.palette.clone();
        let avant_palette = palette.len();

        for ly in coupe.y0..=coupe.y1 {
            for lz in coupe.z0..=coupe.z1 {
                for lx in coupe.x0..=coupe.x1 {
                    let (mx, my, mz) = (
                        origine.x + lx as i32,
                        origine.y + ly as i32,
                        origine.z + lz as i32,
                    );
                    let (px, py, pz) = (mx - self.coin.x, my - self.coin.y, mz - self.coin.z);
                    if px < 0 || py < 0 || pz < 0 {
                        continue;
                    }
                    let Some(id) = self.presse.get(px as u32, py as u32, pz as u32) else {
                        continue;
                    };
                    if id == self.air && !self.avec_air {
                        continue;
                    }
                    debug_assert!(in_section(lx, ly, lz));
                    let i = (ly * 256 + lz * 16 + lx).min(VOL - 1);

                    // **La case porte-t-elle DÉJÀ cet état ?** On le demande à
                    // son indice actuel, pas à la palette.
                    //
                    // Chercher l'état dans la palette et comparer les INDICES
                    // est faux, et c'est l'invariant n° 4 du dépôt : la
                    // palette ne dédoublonne pas, donc le même état y figure
                    // parfois deux fois. Un `position()` rend la PREMIÈRE
                    // occurrence, et une case qui pointait sur la seconde se
                    // voyait réécrite — même valeur, indice différent, donc
                    // des octets différents.
                    //
                    // Trouvé sur un vrai monde 1.20, et par rien d'autre :
                    // reposer un extrait à l'identique produisait six
                    // correctifs de journal pour zéro changement. Aucune
                    // fixture n'a de palette dédoublonnée — seule une save
                    // qu'un `//replace` a déjà traversée en a.
                    if palette.get(idx[i] as usize) == Some(&id) {
                        continue;
                    }
                    let k = match palette.iter().position(|p| *p == id) {
                        Some(k) => k,
                        None => {
                            palette.push(id);
                            palette.len() - 1
                        }
                    };
                    idx[i] = k as u16;
                    ecrits += 1;
                }
            }
        }

        // **Une section que le collage n'a pas changée ne doit pas être
        // TOUCHÉE du tout**, pas même réassignée.
        //
        // Annoncer l'étage bloc fait passer la section par `section_edits`,
        // qui la ré-encode pour la comparer. Or le ré-encodage ne reproduit
        // pas toujours les octets d'origine : une propriété d'état que le
        // fichier écrit dans un autre ordre ressort normalisée, et les octets
        // diffèrent alors que le CONTENU est identique.
        //
        // Mesuré sur un vrai monde 1.20 : reposer un extrait à sa propre
        // place produisait six correctifs de journal pour zéro changement. Une
        // fixture ne pouvait pas le montrer — elle écrit ses propriétés dans
        // l'ordre où le décodeur les relit.
        if ecrits == 0 {
            return Rapport::RIEN;
        }
        debug_assert!(
            palette.len() >= avant_palette,
            "une palette ne rétrécit pas ici"
        );
        section.palette = palette;
        section.repack(&idx);
        Rapport {
            etage: Etage::Bloc,
            blocs: self.compter.then_some(ecrits),
            bornes: bornes_collage(sel, pos, coupe),
        }
    }

    fn compte(&self) -> bool {
        self.compter
    }

    /// Les coffres de l'extrait, traduits en MONDE et coupés à ce chunk.
    ///
    /// Une entité posée sur une case que le collage n'écrit pas serait un
    /// fantôme — une entrée sans bloc pour la porter. C'est pourquoi l'air
    /// sauté l'est ici aussi : sans ça, coller sans l'air un extrait dont un
    /// coffre a été effacé y reposerait son contenu dans le vide.
    fn entites_posees(&self, chunk: ChunkPos) -> Vec<Entite> {
        self.presse
            .entites
            .iter()
            .filter_map(|e| {
                let case = [
                    e.case[0] + self.coin.x,
                    e.case[1] + self.coin.y,
                    e.case[2] + self.coin.z,
                ];
                let p = BlockPos {
                    x: case[0],
                    y: case[1],
                    z: case[2],
                };
                if p.chunk() != chunk {
                    return None;
                }
                let (x, y, z) = (e.case[0], e.case[1], e.case[2]);
                if x < 0 || y < 0 || z < 0 {
                    return None;
                }
                match self.presse.get(x as u32, y as u32, z as u32) {
                    // Hors de l'extrait, ou sur une case que le collage laisse
                    // telle quelle : on ne pose rien.
                    None => None,
                    Some(id) if id == self.air && !self.avec_air => None,
                    Some(_) => Some(Entite {
                        case,
                        nbt: e.nbt.clone(),
                        champs: e.champs,
                    }),
                }
            })
            .collect()
    }
}

/// Les bornes MONDE de ce qu'une section a pu recevoir.
///
/// Une borne SUPÉRIEURE honnête : l'instantané d'annulation et le remaillage
/// s'y fient, et `tf-anvil` compare les octets — une section qui n'a rien
/// changé n'écrit rien même si ses bornes la couvrent.
fn bornes_collage(_sel: &BBox, pos: SectionPos, coupe: LocalBox) -> Option<BBox> {
    let o = pos.min_block();
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
    ))
}
