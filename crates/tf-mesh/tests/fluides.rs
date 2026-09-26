//! La passe de fluides, croisée avec une transcription INDÉPENDANTE du jeu.
//!
//! La passe décide par rangées de bits, fusionne les faces plates et encode
//! ses clés ; la référence ci-dessous fait tout case par case, sans bits ni
//! fusion, en suivant `LiquidBlockRenderer::tesselate` de 1.18 ligne à ligne
//! — y compris la façon dont il place les sommets d'un côté, d'où l'on tire
//! quelle hauteur va à quel bout sans recopier la table de la passe. Les deux
//! doivent rendre les mêmes faces, case pour case, sur des mondes tirés au
//! hasard.
//!
//! Les scénarios écrits à la main disent ensuite ce que le croisement ne peut
//! pas dire : les VALEURS du jeu (8/9 pour une étendue, la chute d'un bord),
//! et que la fusion fusionne vraiment.

use std::collections::HashMap;

use tf_anvil::StateId;
use tf_mesh::fluides::{angle_du_courant, mailler};
use tf_mesh::forme::Cuboide;
use tf_mesh::{
    Face, FaceFluide, Fluide, Formes, GenreFluide, TableFormes, TextureFluide, Voisinage, COTE,
    FACES,
};

const AIR: StateId = 0;
const PIERRE: StateId = 1;
const VERRE: StateId = 2;
const DALLE: StateId = 3;
const FLEUR: StateId = 4;
/// L'eau de niveau `n` vaut `EAU + n`, la lave `LAVE + n`.
const EAU: StateId = 5;
const LAVE: StateId = 21;
const ESCALIER_INONDE: StateId = 37;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new()); // air
    t.pousser(false, true, Vec::new()); // pierre
    t.pousser(false, false, vec![Cuboide::PLEIN]); // verre : plein, translucide
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    ); // dalle
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.8, 0.0, 8.0],
            max: [15.2, 16.0, 8.0],
            faces: 0x3F,
            cull: 0,
        }],
    ); // fleur : un plan, sans épaisseur
    for genre in [GenreFluide::Eau, GenreFluide::Lave] {
        for niveau in 0..16u8 {
            // L'eau est de l'AIR pour les passes de blocs.
            let id = t.pousser(true, false, Vec::new());
            t.marquer_fluide(id, Fluide { genre, niveau });
        }
    }
    let id = t.pousser(
        false,
        false,
        vec![
            Cuboide {
                min: [0.0, 0.0, 0.0],
                max: [16.0, 8.0, 16.0],
                faces: 0x3F,
                cull: 0x3F,
            },
            Cuboide {
                min: [0.0, 8.0, 8.0],
                max: [16.0, 16.0, 16.0],
                faces: 0x3F,
                cull: 0x3F,
            },
        ],
    );
    assert_eq!(id, ESCALIER_INONDE);
    t.marquer_fluide(id, Fluide::source(GenreFluide::Eau));
    t
}

// ── La référence : le jeu, case par case ─────────────────────────────────

/// Un côté tel que `tesselate` l'émet : sa face, ses deux sommets du haut —
/// `(abscisse le long de la face, hauteur)` — et le pas vers sa voisine.
type Cote = (Face, (f64, f32), (f64, f32), [i32; 3]);

/// Une face telle que le jeu la décide, pour UNE case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Attendue {
    genre: GenreFluide,
    texture: TextureFluide,
    /// Dessus : `[NO, SO, SE, NE]`. Côtés : `[bout bas, bout haut, 0, 0]`.
    /// Dessous : zéros.
    hauteurs: [u8; 4],
    angle: u16,
    biome: StateId,
}

fn en_255(h: f32) -> u8 {
    (h * 255.0).round() as u8
}

