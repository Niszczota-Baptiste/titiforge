//! Le rendu, vérifié AU PIXEL.
//!
//! C'est tout l'intérêt d'une cible hors écran : une image se compare, un
//! coup d'œil non. Et le défaut que ces tests visent a vécu des années dans
//! `we-engine` sans que personne ne le voie — parce qu'il était invisible sur
//! un build gris.

use tf_mesh::forme::Cuboide;
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
    let arene = Arene::depuis(&chantier, &|_| 0);
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
    let arene = Arene::depuis(&chantier, &|_| 0);

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
    let arene = Arene::depuis(&chantier, &|_| 0);

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
    let arene = Arene::depuis(&chantier, &|_| 0);
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
