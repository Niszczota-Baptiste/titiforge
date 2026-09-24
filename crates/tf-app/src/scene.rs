//! Le montage : des assets et un monde vers ce que le GPU dessine.
//!
//! La coque ne décode pas les chunks elle-même et ne lit pas les packs : elle
//! assemble ce que `tf-assets`, `tf-mesh` et `tf-render` savent déjà faire.
//! Ce qu'elle ajoute est la COUTURE vers le monde pour viser — un prédicat,
//! comme le mailleur en prend un.

use tf_anvil::{Interner, StateId};
use tf_mesh::{Grille, TableFormes};
use tf_render::{Arene, AreneModeles, Lignes};
use tf_world::coords::BlockPos;
use tf_world::decoupe::{cellules_autour, Niveau};

use crate::etat::Quadrillage;

/// **Le budget de résidence par défaut**, en octets.
///
/// Mesuré, pas choisi au jugé (`cargo run --release -p tf-app --example
/// residence`) : une région BÂTIE de 1 024 chunks laisse 186 Mo résidents —
/// 85 Mo de grille et 101 de maillage — quand une région de TERRAIN n'en
/// laisse que 28. Le rapport entre les deux est de 6,7, et celui de leurs
/// maillages de 168 : c'est pourquoi le plafond se compte en octets et pas en
/// cellules, et pourquoi il se mesure sur du bâti.
///
/// 1,5 Go font donc huit régions bâties, ou cinquante-quatre de terrain. Le
/// reste de l'enveloppe d'un processus 64 bits va aux textures, au pack, aux
/// tampons du GPU et à la copie de travail.
///
/// **C'est une cible, pas une limite dure** : rien de modifié ni d'épinglé
/// n'est jamais évincé pour la tenir. Et c'est un défaut, réglable par
/// `budget_residence` — une machine à 8 Go et une à 64 ne veulent pas le
/// même.
pub const BUDGET_RESIDENCE: usize = 1_500_000_000;

/// Ce qu'une cellule résidente coûte, et de quoi la retirer.
///
/// La fenêtre de résidence est ici un COMPTABLE, pas un magasin : les blocs
/// restent dans la grille et le maillage dans le chantier. Elle ne tient que
/// le poids et la récence, et dit quoi évincer. Y ranger les sections
/// elles-mêmes obligerait à les en ressortir à chaque lecture de bloc, c'est-
/// à-dire des millions de fois par opération.
struct Resident {
    cellule: tf_world::Cellule,
    octets: usize,
}

impl tf_world::Weighed for Resident {
    /// Le poids de la CELLULE, pas celui de la fiche.
    ///
    /// La `Cellule` gardée ici pèse une quarantaine d'octets qu'on ne compte
    /// pas exprès : les additionner ferait diverger `Residency::used` de ce
    /// que la scène porte vraiment, et c'est justement cette égalité qui rend
    /// la comptabilité vérifiable au lieu d'être une fiction.
    fn bytes(&self) -> usize {
        self.octets
    }
}

/// La clé d'une cellule résidente.
///
/// Le NIVEAU en fait partie : la cellule de chunk (3, 4) et la cellule de
/// région (3, 4) ne désignent pas la même chose, et une clé qui les
/// confondrait ferait évincer 512 × 512 blocs en croyant en lâcher 16 × 16.
type Cle = (Niveau, i32, i32);

/// Un maillage parti hors du fil principal, et revenu.
struct Fini {
    id: u64,
    visees: Vec<tf_mesh::Adresse>,
    chantier: tf_mesh::Chantier,
    /// Ce que le maillage a coûté au fil qui l'a fait — pour la découpe.
    duree: std::time::Duration,
    /// Le mailleur a paniqué : le chantier est vide, et on le DIT.
    panique: bool,
}

/// **Le maillage, hors du fil principal.**
///
/// Mesuré en vol sur du bâti, le maillage prenait 3,5 ms par image en
/// médiane et jusqu'à 7 — l'essentiel de la queue au-delà de 8 ms — pour un
/// travail que rien n'oblige à faire sur le fil qui dessine. Il part donc
/// avec un EXTRAIT de la grille (`Grille::extrait`) sur la réserve de fils,
/// et revient plus tard.
///
/// **Les résultats s'appliquent dans l'ORDRE où les travaux sont partis**,
/// jamais dans celui où ils reviennent. Chaque extrait est la grille à son
/// départ ; appliqués dans l'ordre, ils rendent exactement la scène qu'un
/// maillage synchrone aurait rendue — une section refaite par deux travaux
/// finit avec le plus récent. Dans le désordre, un vieux maillage pourrait
/// écraser un neuf.
struct Atelier {
    envoi: std::sync::mpsc::Sender<Fini>,
    retour: std::sync::mpsc::Receiver<Fini>,
    /// Numéro du prochain travail soumis.
    prochain: u64,
    /// Numéro du prochain travail à APPLIQUER.
    attendu: u64,
    /// Travaux revenus avant leur tour.
    prets: std::collections::BTreeMap<u64, Fini>,
    /// Travaux appliqués depuis l'ouverture : ce qui dit à l'hôte que les
    /// arènes ont changé.
    appliques: u64,
    /// Un retard imposé au PROCHAIN travail — pour les tests, qui doivent
    /// pouvoir faire revenir un travail après un plus récent. Sans lui,
    /// l'ordre d'application ne se vérifierait que par chance.
    retard: Option<std::time::Duration>,
}

/// **La réserve de fils du maillage hors fil** — un cœur de moins que la
/// machine.
///
/// La réserve globale de `rayon` prend TOUS les cœurs : le fil principal s'y
/// retrouvait à six fils pour quatre cœurs avec le fil de chargement, et
/// perdait la main au hasard, au milieu de n'importe quelle phase. Mesuré en
/// vol sur du bâti : médiane 3,3 à 4,5 ms et p95 7 à 10 ms avec quatre fils
/// de maillage, 2,7 et 5,4 avec trois — pour le même nombre de cellules
/// posées. Le cœur laissé libre est celui du fil qui dessine.
///
/// Une par processus, pas une par monde ouvert : le compte des cœurs est une
/// affaire de machine.
fn reserve_de_maillage() -> &'static rayon::ThreadPool {
    static RESERVE: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    RESERVE.get_or_init(|| {
        let coeurs = std::thread::available_parallelism().map_or(2, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(coeurs.saturating_sub(1).max(1))
            .thread_name(|i| format!("titiforge-maillage-{i}"))
            .build()
            .expect("la réserve de fils du maillage")
    })
}

impl Atelier {
    fn neuf() -> Atelier {
        let (envoi, retour) = std::sync::mpsc::channel();
        Atelier {
            envoi,
            retour,
            prochain: 0,
            attendu: 0,
            prets: std::collections::BTreeMap::new(),
            appliques: 0,
            retard: None,
        }
    }

    fn en_vol(&self) -> usize {
        (self.prochain - self.attendu) as usize
    }

    /// Range ce qui est revenu. Un travail d'avant un rechargement est JETÉ :
    /// il maillait un monde qui n'est plus.
    fn ranger(&mut self, f: Fini) {
        if f.id >= self.attendu {
            self.prets.insert(f.id, f);
        }
    }

    /// Oublie ce qui est en vol : un rechargement remplace le monde entier.
    fn oublier(&mut self) {
        self.attendu = self.prochain;
        self.prets.clear();
    }
}

/// **Les adresses de section qu'une cellule couvre**, déduites de sa
/// géométrie.
///
/// Pas d'un balayage de la grille : `Grille::adresses` alloue et trie la
/// scène entière, donc l'appeler une fois par cellule coûte O(scène ×
/// cellules) là où la cellule sait elle-même ce qu'elle couvre. Et le
/// balayage filtrait sur `a.0 == cellule.x`, ce qui n'est vrai qu'au niveau
/// CHUNK — une cellule de RÉGION en couvre 32 × 32, donc on n'en retirait
/// qu'un millième.
fn adresses_de(c: &tf_world::Cellule) -> Vec<tf_mesh::Adresse> {
    let b = &c.boite;
    let (cx0, cx1) = (b.min.x.div_euclid(16), b.max.x.div_euclid(16));
    let (cz0, cz1) = (b.min.z.div_euclid(16), b.max.z.div_euclid(16));
    let (sy0, sy1) = (b.min.y.div_euclid(16), b.max.y.div_euclid(16));
    let mut v = Vec::new();
    for cz in cz0..=cz1 {
        for cx in cx0..=cx1 {
            for sy in sy0..=sy1 {
                v.push((cx, cz, sy as i8));
            }
        }
    }
    v
}

/// Tout ce qu'une scène chargée porte.
pub struct Monde {
    pub grille: Grille,
    /// Partagée avec le fil de maillage : l'étendre la recopie si un travail
    /// en cours la tient encore (`Arc::make_mut`), ce qui n'arrive qu'à
    /// l'arrivée d'un état jamais vu.
    pub table: std::sync::Arc<TableFormes>,
    pub arene: Arene,
    pub modeles: AreneModeles,
    pub atlas: tf_assets::Atlas,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub quoi: String,
    pub quads: usize,
    pub poses: usize,
    /// Le maillage, gardé LOT PAR LOT et indexé par section.
    ///
    /// C'est ce qui rend le remaillage incrémental possible : on remplace les
    /// lots des sections touchées au lieu de remailler le monde entier pour
    /// trois blocs — et on les remplace en O(visées), pas en O(scène)
    /// (`tf_mesh::Maillages`).
    maillages: tf_mesh::Maillages,
    /// L'habillage, indexé par `StateId`. Sa LONGUEUR est aussi le nombre
    /// d'états que l'atlas connaît : un état au-delà n'a pas de texture.
    habillage: Vec<tf_assets::apparence::Habillage>,
    /// La table d'états de CETTE scène. Un `StateId` n'a de sens que
    /// relativement à elle.
    interner: Interner,
}