fn reference(
    t: &TableFormes,
    w: &dyn Fn(i32, i32, i32) -> StateId,
    biome: &dyn Fn(i32, i32, i32) -> StateId,
) -> HashMap<([i32; 3], Face), Attendue> {
    let mut out = HashMap::new();
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let Some(propre) = t.fluide(w(x, y, z)) else {
                    continue;
                };
                let g = propre.genre;
                let meme =
                    |x: i32, y: i32, z: i32| t.fluide(w(x, y, z)).map(|f| f.genre) == Some(g);
                let own = |x: i32, y: i32, z: i32| {
                    t.fluide(w(x, y, z))
                        .map_or(0.0, |f| f.quantite() as f32 / 9.0)
                };
                // `getWaterHeight(pos)` : le coin (px, pz) du bloc `pos`.
                let get_water_height = |px: i32, pz: i32| -> f32 {
                    let mut i = 0u32;
                    let mut f = 0.0f32;
                    for j in 0..4 {
                        let (bx, bz) = (px - (j & 1), pz - ((j >> 1) & 1));
                        if meme(bx, y + 1, bz) {
                            return 1.0;
                        }
                        if meme(bx, y, bz) {
                            let h = own(bx, y, bz);
                            if h >= 0.8 {
                                f += h * 10.0;
                                i += 10;
                            } else {
                                f += h;
                                i += 1;
                            }
                        } else if !t.solide(w(bx, y, bz)) {
                            i += 1;
                        }
                    }
                    f / i as f32
                };
                let f7 = get_water_height(x, z);
                let f8 = get_water_height(x, z + 1);
                let f9 = get_water_height(x + 1, z + 1);
                let f10 = get_water_height(x + 1, z);
                let opaque = |x: i32, y: i32, z: i32| t.opaque(w(x, y, z));
                let b = if g == GenreFluide::Eau {
                    biome(x, y, z)
                } else {
                    0
                };

                // Le dessus.
                if !meme(x, y + 1, z) {
                    let min = f7.min(f8).min(f9.min(f10));
                    let cache = min >= 1.0 && opaque(x, y + 1, z);
                    if !cache {
                        // `getFlow`, sa partie horizontale.
                        let mut d0 = 0.0f64;
                        let mut d1 = 0.0f64;
                        for (sx, sz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                            let (nx, nz) = (x + sx, z + sz);
                            let nf = t.fluide(w(nx, y, nz));
                            let affecte = nf.is_none_or(|f| f.genre == g);
                            if !affecte {
                                continue;
                            }
                            let mut f = own(nx, y, nz);
                            let mut f1 = 0.0f32;
                            if f == 0.0 {
                                if !t.solide(w(nx, y, nz)) {
                                    let bf = t.fluide(w(nx, y - 1, nz));
                                    if bf.is_none_or(|f| f.genre == g) {
                                        f = own(nx, y - 1, nz);
                                        if f > 0.0 {
                                            f1 = own(x, y, z) - (f - 0.888_888_9);
                                        }
                                    }
                                }
                            } else if f > 0.0 {
                                f1 = own(x, y, z) - f;
                            }
                            if f1 != 0.0 {
                                d0 += (sx as f32 * f1) as f64;
                                d1 += (sz as f32 * f1) as f64;
                            }
                        }
                        let (texture, angle) = if d0 == 0.0 && d1 == 0.0 {
                            (TextureFluide::Immobile, 0)
                        } else {
                            (TextureFluide::Courant, angle_du_courant(d0, d1))
                        };
                        out.insert(
                            ([x, y, z], Face::PlusY),
                            Attendue {
                                genre: g,
                                texture,
                                hauteurs: [en_255(f7), en_255(f8), en_255(f9), en_255(f10)],
                                angle,
                                biome: b,
                            },
                        );
                    }
                }

                // Le dessous.
                if !meme(x, y - 1, z) && !opaque(x, y - 1, z) {
                    out.insert(
                        ([x, y, z], Face::MoinsY),
                        Attendue {
                            genre: g,
                            texture: TextureFluide::Immobile,
                            hauteurs: [0; 4],
                            angle: 0,
                            biome: b,
                        },
                    );
                }

                // Les quatre côtés, dans l'ordre et avec les sommets du jeu :
                // (bout 1, hauteur 1) et (bout 2, hauteur 2) le long de l'axe
                // horizontal de la face.
                let cotes: [Cote; 4] = [
                    (Face::MoinsZ, (0.0, f7), (1.0, f10), [0, 0, -1]),
                    (Face::PlusZ, (1.0, f9), (0.0, f8), [0, 0, 1]),
                    (Face::MoinsX, (1.0, f8), (0.0, f7), [-1, 0, 0]),
                    (Face::PlusX, (0.0, f10), (1.0, f9), [1, 0, 0]),
                ];
                for (face, a, bout, d) in cotes {
                    let (nx, ny, nz) = (x + d[0], y + d[1], z + d[2]);
                    if meme(nx, ny, nz) || opaque(nx, ny, nz) {
                        continue;
                    }
                    let voisin = w(nx, ny, nz);
                    let voile =
                        g == GenreFluide::Eau && t.cuboides(voisin).iter().any(|c| c.remplit());
                    // Le bout d'abscisse la plus petite d'abord.
                    let (bas, haut) = if a.0 < bout.0 {
                        (a.1, bout.1)
                    } else {
                        (bout.1, a.1)
                    };
                    out.insert(
                        ([x, y, z], face),
                        Attendue {
                            genre: g,
                            texture: if voile {
                                TextureFluide::Voile
                            } else {
                                TextureFluide::Courant
                            },
                            hauteurs: [en_255(bas), en_255(haut), 0, 0],
                            angle: 0,
                            biome: b,
                        },
                    );
                }
            }
        }
    }
    out
}

