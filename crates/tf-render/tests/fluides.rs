//! Les fluides, vérifiés AU PIXEL.
//!
//! Ce que la passe de fluides du mailleur décide — hauteurs de coins, faces,
//! textures — est croisé avec le jeu dans `tf-mesh`. Ici on vérifie ce que le
//! shader en FAIT : que chaque hauteur monte au bon coin, que l'eau laisse
//! voir ce qu'elle recouvre et que la lave non, que l'eau n'écrit pas la
//! profondeur, et qu'une scène suivie montre ce qu'une scène neuve montre.
//!
//! Les coins se vérifient en PROJETANT des points du monde avec la matrice de
//! la caméra, au processeur : un test qui raisonnerait « l'ouest est à gauche
//! de l'écran » se tromperait de côté à la première caméra retournée.

use tf_anvil::{bits_for, pack, Packing, Section, StateId};
use tf_mesh::{
    Chantier, Face, FaceFluide, Fluide, GenreFluide, Grille, Lot, TableFormes, TextureFluide,
};
use tf_render::scene::FORMAT;
use tf_render::{
    empaqueter_fluide, Appareil, Arene, AreneFluides, AreneModeles, AtlasGpu, Camera, Cible,
    InstanceFluide, Scene,
};

fn app() -> Option<Appareil> {
    match Appareil::ouvrir() {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("pas d'adaptateur graphique ici ({e}) : test sauté");
            None
        }
    }
}

const COTE: u32 = 160;

/// Un atlas de quelques couches unies : `couleurs[i]` en RGBA.
fn atlas(app: &Appareil, couleurs: &[[u8; 4]]) -> AtlasGpu {
    let mut px = Vec::new();
    for c in couleurs {
        px.extend(c.repeat(4 * 4));
    }
    AtlasGpu::nouveau(app, 4, couleurs.len() as u32, &px)
}

/// Où un point du monde tombe dans l'image — la matrice même du shader.
fn projeter(cam: &Camera, p: [f32; 3]) -> (u32, u32) {
    let m = cam.gpu(1.0).vue_projection;
    let v = [p[0], p[1], p[2], 1.0];
    let clip: [f32; 4] = std::array::from_fn(|r| (0..4).map(|c| m[c][r] * v[c]).sum());
    let (x, y) = (clip[0] / clip[3], clip[1] / clip[3]);
    (
        ((x + 1.0) * 0.5 * COTE as f32) as u32,
        ((1.0 - y) * 0.5 * COTE as f32) as u32,
    )
}

fn pixel(image: &[u8], (x, y): (u32, u32)) -> [u8; 3] {
    let i = ((y * COTE + x) * 4) as usize;
    [image[i], image[i + 1], image[i + 2]]
}

/// Le fond de la cible, relu sur une scène vide : la couleur d'effacement est
/// donnée en linéaire, la cible est en sRGB — elle se MESURE.
fn fond(app: &Appareil, cam: &Camera) -> [u8; 3] {
    let vide = Chantier::default();
    let a = Arene::depuis(&vide, &|_, _, _| (0, [1.0; 3], tf_render::Sens::DROIT));
    let s = Scene::nouvelle(app, &a, &atlas(app, &[[255; 4]]));
    let (img, _) = s.rendre(&Cible::nouvelle(app, COTE, COTE), cam);
    pixel(&img, (0, 0))
}

/// Une scène faite de ces seules faces de fluide, dans la section (0, 0, 0).
fn scene_de(app: &Appareil, faces: Vec<FaceFluide>, atlas: &AtlasGpu) -> Scene {
    let mut lot = Lot::vide((0, 0, 0));
    lot.fluides = faces;
    let chantier = Chantier {
        lots: vec![lot],
        sautees: 0,
    };
    let arene = Arene::depuis(&chantier, &|_, _, _| (0, [1.0; 3], tf_render::Sens::DROIT));
    let fluides = AreneFluides::depuis(&chantier, arene.emplacements(), &|g, _, _| {
        // La lave sur la couche 0, l'eau sur la couche 1.
        (u32::from(g == GenreFluide::Eau), [1.0; 3])
    });
    Scene::pour(
        app,
        &arene,
        &AreneModeles::default(),
        &fluides,
        atlas,
        FORMAT,
    )
}

fn face(face: Face, genre: GenreFluide, hauteurs: [u8; 4]) -> FaceFluide {
    FaceFluide {
        pos: [8, 8, 8],
        taille: [1, 1],
        face,
        genre,
        texture: TextureFluide::Immobile,
        hauteurs,
        angle: 0,
        biome: 0,
    }
}