impl Monde {
    /// **La couture vers le monde, pour viser.** Un prédicat, comme le
    /// mailleur en prend un : la coque demande « cette case arrête-t-elle le
    /// rayon ? » et ne sait rien d'autre.
    /// **Le nom de l'état posé en `(x, y, z)`**, tel que la scène le tient —
    /// `minecraft:air` hors de ce qu'elle porte.
    ///
    /// Ce que la pipette de la coque lira, et ce qu'un test peut comparer
    /// sans connaître la table d'états : un `StateId` n'a de sens que
    /// relativement à elle.
    pub fn etat_en(&self, x: i32, y: i32, z: i32) -> &str {
        self.interner
            .resolve(self.grille.bloc(x, y, z))
            .unwrap_or("minecraft:air")
    }

    pub fn solide(&self) -> impl Fn([i32; 3]) -> bool + '_ {
        move |c| {
            let id = self.grille.bloc(c[0], c[1], c[2]);
            !tf_mesh::forme::Formes::est_air(&*self.table, id)
        }
    }
}

/// **Le pack, lu UNE fois.**
///
/// Le séparer du monde n'est pas de la cosmétique : sur le pack du serveur,
/// ouvrir le catalogue, bâtir l'atlas complet et classer les translucides
/// coûte des secondes. Une opération d'édition remaille la zone ; elle ne doit
/// pas relire deux mille modèles au passage.
pub struct Assets {
    cat: tf_assets::Catalogue,
    src: tf_assets::Pile,
    disposition: tf_assets::catalogue::Disposition,
    translucides: std::collections::BTreeSet<String>,
    climat: tf_assets::climat::Climat,
    teintes: tf_assets::Teintes,
}

impl Assets {
    pub fn charger(racine: &str) -> Result<Assets, String> {
        let (cat, src, genre) =
            tf_assets::jeu::catalogue(racine).map_err(|e| format!("assets illisibles : {e:?}"))?;
        let disposition = genre.disposition();
        let atlas_complet =
            tf_assets::Atlas::batir(&src, tf_assets::catalogue::textures_citees(&cat), &|n| {
                disposition.chemins_texture(n)
            });
        let translucides = tf_assets::catalogue::blocs_translucides(&cat, &atlas_complet);
        let climat = tf_assets::climat::Climat::charger(&src);
        Ok(Assets {
            cat,
            src,
            disposition,
            translucides,
            climat,
            teintes: tf_assets::Teintes::default(),
        })
    }
}

/// **Une cellule que le fil de chargement vient de rendre.**
///
/// Elle porte sa TABLE d'états : les identifiants de ses palettes sont
/// numérotés dedans et nulle part ailleurs. C'est la même structure que
/// `chargeur::Reponse::Prete`, mais `scene` ne dépend pas du chargeur — un
/// hôte qui lirait autrement (un test, un import de schematic) passe par la
/// même porte.
pub struct Arrivee {
    pub cellule: tf_world::Cellule,
    pub sections: Vec<tf_world::lecture::SectionLue>,
    pub interner: Interner,
}

/// D'où viennent les chunks.
///
/// **La copie de travail compte autant que la save.** Après une opération, ce
/// qu'il faut redessiner est ce que le staging porte — relire la source
/// rendrait le monde d'AVANT, ce qui se lit « le bouton ne fait rien ».
pub enum Ou<'a> {
    /// La fixture de BUILD, quand aucun monde n'est ouvert.
    Fixture,
    /// Une source quelconque : une save, ou la copie de travail par-dessus.
    Source(
        &'a (dyn tf_world::source::RegionSource + 'a),
        [i32; 4],
        String,
    ),
}

/// Charge un monde — une VRAIE save, ou la fixture — avec des assets déjà lus.
///
/// La borne est toujours EXPLICITE : il n'existe aucun état « le monde est
/// chargé », une région pleine faisant déjà cent millions de blocs. Sans
/// `zone`, on prend un petit rectangle de chunks, ce qui est un aperçu et pas
/// un défaut à étendre.
pub fn charger_monde(a: &Assets, ou: Ou) -> Result<Monde, String> {
    let (cat, src) = (&a.cat, &a.src);
    let disposition = a.disposition;
    let mut grille = Grille::new();
    let mut interner = Interner::new();
    let quoi = match ou {
        Ou::Source(source, zone, nom) => {
            let [x0, z0, x1, z1] = zone;
            let sel = tf_world::BBox::new(
                BlockPos::new(x0 * 16, -64, z0 * 16),
                BlockPos::new(x1 * 16 + 15, 319, z1 * 16 + 15),
            );
            let bilan = tf_world::sections_de(
                source,
                &tf_world::Dimension::Overworld,
                tf_world::Folder::Region,
                &sel,
                &mut interner,
                |s| {
                    let y = s.section.y;
                    if let Some(b) = s.biomes {
                        grille.poser_biomes(s.chunk.x, s.chunk.z, y, b);
                    }
                    grille.poser(s.chunk.x, s.chunk.z, s.section);
                },
            );
            format!(
                "{nom} · chunks {x0}..{x1} × {z0}..{z1} · {} chunks, {} sections",
                bilan.chunks, bilan.sections
            )
        }
        Ou::Fixture => {
            // La fixture de BUILD : un bâtiment décoré, pas du terrain. C'est
            // elle qui mesure le rendu dans tout le dépôt — la confondre avec
            // `Terrain` fausserait la comparaison.
            let b = tf_bench::Build {
                side: 4,
                sections: 5,
                ..Default::default()
            };
            let octets = tf_bench::build::region(&b);
            let r = tf_anvil::read(&octets, 0, 0).map_err(|e| format!("fixture : {e:?}"))?;
            for cz in 0..b.side as i32 {
                for cx in 0..b.side as i32 {
                    let Some(brut) = r.get(cx, cz) else { continue };
                    let inflated = tf_anvil::inflate(&brut.payload, brut.compression)
                        .map_err(|e| format!("fixture : {e:?}"))?;
                    let sc = tf_anvil::scan(&inflated).map_err(|e| format!("fixture : {e:?}"))?;
                    for sec in &sc.sections {
                        if let Ok(Some(s)) =
                            tf_anvil::decode_section(&inflated, &sc, sec, &mut interner)
                        {
                            grille.poser(cx, cz, s);
                        }
                    }
                }
            }
            format!("fixture · {} × {} blocs", b.side * 16, b.side * 16)
        }
    };

    let cles: Vec<String> = (0..interner.len() as StateId)
        .map(|i| interner.resolve(i).unwrap_or("minecraft:air").to_string())
        .collect();
    // **On ne monte que les textures des blocs PRÉSENTS.** Un tableau de
    // textures est plafonné à 2 048 couches, et le pack du serveur en cite
    // 2 207 : tout charger dépasse la limite ET paie ce qu'aucun bloc de la
    // scène n'emploie.
    let voulues = tf_assets::textures_des_etats(cat, cles.iter().cloned());
    let atlas = tf_assets::Atlas::batir(src, voulues, &|n| disposition.chemins_texture(n));
    let (climat, teintes) = (&a.climat, &a.teintes);
    let (table, habillage) =
        tf_assets::table_rendu(cat, &atlas, teintes, cles.iter().cloned(), &|n| {
            a.translucides.contains(n)
        });

    let chantier = grille.mailler_parallele(&table);
    let (arene, modeles) = arenes(&chantier, &table, &habillage, &interner, climat);

    let (min, max) = arene.bornes().unwrap_or(([0.0; 3], [64.0; 3]));
    Ok(Monde {
        quads: chantier.quads(),
        poses: chantier.poses(),
        maillages: tf_mesh::Maillages::depuis(chantier),
        quoi,
        grille,
        table: std::sync::Arc::new(table),
        arene,
        modeles,
        atlas,
        min,
        max,
        habillage,
        interner,
    })
}

/// **Les deux arènes GPU, depuis un chantier.** Écrite une fois : le
/// chargement et le remaillage incrémental y passent tous les deux, et deux
/// copies finiraient par teinter différemment ce qui vient d'être édité.
/// L'habillage d'une FACE : sa couche d'atlas et sa teinte finale.
///
/// Écrite une fois et partagée par le chemin complet et le chemin
/// incrémental. Deux copies décideraient de la couleur de chaque face, et
/// elles divergeraient le jour où l'une apprend une teinte que l'autre ignore
/// — ce dépôt a payé QUATRE fois le piège des tables qui divergent.
fn apparence<'a>(
    habillage: &'a [tf_assets::apparence::Habillage],
    interner: &'a Interner,
    climat: &'a tf_assets::climat::Climat,
) -> impl Fn(StateId, tf_mesh::forme::Face, StateId) -> (u32, [f32; 3]) + 'a {
    move |id, face, biome| match habillage.get(id as usize) {
        Some(h) => {
            let a = h.cube[face.indice()];
            (
                a.couche,
                teinte_de(interner, climat, a.genre, biome).unwrap_or(a.teinte),
            )
        }
        None => (0, [1.0; 3]),
    }
}

/// La teinte de biome d'un genre donné, ou rien.
fn teinte_de(
    interner: &Interner,
    climat: &tf_assets::climat::Climat,
    genre: tf_assets::GenreTeinte,
    biome: StateId,
) -> Option<[f32; 3]> {
    if genre == tf_assets::GenreTeinte::Aucune {
        return None;
    }
    let nom = interner.resolve(biome)?;
    let c = match genre {
        tf_assets::GenreTeinte::Herbe => climat.herbe(nom),
        tf_assets::GenreTeinte::Feuillage => climat.feuillage(nom),
        tf_assets::GenreTeinte::Eau => climat.eau(nom),
        tf_assets::GenreTeinte::Aucune => None,
    }?;
    Some(tf_assets::apparence::teinte_finale(c))
}