/// Les axes du plan d'une face, dans l'ordre croissant.
fn axes(face: Face) -> (usize, usize) {
    match face.axe() {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// La sortie de la passe, DÉFUSIONNÉE : une entrée par case. Une case
/// couverte deux fois fait échouer — deux quads l'un sur l'autre se
/// verraient comme une eau deux fois plus sombre.
fn par_case(faces: &[FaceFluide]) -> HashMap<([i32; 3], Face), Attendue> {
    let mut out = HashMap::new();
    for f in faces {
        let (au, av) = axes(f.face);
        for i in 0..f.taille[0] as i32 {
            for j in 0..f.taille[1] as i32 {
                let mut p = [f.pos[0] as i32, f.pos[1] as i32, f.pos[2] as i32];
                p[au] += i;
                p[av] += j;
                let a = Attendue {
                    genre: f.genre,
                    texture: f.texture,
                    hauteurs: f.hauteurs,
                    angle: f.angle,
                    biome: f.biome,
                };
                assert!(
                    out.insert((p, f.face), a).is_none(),
                    "la case {p:?} porte deux faces {:?}",
                    f.face
                );
            }
        }
    }
    out
}

fn voisinage(w: &dyn Fn(i32, i32, i32) -> StateId, biomes: &[StateId]) -> Voisinage {
    let mut v = Voisinage::new();
    v.remplir(w);
    v.poser_biomes(biomes);
    v
}

/// Le biome d'une case, tel que le voisinage le rend (cellules de 4³).
fn biome_de(biomes: &[StateId]) -> impl Fn(i32, i32, i32) -> StateId + '_ {
    move |x, y, z| biomes[tf_mesh::voisinage::cellule(x, y, z)]
}

/// Un tirage déterministe, rejouable.
struct Tirage(u64);
impl Tirage {
    fn sous(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

#[test]
fn la_passe_rend_ce_que_le_jeu_dessine_case_pour_case() {
    let t = table();
    let mut faces_vues = 0usize;
    let mut fusions = 0usize;
    let mut courants = 0usize;
    for graine in 0..60u64 {
        let mut r = Tirage(graine * 7919 + 1);
        // Un monde entier de -1 à 16, peau comprise, tiré case par case —
        // avec, une graine sur deux, une nappe calme par-dessus un fond, pour
        // que la fusion ait de quoi fusionner.
        let calme = graine % 2 == 0;
        let mut monde = HashMap::new();
        for y in -1..=16 {
            for z in -1..=16 {
                for x in -1..=16 {
                    let id = if calme && y < 3 {
                        PIERRE
                    } else if calme && y < 11 && r.sous(10) < 8 {
                        EAU
                    } else {
                        match r.sous(100) {
                            0..=29 => AIR,
                            30..=44 => PIERRE,
                            45..=59 => EAU,
                            60..=69 => EAU + 1 + r.sous(7) as StateId,
                            70..=74 => EAU + 8 + r.sous(8) as StateId,
                            75..=79 => LAVE + r.sous(16) as StateId,
                            80..=84 => VERRE,
                            85..=89 => DALLE,
                            90..=94 => FLEUR,
                            _ => ESCALIER_INONDE,
                        }
                    };
                    monde.insert((x, y, z), id);
                }
            }
        }
        let w = |x: i32, y: i32, z: i32| monde[&(x, y, z)];
        // Des biomes différents d'une cellule à l'autre, et parfois les mêmes.
        let biomes: Vec<StateId> = (0..64).map(|_| 100 + r.sous(3) as StateId).collect();
        let v = voisinage(&w, &biomes);
        let mut faces = Vec::new();
        mailler(&v, &t, &mut faces);
        let obtenu = par_case(&faces);
        let attendu = reference(&t, &w, &biome_de(&biomes));
        for (k, a) in &attendu {
            assert_eq!(
                obtenu.get(k),
                Some(a),
                "graine {graine} : la face {:?} de la case {:?}",
                k.1,
                k.0
            );
        }
        let en_trop: Vec<_> = obtenu
            .iter()
            .filter(|(k, _)| !attendu.contains_key(k))
            .take(5)
            .map(|(k, a)| (k.0, k.1, *a, w(k.0[0], k.0[1], k.0[2])))
            .collect();
        assert_eq!(
            obtenu.len(),
            attendu.len(),
            "graine {graine} : des faces que le jeu ne dessine pas : {en_trop:?}"
        );
        faces_vues += attendu.len();
        fusions += attendu.len() - faces.len();
        courants += attendu
            .values()
            .filter(|a| a.texture == TextureFluide::Courant && a.angle != 0)
            .count();
    }
    // Les prémisses : le croisement a vu des faces, des fusions et des
    // courants — sans elles, il passerait aussi sur une passe qui ne fait rien.
    // Mesuré : 252 616 faces, 24 373 économisées, 22 492 courants.
    assert!(faces_vues > 100_000, "{faces_vues} faces");
    assert!(
        fusions > 10_000,
        "{fusions} faces économisées par la fusion"
    );
    assert!(courants > 10_000, "{courants} surfaces qui courent");
}

// ── Les valeurs du jeu, à la main ────────────────────────────────────────

/// Un monde décrit par une fonction, le reste en air.
fn faces_de(w: impl Fn(i32, i32, i32) -> StateId) -> Vec<FaceFluide> {
    let t = table();
    let v = voisinage(&w, &[7; 64]);
    let mut out = Vec::new();
    mailler(&v, &t, &mut out);
    out
}

#[test]
fn une_etendue_calme_sort_en_un_quad_a_huit_neuviemes() {
    // Un bassin de 16 × 16 sur un fond de pierre, bordé de pierre : la
    // surface est UNE face, à 8/9 de bloc, immobile. Aucun côté, aucun fond.
    let f = faces_de(|x, y, z| {
        let dedans = (0..16).contains(&x) && (0..16).contains(&z);
        match y {
            ..=3 => PIERRE,
            4..=7 if dedans => EAU,
            4..=7 => PIERRE,
            _ => AIR,
        }
    });
    assert_eq!(f.len(), 1, "{f:?}");
    let s = f[0];
    assert_eq!(s.face, Face::PlusY);
    assert_eq!(s.pos, [0, 7, 0]);
    assert_eq!(s.taille, [16, 16]);
    assert_eq!(s.texture, TextureFluide::Immobile);
    assert_eq!(s.hauteurs, [227; 4], "8/9 de bloc, en 255e");
    assert_eq!(s.genre, GenreFluide::Eau);
    assert_eq!(s.biome, 7, "l'eau prend le biome de sa case");
}

#[test]
fn une_source_seule_sur_la_pierre_plonge_vers_ses_bords() {
    // Une source posée sur la pierre, de l'air autour : chaque coin compte la
    // source (8/9, pondérée par dix) et trois colonnes d'air (zéro, une
    // chacune) — 80/9 ÷ 13 = 0,684 de bloc.
    let f = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        _ => AIR,
    });
    let dessus = f.iter().find(|f| f.face == Face::PlusY).expect("un dessus");
    assert_eq!(dessus.hauteurs, [en_255(80.0 / 9.0 / 13.0); 4]);
    assert_eq!(dessus.hauteurs[0], 174);
    // Quatre côtés, pas de dessous : la pierre le cache.
    let mut les_faces: Vec<Face> = f.iter().map(|f| f.face).collect();
    les_faces.sort();
    assert_eq!(
        les_faces,
        vec![
            Face::MoinsX,
            Face::PlusX,
            Face::PlusY,
            Face::MoinsZ,
            Face::PlusZ
        ]
    );
    for c in f.iter().filter(|f| f.face != Face::PlusY) {
        assert_eq!(
            c.hauteurs[..2],
            [174, 174],
            "le haut d'un côté suit ses coins"
        );
        assert_eq!(
            c.texture,
            TextureFluide::Courant,
            "un côté porte le courant"
        );
    }
}

#[test]
fn de_l_eau_au_dessus_monte_les_coins_a_la_case_entiere() {
    // Une colonne d'eau : la case du bas n'a pas de dessus (même fluide
    // au-dessus), et ses côtés montent à la case entière — la chute
    // continue, sans marche à chaque bloc.
    let f = faces_de(|x, y, z| match (x, y, z) {
        (5, 1..=3, 5) => EAU,
        (_, 0, _) => PIERRE,
        _ => AIR,
    });
    let dessus: Vec<_> = f.iter().filter(|f| f.face == Face::PlusY).collect();
    assert_eq!(dessus.len(), 1, "un seul dessus, en haut de la colonne");
    assert_eq!(dessus[0].pos[1], 3);
    // Les deux rangées du bas ont leurs côtés PLEINS, et fusionnent en
    // hauteur ; la rangée du haut est à part.
    let ouest: Vec<_> = f.iter().filter(|f| f.face == Face::MoinsX).collect();
    assert_eq!(ouest.len(), 2, "{ouest:?}");
    let plein = ouest
        .iter()
        .find(|f| f.hauteurs[0] == 255)
        .expect("les rangées pleines");
    assert_eq!(plein.pos[1], 1);
    assert_eq!(
        plein.taille,
        [2, 1],
        "deux rangées fondues (Y est le premier axe du plan)"
    );
    let haut = ouest
        .iter()
        .find(|f| f.hauteurs[0] != 255)
        .expect("la dernière rangée");
    assert_eq!(haut.pos[1], 3);
    assert_eq!(haut.taille, [1, 1]);
}

#[test]
fn un_courant_tourne_sa_texture_vers_l_aval() {
    // Une source à l'ouest, un courant de niveau 1 à l'est, sur la pierre et
    // entre deux murs : l'eau court vers l'EST (+X). Le jeu tourne alors la
    // texture de `atan2(0, 1) − π/2`, soit trois quarts de tour.
    let f = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (_, 1, 4) | (_, 1, 6) => PIERRE,
        (4, 1, 5) => EAU,
        (5, 1, 5) => EAU + 1,
        (3, 1, 5) | (6, 1, 5) => PIERRE,
        _ => AIR,
    });
    let dessus = |x: u8| {
        *f.iter()
            .find(|f| f.face == Face::PlusY && f.pos == [x, 1, 5])
            .expect("un dessus")
    };
    let source = dessus(4);
    let courant = dessus(5);
    assert_eq!(
        source.texture,
        TextureFluide::Courant,
        "la source se vide vers l'est"
    );
    assert_eq!(source.angle, 49_152, "trois quarts de tour");
    assert_eq!(courant.texture, TextureFluide::Courant);
    assert_eq!(courant.angle, 49_152);
    // Et la surface DESCEND vers l'aval : les coins est du courant sont plus
    // bas que ses coins ouest.
    let [no, so, se, ne] = courant.hauteurs;
    assert!(se < so && ne < no, "{:?}", courant.hauteurs);
    // Un dessus qui court ne se fusionne pas : sa texture est tournée.
    assert_eq!(courant.taille, [1, 1]);
}

