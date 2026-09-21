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

/// Charge les assets, et un monde — une VRAIE save, ou la fixture.
///
/// La borne est toujours EXPLICITE : il n'existe aucun état « le monde est
/// chargé », une région pleine faisant déjà cent millions de blocs. Sans
/// `zone`, on prend un petit rectangle de chunks, ce qui est un aperçu et pas
/// un défaut à étendre.
pub fn charger(racine: &str, monde: Option<&str>, zone: [i32; 4]) -> Result<Monde, String> {
    let (cat, src, genre) =
        tf_assets::jeu::catalogue(racine).map_err(|e| format!("assets illisibles : {e:?}"))?;
    let disposition = genre.disposition();

    let mut grille = Grille::new();
    let mut interner = Interner::new();
    let quoi = match monde {
        Some(dir) => {
            let source = tf_world::FsSource::open(dir).map_err(|e| format!("monde : {e:?}"))?;
            let [x0, z0, x1, z1] = zone;
            let sel = tf_world::BBox::new(
                BlockPos::new(x0 * 16, -64, z0 * 16),
                BlockPos::new(x1 * 16 + 15, 319, z1 * 16 + 15),
            );
            let bilan = tf_world::sections_de(
                &source,
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
                "{dir} · chunks {x0}..{x1} × {z0}..{z1} · {} chunks, {} sections",
                bilan.chunks, bilan.sections
            )
        }
        None => {
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
    let voulues = tf_assets::textures_des_etats(&cat, cles.iter().cloned());
    let atlas = tf_assets::Atlas::batir(&src, voulues, &|n| disposition.chemins_texture(n));
    let atlas_complet =
        tf_assets::Atlas::batir(&src, tf_assets::catalogue::textures_citees(&cat), &|n| {
            disposition.chemins_texture(n)
        });
    let translucides = tf_assets::catalogue::blocs_translucides(&cat, &atlas_complet);
    let teintes = tf_assets::Teintes::default();
    let climat = tf_assets::climat::Climat::charger(&src);
    let (table, habillage) =
        tf_assets::table_rendu(&cat, &atlas, &teintes, cles.iter().cloned(), &|n| {
            translucides.contains(n)
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
