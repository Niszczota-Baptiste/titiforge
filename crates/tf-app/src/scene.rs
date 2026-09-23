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
    pub table: TableFormes,
    pub arene: Arene,
    pub modeles: AreneModeles,
    pub atlas: tf_assets::Atlas,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub quoi: String,
    pub quads: usize,
    pub poses: usize,
    /// Le maillage, gardé LOT PAR LOT.
    ///
    /// C'est ce qui rend le remaillage incrémental possible : on remplace les
    /// lots des sections touchées et on reconstruit les arènes, au lieu de
    /// remailler le monde entier pour trois blocs.
    chantier: tf_mesh::Chantier,
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
    pub fn solide(&self) -> impl Fn([i32; 3]) -> bool + '_ {
        move |c| {
            let id = self.grille.bloc(c[0], c[1], c[2]);
            !tf_mesh::forme::Formes::est_air(&self.table, id)
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
        quoi,
        grille,
        table,
        arene,
        modeles,
        atlas,
        min,
        max,
        chantier,
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
            + self.monde.chantier.octets()
            + self.monde.chantier.octets_vive()
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
        self.peser(cs, &visees);
    }

    /// **Pèse et inscrit**, une fois le maillage fait.
    ///
    /// Le poids exact ne se connaît qu'ICI : le maillage d'une région bâtie
    /// pèse 168 fois celui d'une région de terrain (101 Mo contre 0,6),
    /// l'estimer avant reviendrait à inventer un facteur que la mesure dément.
    ///
    /// **Les arrivées ne sont pas les seules à repeser.** Une cellule maigrit
    /// quand sa voisine arrive — les faces de son bord, jusque-là exposées à
    /// du vide, se retrouvent masquées — et regrossit quand cette voisine est
    /// évincée. Un chunk bâti porte environ 6 800 quads ; ses quatre murs de
    /// bord en valent plusieurs fois autant. Ne repeser que les arrivées
    /// laisserait donc chaque cellule inscrite au poids qu'elle avait SEULE,
    /// soit un surcompte durable — et une fenêtre qui tient la moitié de ce
    /// qu'elle pourrait, en croyant tenir le compte.
    ///
    /// On repèse donc toute cellule résidente qui possède une section VISÉE :
    /// ce sont exactement celles que le remaillage vient de changer.
    ///
    /// Les arrivées passent par `insert` — elles viennent d'être demandées,
    /// leur récence est juste. Les autres par `update`, qui corrige le poids
    /// SANS toucher à la récence : les faire remonter ferait garder au LRU
    /// exactement ce qu'il faudrait lâcher.
    ///
    /// **Une seule passe sur le chantier.** Demander à chaque cellule ce que
    /// ses lots pèsent coûterait O(scène × cellules) — la faute exacte que ce
    /// dépôt a payée trois fois sous le nom de « chauffer le build entier
    /// pour écrire trois blocs ».
    fn peser(&mut self, arrivees: Vec<tf_world::Cellule>, visees: &[tf_mesh::Adresse]) {
        // 1. Les cellules à repeser : les arrivées, plus les résidentes qui
        //    possèdent une section visée.
        let mut cles: std::collections::HashSet<Cle> =
            arrivees.iter().map(|c| (c.niveau, c.x, c.z)).collect();
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

        // 2. Le poids de chaque cellule, en UNE passe sur les lots.
        let mut octets: std::collections::HashMap<Cle, usize> =
            std::collections::HashMap::with_capacity(cles.len());
        let mut ou: std::collections::HashMap<tf_mesh::Adresse, Cle> =
            std::collections::HashMap::new();
        for c in arrivees.iter().chain(touchees.iter()) {
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
        }
        for l in &self.monde.chantier.lots {
            if let Some(k) = ou.get(&l.adresse) {
                // Les DEUX formes : le `Vec<Quad>` en mémoire vive et sa copie
                // packée dans l'arène. Ne compter que l'une sous-compterait le
                // maillage d'un tiers, sur la moitié la plus lourde d'une
                // région bâtie.
                *octets.get_mut(k).expect("clé posée juste au-dessus") +=
                    l.octets() + l.octets_vive();
            }
        }

        // 3. Les corrections d'abord, les arrivées ensuite — c'est l'ordre qui
        //    compte. Évincer sur des poids encore faux ferait lâcher la
        //    mauvaise cellule.
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
        for c in arrivees {
            let k = (c.niveau, c.x, c.z);
            let n = octets.get(&k).copied().unwrap_or(0);
            let ev = self.residence.insert(
                k,
                Resident {
                    cellule: c,
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
        let Some(st) = &self.staging else {
            return Err("la fixture n'a pas de save derrière elle".into());
        };
        let Some(b) = bornes else {
            return self.recharger();
        };
        let visees =
            Grille::sections_autour([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
        if visees.is_empty() {
            return Ok(());
        }
        // **On relit les SECTIONS visées, et rien de plus.**
        //
        // Premier jet : la boîte allait de y = −64 à 319 « pour être sûr ».
        // Mesuré, le remaillage incrémental gagnait ×1,1 sur un rechargement
        // complet — autant dire rien : pour trois blocs je relisais neuf
        // chunks sur TOUTE la hauteur du monde, là où la zone entière n'en
        // faisait que quatre. Le chemin rapide lisait plus que le lent.
        //
        // La hauteur se borne donc aux sections visées, et le rectangle à
        // l'intersection avec la ZONE affichée : un remaillage n'a pas à
        // charger des chunks que la scène ne montre pas.
        let [zx0, zz0, zx1, zz1] = self.zone;
        let serre = |v: i32, bas: i32, haut: i32| v.clamp(bas, haut);
        let x0 = serre(
            visees.iter().map(|a| a.0).min().unwrap() * 16,
            zx0 * 16,
            zx1 * 16 + 15,
        );
        let x1 = serre(
            visees.iter().map(|a| a.0).max().unwrap() * 16 + 15,
            zx0 * 16,
            zx1 * 16 + 15,
        );
        let z0 = serre(
            visees.iter().map(|a| a.1).min().unwrap() * 16,
            zz0 * 16,
            zz1 * 16 + 15,
        );
        let z1 = serre(
            visees.iter().map(|a| a.1).max().unwrap() * 16 + 15,
            zz0 * 16,
            zz1 * 16 + 15,
        );
        let y0 = visees.iter().map(|a| a.2 as i32).min().unwrap() * 16;
        let y1 = visees.iter().map(|a| a.2 as i32).max().unwrap() * 16 + 15;
        let lu = tf_world::BBox::new(BlockPos::new(x0, y0, z0), BlockPos::new(x1, y1, z1));
        let t0 = std::time::Instant::now();
        let connus = self.monde.interner.len();
        let mut interner = std::mem::take(&mut self.monde.interner);
        let grille = &mut self.monde.grille;
        // **Les sections visées sont RETIRÉES d'abord.** Une section que la
        // save n'a plus ne revient pas de la relecture : sans ce retrait, son
        // ancien contenu resterait dans la grille et les blocs effacés
        // resteraient à l'écran.
        //
        // **Aujourd'hui, ce retrait n'est pas observable**, et c'est mesuré :
        // la mutation qui le supprime ne fait rougir aucun test, y compris
        // celui qui vide une section entière. La raison est que NOTRE écrivain
        // ne supprime jamais une section — le splice garde la section, avec
        // une palette d'air. Le jeu, lui, les supprime. Le retrait protège donc
        // d'un écrivain, pas d'un bug : le jour où l'on laisse tomber les
        // sections tout-air à l'écriture (ce qui serait légitime), son absence
        // laisserait de la géométrie fantôme sans qu'aucun test ne le dise.
        // Il reste, et l'hypothèse qu'il couvre est écrite ici.
        for a in &visees {
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
                if let Some(bi) = s.biomes {
                    grille.poser_biomes(s.chunk.x, s.chunk.z, y, bi);
                }
                grille.poser(s.chunk.x, s.chunk.z, s.section);
            },
        );
        self.monde.interner = interner;
        phase("relecture", t0);
        self.refaire(&visees, connus)
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
        // Rien à poser, rien à dégager, et rien au-dessus du budget : il n'y a
        // pas de raison de recopier les arènes.
        if lot.is_empty()
            && self.a_degager.is_empty()
            && self.residence.used() <= self.residence.budget()
        {
            return Ok(0);
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
        let coupes = self.residence.trim();
        self.deborde = coupes.over_budget;
        self.a_degager
            .extend(coupes.items.into_iter().map(|(_, r)| r.cellule));
        for c in std::mem::take(&mut self.a_degager) {
            self.degagees += 1;
            for a in adresses_de(&c) {
                self.monde.grille.retirer(a);
            }
            let b = &c.boite;
            visees.extend(Grille::sections_autour(
                [b.min.x, b.min.y, b.min.z],
                [b.max.x, b.max.y, b.max.z],
            ));
        }

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

            // **Les visées DÉBORDENT de la cellule d'une case.** Poser une
            // cellule change les faces visibles de ses VOISINES : sans la
            // marge, un mur de faces fantômes resterait le long de chaque
            // frontière, et il faudrait tout remailler pour le faire
            // disparaître.
            let b = cellule.boite;
            visees.extend(Grille::sections_autour(
                [b.min.x, b.min.y, b.min.z],
                [b.max.x, b.max.y, b.max.z],
            ));
            arrivees.push(cellule);
        }
        // Deux cellules voisines partagent leur marge : sans dédoublonnage, la
        // section frontière serait maillée deux fois et `Chantier::remplacer`
        // en garderait deux lots.
        visees.sort_unstable();
        visees.dedup();
        let avant = self.rechargements;
        self.refaire(&visees, connus)?;
        // **La pesée vient APRÈS le maillage**, et l'éviction qu'elle
        // déclenche part au prochain appel : voir `a_degager`.
        //
        // Sauf si le remaillage a fini par RECHARGER — le repli du cas où
        // l'atlas ne peut pas s'étendre. Le rechargement remplace le monde
        // par la seule zone, donc les cellules qu'on vient de poser n'y sont
        // plus : les inscrire les ferait compter pour zéro octet, et la
        // fenêtre croirait tenir des cellules absentes. `recharger` a déjà
        // réinscrit ce qui reste.
        if self.rechargements == avant {
            self.peser(arrivees, &visees);
        }
        self.monde.quads = self.monde.chantier.quads();
        self.monde.poses = self.monde.chantier.poses();
        Ok(posees)
    }

    /// **Ce qui suit toute arrivée de blocs** : accueillir les états neufs,
    /// remailler les sections visées, remplacer les tranches d'arène.
    ///
    /// Écrite UNE fois et partagée par le remaillage d'édition et
    /// l'intégration d'une cellule chargée. Ce dépôt a payé quatre fois le
    /// piège des deux implémentations d'une même règle, et celle-ci en porte
    /// trois d'un coup — l'extension d'atlas, l'ordre des tranches, la somme
    /// préfixe des poses.
    ///
    /// `connus` est la taille de la table d'états AVANT l'arrivée : ce qui est
    /// au-delà est neuf et n'a pas encore de texture.
    fn refaire(&mut self, visees: &[tf_mesh::Adresse], connus: usize) -> Result<(), String> {
        self.sections_remaillees = visees.len();
        // **Un état jamais vu ÉTEND l'atlas ; il ne recharge pas la zone.**
        //
        // C'était la dernière porte vers le chemin complet, et elle s'ouvrait
        // au geste le plus banal d'un éditeur : prendre un bloc dans la
        // palette et le poser. Mesuré sur une zone de 64 chunks, poser un bloc
        // connu coûtait 0,7 ms et poser un bloc NEUF 22,3 — × 33 — et ce coût
        // est en O(zone), donc 867 ms sur une région bâtie : une seconde de
        // fenêtre figée pour un bloc. C'est le `warmup(extent)`
        // d'`ExeWorldEdit` (5,2 s pour une sphère de 62 blocs), qui a coûté
        // cher deux applications de suite.
        //
        // L'extension est sûre parce que les indices de couche déjà attribués
        // ne bougent pas, et que `TableFormes` comme l'habillage sont indexés
        // par `StateId` dans l'ORDRE d'internement : les états neufs portent
        // les identifiants suivants, donc s'ajoutent à la fin. Rien de ce qui
        // est déjà maillé ne change de sens.
        if self.monde.interner.len() > connus && !self.etendre_atlas(connus) {
            return self.recharger();
        }
        let t1 = std::time::Instant::now();
        let neufs = self.monde.grille.mailler_ces(&self.monde.table, visees);
        self.monde.chantier.remplacer(visees, neufs);
        phase("maillage ", t1);
        let t2 = std::time::Instant::now();
        // **L'arène des quads se REMPLACE tranche par tranche.** La rebâtir
        // entière coûtait 52 ms sur les 80 d'une édition de trois blocs, sur
        // une région bâtie de 256 chunks : du travail en O(scène) pour un
        // geste en O(édition), la même famille que le rechargement d'atlas.
        self.monde.arene.remplacer(
            &self.monde.chantier,
            visees,
            &apparence(
                &self.monde.habillage,
                &self.monde.interner,
                &self.assets.climat,
            ),
        );
        // **La passe de MODÈLES se remplace aussi.** Une fois l'arène des
        // quads corrigée, c'est elle qui dominait : 23 à 29 ms des 35 pour
        // trois blocs posés sur une région bâtie.
        self.monde.modeles.remplacer(
            &self.monde.chantier,
            visees,
            &modele_de(
                &self.monde.table,
                &self.monde.habillage,
                &self.monde.interner,
                &self.assets.climat,
            ),
        );
        phase("arènes   ", t2);
        self.monde.quads = self.monde.chantier.quads();
        self.monde.poses = self.monde.chantier.poses();
        Ok(())
    }

    /// **Accueille les états découverts depuis `connus`**, sans rien rebâtir.
    ///
    /// Rend `false` quand l'extension n'est pas possible — une texture neuve
    /// plus grande que le côté du tableau, qu'on ne veut pas réduire — et
    /// l'appelant recharge alors. Jamais silencieux : réduire une tuile
    /// perdrait la moitié de ses pixels, et la règle du dépôt est d'agrandir
    /// les petites.
    fn etendre_atlas(&mut self, connus: usize) -> bool {
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
        if let Some(nom) = ajout.trop_grande {
            eprintln!("texture « {nom} » plus grande que l'atlas : rechargement");
            return false;
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
            &mut self.monde.table,
            &mut self.monde.habillage,
        );
        debug_assert_eq!(
            self.monde.habillage.len(),
            self.monde.interner.len(),
            "l'habillage est indexé par StateId : il doit couvrir tous les états"
        );
        true
    }

    /// Tout relire et tout remailler — y compris l'atlas.
    ///
    /// **Le chemin coûteux, et il se COMPTE** (`rechargements`). Il paie la
    /// ZONE, pas ce qui a changé : sur une région bâtie, 867 ms contre
    /// quelques millisecondes pour un remaillage incrémental. Tout appel
    /// après une édition ordinaire est un bug, et un test le vérifie.
    fn recharger(&mut self) -> Result<(), String> {
        self.rechargements += 1;
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
fn phase(quoi: &str, depuis: std::time::Instant) {
    if std::env::var_os("TF_PHASES").is_some() {
        eprintln!(
            "  {quoi} : {:.1} ms",
            depuis.elapsed().as_secs_f64() * 1000.0
        );
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