#[test]
fn le_sens_de_la_texture_suit_celui_du_jeu() {
    // `atan2(dz, dx) − π/2`, en 65 536e de tour, ramené dans [0, 1).
    assert_eq!(angle_du_courant(1.0, 0.0), 49_152, "vers l'est");
    assert_eq!(angle_du_courant(0.0, 1.0), 0, "vers le sud");
    assert_eq!(angle_du_courant(-1.0, 0.0), 16_384, "vers l'ouest");
    assert_eq!(angle_du_courant(0.0, -1.0), 32_768, "vers le nord");
    assert_eq!(angle_du_courant(1.0, 1.0), 57_344, "vers le sud-est");
}

#[test]
fn contre_le_verre_l_eau_montre_son_voile() {
    let f = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        (6, 1, 5) => VERRE,
        (4, 1, 5) => DALLE,
        _ => AIR,
    });
    let cote = |face: Face| f.iter().find(|f| f.face == face).expect("un côté").texture;
    assert_eq!(cote(Face::PlusX), TextureFluide::Voile, "contre le verre");
    assert_eq!(
        cote(Face::MoinsX),
        TextureFluide::Courant,
        "contre une dalle"
    );
    assert_eq!(cote(Face::PlusZ), TextureFluide::Courant, "contre l'air");
}

