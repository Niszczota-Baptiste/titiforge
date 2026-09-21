//! Le rendu, vérifié AU PIXEL.
//!
//! C'est tout l'intérêt d'une cible hors écran : une image se compare, un
//! coup d'œil non. Et le défaut que ces tests visent a vécu des années dans
//! `we-engine` sans que personne ne le voie — parce qu'il était invisible sur
//! un build gris.

use tf_mesh::forme::{Cuboide, Formes};
use tf_mesh::{Grille, TableFormes};
use tf_render::{Appareil, Arene, AtlasGpu, Camera, Cible, Scene};

use tf_anvil::{bits_for, pack, Packing, Section, StateId};

const AIR: StateId = 0;
const CUBE: StateId = 1;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    );
    t
}

fn section(y: i8, mut f: impl FnMut(i32, i32, i32) -> StateId) -> Section {
    let mut palette: Vec<StateId> = Vec::new();
    let mut idx = vec![0u16; 4096];
    for by in 0..16i32 {
        for bz in 0..16i32 {
            for bx in 0..16i32 {
                let id = f(bx, by, bz);
                let k = match palette.iter().position(|p| *p == id) {
                    Some(k) => k,
                    None => {
                        palette.push(id);
                        palette.len() - 1
                    }
                };
                idx[(by * 256 + bz * 16 + bx) as usize] = k as u16;
            }
        }
    }
    let bits = bits_for(palette.len());
    let data = if palette.len() == 1 {
        Vec::new()
    } else {
        pack(&idx, bits as usize, Packing::NoStraddle)
    };
    Section {
        y,
        palette,
        bits,
        data: data.into_boxed_slice(),
        packing: Packing::NoStraddle,
    }
}

/// Un atlas d'une seule couche, blanche. Le BLANC est délibéré : il laisse
/// l'ombrage seul décider de la couleur, donc le rend mesurable.
fn atlas_blanc(app: &Appareil) -> AtlasGpu {
    AtlasGpu::nouveau(app, 4, 1, &[255u8; 4 * 4 * 4])
}

/// Un seul cube, rendu, et les pixels rendus.
fn rendre_un_cube(app: &Appareil, cote: u32) -> (Vec<u8>, u32, u32) {
    let t = table();
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |x, y, z| {
            if x == 8 && y == 8 && z == 8 {
                CUBE
            } else {
                AIR
            }
        }),
    );
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    assert_eq!(arene.len(), 6, "un cube isolé montre ses six faces");

    let cible = Cible::nouvelle(app, cote, cote);
    let scene = Scene::nouvelle(app, &arene, &atlas_blanc(app));
    let (min, max) = arene.bornes().unwrap();
    let cam = Camera::cadrer(min, max, 1.0);
    let (pixels, compte) = scene.rendre(&cible, &cam);
    assert_eq!(compte.appels_de_dessin, 1);
    (pixels, cote, cote)
}

fn app() -> Option<Appareil> {
    match Appareil::ouvrir() {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("pas d'adaptateur graphique ici ({e}) : test sauté");
            None
        }
    }
}

/// Luminance moyenne d'une bande horizontale de l'image, pixels de fond exclus.
fn luminance(pixels: &[u8], largeur: u32, y0: u32, y1: u32, fond: [u8; 3]) -> f32 {
    let mut somme = 0f64;
    let mut n = 0u32;
    for y in y0..y1 {
        for x in 0..largeur {
            let i = ((y * largeur + x) * 4) as usize;
            let p = [pixels[i], pixels[i + 1], pixels[i + 2]];
            if p == fond {
                continue;
            }
            somme += (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        (somme / n as f64) as f32
    }
}

#[test]
fn un_cube_se_dessine_et_ne_remplit_pas_toute_l_image() {
    let Some(app) = app() else { return };
    let (pixels, l, h) = rendre_un_cube(&app, 200);
    let fond = [pixels[0], pixels[1], pixels[2]];
    let dessines = (0..(l * h))
        .filter(|i| {
            let k = (*i * 4) as usize;
            [pixels[k], pixels[k + 1], pixels[k + 2]] != fond
        })
        .count();
    assert!(
        dessines > 500,
        "le cube doit couvrir une bonne part du cadre : {dessines} pixels"
    );
    assert!(
        dessines < (l * h) as usize,
        "mais pas toute l'image : il resterait du fond visible"
    );
}

#[test]
fn le_dessus_d_un_cube_est_plus_clair_que_ses_cotes() {
    // C'est LE test qui manquait à `we-engine`. Une table d'ombrage y annonçait
    // « −X +X +Y −Y » pour un mailleur qui produit « −X +X −Y +Y » : le dessus
    // des blocs était assombri et le dessous éclairé à plein, depuis toujours.
    // Invisible sur un build gris, et sorti seulement en posant des textures
    // dessus — l'herbe s'affichait en terre.
    //
    // Un ordre d'indices se MESURE contre le code qui le produit.
    let Some(app) = app() else { return };
    let (pixels, l, h) = rendre_un_cube(&app, 240);
    let fond = [pixels[0], pixels[1], pixels[2]];

    // Vue de trois quarts par au-dessus : le tiers HAUT de la silhouette est
    // le dessus du cube, le tiers BAS ses côtés.
    let haut = luminance(&pixels, l, h / 4, h * 5 / 12, fond);
    let bas = luminance(&pixels, l, h * 7 / 12, h * 3 / 4, fond);
    assert!(
        haut > 0.0 && bas > 0.0,
        "les deux bandes doivent voir du cube"
    );
    assert!(
        haut > bas * 1.15,
        "le DESSUS doit être nettement plus clair que les côtés : {haut:.0} contre {bas:.0}"
    );
}

#[test]
fn une_texture_blanche_ne_sort_pas_blanche() {
    // Une face texturée ne porte plus que l'ombrage dans sa couleur. Si elle
    // sortait à 255, c'est que l'ombrage n'est pas appliqué ; si elle sortait
    // beaucoup plus sombre, c'est qu'il l'est DEUX fois — le défaut qui rendait
    // tout un build deux fois trop sombre dans `we-engine`.
    let Some(app) = app() else { return };
    let (pixels, l, h) = rendre_un_cube(&app, 240);
    let fond = [pixels[0], pixels[1], pixels[2]];
    let dessus = luminance(&pixels, l, h / 4, h * 5 / 12, fond);
    assert!(
        dessus > 200.0,
        "le dessus est à pleine lumière (× 1,0) sur une texture blanche : {dessus:.0}"
    );
    let cotes = luminance(&pixels, l, h * 7 / 12, h * 3 / 4, fond);
    assert!(
        cotes > 100.0 && cotes < 230.0,
        "les côtés portent un ombrage, pas deux : {cotes:.0}"
    );
}

#[test]
fn l_arene_place_chaque_section_a_son_origine() {
    // Les quads sont LOCAUX à leur section. Sans ajouter l'origine, tout le
    // monde se dessinerait empilé sur la section zéro — un build de 800
    // régions tiendrait dans un chunk.
    let t = table();
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |x, y, z| if x + y + z == 0 { CUBE } else { AIR }),
    );
    g.poser(
        3,
        5,
        section(2, |x, y, z| if x + y + z == 0 { CUBE } else { AIR }),
    );
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));

    let (min, max) = arene.bornes().unwrap();
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert!(
        max[0] >= 48.0 && max[2] >= 80.0 && max[1] >= 32.0,
        "la seconde section est en (3, 2, 5) de chunk : {max:?}"
    );
}