fn camera(oeil: [f32; 3], cible: [f32; 3]) -> Camera {
    Camera {
        oeil,
        cible,
        fov: 0.9,
        proche: 0.05,
        loin: 100.0,
    }
}

#[test]
fn une_instance_de_fluide_fait_vingt_octets() {
    assert_eq!(std::mem::size_of::<InstanceFluide>(), 20);
    assert_eq!(tf_mesh::OCTETS_FACE_FLUIDE, 20, "ce que le lot compte");
}

#[test]
fn les_champs_empaquetes_ne_se_marchent_pas_dessus() {
    let f = FaceFluide {
        pos: [15, 1, 7],
        taille: [16, 3],
        face: Face::PlusZ,
        genre: GenreFluide::Lave,
        texture: TextureFluide::Voile,
        hauteurs: [1, 2, 3, 255],
        angle: 0xBEEF,
        biome: 9,
    };
    let i = empaqueter_fluide(&f, 2047, [1.0, 0.5, 0.0], 77);
    assert_eq!(i.geo & 31, 15);
    assert_eq!((i.geo >> 5) & 31, 1);
    assert_eq!((i.geo >> 10) & 31, 7);
    assert_eq!((i.geo >> 15) & 15, 15, "taille 16 − 1");
    assert_eq!((i.geo >> 19) & 15, 2);
    assert_eq!(i.face(), Face::PlusZ as u32);
    assert!(i.opaque(), "la lave");
    assert_eq!((i.geo >> 27) & 3, TextureFluide::Voile as u32);
    assert_eq!(i.hauteurs.to_le_bytes(), [1, 2, 3, 255]);
    assert_eq!(i.couche_angle & 0xFFFF, 2047);
    assert_eq!(i.couche_angle >> 16, 0xBEEF);
    assert_eq!(i.section, 77);
    assert!(
        !empaqueter_fluide(&face(Face::PlusY, GenreFluide::Eau, [0; 4]), 0, [1.0; 3], 0).opaque()
    );
}

/// Un côté monte du côté de sa PREMIÈRE hauteur — le bout de plus petite
/// coordonnée le long de l'axe horizontal —, pour les deux familles de
/// côtés : ±Z, où cet axe est X, et ±X, où c'est Z et où il est le SECOND
/// axe du plan.
#[test]
fn un_cote_monte_du_cote_de_sa_premiere_hauteur() {
    let Some(app) = app() else { return };
    let a = atlas(&app, &[[230, 110, 30, 255], [150, 150, 150, 180]]);
    // (face, œil, point du monde près du bout bas, point près du bout haut),
    // les deux points à 0,75 de hauteur dans la case.
    type Point = [f32; 3];
    let cas: [(Face, Point, Point, Point); 2] = [
        (
            Face::MoinsZ,
            [8.5, 8.5, 5.0],
            [8.15, 8.75, 8.0],
            [8.85, 8.75, 8.0],
        ),
        (
            Face::PlusX,
            [12.0, 8.5, 8.5],
            [9.0, 8.75, 8.15],
            [9.0, 8.75, 8.85],
        ),
    ];
    for (f, oeil, pres_du_bas, pres_du_haut) in cas {
        let cam = camera(oeil, [8.5, 8.5, 8.5]);
        let fond = fond(&app, &cam);
        for (hauteurs, monte_au_debut) in [([255, 0, 0, 0], true), ([0, 255, 0, 0], false)] {
            let s = scene_de(&app, vec![face(f, GenreFluide::Lave, hauteurs)], &a);
            let (img, compte) = s.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
            assert_eq!(compte.appels_de_dessin, 3, "quads, lave, eau");
            let debut = pixel(&img, projeter(&cam, pres_du_bas));
            let fin = pixel(&img, projeter(&cam, pres_du_haut));
            assert_eq!(
                debut != fond,
                monte_au_debut,
                "{f:?} {hauteurs:?} : le bout de plus petite coordonnée"
            );
            assert_eq!(
                fin != fond,
                !monte_au_debut,
                "{f:?} {hauteurs:?} : l'autre bout"
            );
        }
    }
}