#[test]
fn un_bloc_inonde_est_de_l_eau_pour_sa_voisine() {
    // Un escalier inondé à côté d'une source : aucune face entre les deux,
    // et leurs surfaces sont à la même hauteur.
    let f = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        (6, 1, 5) => ESCALIER_INONDE,
        (4, 1, 5) | (7, 1, 5) => PIERRE,
        (_, 1, 4) | (_, 1, 6) => PIERRE,
        _ => AIR,
    });
    assert!(
        f.iter().all(|f| f.face == Face::PlusY),
        "rien que des dessus : {f:?}"
    );
    let dessus: Vec<_> = f.iter().collect();
    assert_eq!(dessus.len(), 1, "les deux surfaces se fondent : {dessus:?}");
    assert_eq!(dessus[0].taille, [2, 1]);
    assert_eq!(dessus[0].hauteurs, [227; 4]);
}

#[test]
fn la_lave_n_a_pas_de_biome_et_ne_voit_pas_l_eau() {
    // Lave et eau côte à côte : chacune montre sa face à l'autre. La lave ne
    // prend pas de couleur de biome — sa clé de fusion n'en porte pas.
    let f = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        (6, 1, 5) => LAVE,
        _ => AIR,
    });
    let lave: Vec<_> = f.iter().filter(|f| f.genre == GenreFluide::Lave).collect();
    assert!(lave.iter().all(|f| f.biome == 0));
    assert!(lave.iter().any(|f| f.face == Face::MoinsX), "face à l'eau");
    assert!(
        f.iter()
            .any(|f| f.genre == GenreFluide::Eau && f.face == Face::PlusX),
        "et l'eau face à la lave"
    );
}