#[test]
fn les_tranches_couvrent_toute_l_arene_sans_trou_ni_recouvrement() {
    // Une tranche est ce qui permettra de remailler une section sans toucher
    // aux autres, et de ne dessiner que les tranches visibles. Un trou ou un
    // chevauchement s'y verrait comme un morceau de monde manquant.
    let t = table();
    let mut g = Grille::new();
    let mut n = 3u32;
    for cz in 0..3i32 {
        for cx in 0..3i32 {
            g.poser(
                cx,
                cz,
                section(0, |_, _, _| {
                    n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    if n % 3 == 0 {
                        CUBE
                    } else {
                        AIR
                    }
                }),
            );
        }
    }
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));

    let mut attendu = 0u32;
    for tr in &arene.tranches {
        assert_eq!(tr.debut, attendu, "trou ou recouvrement à {:?}", tr.adresse);
        attendu += tr.nombre;
    }
    assert_eq!(attendu as usize, arene.len());
}

#[test]
fn la_camera_cadre_le_contenu_sans_le_couper() {
    // Une distance devinée ferait sortir le build du cadre dès qu'il change de
    // taille — exactement ce qu'une capture automatique doit éviter.
    let Some(app) = app() else { return };
    let t = table();
    let mut g = Grille::new();
    for cz in 0..2i32 {
        for cx in 0..2i32 {
            g.poser(cx, cz, section(0, |_, _, _| CUBE));
        }
    }
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    let cible = Cible::nouvelle(&app, 200, 200);
    let scene = Scene::nouvelle(&app, &arene, &atlas_blanc(&app));
    let (min, max) = arene.bornes().unwrap();
    let (pixels, _) = scene.rendre(&cible, &Camera::cadrer(min, max, 1.0));

    let fond = [pixels[0], pixels[1], pixels[2]];
    // Le bord de l'image doit rester du FOND : si le build touchait le bord,
    // c'est qu'il déborde.
    let mut bord_touche = 0;
    for x in 0..200u32 {
        for y in [0u32, 199] {
            let i = ((y * 200 + x) * 4) as usize;
            if [pixels[i], pixels[i + 1], pixels[i + 2]] != fond {
                bord_touche += 1;
            }
        }
    }
    assert_eq!(
        bord_touche, 0,
        "{bord_touche} pixels de build touchent le bord"
    );
}

// ── la teinte ───────────────────────────────────────────────────────────────

/// Un atlas d'une couche GRISE — la valeur mesurée de `grass_block_top.png`.
///
/// Grise exprès : c'est ce que le jeu livre pour tout bloc teinté, et c'est
/// précisément ce qui rendait le sol de tout terrain blanchâtre quand la
/// teinte était ignorée. Un atlas blanc ne montrerait pas le défaut.
fn atlas_gris_147(app: &Appareil) -> AtlasGpu {
    let mut px = Vec::new();
    for _ in 0..(4 * 4) {
        px.extend([147u8, 147, 147, 255]);
    }
    AtlasGpu::nouveau(app, 4, 1, &px)
}

/// Le dessus du cube, moyenné : `+Y` est la seule face à ombrage 1,0, donc la
/// seule où la couleur rendue est celle de la teinte et rien d'autre.
///
/// La bande haute du cadrage, et le fond EXCLU — il est bleu-gris, pas noir,
/// et le moyenner ferait passer un cube invisible pour un cube terne.
fn dessus(pixels: &[u8], cote: u32) -> [u32; 3] {
    let fond = [pixels[0], pixels[1], pixels[2]];
    let (mut s, mut n) = ([0u64; 3], 0u64);
    // La même bande que `luminance` pour le dessus : le cadrage centre le
    // cube, donc le tiers supérieur de l'image est du ciel.
    for y in cote / 4..cote * 5 / 12 {
        for x in 0..cote {
            let i = ((y * cote + x) * 4) as usize;
            let p = [pixels[i], pixels[i + 1], pixels[i + 2]];
            if p == fond {
                continue;
            }
            for k in 0..3 {
                s[k] += p[k] as u64;
            }
            n += 1;
        }
    }
    assert!(n > 0, "rien de dessiné");
    [(s[0] / n) as u32, (s[1] / n) as u32, (s[2] / n) as u32]
}

fn rendre_un_cube_teinte(app: &Appareil, teinte: [f32; 3]) -> Vec<u8> {
    let t = table();
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |x, y, z| {
            if x == 8 && y == 8 && z == 8 {
                CUBE
            } else {
                AIR
            }
        }),
    );
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, teinte));
    let cible = Cible::nouvelle(app, 96, 96);
    let scene = Scene::nouvelle(app, &arene, &atlas_gris_147(app));
    let (min, max) = arene.bornes().unwrap();
    let cam = Camera::cadrer(min, max, 1.0);
    scene.rendre(&cible, &cam).0
}

