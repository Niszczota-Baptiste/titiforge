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
fn arenes(
    chantier: &tf_mesh::Chantier,
    table: &TableFormes,
    habillage: &[tf_assets::apparence::Habillage],
    interner: &Interner,
    climat: &tf_assets::climat::Climat,
) -> (Arene, AreneModeles) {
    let teinte_de = |genre: tf_assets::GenreTeinte, biome: StateId| -> Option<[f32; 3]> {
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
    };
    let arene = Arene::depuis(
        chantier,
        &|id, face, biome| match habillage.get(id as usize) {
            Some(h) => {
                let a = h.cube[face.indice()];
                (a.couche, teinte_de(a.genre, biome).unwrap_or(a.teinte))
            }
            None => (0, [1.0; 3]),
        },
    );
    let modeles = AreneModeles::depuis(chantier, &|id, biome| {
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
    });
    (arene, modeles)
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
        Ok(Ouvert {
            assets,
            monde: m,
            staging: Some(staging),
            zone,
            nom: dir.to_string(),
            couche: Some(couche),
        })
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
        if self.monde.interner.len() > connus {
            // Un état que l'atlas ne connaît pas : on recharge tout plutôt que
            // de lui donner la texture d'un autre.
            return self.recharger();
        }
        phase("relecture", t0);
        let t1 = std::time::Instant::now();
        let neufs = self.monde.grille.mailler_ces(&self.monde.table, &visees);
        self.monde.chantier.remplacer(&visees, neufs);
        phase("maillage ", t1);
        let t2 = std::time::Instant::now();
        let (arene, modeles) = arenes(
            &self.monde.chantier,
            &self.monde.table,
            &self.monde.habillage,
            &self.monde.interner,
            &self.assets.climat,
        );
        self.monde.arene = arene;
        self.monde.modeles = modeles;
        phase("arènes   ", t2);
        self.monde.quads = self.monde.chantier.quads();
        self.monde.poses = self.monde.chantier.poses();
        Ok(())
    }

    /// Tout relire et tout remailler — y compris l'atlas.
    fn recharger(&mut self) -> Result<(), String> {
        let st = self
            .staging
            .as_ref()
            .expect("un monde éditable a un staging");
        self.monde = charger_monde(
            &self.assets,
            Ou::Source(st.as_ref(), self.zone, self.nom.clone()),
        )?;
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