#[test]
fn une_frontiere_de_biome_coupe_la_surface_de_l_eau_pas_celle_de_la_lave() {
    // Deux biomes de part et d'autre de x = 8. L'eau en prend la couleur :
    // sa surface se coupe en deux. La lave non : elle reste d'un seul tenant.
    let biomes: Vec<StateId> = (0..64).map(|c| if c % 4 < 2 { 1 } else { 2 }).collect();
    for (fluide, attendu) in [(EAU, 2), (LAVE, 1)] {
        let t = table();
        let v = voisinage(
            &|_, y, _| match y {
                ..=3 => PIERRE,
                4 => fluide,
                _ => AIR,
            },
            &biomes,
        );
        // Un fond et des bords de pierre : seul le dessus se voit.
        let mut v2 = v.clone();
        for y in -1..=16 {
            for z in -1..=16 {
                for x in -1..=16 {
                    if y == 4 && !Voisinage::dedans(x, 0, z) {
                        v2.set(x, y, z, PIERRE);
                    }
                }
            }
        }
        let mut out = Vec::new();
        mailler(&v2, &t, &mut out);
        assert_eq!(out.len(), attendu, "{fluide} : {out:?}");
    }
}

#[test]
fn une_section_d_eau_profonde_ne_rend_rien() {
    // De l'eau partout, peau comprise : aucune face. C'est le cas de toute
    // section sous la surface d'un océan — elle doit coûter peu et ne rien
    // dessiner.
    let f = faces_de(|_, _, _| EAU);
    assert!(f.is_empty(), "{f:?}");
    // La même sous un ciel : UN quad de 16 × 16, sa surface — la peau porte
    // de l'eau sur les côtés, donc aucun côté.
    let f = faces_de(|_, y, _| if y <= 15 { EAU } else { AIR });
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].face, Face::PlusY);
    assert_eq!(f[0].taille, [16, 16]);
    assert_eq!(f[0].hauteurs, [227; 4]);
}