fn arenes(
    chantier: &tf_mesh::Chantier,
    table: &TableFormes,
    habillage: &[tf_assets::apparence::Habillage],
    interner: &Interner,
    climat: &tf_assets::climat::Climat,
) -> (Arene, AreneModeles) {
    let arene = Arene::depuis(chantier, &apparence(habillage, interner, climat));
    let modeles = arene_modeles(chantier, table, habillage, interner, climat);
    (arene, modeles)
}

/// **La passe de modèles, seule.**
///
/// Séparée parce que le chemin incrémental n'a plus besoin de la passe des
/// quads : les appeler ensemble faisait rebâtir l'arène des quads pour la
/// jeter aussitôt — mesuré, la correction ne gagnait rien du tout, et le
/// chiffre l'a dit avant que je l'annonce.
fn arene_modeles(
    chantier: &tf_mesh::Chantier,
    table: &TableFormes,
    habillage: &[tf_assets::apparence::Habillage],
    interner: &Interner,
    climat: &tf_assets::climat::Climat,
) -> AreneModeles {
    AreneModeles::depuis(chantier, &modele_de(table, habillage, interner, climat))
}

/// La géométrie d'un état pour un biome : ses faces, habillées.
///
/// Écrite une fois et partagée par le chemin complet et le chemin
/// incrémental, comme [`apparence`] — deux copies décideraient de la FORME de
/// chaque bloc-modèle.
fn modele_de<'a>(
    table: &'a TableFormes,
    habillage: &'a [tf_assets::apparence::Habillage],
    interner: &'a Interner,
    climat: &'a tf_assets::climat::Climat,
) -> impl Fn(StateId, StateId) -> Vec<tf_render::FaceModele> + 'a {
    let teinte_de = move |genre: tf_assets::GenreTeinte, biome: StateId| -> Option<[f32; 3]> {
        teinte_de(interner, climat, genre, biome)
    };
    move |id, biome| {
        let Some(h) = habillage.get(id as usize) else {
            return Vec::new();
        };
        let hab: Vec<tf_render::HabillageFaces> = h
            .cuboides
            .iter()
            .map(|f| {
                std::array::from_fn(|k| {
                    (
                        f[k].couche,
                        teinte_de(f[k].genre, biome).unwrap_or(f[k].teinte),
                        f[k].uv,
                    )
                })
            })
            .collect();
        tf_render::faces_de(tf_mesh::forme::Formes::cuboides(table, id), &hab)
    }
}

/// **Le monde OUVERT : le pack, la copie de travail, et ce que le GPU dessine.**
///
/// Les trois ensemble parce qu'ils ne se séparent pas en pratique : remailler
/// après une opération demande le pack (déjà lu), la copie de travail (pas la
/// save — elle rendrait le monde d'AVANT) et la zone regardée.
pub struct Ouvert {
    pub assets: Assets,
    pub monde: Monde,
    /// La copie de travail, PARTAGÉE avec le fil moteur.
    ///
    /// Le fil écrit, la coque relit. Un `Arc` et pas un verrou : `Staging`
    /// prend `&self` partout, et tout ce qui écrit passe par le fil — la
    /// coque ne fait que lire. `None` pour la fixture, qui n'a pas de save
    /// derrière elle et n'est donc pas éditable.
    pub staging: Option<std::sync::Arc<tf_world::Staging<tf_world::FsSource, tf_world::FsSource>>>,
    pub zone: [i32; 4],
    pub nom: String,
    /// Le dossier temporaire de la copie de travail, à effacer en partant.
    couche: Option<std::path::PathBuf>,
    /// **Combien de fois la ZONE ENTIÈRE a été rechargée.**
    ///
    /// Un compteur, pas un chronomètre : ce qu'on veut interdire est un
    /// rechargement de zone après une édition de trois blocs, et un
    /// chronomètre ne sait le dire qu'avec un seuil, c'est-à-dire une
    /// opinion. Le compteur, lui, répond oui ou non.
    ///
    /// C'est le défaut qui a coûté le plus cher aux deux applications qui
    /// précèdent celle-ci : `ExeWorldEdit` chauffait le build ENTIER avant
    /// toute opération (`warmup(extent)`) — mesuré chez un utilisateur, une
    /// sphère de 62 blocs prenait 5,2 s sur une sélection de 413 millions de
    /// cases, dont 47 % à décoder des chunks jamais lus et 52 % à recoller
    /// l'aperçu entier. Un test qui lit ce compteur est le seul moyen de ne
    /// pas le repayer une troisième fois.
    pub rechargements: u32,
    /// **Combien de sections le dernier remaillage a refaites.**
    ///
    /// Un compteur, pas un chronomètre — encore une fois, et pour la raison
    /// que ce dépôt répète : les temps ABSOLUS dérivent avec la charge de la
    /// machine (facteur 2,4 mesuré à code identique). Une assertion « une
    /// édition ne paie pas la zone » écrite en millisecondes passe seule et
    /// tombe quand la suite entière tourne en parallèle — c'est arrivé, et
    /// c'est le genre d'échec qui fait douter du code au lieu du test.
    ///
    /// Le nombre de sections refaites, lui, ne dépend de rien : il vaut la
    /// poignée que l'édition a touchée, ou la grille entière quand on
    /// recharge.
    pub sections_remaillees: usize,
    /// **La fenêtre de résidence** : ce que la scène s'autorise à tenir.
    ///
    /// Sans elle, un vol continu ne rend jamais rien — mesuré, une région
    /// bâtie laisse 186 Mo, donc onze tiennent dans 2 Go et la douzième tue
    /// l'application. C'est la moitié du contrat de la phase 5 : la demande
    /// dit quoi charger, la résidence dit quoi LÂCHER.
    ///
    /// Un comptable et pas un magasin : la valeur ne porte que le poids et de
    /// quoi retrouver la cellule (voir `Resident`).
    residence: tf_world::Residency<Cle, Resident>,
    /// **Les cellules évincées qu'il reste à retirer de la scène.**
    ///
    /// Le poids exact d'une cellule n'est connu QU'APRÈS son maillage : le
    /// maillage d'une région bâtie pèse 168 fois celui d'une région de
    /// terrain, donc l'estimer avant reviendrait à inventer un facteur. On
    /// l'inscrit donc après `refaire`, et l'éviction que cette inscription
    /// déclenche est reportée à l'appel SUIVANT.
    ///
    /// Ce report est ce qui garde **une seule recopie d'arène par appel** :
    /// retirer tout de suite demanderait un second `refaire`, c'est-à-dire un
    /// second travail en O(scène) — 27 ms mesurés — à chaque image d'un vol.
    /// C'est exactement la famille de défaut que ce dépôt traque.
    ///
    /// Le dépassement est donc borné par ce qu'UN lot fait évincer, et il se
    /// résorbe au prochain appel. `integrer(vec![])` suffit à le vider : un
    /// hôte qui appelle à chaque image converge même quand plus rien
    /// n'arrive.
    a_degager: Vec<tf_world::Cellule>,
    /// **Les cellules que la caméra REGARDE**, protégées de l'éviction.
    ///
    /// Sans elles, un budget plus petit que le champ de vision fait tourner la
    /// machine à vide pour toujours : le LRU évince une cellule que la caméra
    /// veut encore, la demande la redemande aussitôt, elle arrive, elle en
    /// évince une autre du champ, et ainsi de suite. Mesuré sur un budget à la
    /// moitié du disque : **180 évictions en 100 images**, sans fin, pour une
    /// scène qui n'avance pas d'un bloc — et rien ne le dit, la fenêtre répond
    /// et le monde a l'air de charger.
    ///
    /// Épinglées, elles ne peuvent plus être évincées : le LRU ne prend que
    /// dans la TRAÎNÉE, ce qui est exactement ce qu'on veut lâcher. Et si le
    /// champ à lui seul dépasse le budget, on le DÉPASSE en le disant
    /// (`deborde`) plutôt que de tourner en rond — « le plafond est une CIBLE,
    /// pas une limite dure », et montrer ce que l'utilisateur regarde vaut
    /// mieux que de ne rien montrer.
    ///
    /// L'ensemble qu'on a soi-même épinglé, et pas un compteur remis à zéro :
    /// les épingles se COMPTENT, et une opération en cours peut tenir le même
    /// chunk.
    protegees: std::collections::HashSet<Cle>,
    /// Le budget est-il dépassé faute de candidat évinçable ?
    deborde: bool,
    /// **Combien de cellules ont été RETIRÉES de la scène depuis l'ouverture.**
    ///
    /// Un compteur, comme `rechargements` et `sections_remaillees`. Et
    /// distinct de `Residency::evictions` : évincer, c'est sortir de la
    /// comptabilité ; DÉGAGER, c'est retirer les sections de la grille et
    /// refaire le maillage. Les deux sont séparés d'un appel (voir
    /// `a_degager`), et c'est le second qui change ce qui est à l'écran —
    /// donc le seul qui dise à l'hôte qu'il doit regarnir son GPU.
    degagees: usize,
    /// Le maillage hors du fil principal.
    atelier: Atelier,
}