/// **Un côté FUSIONNÉ sur ±X : ses rangées sont le PREMIER axe du plan.**
///
/// Pour ±X le plan est (Y, Z), donc `taille[0]` compte les rangées et
/// `taille[1]` la longueur ; pour ±Z c'est l'inverse. Une face d'une case ne
/// voit pas la différence — il faut deux rangées sur trois de long.
#[test]
fn un_cote_fusionne_sur_x_a_ses_rangees_en_premier() {
    let Some(app) = app() else { return };
    let a = atlas(&app, &[[230, 110, 30, 255], [150, 150, 150, 180]]);
    let cam = camera([13.0, 9.0, 9.5], [9.0, 9.0, 9.5]);
    let fond = fond(&app, &cam);
    let f = FaceFluide {
        taille: [2, 3],
        ..face(Face::PlusX, GenreFluide::Lave, [255, 255, 0, 0])
    };
    let s = scene_de(&app, vec![f], &a);
    let (img, _) = s.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    // Deux rangées de haut (y de 8 à 10), trois de long (z de 8 à 11).
    let dedans = pixel(&img, projeter(&cam, [9.0, 9.5, 10.5]));
    let trop_haut = pixel(&img, projeter(&cam, [9.0, 10.5, 8.5]));
    assert_ne!(dedans, fond, "la troisième case le long de Z");
    assert_eq!(trop_haut, fond, "pas de troisième rangée");
}

/// Chaque octet de hauteur d'un dessus monte SON coin : `[NO, SO, SE, NE]`.
///
/// Vu d'en haut en perspective, un coin plus haut est plus près de l'œil et
/// sort de la projection de la case posée à plat : on vérifie qu'un point
/// juste sous le coin levé, à 0,95 de hauteur, est couvert — et qu'il ne
/// l'est pas quand c'est un autre coin qui monte.
#[test]
fn le_dessus_monte_chacun_de_ses_quatre_coins() {
    let Some(app) = app() else { return };
    let a = atlas(&app, &[[230, 110, 30, 255], [150, 150, 150, 180]]);
    let cam = camera([8.5, 11.0, 6.9], [8.5, 8.0, 8.5]);
    let fond = fond(&app, &cam);
    // Les coins dans l'ordre des octets, et un point à 0,95 près de chacun.
    let pres: [[f32; 3]; 4] = [
        [8.05, 8.95, 8.05], // NO : (x0, z0)
        [8.05, 8.95, 8.95], // SO : (x0, z1)
        [8.95, 8.95, 8.95], // SE : (x1, z1)
        [8.95, 8.95, 8.05], // NE : (x1, z0)
    ];
    for leve in 0..4 {
        let mut h = [0u8; 4];
        h[leve] = 255;
        let s = scene_de(&app, vec![face(Face::PlusY, GenreFluide::Lave, h)], &a);
        let (img, _) = s.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
        for (k, p) in pres.iter().enumerate() {
            assert_eq!(
                pixel(&img, projeter(&cam, *p)) != fond,
                k == leve,
                "coin {leve} levé : le point près du coin {k}"
            );
        }
    }
}

// ── de l'eau sur un fond, de bout en bout ───────────────────────────────────

const AIR: StateId = 0;
const FOND: StateId = 1;
const EAU: StateId = 2;
const LAVE: StateId = 3;

fn table() -> TableFormes {
    let mut t = TableFormes::new();
    t.pousser(true, false, Vec::new());
    t.pousser(false, true, Vec::new());
    for g in [GenreFluide::Eau, GenreFluide::Lave] {
        let id = t.pousser(true, false, Vec::new());
        t.marquer_fluide(id, Fluide::source(g));
    }
    t
}

