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
        &chantier,
        &|id, face, biome| match habillage.get(id as usize) {
            Some(h) => {
                let a = h.cube[face.indice()];
                (a.couche, teinte_de(a.genre, biome).unwrap_or(a.teinte))
            }
            None => (0, [1.0; 3]),
        },
    );
    let modeles = AreneModeles::depuis(&chantier, &|id, biome| {
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
        tf_render::faces_de(tf_mesh::forme::Formes::cuboides(&table, id), &hab)
    });

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
    })
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
        let source = tf_world::FsSource::open(dir).map_err(|e| format!("monde : {e:?}"))?;
        // **La copie de travail vit à côté.** La save n'est pas ouverte en
        // écriture tant qu'on ne l'a pas demandé — invariant n° 1.
        let couche = std::env::temp_dir().join(format!("titiforge-{}", std::process::id()));
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

    /// Relit la zone et remaille. **Depuis la copie de travail**, pas la
    /// source : c'est elle qui porte ce qu'on vient d'écrire.
    ///
    /// Toute la zone, pas seulement ce qui a bougé. Le remaillage incrémental
    /// viendra ; le faire maintenant demanderait de découper l'arène GPU, et
    /// une arène mal recousue affiche un mur là où il n'y en a plus — un défaut
    /// qu'on met des heures à voir. Sur la zone d'aperçu, tout refaire se
    /// mesure en dizaines de millisecondes.
    pub fn remailler(&mut self) -> Result<(), String> {
        let Some(st) = &self.staging else {
            return Err("la fixture n'a pas de save derrière elle".into());
        };
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