/// **Une texture grise teintée doit sortir VERTE.**
///
/// Sans la teinte, `grass_block_top.png` (147, 147, 147) s'affiche tel quel et
/// le sol de tout terrain sort blanchâtre : la texture est là, simplement pas
/// de la bonne couleur — ce qui se lit « les blocs ont la mauvaise couleur »
/// et ne désigne pas la cause.
///
/// La cible est la couleur du JEU : (84, 109, 51), soit `texel × teinte`. Deux
/// façons de la rater, et le test les attrape toutes les deux :
///
/// - compenser le gris de la tuile écrête le canal vert et rend un OLIVE,
///   (145, 147, 89) — le vert perd son avance sur le rouge ;
/// - passer une teinte sRGB dans un mélange LINÉAIRE rend un délavé,
///   (113, 128, 90).
///
/// Les deux ont du vert en tête ; seule la comparaison au rouge les sépare.
#[test]
fn une_tuile_grise_teintee_sort_verte_et_pas_olive() {
    let Some(app) = app() else { return };
    let herbe = tf_assets::apparence::teinte_finale([0x91, 0xBD, 0x59]);
    let c = dessus(&rendre_un_cube_teinte(&app, herbe), 96);

    assert!(
        c[1] > c[0] && c[1] > c[2],
        "le vert doit dominer, il sort {c:?}"
    );
    // Le rapport vert/rouge sépare les trois issues : 1,30 pour le jeu, 1,01
    // pour l'olive de la compensation, 1,13 pour le délavé sRGB.
    let vr = c[1] as f32 / c[0] as f32;
    assert!(
        vr > 1.22,
        "vert/rouge = {vr:.2} : trop proche du gris — la teinte est appliquée \
         dans le mauvais espace, ou compensée puis écrêtée ({c:?})"
    );
    // Et la couleur elle-même, à la tolérance d'un rastériseur logiciel près.
    for (k, attendu) in [84u32, 109, 51].iter().enumerate() {
        let ecart = c[k].abs_diff(*attendu);
        assert!(
            ecart <= 10,
            "canal {k} : {} au lieu de {attendu} ({c:?})",
            c[k]
        );
    }
}

/// Une face NON teintée ne doit rien perdre. Appliquer la teinte partout
/// assombrirait tout le build — c'est le pendant exact du piège précédent.
#[test]
fn une_face_non_teintee_garde_sa_couleur() {
    let Some(app) = app() else { return };
    let c = dessus(&rendre_un_cube_teinte(&app, [1.0; 3]), 96);
    for k in 0..3 {
        assert!(
            c[k].abs_diff(147) <= 6,
            "teinte neutre : {c:?} devrait rester (147, 147, 147)"
        );
    }
}

// ── la passe de MODÈLES ─────────────────────────────────────────────────────

use tf_render::{faces_de, AreneModeles, FaceModele, HabillageFaces, Origine, Pose};

/// **Un `vec3<f32>` s'aligne sur SEIZE octets en WGSL, pas sur quatre.**
///
/// Une structure Rust en `[f32; 3]` mise en face décale tout ce qui suit d'un
/// champ sur deux, et le shader lit des bornes prises au hasard dans la table
/// voisine. Mesuré : des traînées qui filent à l'infini depuis le build, sans
/// la moindre erreur de validation — les deux côtés sont valides séparément,
/// c'est leur RACCORD qui est faux.
///
/// Le test n'ouvre pas le WGSL ; il fige la règle qui rend le raccord
/// possible : tout ce qui traverse la frontière est en `vec4`, donc aligné et
/// dimensionné sur seize.
#[test]
fn les_structures_du_shader_sont_alignees_sur_seize() {
    for (nom, taille) in [
        ("FaceModele", std::mem::size_of::<FaceModele>()),
        ("Origine", std::mem::size_of::<Origine>()),
    ] {
        assert_eq!(
            taille % 16,
            0,
            "{nom} fait {taille} octets : un multiple de seize, ou le shader \
             lira la structure suivante"
        );
    }
    // Les bornes et les uv doivent tomber sur des frontières de seize.
    assert_eq!(std::mem::offset_of!(FaceModele, min), 0);
    assert_eq!(std::mem::offset_of!(FaceModele, max), 16);
    assert_eq!(std::mem::offset_of!(FaceModele, uv), 32);
    assert_eq!(std::mem::offset_of!(FaceModele, face), 48);
    // Une pose n'a que des u32 : rien à aligner, mais sa taille doit rester
    // celle que le shader suppose.
    assert_eq!(std::mem::size_of::<Pose>(), 16);
}

fn blanc(n: usize) -> Vec<HabillageFaces> {
    vec![std::array::from_fn(|_| (0u32, [1.0f32; 3], [0.0, 0.0, 16.0, 16.0])); n]
}

/// Une scène d'UN bloc à la position (8, 8, 8), et la caméra figée sur lui.
///
/// Figée, parce que `Camera::cadrer` cadre sur ce qu'on lui donne : laisser la
/// dalle cadrer sur elle-même la ferait remplir l'image autant que le cube, et
/// le test ne comparerait plus rien.
fn rendre_un_bloc(app: &Appareil, id: StateId) -> (Vec<u8>, u32, tf_render::Compte) {
    let t = table();
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(
            0,
            |x, y, z| {
                if x == 8 && y == 8 && z == 8 {
                    id
                } else {
                    AIR
                }
            },
        ),
    );
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    let modeles = AreneModeles::sans_biome(&chantier, &|s| {
        let c = t.cuboides(s);
        faces_de(c, &blanc(c.len()))
    });

    let cote = 128;
    let cible = Cible::nouvelle(app, cote, cote);
    let scene = Scene::avec_modeles(app, &arene, &modeles, &atlas_blanc(app));
    // La boîte du BLOC, pas celle du contenu : les deux scènes se comparent.
    let cam = Camera::cadrer([8.0, 8.0, 8.0], [9.0, 9.0, 9.0], 1.0);
    let (pixels, compte) = scene.rendre(&cible, &cam);
    (pixels, cote, compte)
}