fn section(y: i8, f: impl Fn(i32, i32, i32) -> StateId) -> Section {
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

/// Un fond de 16 × 16 et, par-dessus, une nappe de `fluide` bordée du même
/// fond — maillés par les TROIS passes et rendus vus d'en haut. Le fond est
/// de la couleur `sol`.
fn bassin(app: &Appareil, fluide: StateId, sol: [u8; 4]) -> (Vec<u8>, tf_render::Compte) {
    let t = table();
    let mut g = Grille::new();
    g.poser(
        0,
        0,
        section(0, |x, y, z| {
            let bord = x == 0 || x == 15 || z == 0 || z == 15;
            match y {
                0..=3 => FOND,
                4 if bord => FOND,
                4 => fluide,
                _ => AIR,
            }
        }),
    );
    let c = g.mailler(&t);
    let arene = Arene::depuis(&c, &|_, _, _| (0, [1.0; 3], tf_render::Sens::DROIT));
    let fluides = AreneFluides::depuis(&c, arene.emplacements(), &|g, _, _| match g {
        GenreFluide::Eau => (1, [0.25, 0.46, 0.89]),
        GenreFluide::Lave => (2, [1.0; 3]),
    });
    let a = atlas(app, &[sol, [150, 150, 150, 180], [230, 110, 30, 255]]);
    let s = Scene::pour(app, &arene, &AreneModeles::default(), &fluides, &a, FORMAT);
    s.rendre(
        &Cible::nouvelle(app, COTE, COTE),
        &camera([8.0, 14.0, 7.0], [8.0, 4.0, 8.0]),
    )
}

#[test]
fn l_eau_laisse_voir_ce_qu_elle_recouvre_la_lave_non() {
    let Some(app) = app() else { return };
    let centre = (COTE / 2, COTE / 2);
    let rouge = [200, 30, 30, 255];
    let bleu = [30, 30, 200, 255];
    let (eau_rouge, compte) = bassin(&app, EAU, rouge);
    assert_eq!(compte.appels_de_dessin, 3, "quads, lave, eau");
    let (eau_bleue, _) = bassin(&app, EAU, bleu);
    let (sec_rouge, _) = bassin(&app, AIR, rouge);
    let (lave_rouge, _) = bassin(&app, LAVE, rouge);
    let (lave_bleue, _) = bassin(&app, LAVE, bleu);
    let [er, eb, sr] = [&eau_rouge, &eau_bleue, &sec_rouge].map(|i| pixel(i, centre));
    assert_ne!(er, sr, "l'eau se voit : {er:?} contre {sr:?} à sec");
    assert!(
        er[0] > eb[0] + 20 && eb[2] > er[2] + 20,
        "le fond se voit À TRAVERS l'eau : {er:?} sur du rouge, {eb:?} sur du bleu"
    );
    assert_eq!(
        pixel(&lave_rouge, centre),
        pixel(&lave_bleue, centre),
        "la lave cache son fond"
    );
    assert_ne!(pixel(&lave_rouge, centre), sr, "et elle se voit");
}

#[test]
fn l_eau_n_ecrit_pas_la_profondeur() {
    // Deux nappes d'eau, la PROCHE rangée avant la lointaine : si l'eau
    // écrivait la profondeur, la lointaine échouerait au test partout où la
    // proche la recouvre, et ce qu'on voit dépendrait de l'ordre des
    // instances. Sans, les deux se mélangent.
    let Some(app) = app() else { return };
    let a = atlas(&app, &[[230, 110, 30, 255], [150, 150, 150, 180]]);
    let cam = camera([8.5, 8.5, 3.0], [8.5, 8.5, 9.0]);
    let proche = FaceFluide {
        pos: [8, 8, 7],
        ..face(Face::MoinsZ, GenreFluide::Eau, [255, 255, 0, 0])
    };
    let lointaine = FaceFluide {
        pos: [8, 8, 10],
        ..face(Face::MoinsZ, GenreFluide::Eau, [255, 255, 0, 0])
    };
    let seule = scene_de(&app, vec![proche], &a);
    let deux = scene_de(&app, vec![proche, lointaine], &a);
    let p = projeter(&cam, [8.5, 8.5, 7.0]);
    let (i1, _) = seule.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    let (i2, _) = deux.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    assert_ne!(
        pixel(&i1, p),
        pixel(&i2, p),
        "la nappe lointaine doit se voir à travers la proche"
    );
    // Et la lave, elle, l'écrit : derrière elle, l'eau ne se voit pas.
    let lave = FaceFluide {
        pos: [8, 8, 7],
        ..face(Face::MoinsZ, GenreFluide::Lave, [255, 255, 0, 0])
    };
    let (l1, _) = scene_de(&app, vec![lave], &a).rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    let (l2, _) =
        scene_de(&app, vec![lave, lointaine], &a).rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    assert_eq!(
        pixel(&l1, p),
        pixel(&l2, p),
        "rien ne se voit derrière la lave"
    );
}

#[test]
fn une_scene_suivie_montre_les_fluides_d_une_scene_neuve() {
    // Une nappe, puis la moitié retirée par un remplacement : la scène qui
    // SUIT par `synchroniser` et une scène bâtie de neuf doivent rendre la
    // même image — y compris quand la nappe disparaît tout à fait.
    let Some(app) = app() else { return };
    let t = table();
    let a = atlas(
        &app,
        &[[90, 90, 90, 255], [150, 150, 150, 180], [230, 110, 30, 255]],
    );
    let cam = camera([8.0, 14.0, 7.0], [8.0, 4.0, 8.0]);
    let apparence = |g: GenreFluide, _: TextureFluide, _: StateId| match g {
        GenreFluide::Eau => (1, [0.25, 0.46, 0.89]),
        GenreFluide::Lave => (2, [1.0; 3]),
    };
    let nappe = |jusqu_a: i32| {
        move |x: i32, y: i32, _z: i32| match y {
            0..=3 => FOND,
            4 if x < jusqu_a => EAU,
            _ => AIR,
        }
    };
    let mut g = Grille::new();
    g.poser(0, 0, section(0, nappe(16)));
    let c = g.mailler(&t);
    let mut arene = Arene::depuis(&c, &|_, _, _| (0, [1.0; 3], tf_render::Sens::DROIT));
    let mut modeles = AreneModeles::default();
    let mut fluides = AreneFluides::depuis(&c, arene.emplacements(), &apparence);
    let mut suivie = Scene::vide(&app, &a, FORMAT);
    suivie.synchroniser(&mut arene, &mut modeles, &mut fluides);
    for jusqu_a in [8, 0, 12] {
        g.poser(0, 0, section(0, nappe(jusqu_a)));
        let neufs = g.mailler(&t);
        let visees = [(0, 0, 0)];
        arene.remplacer(&visees, &neufs.lots, &|_, _, _| {
            (0, [1.0; 3], tf_render::Sens::DROIT)
        });
        fluides.remplacer(arene.emplacements(), &visees, &neufs.lots, &apparence);
        let envoyes = suivie.synchroniser(&mut arene, &mut modeles, &mut fluides);
        assert!(
            envoyes > 0,
            "nappe jusqu'à {jusqu_a} : quelque chose est parti"
        );
        let neuve = Scene::pour(&app, &arene, &modeles, &fluides, &a, FORMAT);
        let (x, cx) = suivie.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
        let (y, cy) = neuve.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
        assert_eq!(cx, cy, "nappe jusqu'à {jusqu_a} : les comptes");
        assert!(
            x == y,
            "nappe jusqu'à {jusqu_a} : la scène suivie diffère de la neuve"
        );
    }

    // La section PART tout entière : aucun lot neuf pour elle. Sa nappe doit
    // partir avec — c'est le piège « un chunk qui se vide ne figure plus dans
    // la liste des chunks ».
    let avant = suivie.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam).0;
    g.retirer((0, 0, 0));
    let visees = [(0, 0, 0)];
    arene.remplacer(&visees, &[], &|_, _, _| {
        (0, [1.0; 3], tf_render::Sens::DROIT)
    });
    fluides.remplacer(arene.emplacements(), &visees, &[], &apparence);
    suivie.synchroniser(&mut arene, &mut modeles, &mut fluides);
    let (x, _) = suivie.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    assert_ne!(x, avant, "la prémisse : il y avait quelque chose à voir");
    assert_eq!(
        x,
        fond(&app, &cam)
            .repeat((COTE * COTE) as usize)
            .iter()
            .enumerate()
            .fold(Vec::new(), |mut v, (i, c)| {
                v.push(*c);
                if i % 3 == 2 {
                    v.push(255);
                }
                v
            }),
        "une section partie ne laisse pas son eau à l'écran"
    );

    // **Une scène NEUVE qui suit une arène déjà suivie** — ce que fait la
    // fenêtre quand elle refait sa scène : les plages sales ont été prises
    // par l'ancienne, et la neuve doit quand même tout recevoir.
    g.poser(0, 0, section(0, nappe(16)));
    let neufs = g.mailler(&t);
    arene.remplacer(&visees, &neufs.lots, &|_, _, _| {
        (0, [1.0; 3], tf_render::Sens::DROIT)
    });
    fluides.remplacer(arene.emplacements(), &visees, &neufs.lots, &apparence);
    suivie.synchroniser(&mut arene, &mut modeles, &mut fluides);
    let mut refaite = Scene::vide(&app, &a, FORMAT);
    refaite.synchroniser(&mut arene, &mut modeles, &mut fluides);
    let (x, _) = refaite.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    let (y, _) = suivie.rendre(&Cible::nouvelle(&app, COTE, COTE), &cam);
    assert!(
        x == y,
        "une scène refaite montre ce que l'ancienne montrait"
    );
}