impl Ouvert {
    pub fn ouvrir(racine: &str, monde: Option<&str>, zone: [i32; 4]) -> Result<Ouvert, String> {
        let assets = Assets::charger(racine)?;
        let Some(dir) = monde else {
            let m = charger_monde(&assets, Ou::Fixture)?;
            return Ok(Ouvert {
                assets,
                monde: m,
                staging: None,
                zone,
                nom: "fixture".into(),
                couche: None,
                rechargements: 0,
                sections_remaillees: 0,
                // **La fixture n'inscrit rien**, et c'est voulu : son contenu
                // ne vient pas de `zone` mais d'un bâtiment engendré en
                // mémoire. Inscrire la zone ferait tenir la comptabilité sur
                // des cellules qui ne correspondent à rien, et évincer
                // effacerait le seul contenu qu'il y ait à montrer.
                residence: tf_world::Residency::new(BUDGET_RESIDENCE),
                a_degager: Vec::new(),
                protegees: std::collections::HashSet::new(),
                deborde: false,
                degagees: 0,
                atelier: Atelier::neuf(),
            });
        };
        // Le ménage d'abord : une séance qui s'est mal terminée a laissé sa
        // copie de travail, et personne ne la verra jamais autrement.
        let balayes = balayer_les_abandons();
        if balayes > 0 {
            eprintln!("{balayes} copie(s) de travail abandonnée(s) effacée(s)");
        }
        let source = tf_world::FsSource::open(dir).map_err(|e| format!("monde : {e:?}"))?;
        // **La copie de travail vit à côté.** La save n'est pas ouverte en
        // écriture tant qu'on ne l'a pas demandé — invariant n° 1.
        //
        // Le nom porte un compteur en plus du processus : deux mondes ouverts
        // en même temps partageraient sinon la même couche, et le second
        // écrirait par-dessus les régions du premier. Trouvé par deux tests
        // qui tournaient en parallèle — un utilisateur qui ouvre deux fenêtres
        // l'aurait trouvé autrement.
        static SUIVANT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SUIVANT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let couche = std::env::temp_dir().join(format!("titiforge-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&couche).map_err(|e| format!("copie de travail : {e}"))?;
        let overlay =
            tf_world::FsSource::open(&couche).map_err(|e| format!("copie de travail : {e:?}"))?;
        let staging = std::sync::Arc::new(tf_world::Staging::new(source, overlay));
        let m = charger_monde(&assets, Ou::Source(staging.as_ref(), zone, dir.to_string()))?;
        let mut o = Ouvert {
            assets,
            monde: m,
            staging: Some(staging),
            zone,
            nom: dir.to_string(),
            couche: Some(couche),
            rechargements: 0,
            sections_remaillees: 0,
            residence: tf_world::Residency::new(BUDGET_RESIDENCE),
            a_degager: Vec::new(),
            protegees: std::collections::HashSet::new(),
            deborde: false,
            degagees: 0,
            atelier: Atelier::neuf(),
        };
        o.inscrire_la_zone();
        Ok(o)
    }

    /// Le plafond de résidence, en octets, et de quoi le changer.
    ///
    /// Réglable parce qu'une machine à 8 Go et une à 64 ne veulent pas le
    /// même — et parce qu'un test doit pouvoir le serrer assez pour que
    /// l'éviction ARRIVE : un budget de 1,5 Go ne se remplit pas avec une
    /// fixture, donc un test qui garderait le défaut ne prouverait rien.
    ///
    /// Ne provoque aucune éviction immédiate : elle a lieu à la prochaine
    /// intégration, comme le reste.
    pub fn budget_residence(&mut self, octets: usize) {
        self.residence.set_budget(octets);
    }

    /// Ce que la fenêtre de résidence CROIT tenir.
    pub fn octets_comptes(&self) -> usize {
        self.residence.used()
    }

    /// Le plafond en vigueur.
    pub fn budget_actuel(&self) -> usize {
        self.residence.budget()
    }

    /// **Protège de l'éviction ce que la caméra regarde**, et rend le reste
    /// évinçable.
    ///
    /// À appeler avec la demande du moment. Une cellule pas encore résidente
    /// est notée quand même : elle sera épinglée à son arrivée, sinon une
    /// cellule chargée au dernier moment se ferait évincer par la suivante
    /// alors qu'elle est en plein champ.
    pub fn proteger(&mut self, voulues: &[tf_world::Cellule]) {
        let neuf: std::collections::HashSet<Cle> =
            voulues.iter().map(|c| (c.niveau, c.x, c.z)).collect();
        // On ne retire QUE ses propres épingles : elles se comptent, et une
        // opération en cours peut tenir le même chunk.
        for k in self.protegees.difference(&neuf) {
            self.residence.unpin(k);
        }
        for k in neuf.difference(&self.protegees) {
            self.residence.pin(k);
        }
        self.protegees = neuf;
    }

    /// **Le budget est-il dépassé faute de candidat évinçable ?**
    ///
    /// Vrai quand tout ce qui reste est protégé : le champ de vision à lui
    /// seul ne tient pas dans le budget. L'hôte doit le DIRE — réduire la
    /// distance d'affichage ou relever le plafond — plutôt que de laisser
    /// l'utilisateur deviner pourquoi sa machine rame.
    pub fn deborde(&self) -> bool {
        self.deborde
    }

    /// Combien de cellules ont été retirées de la scène depuis l'ouverture.
    pub fn degagees(&self) -> usize {
        self.degagees
    }

    /// Combien de cellules sont résidentes.
    pub fn residentes(&self) -> usize {
        self.residence.len()
    }

    /// **Les cellules résidentes**, de la plus récemment vue à la plus
    /// froide, pour les croiser à la demande de la caméra (`Suivi`).
    ///
    /// Les cellules et non leurs clés : `planifier` compare des cellules, et
    /// reconstruire une `Cellule` depuis une clé demanderait de refabriquer
    /// sa boîte — donc de réécrire, ailleurs, la règle qui la produit.
    pub fn cellules_residentes(&self) -> Vec<tf_world::Cellule> {
        self.residence
            .keys_mru()
            .into_iter()
            .filter_map(|k| self.residence.peek(&k).map(|r| r.cellule.clone()))
            .collect()
    }

    /// **Combien de cellules évincées attendent encore d'être retirées.**
    ///
    /// Zéro veut dire que la comptabilité est EXACTE : `octets_comptes` et
    /// `octets_residents` sont alors égaux. Non nul, la scène porte en plus
    /// ce que le prochain appel va dégager.
    pub fn en_attente(&self) -> usize {
        self.a_degager.len()
    }

    /// Combien de cellules ont été évincées depuis l'ouverture.
    ///
    /// Un compteur, comme `rechargements` et `sections_remaillees` — et pour
    /// la même raison : un test qui vérifie qu'un vol continu se borne doit
    /// pouvoir dire que l'éviction a EU LIEU, sinon il passe aussi bien sur
    /// un monde trop petit pour la déclencher.
    pub fn evictions(&self) -> u64 {
        self.residence.evictions()
    }

    /// Ce que la scène porte VRAIMENT : les sections de la grille, le
    /// maillage en mémoire vive, et sa forme packée pour le GPU.
    ///
    /// Compté depuis les structures elles-mêmes, pas depuis la fenêtre de
    /// résidence. C'est ce qui rend la comptabilité vérifiable : une fois
    /// dégagé ce qui est en attente, les deux nombres doivent être ÉGAUX —
    /// un budget qui ne se compare à rien est un budget qu'on peut tenir en
    /// se trompant.
    pub fn octets_residents(&self) -> usize {
        self.monde.grille.octets()
            + self.monde.maillages.octets()
            + self.monde.maillages.octets_vive()
    }

    /// **Inscrit la zone d'ouverture dans la fenêtre de résidence.**
    ///
    /// Sans ça elle ne serait jamais évinçable : on ouvre un monde à un
    /// endroit, on vole cinq mille blocs plus loin, et les chunks du départ
    /// restent en mémoire pour toujours. C'est exactement la fuite que la
    /// fenêtre existe pour empêcher, et elle serait passée inaperçue — la
    /// scène a l'air bornée tant qu'on ne regarde que ce qui ARRIVE.
    fn inscrire_la_zone(&mut self) {
        let [x0, z0, x1, z1] = self.zone;
        let mut cs = Vec::new();
        for cz in z0..=z1 {
            for cx in x0..=x1 {
                cs.push(tf_world::Cellule {
                    niveau: Niveau::Chunk,
                    x: cx,
                    z: cz,
                    // La même hauteur que `charger_monde` a lue : une boîte
                    // plus courte laisserait des sections hors de toute
                    // cellule, donc hors de tout budget.
                    boite: tf_world::BBox::new(
                        BlockPos::new(cx * 16, -64, cz * 16),
                        BlockPos::new(cx * 16 + 15, 319, cz * 16 + 15),
                    ),
                    region: tf_world::coords::RegionPos {
                        x: cx.div_euclid(32),
                        z: cz.div_euclid(32),
                    },
                });
            }
        }
        // Les visées : la zone est TOUTE la scène à ce moment, donc repeser
        // les cellules touchées revient à peser les cellules posées. On passe
        // leurs adresses telles quelles.
        let visees: Vec<tf_mesh::Adresse> = cs.iter().flat_map(adresses_de).collect();
        self.inscrire(&cs);
        self.repeser(&visees);
    }

    /// **Inscrit les cellules qui arrivent**, avec le poids de leurs seules
    /// sections : leur maillage n'existe pas encore, il est parti au fil de
    /// maillage (`Atelier`). [`Ouvert::repeser`] corrige quand il revient.
    ///
    /// Inscrites DÈS leur arrivée, et pas au retour du maillage : c'est ce qui
    /// empêche la demande de les redemander pendant qu'elles se maillent — et
    /// ce qui les fait compter tout de suite dans le budget.
    ///
    /// `insert` pour elles — elles viennent d'être demandées, leur récence est
    /// juste. L'éviction qu'il déclenche part à l'appel SUIVANT (`a_degager`).
    fn inscrire(&mut self, arrivees: &[tf_world::Cellule]) {
        for c in arrivees {
            let k = (c.niveau, c.x, c.z);
            let n: usize = adresses_de(c)
                .into_iter()
                .map(|a| self.monde.grille.octets_de(a))
                .sum();
            let ev = self.residence.insert(
                k,
                Resident {
                    cellule: c.clone(),
                    octets: n,
                },
                tf_world::State::Clean,
            );
            self.deborde = ev.over_budget;
            self.a_degager
                .extend(ev.items.into_iter().map(|(_, r)| r.cellule));
            // **Une cellule en plein champ arrive épinglée.** Sans ça, la
            // dernière chargée serait évincée par la suivante avant même
            // d'être dessinée, et la demande la redemanderait aussitôt.
            // `insert` ne touche pas aux épingles d'une clé déjà là, d'où la
            // garde : ré-épingler doublerait le compte à chaque rechargement.
            if self.protegees.contains(&k) && self.residence.pins(&k) == 0 {
                self.residence.pin(&k);
            }
        }
    }

    /// **Repèse les cellules résidentes qui possèdent une section VISÉE** :
    /// ce sont exactement celles dont un remaillage vient de changer le poids
    /// — sections et maillage.
    ///
    /// Par `update`, qui corrige le poids SANS toucher à la récence : les
    /// faire remonter ferait garder au LRU exactement ce qu'il faudrait lâcher.
    /// Les cellules qui viennent d'arriver sont déjà inscrites
    /// ([`Ouvert::inscrire`]) : elles sont repesées comme les autres.
    ///
    /// **Une seule passe sur les visées.** Demander à chaque cellule ce que
    /// ses lots pèsent coûterait O(scène × cellules) — la faute exacte que ce
    /// dépôt a payée trois fois sous le nom de « chauffer le build entier
    /// pour écrire trois blocs ».
    fn repeser(&mut self, visees: &[tf_mesh::Adresse]) {
        // 1. Les cellules à repeser : les résidentes qui possèdent une section
        //    visée.
        let mut cles: std::collections::HashSet<Cle> = std::collections::HashSet::new();
        let mut touchees: Vec<tf_world::Cellule> = Vec::new();
        for a in visees {
            // Les deux niveaux, plutôt qu'un champ qui dirait lequel la scène
            // emploie : deux constantes pour une même vérité finissent par
            // diverger, et celle-ci ne coûte qu'une recherche de plus.
            for n in [Niveau::Chunk, Niveau::Region] {
                let k = (n, n.cellule_axe(a.0 * 16), n.cellule_axe(a.1 * 16));
                if cles.contains(&k) {
                    continue;
                }
                if let Some(r) = self.residence.peek(&k) {
                    touchees.push(r.cellule.clone());
                    cles.insert(k);
                }
            }
        }
        if cles.is_empty() {
            return;
        }

        // 2. Le poids de chaque cellule.
        let mut octets: std::collections::HashMap<Cle, usize> =
            std::collections::HashMap::with_capacity(cles.len());
        let mut ou: std::collections::HashMap<tf_mesh::Adresse, Cle> =
            std::collections::HashMap::new();
        for c in &touchees {
            let k = (c.niveau, c.x, c.z);
            for a in adresses_de(c) {
                ou.insert(a, k);
            }
            octets.insert(k, 0);
        }
        // **Une section ne compte que pour UNE cellule.** Sommer les adresses
        // cellule par cellule compterait deux fois ce que deux cellules
        // couvrent toutes les deux — ce qui arrive dès qu'un hôte mêle les
        // niveaux, une cellule de RÉGION contenant 32 × 32 cellules de chunk.
        // La table d'appartenance tranche : un propriétaire par adresse.
        for (a, k) in &ou {
            *octets.get_mut(k).expect("clé posée juste au-dessus") +=
                self.monde.grille.octets_de(*a);
            // Par RECHERCHE, pas par parcours : les lots de la scène entière
            // étaient visités pour en trouver quelques dizaines, à chaque
            // image — 0,4 ms à 264 Mo résidents, et ça grandissait avec elle.
            if let Some(l) = self.monde.maillages.lot(a) {
                // Les DEUX formes : le `Vec<Quad>` en mémoire vive et sa copie
                // packée dans l'arène. Ne compter que l'une sous-compterait le
                // maillage d'un tiers, sur la moitié la plus lourde d'une
                // région bâtie.
                *octets.get_mut(k).expect("clé posée juste au-dessus") +=
                    l.octets() + l.octets_vive();
            }
        }

        // 3. Les corrections, sans toucher à la récence.
        for c in touchees {
            let k = (c.niveau, c.x, c.z);
            let n = octets.get(&k).copied().unwrap_or(0);
            self.residence.update(
                &k,
                Resident {
                    cellule: c,
                    octets: n,
                },
            );
        }
    }

    /// Relit et remaille. **Depuis la copie de travail**, pas la source :
    /// c'est elle qui porte ce qu'on vient d'écrire.
    ///
    /// `bornes` est ce que l'opération a VRAIMENT écrit. Sans elles, on
    /// recharge tout ; avec, on ne relit et ne remaille que les sections
    /// touchées, **plus une case de débordement** — le mailleur travaille avec
    /// un padding, donc un bloc au bord d'une section change les faces
    /// visibles de la voisine.
    ///
    /// **Un état inconnu force le rechargement complet.** L'atlas ne monte que
    /// les textures des blocs PRÉSENTS : poser un bloc dont la scène n'avait
    /// jamais vu l'état lui donnerait une texture prise au hasard dans la
    /// table voisine. Ça arrive une fois par type de bloc et par séance, et
    /// c'est le seul moment où l'on paie le prix fort.
    pub fn remailler(&mut self, bornes: Option<tf_world::coords::BBox>) -> Result<(), String> {
        if self.staging.is_none() {
            return Err("la fixture n'a pas de save derrière elle".into());
        }
        let Some(b) = bornes else {
            return self.recharger();
        };
        // **Ce qui est parti au mailleur revient D'ABORD.** L'édition se
        // maille ici, sur ce fil ; un travail plus ancien appliqué après elle
        // remettrait l'ancien maillage par-dessus le neuf.
        self.attendre_maillage()?;
        let st = self.staging.as_ref().expect("vérifié juste au-dessus");
        let visees =
            Grille::sections_touchees([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
        if visees.is_empty() {
            return Ok(());
        }
        // **On relit ce que la scène PORTE, là où l'édition a eu lieu** : les
        // visées dont la cellule est résidente — la zone d'ouverture l'est
        // aussi, elle y est inscrite. La lecture était serrée sur la ZONE,
        // règle d'avant le streaming : toutes les visées étaient retirées,
        // seules celles de la zone relues, et éditer là où l'on avait volé
        // effaçait la cellule de la scène — un trou définitif, puisqu'elle
        // restait inscrite et que personne ne la redemandait.
        //
        // Ce qui n'est pas résident n'est pas relu : le poser ferait naître
        // des sections qu'aucune cellule ne porte, donc qu'aucune éviction
        // ne lâcherait. L'édition est dans la copie de travail ; elle
        // arrivera avec la cellule, le jour où la caméra la demandera.
        let presentes: std::collections::HashSet<tf_mesh::Adresse> =
            visees.iter().copied().filter(|a| self.porte(a)).collect();
        if presentes.is_empty() {
            return Ok(());
        }
        let x0 = presentes.iter().map(|a| a.0).min().expect("non vide") * 16;
        let x1 = presentes.iter().map(|a| a.0).max().expect("non vide") * 16 + 15;
        let z0 = presentes.iter().map(|a| a.1).min().expect("non vide") * 16;
        let z1 = presentes.iter().map(|a| a.1).max().expect("non vide") * 16 + 15;
        let y0 = presentes
            .iter()
            .map(|a| a.2 as i32)
            .min()
            .expect("non vide")
            * 16;
        let y1 = presentes
            .iter()
            .map(|a| a.2 as i32)
            .max()
            .expect("non vide")
            * 16
            + 15;
        let lu = tf_world::BBox::new(BlockPos::new(x0, y0, z0), BlockPos::new(x1, y1, z1));
        let t0 = std::time::Instant::now();
        let connus = self.monde.interner.len();
        let mut interner = std::mem::take(&mut self.monde.interner);
        let grille = &mut self.monde.grille;
        for a in &presentes {
            grille.retirer(*a);
        }
        tf_world::sections_de(
            st.as_ref(),
            &tf_world::Dimension::Overworld,
            tf_world::Folder::Region,
            &lu,
            &mut interner,
            |s| {
                let y = s.section.y;
                // La boîte lue peut déborder des présentes — deux cellules
                // résidentes en diagonale l'étirent sur des colonnes que la
                // scène ne porte pas.
                if !presentes.contains(&(s.chunk.x, s.chunk.z, y)) {
                    return;
                }
                if let Some(bi) = s.biomes {
                    grille.poser_biomes(s.chunk.x, s.chunk.z, y, bi);
                }
                grille.poser(s.chunk.x, s.chunk.z, s.section);
            },
        );
        self.monde.interner = interner;
        phase("relecture", t0);
        // Le poids des cellules éditées change — leurs sections comme leur
        // maillage : `appliquer` les repèse. Sans cette repesée, la fenêtre
        // de résidence dériverait à chaque geste d'édition.
        self.refaire(&visees, connus);
        Ok(())
    }

    /// La scène porte-t-elle cette section — c'est-à-dire sa cellule est-elle
    /// RÉSIDENTE, à l'un des deux niveaux ?
    fn porte(&self, a: &tf_mesh::Adresse) -> bool {
        [Niveau::Chunk, Niveau::Region].iter().any(|n| {
            self.residence
                .peek(&(*n, n.cellule_axe(a.0 * 16), n.cellule_axe(a.1 * 16)))
                .is_some()
        })
    }

    /// **Intègre une cellule que le chargeur vient de rendre.**
    ///
    /// C'est la jonction entre le fil et la scène, et elle n'existe qu'ici :
    /// écrite dans chaque appelant, elle se tromperait à trois endroits — la
    /// fusion des tables d'états, le retrait de ce qui n'existe plus, et la
    /// marge du remaillage.
    ///
    /// **La table d'états d'abord.** Un `StateId` n'a de sens que relativement
    /// à SON interner : ceux que le fil rend sont numérotés dans sa table à
    /// lui. On fusionne, on remappe les palettes, et alors seulement les
    /// identifiants veulent dire quelque chose ici. Sauter ce pas ferait
    /// prendre à chaque bloc l'état d'un autre, sans la moindre erreur.
    ///
    /// **Un LOT de cellules, pas une seule**, et c'est la mesure qui l'impose.
    ///
    /// Le remplacement de tranches recopie les deux arènes, donc son coût est
    /// en O(scène) quel que soit le nombre de cellules intégrées. Mesuré une
    /// par une sur du bâti : **médiane 23 ms, pire 84** — trois à dix fois le
    /// budget d'image, et ça EMPIRE à mesure que la scène grandit. Intégrer
    /// par lot amortit cette recopie sur tout le lot ; l'hôte passe ce que le
    /// fil lui a rendu dans l'image, et paie une recopie au lieu de N.
    ///
    /// C'est la même leçon que l'unité de LECTURE du chargeur, à l'autre bout
    /// de la chaîne : le coût fixe décide du grain, pas l'envie d'une API
    /// simple.
    ///
    /// Rend le nombre de sections posées.
    pub fn integrer(&mut self, lot: Vec<Arrivee>) -> Result<usize, String> {
        // **Ce que le mailleur a rendu passe d'abord**, dans l'ordre où c'est
        // parti : les arènes rattrapent ce que la grille porte depuis les
        // images précédentes.
        let echec = self.recolter();
        let posees = self.poser_et_mailler(lot);
        match echec {
            Some(e) => Err(e),
            None => Ok(posees),
        }
    }

    /// Pose les arrivées dans la grille, retire ce qui a été évincé, et
    /// envoie le remaillage au fil de maillage. Rend le nombre de sections
    /// posées.
    fn poser_et_mailler(&mut self, lot: Vec<Arrivee>) -> usize {
        // Rien à poser, rien à dégager, et rien au-dessus du budget : il n'y a
        // rien à remailler.
        if lot.is_empty()
            && self.a_degager.is_empty()
            && self.residence.used() <= self.residence.budget()
        {
            return 0;
        }
        let connus = self.monde.interner.len();
        let mut posees = 0;
        let mut visees: Vec<tf_mesh::Adresse> = Vec::new();
        let mut arrivees: Vec<tf_world::Cellule> = Vec::new();

        // **Ce que l'appel précédent a évincé part d'ABORD**, et dans le même
        // remaillage que ce qui arrive. C'est tout l'intérêt du report : une
        // seule recopie d'arène par appel au lieu de deux.
        //
        // La marge vaut pour un RETRAIT autant que pour une pose : retirer une
        // cellule découvre les faces de ses voisines, et sans le débordement
        // d'une case il resterait un mur de faces fantômes le long de chaque
        // frontière dégagée.
        //
        // Le `trim` vient AVANT le dégagement, donc ce qu'il évince part dans
        // le même remaillage. C'est ce qui fait qu'un budget qu'on RESSERRE
        // prend effet : sans lui, l'éviction n'aurait lieu qu'à la prochaine
        // arrivée, et une caméra immobile garderait indéfiniment ce qu'on
        // vient de lui interdire. Il décide sur les poids du dernier appel,
        // qui sont justes — la correction, elle, n'a lieu qu'après le
        // maillage.
        let t0 = std::time::Instant::now();
        let coupes = self.residence.trim();
        self.deborde = coupes.over_budget;
        self.a_degager
            .extend(coupes.items.into_iter().map(|(_, r)| r.cellule));
        for c in std::mem::take(&mut self.a_degager) {
            self.degagees += 1;
            // Ce que la cellule touchait, lu AVANT de la retirer : une
            // voisine ne change que si la couche qui la bordait avait
            // quelque chose d'opaque.
            visees.extend(self.touchees(&c.boite));
            for a in adresses_de(&c) {
                self.monde.grille.retirer(a);
            }
        }
        let mut boites = Vec::new();

        for Arrivee {
            cellule,
            mut sections,
            interner: local,
        } in lot
        {
            // **La table d'états d'abord.** Un `StateId` n'a de sens que
            // relativement à SON interner : ceux que le fil rend sont
            // numérotés dans sa table à lui. On fusionne, on remappe, et
            // alors seulement les identifiants veulent dire quelque chose
            // ici. Sauter ce pas ferait prendre à chaque bloc l'état d'un
            // autre, sans la moindre erreur.
            //
            // La correspondance se calcule UNE fois par cellule : c'est ce
            // que `merge_from` est fait pour, mesuré à 1,06 ms par région
            // pleine, soit 3,9 % du décodage.
            let corr = self.monde.interner.merge_from(&local);
            for s in &mut sections {
                Interner::remap_palette(&corr, &mut s.section.palette);
                if let Some(b) = s.biomes.as_mut() {
                    Interner::remap_palette(&corr, b);
                }
            }

            // **On retire ce que la cellule portait AVANT de poser.** Une
            // section que la save n'a plus ne revient pas du chargement :
            // sans ce retrait son ancien contenu resterait, et les blocs
            // effacés resteraient à l'écran. Contrairement au cas de
            // `remailler`, celui-ci est RÉEL — une cellule évincée puis
            // rechargée peut avoir changé entre-temps.
            //
            // Les adresses se DÉDUISENT de la cellule. Le balayage qui était
            // écrit ici allouait et triait la grille entière par cellule
            // intégrée — O(scène × cellules) — et filtrait sur `a.0 ==
            // cellule.x`, ce qui n'est vrai qu'au niveau CHUNK : d'une cellule
            // de RÉGION il n'aurait retiré qu'un millième des sections.
            visees.extend(self.touchees(&cellule.boite));
            for a in adresses_de(&cellule) {
                self.monde.grille.retirer(a);
            }

            posees += sections.len();
            for s in sections {
                let y = s.section.y;
                if let Some(b) = s.biomes {
                    self.monde.grille.poser_biomes(s.chunk.x, s.chunk.z, y, b);
                }
                self.monde.grille.poser(s.chunk.x, s.chunk.z, s.section);
            }

            boites.push(cellule.boite);
            arrivees.push(cellule);
        }
        // **Les visées DÉBORDENT de la cellule — là où elle touche.** Poser
        // une cellule change les faces visibles de ses VOISINES : sans la
        // marge, un mur de faces fantômes resterait le long de chaque
        // frontière. Mais une voisine ne lit d'elle que l'opacité de la couche
        // qui la borde : une couche sans rien d'opaque, avant comme après,
        // ne lui change rien (`touchees_par_le_contenu`). La croix entière
        // remaillait quatre colonnes voisines par cellule quoi qu'elle porte.
        //
        // Les états neufs entrent dans la table AVANT ce relevé : un état
        // qu'elle ne connaît pas se lit « pas opaque », et la voisine qu'il
        // bouche garderait son mur de faces.
        if self.monde.interner.len() > connus {
            self.etendre_atlas(connus);
        }
        for b in &boites {
            visees.extend(self.touchees(b));
        }
        // Deux cellules voisines partagent leur marge : sans dédoublonnage, la
        // section frontière serait maillée deux fois et `Chantier::remplacer`
        // en garderait deux lots.
        visees.sort_unstable();
        visees.dedup();
        phase("poser    ", t0);
        // **Inscrites maintenant, repesées au retour du maillage.** La
        // demande ne doit pas redemander ce qui se maille, et le budget doit
        // compter ce qui est déjà dans la grille.
        self.inscrire(&arrivees);
        self.soumettre(visees);
        posees
    }

    /// **Le remaillage SYNCHRONE d'une édition** : accueillir les états
    /// neufs, remailler les visées sur ce fil, et appliquer.
    ///
    /// Une édition attend son résultat — on la voit tout de suite, et elle ne
    /// porte que quelques sections. C'est l'hôte qui a d'abord vidé l'atelier
    /// (`attendre_maillage`) : un vieux travail appliqué APRÈS l'édition
    /// remettrait l'ancien maillage par-dessus.
    ///
    /// `connus` est la taille de la table d'états AVANT l'arrivée : ce qui est
    /// au-delà est neuf et n'a pas encore de texture.
    fn refaire(&mut self, visees: &[tf_mesh::Adresse], connus: usize) {
        self.sections_remaillees = visees.len();
        // **Un état jamais vu ÉTEND l'atlas ; il ne recharge pas la zone.**
        //
        // C'était la dernière porte vers le chemin complet, et elle s'ouvrait
        // au geste le plus banal d'un éditeur : prendre un bloc dans la
        // palette et le poser. Mesuré sur une zone de 64 chunks, poser un bloc
        // connu coûtait 0,7 ms et poser un bloc NEUF 22,3 — × 33 — et ce coût
        // est en O(zone), donc 867 ms sur une région bâtie.
        if self.monde.interner.len() > connus {
            self.etendre_atlas(connus);
        }
        let t1 = std::time::Instant::now();
        let neufs = self
            .monde
            .grille
            .mailler_ces_parallele(&*self.monde.table, visees);
        phase("mailler  ", t1);
        self.appliquer(visees.to_vec(), neufs);
    }

    /// **Envoie le remaillage des visées au fil de maillage**, avec un
    /// extrait de la grille telle qu'elle est maintenant.
    fn soumettre(&mut self, visees: Vec<tf_mesh::Adresse>) {
        self.sections_remaillees = visees.len();
        let id = self.atelier.prochain;
        self.atelier.prochain += 1;
        let t = std::time::Instant::now();
        let extrait = self.monde.grille.extrait(&visees);
        phase("extraire ", t);
        let table = std::sync::Arc::clone(&self.monde.table);
        let envoi = self.atelier.envoi.clone();
        let retard = self.atelier.retard.take();
        reserve_de_maillage().spawn(move || {
            if let Some(d) = retard {
                std::thread::sleep(d);
            }
            let t = std::time::Instant::now();
            // Un mailleur qui panique ne doit pas bloquer l'ordre : le
            // travail revient VIDE, et l'hôte le dit.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                extrait.mailler_ces_parallele(&*table, &visees)
            }));
            let (chantier, panique) = match r {
                Ok(c) => (c, false),
                Err(_) => (tf_mesh::Chantier::default(), true),
            };
            // L'hôte a pu fermer entre-temps : personne n'attend plus.
            let _ = envoi.send(Fini {
                id,
                visees,
                chantier,
                duree: t.elapsed(),
                panique,
            });
        });
    }

    /// **Applique ce que le mailleur a rendu**, dans l'ordre de départ, sans
    /// attendre ce qui n'est pas revenu. Rend l'échec d'un travail, s'il y en
    /// a eu un.
    fn recolter(&mut self) -> Option<String> {
        while let Ok(f) = self.atelier.retour.try_recv() {
            self.atelier.ranger(f);
        }
        self.appliquer_dans_l_ordre()
    }

    fn appliquer_dans_l_ordre(&mut self) -> Option<String> {
        let mut echec = None;
        while let Some(f) = self.atelier.prets.remove(&self.atelier.attendu) {
            self.atelier.attendu += 1;
            phase_texte(&format!(
                "mailler (fil) : {:.1} ms",
                f.duree.as_secs_f64() * 1000.0
            ));
            if f.panique {
                echec = Some(format!(
                    "le maillage de {} section(s) a échoué : elles ne sont pas dessinées",
                    f.visees.len()
                ));
            }
            self.appliquer(f.visees, f.chantier);
        }
        echec
    }

    /// **Attend que tout ce qui est parti au mailleur soit revenu**, et
    /// l'applique. Pour ce qui doit voir la scène À JOUR : une édition, qui
    /// passerait sinon sous un vieux travail, et les tests.
    pub fn attendre_maillage(&mut self) -> Result<(), String> {
        let mut echec = self.recolter();
        while self.atelier.en_vol() > 0 {
            // Un délai, pas une attente sans fin : un fil de maillage perdu
            // ne doit pas figer l'éditeur sans rien dire.
            match self
                .atelier
                .retour
                .recv_timeout(std::time::Duration::from_secs(120))
            {
                Ok(f) => self.atelier.ranger(f),
                Err(_) => return Err("le maillage ne revient pas".into()),
            }
            if let Some(e) = self.appliquer_dans_l_ordre() {
                echec = Some(e);
            }
        }
        match echec {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Travaux partis au mailleur et pas encore appliqués.
    pub fn maillages_en_vol(&self) -> usize {
        self.atelier.en_vol()
    }

    /// Fait attendre le PROCHAIN travail de maillage avant de commencer.
    /// Pour les tests : c'est ce qui fait revenir un travail après un plus
    /// récent, donc ce qui rend l'ordre d'application vérifiable.
    #[doc(hidden)]
    pub fn retarder_le_prochain_maillage(&mut self, d: std::time::Duration) {
        self.atelier.retard = Some(d);
    }

    /// **Le maillage de la scène est-il celui de sa grille ?** Remaille
    /// tout et compare, lot par lot. Coûteux — pour les tests, qui ne peuvent
    /// pas deviner quel travail aurait dû gagner.
    #[doc(hidden)]
    pub fn maillage_juste(&self) -> Result<(), String> {
        let mut complet = self.monde.grille.mailler(&*self.monde.table);
        complet.trier();
        let tenus: Vec<&tf_mesh::Lot> = self.monde.maillages.lots().collect();
        if tenus.len() != complet.lots.len() {
            return Err(format!(
                "{} lots tenus pour {} dans la grille",
                tenus.len(),
                complet.lots.len()
            ));
        }
        for (a, b) in tenus.iter().zip(complet.lots.iter()) {
            if a.adresse != b.adresse
                || a.quads.quads != b.quads.quads
                || a.poses.poses != b.poses.poses
            {
                return Err(format!(
                    "la section {:?} ne porte pas le maillage de son contenu",
                    b.adresse
                ));
            }
        }
        Ok(())
    }

    /// Travaux de maillage appliqués depuis l'ouverture. Quand il bouge, les
    /// arènes ont changé : c'est ce qui dit à l'hôte de regarnir son GPU.
    pub fn maillages_appliques(&self) -> u64 {
        self.atelier.appliques
    }

    /// **Remplace les tranches des visées** dans les deux arènes et dans le
    /// maillage de la scène, puis repèse ce qui a changé.
    ///
    /// Écrite UNE fois et partagée par le maillage hors fil et celui d'une
    /// édition. Ce dépôt a payé quatre fois le piège des deux implémentations
    /// d'une même règle, et celle-ci en porte trois d'un coup — l'ordre des
    /// tranches, la somme préfixe des poses, la pesée.
    fn appliquer(&mut self, visees: Vec<tf_mesh::Adresse>, neufs: tf_mesh::Chantier) {
        let t2 = std::time::Instant::now();
        // **Les arènes reçoivent les lots NEUFS, pas le chantier.** Elles
        // rangent chaque section à une place stable et ne réécrivent que les
        // visées.
        self.monde.arene.remplacer(
            &visees,
            &neufs.lots,
            &apparence(
                &self.monde.habillage,
                &self.monde.interner,
                &self.assets.climat,
            ),
        );
        phase("quads    ", t2);
        let t3 = std::time::Instant::now();
        self.monde.modeles.remplacer(
            self.monde.arene.emplacements(),
            &visees,
            &neufs.lots,
            &modele_de(
                &self.monde.table,
                &self.monde.habillage,
                &self.monde.interner,
                &self.assets.climat,
            ),
        );
        phase("modèles  ", t3);
        let t1b = std::time::Instant::now();
        self.monde.maillages.remplacer(&visees, neufs);
        phase("chantier ", t1b);
        self.monde.quads = self.monde.maillages.quads();
        self.monde.poses = self.monde.maillages.poses();
        // **La pesée vient APRÈS le maillage** — le poids ne se connaît
        // qu'alors — et l'éviction qu'elle déclenche part au prochain appel :
        // voir `a_degager`.
        let t = std::time::Instant::now();
        self.repeser(&visees);
        phase("peser    ", t);
        self.atelier.appliques += 1;
    }

    /// Ce qu'un changement du contenu de la boîte oblige à remailler, d'après
    /// ce que la grille porte MAINTENANT — voir
    /// `Grille::touchees_par_le_contenu`, à appeler avant et après.
    fn touchees(&self, b: &tf_world::BBox) -> Vec<tf_mesh::Adresse> {
        self.monde.grille.touchees_par_le_contenu(
            [b.min.x, b.min.y, b.min.z],
            [b.max.x, b.max.y, b.max.z],
            &*self.monde.table,
        )
    }

    /// **Accueille les états découverts depuis `connus`**, sans rien rebâtir.
    ///
    /// Toujours possible : une texture neuve plus grande que le côté du
    /// tableau le fait grandir sur place (`Atlas::etendre`). Ce cas
    /// RECHARGEAIT la zone, et sous le streaming il rechargeait en boucle —
    /// voir `une_texture_plus_grande_qui_arrive_en_volant_ne_recharge_pas`.
    fn etendre_atlas(&mut self, connus: usize) {
        let neuves: Vec<String> = (connus..self.monde.interner.len())
            .map(|i| {
                self.monde
                    .interner
                    .resolve(i as StateId)
                    .unwrap_or("minecraft:air")
                    .to_string()
            })
            .collect();
        let voulues = tf_assets::textures_des_etats(&self.assets.cat, neuves.iter().cloned());
        let ajout = self.monde.atlas.etendre(&self.assets.src, voulues, &|n| {
            self.assets.disposition.chemins_texture(n)
        });
        if let Some((avant, apres)) = ajout.agrandi {
            phase_texte(&format!("atlas agrandi : {avant} → {apres} px"));
        }
        // La table et l'habillage se prolongent par les états neufs, DANS
        // l'ordre d'internement : c'est ce qui garde `StateId` valide comme
        // indice des deux.
        tf_assets::catalogue::prolonger_rendu(
            &self.assets.cat,
            &self.monde.atlas,
            &self.assets.teintes,
            neuves.into_iter(),
            &|n| self.assets.translucides.contains(n),
            // Recopiée seulement si un travail de maillage la tient encore.
            std::sync::Arc::make_mut(&mut self.monde.table),
            &mut self.monde.habillage,
        );
        debug_assert_eq!(
            self.monde.habillage.len(),
            self.monde.interner.len(),
            "l'habillage est indexé par StateId : il doit couvrir tous les états"
        );
    }

    /// Tout relire et tout remailler — y compris l'atlas.
    ///
    /// **Le chemin coûteux, et il se COMPTE** (`rechargements`). Il paie la
    /// ZONE, pas ce qui a changé : sur une région bâtie, 867 ms contre
    /// quelques millisecondes pour un remaillage incrémental. Tout appel
    /// après une édition ordinaire est un bug, et un test le vérifie.
    fn recharger(&mut self) -> Result<(), String> {
        self.rechargements += 1;
        // Ce qui est parti au mailleur maillait le monde qu'on remplace : il
        // est oublié, et jeté à son retour.
        self.atelier.oublier();
        let st = self
            .staging
            .as_ref()
            .expect("un monde éditable a un staging");
        self.monde = charger_monde(
            &self.assets,
            Ou::Source(st.as_ref(), self.zone, self.nom.clone()),
        )?;
        // Un rechargement refait TOUT : c'est ce que le compteur doit dire.
        self.sections_remaillees = self.monde.grille.len();
        // **La fenêtre de résidence repart de zéro avec lui.** `recharger`
        // remplace le monde par la seule ZONE : les cellules streamées ne sont
        // plus là, et les laisser inscrites ferait évincer des sections qui
        // n'existent plus tout en comptant des octets que personne ne porte.
        let budget = self.residence.budget();
        self.residence = tf_world::Residency::new(budget);
        self.a_degager.clear();
        // Les épingles portaient sur des cellules qui n'existent plus. Les
        // garder ferait épingler, à la prochaine arrivée, des clés que la
        // caméra ne demande peut-être plus.
        self.protegees.clear();
        self.deborde = false;
        self.inscrire_la_zone();
        Ok(())
    }

    /// Le monde est-il éditable ? La fixture ne l'est pas, et l'interface doit
    /// le DIRE plutôt que de griser un bouton sans raison.
    pub fn editable(&self) -> bool {
        self.staging.is_some()
    }
}

impl Drop for Ouvert {
    fn drop(&mut self) {
        // La copie de travail est jetable par construction : la save n'a pas
        // été touchée. La laisser derrière remplirait le disque d'un
        // utilisateur qui ouvre dix mondes.
        if let Some(c) = &self.couche {
            let _ = std::fs::remove_dir_all(c);
        }
    }
}

/// Depuis quand une copie de travail abandonnée peut être effacée.
///
/// Généreux exprès. Le risque de ce balayage n'est PAS de perdre une save —
/// la source n'est jamais touchée — mais de jeter la copie de travail d'une
/// séance encore ouverte, donc les opérations non écrites. Vingt-quatre heures
/// veulent dire qu'il faudrait laisser l'application ouverte un jour entier
/// SANS une seule édition pour que ça arrive — à condition de mesurer l'âge
/// là où une édition se voit, ce que fait [`plus_recente`].
const AGE_ABANDON: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Combien d'entrées le balayage consent à regarder, en tout.
///
/// Un budget, pas une limite de profondeur : une copie de travail, c'est
/// `level.dat` et les régions matérialisées, donc quelques dizaines
/// d'entrées. Le budget est là pour qu'un dossier temporaire inattendu ne
/// puisse pas retarder l'ouverture d'un monde, jamais pour tronquer un cas
/// normal. Épuisé, on garde le dossier (voir [`plus_recente`]).
const BUDGET_BALAYAGE: u32 = 10_000;

/// **La date la plus récente de l'arborescence** — surtout pas celle du
/// dossier racine.
///
/// Mesuré : réécrire `region/r.0.0.mca` ne change NI la date du dossier
/// racine, NI celle de `region/`. Un dossier ne voit passer que les créations
/// et les suppressions d'entrées ; une édition qui réécrit une région DÉJÀ
/// matérialisée ne touche que le fichier. Se fier à la date de la racine
/// reviendrait donc à effacer la copie de travail d'une séance ouverte depuis
/// un jour et toujours en train d'éditer — c'est-à-dire ses opérations non
/// écrites, le seul endroit du programme où elles existent.
///
/// Rend `None` quand rien n'est lisible ou que le budget est épuisé.
/// L'appelant traite `None` comme « pas vieux » : dans le doute, on garde.
fn plus_recente(dir: &std::path::Path, reste: &mut u32) -> Option<std::time::SystemTime> {
    let entrees = std::fs::read_dir(dir).ok()?;
    let mut max = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    for e in entrees.flatten() {
        if *reste == 0 {
            return None;
        }
        *reste -= 1;
        let chemin = e.path();
        let date = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            plus_recente(&chemin, reste)?
        } else {
            e.metadata().and_then(|m| m.modified()).ok()?
        };
        if max.is_none_or(|m| date > m) {
            max = Some(date);
        }
    }
    max
}

/// **Efface les copies de travail qu'un arrêt brutal a laissées.**
///
/// Une copie de travail se supprime à la fermeture (`Drop`), mais un `kill`,
/// une panne de courant ou un plantage la laissent derrière. Elles ne se
/// voient pas — elles vivent dans le dossier temporaire — et elles pèsent ce
/// que pèsent les régions qu'on a éditées : des mégaoctets par séance, sans
/// fin. Mesuré après une séance de développement : dix-sept mégaoctets en
/// dix-sept dossiers.
///
/// **On ne touche qu'à ce qui porte notre préfixe et qui est VIEUX.** Un
/// processus vivant n'est pas détectable de façon portable — c'est la même
/// limite que `session.lock` de Minecraft, qui n'est consultable que sous
/// Windows — donc on se fie à l'âge plutôt que d'affirmer qu'un dossier est
/// abandonné. Et « vieux » se mesure sur le fichier le plus récent de
/// l'arborescence ([`plus_recente`]), pas sur le dossier racine, dont la date
/// ne bouge plus une fois la copie faite.
///
/// Rend le nombre de dossiers effacés. Une erreur ne remonte pas : ne pas
/// pouvoir faire le ménage n'est pas une raison de refuser d'ouvrir un monde.
pub fn balayer_les_abandons() -> usize {
    let base = std::env::temp_dir();
    let Ok(entrees) = std::fs::read_dir(&base) else {
        return 0;
    };
    let maintenant = std::time::SystemTime::now();
    let mut budget = BUDGET_BALAYAGE;
    let mut n = 0;
    for e in entrees.flatten() {
        let nom = e.file_name();
        let Some(nom) = nom.to_str() else { continue };
        if !nom.starts_with("titiforge-") {
            continue;
        }
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let vieux = plus_recente(&e.path(), &mut budget)
            .and_then(|t| maintenant.duration_since(t).ok())
            .map(|d| d > AGE_ABANDON)
            .unwrap_or(false);
        if vieux && std::fs::remove_dir_all(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// **Ce que chaque phase du remaillage coûte**, quand on le demande
/// (`TF_PHASES=1`).
///
/// Ce n'est pas du débogage oublié : découper une chaîne AVANT de choisir quoi
/// accélérer est la seule façon de ne pas travailler pour rien, et ce dépôt l'a
/// payé deux fois. Ici, la découpe a dit que le remaillage « incrémental » que
/// je venais d'écrire passait 18 ms sur 18,3 à RELIRE — le maillage optimisé
/// pesait 0,2. La ligne reste pour que la prochaine mesure soit une commande et
/// pas une réécriture.
pub fn phase(quoi: &str, depuis: std::time::Instant) {
    phase_texte(&format!(
        "{quoi} : {:.1} ms",
        depuis.elapsed().as_secs_f64() * 1000.0
    ));
}

/// Une ligne de la même découpe, qui n'est pas un temps.
pub fn phase_texte(quoi: &str) {
    if std::env::var_os("TF_PHASES").is_some() {
        eprintln!("  {quoi}");
    }
}

/// **L'union de deux emprises.** Plusieurs opérations peuvent répondre dans la
/// même image, et plusieurs images peuvent passer avant un remaillage : on
/// prend l'union, jamais la dernière. Ne garder que la dernière laisserait les
/// précédentes à l'écran, et remailler trois fois coûterait trois fois pour le
/// même résultat.
///
/// Écrite ici parce qu'elle servait déjà à DEUX endroits, ce qui est
/// exactement une de trop.
pub fn unir(a: Option<tf_world::BBox>, b: tf_world::BBox) -> tf_world::BBox {
    let Some(a) = a else { return b };
    tf_world::BBox::new(
        BlockPos::new(
            a.min.x.min(b.min.x),
            a.min.y.min(b.min.y),
            a.min.z.min(b.min.z),
        ),
        BlockPos::new(
            a.max.x.max(b.max.x),
            a.max.y.max(b.max.y),
            a.max.z.max(b.max.z),
        ),
    )
}

/// Le quadrillage à dessiner, depuis l'état et le point regardé.
///
/// Les chunks D'ABORD : les `.mca` passent par-dessus et restent lisibles là
/// où les deux se superposent. L'inverse noierait la frontière de fichier dans
/// le quadrillage fin.
pub fn quadrillage(q: &Quadrillage, centre: BlockPos, y: (i32, i32)) -> Lignes {
    let mut l = Lignes::new();
    let coins = |c: &tf_world::decoupe::Cellule| {
        let b = c.boite;
        (
            [b.min.x as f32, b.min.y as f32, b.min.z as f32],
            [
                b.max.x as f32 + 1.0,
                b.max.y as f32 + 1.0,
                b.max.z as f32 + 1.0,
            ],
        )
    };
    if let Some(r) = q.chunks {
        for c in cellules_autour(centre, r, Niveau::Chunk, y) {
            let (a, b) = coins(&c);
            // La parité du .MCA, pas celle du chunk : c'est elle qui fait voir
            // à quel fichier appartient ce qu'on regarde.
            let t = if (c.region.x.rem_euclid(2) ^ c.region.z.rem_euclid(2)) == 0 {
                tf_render::rgba(90, 170, 255, 110)
            } else {
                tf_render::rgba(255, 190, 90, 110)
            };
            l.contour(a, b, t);
        }
    }
    if let Some(r) = q.mca {
        for c in cellules_autour(centre, r, Niveau::Region, y) {
            let (a, b) = coins(&c);
            l.contour(a, b, tf_render::rgba(255, 90, 90, 230));
        }
    }
    l
}

/// Le contour de la sélection, en vert.
pub fn contour_selection(sel: &tf_world::Selection) -> Lignes {
    let mut l = Lignes::new();
    if let Some(b) = sel.boite() {
        let (min, max) = b.coins();
        l.contour(min, max, tf_render::rgba(120, 220, 140, 255));
    }
    l
}