/// La boîte des pixels dessinés : `(x0, y0, x1, y1)`, bornes exclues à droite.
fn boite_dessinee(pixels: &[u8], cote: u32) -> Option<(u32, u32, u32, u32)> {
    let fond = [pixels[0], pixels[1], pixels[2]];
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..cote {
        for x in 0..cote {
            let i = ((y * cote + x) * 4) as usize;
            if [pixels[i], pixels[i + 1], pixels[i + 2]] == fond {
                continue;
            }
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

fn dessines(pixels: &[u8], cote: u32) -> u32 {
    let fond = [pixels[0], pixels[1], pixels[2]];
    let mut n = 0;
    for y in 0..cote {
        for x in 0..cote {
            let i = ((y * cote + x) * 4) as usize;
            if [pixels[i], pixels[i + 1], pixels[i + 2]] != fond {
                n += 1;
            }
        }
    }
    n
}

/// **Un bloc-modèle se DESSINE.**
///
/// Sur la cible Minefield, deux tiers du catalogue ne sont pas des cubes. La
/// passe gloutonne ne les voit pas ; jusqu'ici rien d'autre ne les dessinait,
/// et sur la première capture d'une vraie save 3 957 blocs manquaient à
/// l'appel sans que l'image ne le dise. Une absence ne se voit pas — d'où ce
/// test plutôt qu'un coup d'œil.
#[test]
fn une_dalle_se_dessine() {
    let Some(app) = app() else { return };
    let (pixels, cote, compte) = rendre_un_bloc(&app, 2);
    assert_eq!(compte.appels_de_dessin, 2, "gloutons + modèles");
    assert!(
        dessines(&pixels, cote) > 100,
        "une dalle doit couvrir des pixels, elle n'en couvre aucun"
    );
}

/// Et elle occupe la MOITIÉ BASSE de sa case.
///
/// Dessiner un cube plein à sa place donnerait une image plausible et fausse :
/// c'est la hauteur qui prouve que la géométrie du modèle est lue, et pas
/// seulement qu'« il y a quelque chose ».
#[test]
fn une_dalle_n_occupe_que_le_bas_de_sa_case() {
    let Some(app) = app() else { return };
    let (cube, cote, _) = rendre_un_bloc(&app, CUBE);
    let (dalle, _, _) = rendre_un_bloc(&app, 2);
    let (_, hc0, _, hc1) = boite_dessinee(&cube, cote).expect("le cube se dessine");
    let (_, hd0, _, hd1) = boite_dessinee(&dalle, cote).expect("la dalle se dessine");

    // Le bas des deux coïncide : la dalle est POSÉE au sol de sa case.
    assert!(
        hd1.abs_diff(hc1) <= 2,
        "la dalle devrait reposer au même niveau que le cube ({hd1} contre {hc1})"
    );
    // Son haut est nettement plus bas. Pas « la moitié » au pixel près : en
    // vue de trois quarts, une hauteur de bloc n'est pas une hauteur d'écran.
    assert!(
        hd0 > hc0 + (hc1 - hc0) / 5,
        "la dalle monte aussi haut que le cube : la géométrie du modèle n'est \
         pas lue ({hd0} contre {hc0}, boîte {hc0}..{hc1})"
    );
    assert!(
        dessines(&dalle, cote) < dessines(&cube, cote),
        "une dalle couvre moins de pixels qu'un cube"
    );
}

/// Une scène SANS bloc-modèle ne paie pas la passe.
///
/// Un pipeline qu'on branche pour dessiner zéro instance reste un changement
/// de pipeline, et le projet compte ses appels de dessin.
#[test]
fn une_scene_sans_modele_garde_un_seul_appel() {
    let Some(app) = app() else { return };
    let (_, _, compte) = rendre_un_bloc(&app, CUBE);
    assert_eq!(compte.appels_de_dessin, 1);
}

/// La géométrie d'un état vit UNE fois, quel que soit le nombre de blocs.
///
/// C'est toute la raison d'être de la pose : 349 000 blocs-modèles produisaient
/// 5,8 millions de quads, neuf dixièmes du maillage, alors que ces quads sont
/// la même géométrie répétée.
#[test]
fn mille_dalles_ne_stockent_qu_un_seul_modele() {
    let t = table();
    let mut g = Grille::new();
    // Une couche pleine de dalles : 256 blocs, un seul état.
    g.poser(0, 0, section(0, |_, y, _| if y == 8 { 2 } else { AIR }));
    let chantier = g.mailler(&t);
    let a = AreneModeles::sans_biome(&chantier, &|s| {
        let c = t.cuboides(s);
        faces_de(c, &blanc(c.len()))
    });
    assert_eq!(a.poses.len(), 256, "une pose par bloc");
    assert_eq!(
        a.faces.len(),
        6,
        "mais une seule géométrie : les six faces du cuboïde de la dalle"
    );
    assert_eq!(a.faces_a_dessiner, 256 * 6);
    // Et les rangs de départ sont STRICTEMENT croissants — c'est ce que la
    // dichotomie du shader suppose. Une pose sans face les casserait.
    for i in 1..a.poses.len() {
        assert!(
            a.poses[i].debut_face > a.poses[i - 1].debut_face,
            "les rangs doivent croître strictement, sinon la dichotomie rend \
             la mauvaise pose"
        );
    }
}

// ── l'empaquetage d'un quad glouton ─────────────────────────────────────────

use tf_render::{depaqueter, empaqueter};

/// **Un quad glouton tient dans 26 bits, et l'aller-retour est EXACT.**
///
/// Seize octets par quad au lieu de trente-deux : sur une région bâtie, l'arène
/// passe de 132 Mo à 66. Ce n'est pas un confort — la fenêtre de résidence est
/// plafonnée en octets (invariant n° 7), donc c'est autant de monde en plus.
///
/// L'exactitude n'est pas négociable : une position tronquée d'un bloc ne
/// planterait rien, elle déplacerait un mur.
#[test]
fn l_aller_retour_d_un_quad_est_exact_sur_tout_le_domaine() {
    let mut vus = 0;
    for x in 0..=16u32 {
        for y in [0u32, 1, 7, 15, 16] {
            for z in [0u32, 1, 8, 16] {
                for l in 1..=16u32 {
                    for h in [1u32, 2, 9, 16] {
                        for face in 0..6u32 {
                            let geo = empaqueter(
                                [x as f32 * 16.0, y as f32 * 16.0, z as f32 * 16.0],
                                [l as f32 * 16.0, h as f32 * 16.0],
                                face,
                            );
                            assert_eq!(
                                depaqueter(geo),
                                ([x, y, z], [l, h], face),
                                "x{x} y{y} z{z} {l}×{h} face{face}"
                            );
                            vus += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(vus > 20_000, "le domaine doit être balayé, pas effleuré");
}

/// Les 26 bits utilisés ne débordent pas les uns sur les autres.
///
/// Un décalage d'un bit ferait passer une taille dans la position : le quad
/// serait au bon endroit à un bloc près, ce qui est exactement le genre de
/// défaut qu'on ne voit pas sur une capture.
#[test]
fn les_champs_empaquetes_ne_se_marchent_pas_dessus() {
    // Tout au maximum : x = y = z = 16, taille 16 × 16, face 5.
    let geo = empaqueter([256.0, 256.0, 256.0], [256.0, 256.0], 5);
    assert_eq!(depaqueter(geo), ([16, 16, 16], [16, 16], 5));
    assert_eq!(geo >> 26, 0, "rien au-delà du 26e bit");
    // Et un seul champ à la fois.
    assert_eq!(
        depaqueter(empaqueter([256.0, 0.0, 0.0], [16.0, 16.0], 0)).0,
        [16, 0, 0]
    );
    assert_eq!(
        depaqueter(empaqueter([0.0, 0.0, 0.0], [256.0, 16.0], 0)).1,
        [16, 1]
    );
    assert_eq!(
        depaqueter(empaqueter([0.0, 0.0, 0.0], [16.0, 16.0], 5)).2,
        5
    );
}

#[test]
fn une_instance_de_quad_fait_seize_octets() {
    assert_eq!(std::mem::size_of::<tf_render::InstanceQuad>(), 16);
}

/// **Les deux arènes indexent la MÊME table d'origines.**
///
/// Deux tables se décaleraient le jour où l'une saute une section vide, et tout
/// un pan du build se dessinerait ailleurs — sans la moindre erreur. L'index
/// d'une section EST son rang de lot, et ce test le fige : la passe de modèles
/// saute les sections sans pose, donc rien ne garantit l'accord sauf cette
/// règle.
#[test]
fn les_deux_arenes_designent_la_meme_section() {
    let t = table();
    let mut g = Grille::new();
    // Une section de cubes SEULS — donc aucune pose, donc un lot que la passe
    // de modèles n'aurait pas compté si elle numérotait de son côté.
    g.poser(
        0,
        0,
        section(0, |x, y, z| if x + y + z == 0 { CUBE } else { AIR }),
    );
    // Puis une section de dalles, loin de là.
    g.poser(
        4,
        7,
        section(3, |x, y, z| if x + y + z == 0 { 2 } else { AIR }),
    );
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    let modeles = AreneModeles::sans_biome(&chantier, &|s| {
        let c = t.cuboides(s);
        faces_de(c, &blanc(c.len()))
    });

    assert!(!arene.instances.is_empty() && !modeles.poses.is_empty());
    let origines = tf_render::origines(&chantier);
    assert_eq!(arene.origines, origines, "l'arène porte la table partagée");

    // La dalle est en chunk (4, 7), section y = 3 : son origine doit le dire.
    let p = modeles.poses[0];
    let o = origines[p.section as usize].position;
    assert_eq!(
        [o[0] / 16.0, o[2] / 16.0],
        [4.0 * 16.0, 7.0 * 16.0],
        "la pose désigne l'origine de SON chunk, pas celle du lot précédent"
    );
    // Et le cube est bien dans l'autre.
    let q = arene.instances[0];
    let o = origines[q.section as usize].position;
    assert_eq!([o[0], o[2]], [0.0, 0.0]);
}

/// **Le biome doit arriver jusqu'à la TEINTE de l'instance GPU.**
///
/// Le maillage le porte, mais entre le quad et l'octet envoyé à la carte il y
/// a une fonction d'habillage. Si elle ignore son troisième argument, tout
/// compile, tous les tests de maillage passent, et le sol reste uniformément
/// « plaines ». C'est la forme « déclaré, branché, testé — et inatteignable »,
/// et la seule parade est d'exiger la couleur, pas la plomberie.
#[test]
fn deux_biomes_donnent_deux_teintes_dans_l_arene() {
    use tf_anvil::{Section, StateId};
    use tf_mesh::forme::{Cuboide, TableFormes};
    use tf_mesh::Grille;

    const HERBE: StateId = 1;
    const PLAINES: StateId = 10;
    const DESERT: StateId = 11;

    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    let h = t.pousser(false, true, vec![Cuboide::PLEIN]);
    t.marquer_teinte(h);

    let mut s = Section::uniform(0, 0);
    let mut idx = vec![0u16; 4096];
    s.palette = vec![0, HERBE];
    for (i, c) in idx.iter_mut().enumerate() {
        if i < 8 * 256 {
            *c = 1;
        }
    }
    s.repack(&idx);

    // Moitié plaines, moitié désert, coupé en X.
    let mut cells = vec![PLAINES; 64];
    for y in 0..4 {
        for z in 0..4 {
            for x in 2..4 {
                cells[(y << 4) | (z << 2) | x] = DESERT;
            }
        }
    }
    let mut g = Grille::new();
    g.poser(0, 0, s);
    assert!(g.poser_biomes(0, 0, 0, cells));
    let chantier = g.mailler(&t);

    // On ENREGISTRE ce que l'habillage reçoit : c'est la seule façon de
    // distinguer « la teinte est juste » de « la teinte est constante ».
    let vus = std::cell::RefCell::new(std::collections::BTreeMap::<StateId, usize>::new());
    let arene = Arene::depuis(&chantier, &|_, _, biome| {
        *vus.borrow_mut().entry(biome).or_default() += 1;
        let c = match biome {
            PLAINES => [0.1, 0.9, 0.2],
            DESERT => [0.9, 0.8, 0.1],
            _ => [1.0, 0.0, 1.0],
        };
        (0, c)
    });
    let vus = vus.into_inner();
    assert_eq!(
        vus.keys().copied().collect::<Vec<_>>(),
        vec![PLAINES, DESERT],
        "l'habillage doit voir les DEUX biomes, et aucun autre"
    );

    let teintes: std::collections::BTreeSet<u32> =
        arene.instances.iter().map(|i| i.teinte).collect();
    assert_eq!(
        teintes.len(),
        2,
        "et deux teintes distinctes doivent en sortir, pas une"
    );
}

/// **Un bloc-MODÈLE aussi prend la couleur de son biome.**
///
/// Les feuilles et les vignes ne sont pas des cubes : elles ne passent pas par
/// la gloutonne, donc la teinte branchée sur le quad ne les atteignait pas —
/// un chêne sortait GRIS au milieu d'un terrain vert, parce que
/// `oak_leaves.png` vaut (97, 97, 97) et que c'est le jeu qui le colore.
///
/// La table de géométrie est mémoïsée : la teinte y voyage donc en mémoïsant
/// sur `(état, biome)` au lieu de l'état seul. Ce test exige les deux copies
/// ET leurs deux couleurs — sans la seconde exigence, une implémentation qui
/// duplique la table sans changer la teinte passerait.
#[test]
fn un_bloc_modele_prend_la_couleur_de_son_biome() {
    use tf_render::{faces_de, AreneModeles, FaceModele, HabillageFaces};

    const FEUILLE: StateId = 1;
    const PLAINES: StateId = 10;
    const DESERT: StateId = 11;

    // Un bloc-modèle : un cuboïde qui ne remplit PAS la case, donc non opaque.
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    let f = t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    );
    assert_eq!(f, FEUILLE);
    t.marquer_teinte(f);

    // Une couche de feuilles en bas, coupée en deux biomes sur X.
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |_, y, _| if y == 0 { FEUILLE } else { AIR }),
    );
    let mut cells = vec![PLAINES; 64];
    for y in 0..4 {
        for z in 0..4 {
            for x in 2..4 {
                cells[(y << 4) | (z << 2) | x] = DESERT;
            }
        }
    }
    assert!(g.poser_biomes(0, 0, 0, cells));
    let chantier = g.mailler(&t);

    let vus = std::cell::RefCell::new(std::collections::BTreeSet::<StateId>::new());
    let arene = AreneModeles::depuis(&chantier, &|id, biome| {
        vus.borrow_mut().insert(biome);
        let teinte = match biome {
            PLAINES => [0.1, 0.9, 0.2],
            DESERT => [0.9, 0.8, 0.1],
            _ => [1.0, 0.0, 1.0],
        };
        let c = t.cuboides(id);
        let hab: Vec<HabillageFaces> = (0..c.len())
            .map(|_| std::array::from_fn(|_| (0u32, teinte, [0.0, 0.0, 16.0, 16.0])))
            .collect();
        faces_de(c, &hab)
    });

    assert_eq!(
        vus.into_inner().into_iter().collect::<Vec<_>>(),
        vec![PLAINES, DESERT],
        "la table doit être demandée pour les DEUX biomes, et aucun autre"
    );
    let teintes: std::collections::BTreeSet<u32> =
        arene.faces.iter().map(|f: &FaceModele| f.teinte).collect();
    assert_eq!(
        teintes.len(),
        2,
        "deux couleurs distinctes doivent atteindre le GPU, pas une"
    );

    // Et les poses ne pointent pas toutes sur la même table.
    let debuts: std::collections::BTreeSet<u32> =
        arene.poses.iter().map(|p| p.debut_modele).collect();
    assert_eq!(debuts.len(), 2, "une table par biome, désignée par la pose");
}

/// **Ce qui ne se teinte pas ne se duplique pas** — et c'est ce qui rend la
/// solution tenable.
///
/// Mémoïser sur `(état, biome)` ferait autant de copies de la géométrie d'un
/// escalier qu'il y a de biomes dans la scène si le mailleur écrivait le biome
/// partout. Il écrit ZÉRO pour un état non teinté, exactement comme la clé de
/// fusion gloutonne — donc la quasi-totalité du catalogue garde UNE table,
/// comme avant que les biomes existent. C'est la règle « aucune évolution
/// future ne doit dégrader le cœur », et rien d'autre ne la vérifie.
#[test]
fn un_etat_non_teinte_ne_paie_pas_les_biomes() {
    use tf_render::{faces_de, AreneModeles, HabillageFaces};

    const DALLE: StateId = 1;

    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    let d = t.pousser(
        false,
        false,
        vec![Cuboide {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 8.0, 16.0],
            faces: 0x3F,
            cull: 0x3F,
        }],
    );
    assert_eq!(d, DALLE);
    // PAS de `marquer_teinte` : c'est tout le propos.

    let mut g = Grille::new();
    g.poser(0, 0, section(0, |_, y, _| if y == 0 { DALLE } else { AIR }));
    let mut cells = vec![10 as StateId; 64];
    for (i, c) in cells.iter_mut().enumerate() {
        *c = 10 + (i as StateId % 7); // sept biomes, bien visibles
    }
    assert!(g.poser_biomes(0, 0, 0, cells));
    let chantier = g.mailler(&t);

    let appels = std::cell::Cell::new(0usize);
    let arene = AreneModeles::depuis(&chantier, &|id, biome| {
        appels.set(appels.get() + 1);
        assert_eq!(biome, 0, "un état non teinté doit recevoir le biome ZÉRO");
        let c = t.cuboides(id);
        let hab: Vec<HabillageFaces> = (0..c.len())
            .map(|_| std::array::from_fn(|_| (0u32, [1.0; 3], [0.0, 0.0, 16.0, 16.0])))
            .collect();
        faces_de(c, &hab)
    });

    assert!(!arene.poses.is_empty(), "le test ne prouve rien sans pose");
    assert_eq!(appels.get(), 1, "UNE seule table, malgré les sept biomes");
    let debuts: std::collections::BTreeSet<u32> =
        arene.poses.iter().map(|p| p.debut_modele).collect();
    assert_eq!(debuts.len(), 1, "et toutes les poses la partagent");
}

// ── le quadrillage ──────────────────────────────────────────────────────────

/// Combien de pixels portent cette couleur, à la tolérance près.
fn pixels_de(image: &[u8], couleur: [u8; 3], tol: i32) -> u32 {
    image
        .chunks_exact(4)
        .filter(|p| (0..3).all(|k| (p[k] as i32 - couleur[k] as i32).abs() <= tol))
        .count() as u32
}

/// Une scène d'un seul cube (la section 0..16 BLOCS), avec un quadrillage.
///
/// Le cadrage se fait en unités de RENDU — des seizièmes — parce que c'est
/// ce que `Camera::cadrer` attend, comme tout le reste du rendu.
fn rendre_avec_lignes(app: &Appareil, lignes: &tf_render::Lignes) -> (Vec<u8>, u32) {
    rendre_lignes_et_decor(app, lignes, true)
}

/// La même, en choisissant s'il y a un décor pour occulter.
fn rendre_lignes_et_decor(
    app: &Appareil,
    lignes: &tf_render::Lignes,
    decor: bool,
) -> (Vec<u8>, u32) {
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, section(0, |_, _, _| if decor { CUBE } else { AIR }));
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    let atlas = atlas_blanc(app);
    let mut scene = Scene::nouvelle(app, &arene, &atlas);
    scene.poser_lignes(lignes);

    let cote = 128;
    let cible = Cible::nouvelle(app, cote, cote);
    // En BLOCS : c'est l'unité de la caméra.
    let camera = Camera::cadrer([-2.0; 3], [18.0, 18.0, 18.0], 1.0);
    let (pixels, _) = scene.rendre(&cible, &camera);
    (pixels, cote)
}

/// La boîte, en pixels, des pixels qui satisfont un prédicat.
fn emprise(
    image: &[u8],
    cote: u32,
    garde: impl Fn([u8; 3]) -> bool,
) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (i, p) in image.chunks_exact(4).enumerate() {
        if garde([p[0], p[1], p[2]]) {
            let (x, y) = (i as u32 % cote, i as u32 / cote);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

/// **Le quadrillage et la géométrie sont dans la MÊME unité — vérifié en
/// TRAVERSANT.**
///
/// Tout l'espace de rendu est en seizièmes de bloc ; le découpage, lui, vient
/// du domaine, où un chunk fait 16 BLOCS. Sans conversion, la grille d'une
/// section se dessinait à un seizième de sa taille, tapie dans le coin du
/// premier bloc — et l'image restait parfaitement plausible. Trouvé par une
/// mutation qui visait autre chose : le test de profondeur que je cassais
/// ne changeait rien, parce que le contour n'était pas là où je croyais.
///
/// Chaque moitié était juste. C'est leur JONCTION qui ne l'était pas, et
/// c'est le piège de `toHeights` / `applyHeightmap` sous une autre forme :
/// une unité qui traverse une frontière se vérifie EN TRAVERSANT.
#[test]
fn le_quadrillage_encadre_exactement_la_geometrie() {
    let Some(app) = app() else { return };
    const ROUGE: [u8; 3] = [255, 0, 0];
    let rouge = |p: [u8; 3]| (0..3).all(|k| (p[k] as i32 - ROUGE[k] as i32).abs() <= 12);
    // Le cube seul : son emprise à l'écran.
    let (sans, cote) = rendre_avec_lignes(&app, &tf_render::Lignes::new());
    // **Le fond se MESURE, il ne se recopie pas.** La couleur d'effacement est
    // donnée en LINÉAIRE et la cible est en sRGB : les octets relus ne sont
    // pas ceux de la constante, ils sont son encodage. Un fond recopié à la
    // main rend un prédicat qui accepte tout, donc une emprise qui couvre
    // l'image — et un test qui échoue en accusant la mauvaise moitié.
    let fond = [sans[0], sans[1], sans[2]];
    let decor =
        move |p: [u8; 3]| !rouge(p) && (0..3).any(|k| (p[k] as i32 - fond[k] as i32).abs() > 8);
    let cube = emprise(&sans, cote, decor).expect("le cube doit se dessiner");

    // Le contour de la SECTION, en blocs : 0..16.
    let mut l = tf_render::Lignes::new();
    l.contour([0.0; 3], [16.0; 3], tf_render::rgba(255, 0, 0, 255));
    let (avec, _) = rendre_avec_lignes(&app, &l);
    let grille = emprise(&avec, cote, rouge).expect("le contour doit se dessiner");

    // Les deux emprises doivent coïncider à quelques pixels près. À un
    // seizième de l'échelle, la grille tiendrait dans un coin — l'écart se
    // compterait en dizaines de pixels sur une image de 128.
    for (a, b, quoi) in [
        (cube.0, grille.0, "gauche"),
        (cube.1, grille.1, "haut"),
        (cube.2, grille.2, "droite"),
        (cube.3, grille.3, "bas"),
    ] {
        assert!(
            (a as i32 - b as i32).abs() <= 3,
            "bord {quoi} : le cube est à {a}, la grille à {b} — \
             elles ne sont pas à la même échelle. cube {cube:?} grille {grille:?}"
        );
    }
}

/// **Le quadrillage se DESSINE.**
///
/// C'est le pendant de « une absence ne se voit pas » : les blocs-modèles ont
/// été maillés, comptés, affichés dans le rapport — et jamais dessinés, sur
/// une vraie save, sans que rien ne le dise. Une grille demandée et absente
/// donne une image parfaitement plausible.
#[test]
fn un_quadrillage_demande_apparait_a_l_ecran() {
    let Some(app) = app() else { return };
    const ROUGE: [u8; 3] = [255, 0, 0];

    let vide = tf_render::Lignes::new();
    let (sans, _) = rendre_avec_lignes(&app, &vide);
    assert_eq!(pixels_de(&sans, ROUGE, 12), 0, "rien de rouge sans grille");

    let mut l = tf_render::Lignes::new();
    l.contour([0.0; 3], [16.0; 3], tf_render::rgba(255, 0, 0, 255));
    assert_eq!(l.len(), 12, "une boîte a douze arêtes");
    let (avec, _) = rendre_avec_lignes(&app, &l);
    assert!(
        pixels_de(&avec, ROUGE, 12) > 50,
        "le contour doit se voir : {} pixels rouges",
        pixels_de(&avec, ROUGE, 12)
    );
}

/// **Le quadrillage est un CALQUE : il ne se cache pas derrière le décor.**
///
/// Un repère qui disparaît derrière le mur qu'on est en train d'aligner n'est
/// pas un repère. Le contour tracé ICI est à l'intérieur de la section pleine
/// de cubes : avec un test de profondeur, il serait entièrement masqué.
#[test]
fn le_quadrillage_passe_devant_le_decor() {
    let Some(app) = app() else { return };
    const VERT: [u8; 3] = [0, 255, 0];
    let mut l = tf_render::Lignes::new();
    // Au cœur de la section, donc enfoui sous des blocs opaques.
    l.contour(
        [6.0, 6.0, 6.0],
        [10.0, 10.0, 10.0],
        tf_render::rgba(0, 255, 0, 255),
    );

    // **Le témoin est le MÊME contour SANS rien devant.** Un seuil choisi à la
    // main ne prouve rien ici : mesuré, un test de profondeur actif laisse
    // quand même passer 49 pixels sur 111 — les arêtes que la silhouette du
    // cube ne couvre pas. Un « plus de vingt pixels » aurait donc été vert
    // dans les deux cas, et c'est ce qu'il était. La propriété juste est
    // l'ÉGALITÉ : le décor ne doit rien retirer du tout.
    let (libre, _) = rendre_lignes_et_decor(&app, &l, false);
    let (enfoui, _) = rendre_lignes_et_decor(&app, &l, true);
    let (a, b) = (pixels_de(&libre, VERT, 12), pixels_de(&enfoui, VERT, 12));
    assert!(
        a > 50,
        "le témoin doit dessiner un vrai contour : {a} pixels"
    );
    assert_eq!(
        b, a,
        "un contour enfoui doit rester ENTIER : {b} pixels sous le décor \
         contre {a} à l'air libre"
    );
}

/// Une scène sans quadrillage ne paie pas un appel de dessin de plus. Une
/// passe qui ne dessine rien reste un changement de pipeline.
#[test]
fn un_quadrillage_vide_ne_coute_pas_un_appel() {
    let Some(app) = app() else { return };
    let t = table();
    let mut g = Grille::new();
    g.poser(0, 0, section(0, |_, _, _| CUBE));
    let chantier = g.mailler(&t);
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3]));
    let atlas = atlas_blanc(&app);
    let mut scene = Scene::nouvelle(&app, &arene, &atlas);
    let cible = Cible::nouvelle(&app, 64, 64);
    let camera = Camera::cadrer([0.0; 3], [16.0; 3], 1.0);

    let (_, sans) = scene.rendre(&cible, &camera);
    assert_eq!(sans.appels_de_dessin, 1);

    scene.poser_lignes(&tf_render::Lignes::new());
    let (_, toujours) = scene.rendre(&cible, &camera);
    assert_eq!(toujours.appels_de_dessin, 1, "vide ne coûte rien");

    let mut l = tf_render::Lignes::new();
    l.contour([0.0; 3], [16.0; 3], tf_render::rgba(255, 255, 255, 255));
    scene.poser_lignes(&l);
    let (_, avec) = scene.rendre(&cible, &camera);
    assert_eq!(avec.appels_de_dessin, 2, "un appel pour le calque");
}

/// **Une DEMI-teinte ne se délave pas** — et c'est le seul test qui puisse le
/// dire.
///
/// Le test d'à côté n'essaie que des primaires SATURÉES, et 0 comme 255 sont
/// des points FIXES de la conversion sRGB ↔ linéaire : il passait des deux
/// côtés pendant que le quadrillage sortait délavé. Mesuré, (120, 220, 140)
/// s'affichait en (184, 240, 196) — un vert qui se lit BLANC. C'est le piège
/// des couleurs de sommet d'`ExeWorldEdit`, mot pour mot, dans un autre
/// moteur.
///
/// La tolérance de 6 est ce que coûte l'aller-retour sur huit bits ; l'écart
/// qu'on cherche est de SOIXANTE-QUATRE. Le test discrimine d'un facteur dix,
/// il ne tient pas à un cheveu.
#[test]
fn une_demi_teinte_ne_se_delave_pas() {
    let Some(app) = app() else { return };
    for (r, v, b) in [(120u8, 220u8, 140u8), (128, 128, 128), (90, 170, 255)] {
        let mut l = tf_render::Lignes::new();
        l.contour([0.0; 3], [16.0; 3], tf_render::rgba(r, v, b, 255));
        let (image, _) = rendre_avec_lignes(&app, &l);
        let n = pixels_de(&image, [r, v, b], 6);
        assert!(
            n > 50,
            "couleur ({r}, {v}, {b}) : {n} pixels seulement — la demi-teinte \
             a été déplacée par la conversion"
        );
    }
}

/// **Les octets de la couleur traversent dans le bon sens.**
///
/// Deux conventions inverses entre `rgba()` et le shader donneraient un
/// quadrillage bleu là où on a demandé du rouge, sans la moindre erreur — et
/// « le bleu et le rouge sont inversés » est le défaut qu'on attribue au
/// thème avant de l'attribuer au code. Il ne dit rien de l'ESPACE de couleur,
/// pour la raison ci-dessus : c'est le test d'à côté qui le tient.
#[test]
fn la_couleur_demandee_est_la_couleur_dessinee() {
    let Some(app) = app() else { return };
    for (r, v, b) in [(255u8, 0u8, 0u8), (0, 255, 0), (0, 0, 255)] {
        let mut l = tf_render::Lignes::new();
        l.contour([0.0; 3], [16.0; 3], tf_render::rgba(r, v, b, 255));
        let (image, _) = rendre_avec_lignes(&app, &l);
        let n = pixels_de(&image, [r, v, b], 12);
        assert!(n > 50, "couleur ({r}, {v}, {b}) : {n} pixels seulement");
    }
}