#[test]
fn un_dessus_plein_sous_la_pierre_se_cache_un_dessus_plus_bas_non() {
    // Une case d'eau sous la pierre, avec de l'eau au-dessus d'une colonne
    // voisine : ses coins montent à la case entière et le dessus se cache
    // contre la pierre. Sans eau voisine au-dessus, il reste à 8/9, sous la
    // pierre, et le jeu le dessine.
    let avec = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        (5, 2, 5) => PIERRE,
        (4..=6, 1..=2, 4..=6) => EAU,
        _ => PIERRE,
    });
    assert!(
        !avec
            .iter()
            .any(|f| f.face == Face::PlusY && f.pos == [5, 1, 5]),
        "{avec:?}"
    );
    let sans = faces_de(|x, y, z| match (x, y, z) {
        (_, 0, _) => PIERRE,
        (5, 1, 5) => EAU,
        (5, 2, 5) => PIERRE,
        (4..=6, 1, 4..=6) => EAU,
        _ => PIERRE,
    });
    assert!(
        sans.iter().any(|f| f.face == Face::PlusY && f.pos[1] == 1),
        "{sans:?}"
    );
}

#[test]
fn sans_fluide_la_passe_ne_rend_rien() {
    let t = table();
    let mut v = Voisinage::new();
    v.remplir(|_, y, _| if y < 8 { PIERRE } else { AIR });
    let mut out = Vec::new();
    mailler(&v, &t, &mut out);
    assert!(out.is_empty());
    // Et un hôte qui ne connaît pas les fluides maille comme avant.
    struct SansFluides;
    impl Formes for SansFluides {
        fn est_air(&self, id: StateId) -> bool {
            id == AIR
        }
        fn opaque(&self, id: StateId) -> bool {
            id == PIERRE
        }
        fn cuboides(&self, _: StateId) -> &[Cuboide] {
            &[]
        }
    }
    v.remplir(|_, _, _| EAU);
    mailler(&v, &SansFluides, &mut out);
    assert!(out.is_empty());
}

#[test]
fn les_six_faces_d_une_case_isolee_suivent_l_ordre_du_mailleur() {
    // Une goutte en plein air : six faces, une par direction, et aucune en
    // double. L'ordre est celui de `FACES`, que le shader relit.
    let f = faces_de(|x, y, z| if (x, y, z) == (8, 8, 8) { EAU + 7 } else { AIR });
    let mut vues: Vec<Face> = f.iter().map(|f| f.face).collect();
    vues.sort();
    assert_eq!(vues, FACES.to_vec());
    let _ = COTE;
}
