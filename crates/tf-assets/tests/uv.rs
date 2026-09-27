//! **Le sens d'une texture** : la transcription du jeu (`tf_assets::uv`),
//! éprouvée par ce qu'elle doit VOULOIR DIRE — pas relue.
//!
//! Trois propriétés, et aucune ne recopie une table :
//!
//! 1. les uv par défaut sont la PROJECTION de l'élément sur sa face — la
//!    texture posée à plat sur le monde, face par face ;
//! 2. sans `uvlock`, la texture SUIT la géométrie : ce que la face tournée
//!    donne en chaque sommet est ce que le jeu y attache avant rotation ;
//! 3. avec `uvlock`, elle reste ALIGNÉE SUR LE MONDE : une face aux uv par
//!    défaut, tournée, montre la projection de l'élément TOURNÉ.
//!
//! La troisième est ce qu'`uvlock` veut dire, et elle ne dépend pas de la
//! façon dont `recomputeUVs` s'y prend : si la transcription se trompe d'un
//! signe, d'un ordre de composition ou d'un sens de rotation, elle tombe.

use tf_assets::rotation::{axes, tourner_face, tourner_point, Axes, IDENTITE};
use tf_assets::uv::{
    axes_du_plan, poser, pour_uvlock, sommets, uv_au_point, uv_du_sommet, uv_par_defaut,
};
use tf_mesh::forme::{Face, FACES};

/// La texture posée à plat sur le monde, face par face : ce qu'on voit d'un
/// bloc qui n'est pas tourné. Écrite depuis ce qu'on VOIT de dehors — une
/// face montre sa texture debout, non retournée — et non depuis le code.
fn a_plat(face: Face, p: [f32; 3]) -> [f32; 2] {
    let [x, y, z] = p;
    match face {
        Face::MoinsY => [x, 16.0 - z],
        Face::PlusY => [x, z],
        Face::MoinsZ => [16.0 - x, 16.0 - y],
        Face::PlusZ => [x, 16.0 - y],
        Face::MoinsX => [z, 16.0 - y],
        Face::PlusX => [16.0 - z, 16.0 - y],
    }
}

struct Graine(u64);

impl Graine {
    fn tirer(&mut self, borne: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % borne
    }
    /// Deux bornes distinctes dans `0..=16`, rangées.
    fn intervalle(&mut self) -> (f32, f32) {
        loop {
            let (a, b) = (self.tirer(17) as f32, self.tirer(17) as f32);
            if a != b {
                return (a.min(b), a.max(b));
            }
        }
    }
    fn element(&mut self) -> ([f32; 3], [f32; 3]) {
        let (x0, x1) = self.intervalle();
        let (y0, y1) = self.intervalle();
        let (z0, z1) = self.intervalle();
        ([x0, y0, z0], [x1, y1, z1])
    }
    fn angle(&mut self) -> u16 {
        self.tirer(4) as u16 * 90
    }
}

/// La position d'un sommet tourné dans le plan de sa face : `(s, t)` en
/// fraction de ses deux axes croissants.
fn dans_le_plan(face: Face, p: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> (f32, f32) {
    let (a, b) = axes_du_plan(face);
    (
        (p[a] - lo[a]) / (hi[a] - lo[a]),
        (p[b] - lo[b]) / (hi[b] - lo[b]),
    )
}

/// Les sommets d'une face tournée, et les bornes de la face.
fn tournes(
    face: Face,
    min: [f32; 3],
    max: [f32; 3],
    a: Axes,
) -> ([[f32; 3]; 4], [f32; 3], [f32; 3]) {
    let p = sommets(face, min, max).map(|s| tourner_point(s, a));
    let lo: [f32; 3] =
        std::array::from_fn(|k| p.iter().map(|q| q[k]).fold(f32::INFINITY, f32::min));
    let hi: [f32; 3] =
        std::array::from_fn(|k| p.iter().map(|q| q[k]).fold(f32::NEG_INFINITY, f32::max));
    (p, lo, hi)
}

#[test]
fn les_uv_par_defaut_sont_la_projection_de_l_element() {
    let mut g = Graine(7);
    for _ in 0..500 {
        let (min, max) = g.element();
        for f in FACES {
            let uv = uv_par_defaut(f, min, max);
            for (i, s) in sommets(f, min, max).iter().enumerate() {
                assert_eq!(
                    uv_du_sommet(uv, 0, i),
                    a_plat(f, *s),
                    "{f:?} sommet {i} de {min:?}..{max:?}"
                );
            }
        }
    }
}

#[test]
fn la_rotation_d_une_face_fait_tourner_ses_coins() {
    // Un quart de tour décale les coins d'un sommet : le sommet 0 prend ce
    // que le 1 avait, et ainsi de suite (`BlockFaceUV::getShiftedIndex`).
    let uv = [1.0, 2.0, 3.0, 4.0];
    let coins: Vec<[f32; 2]> = (0..4).map(|i| uv_du_sommet(uv, 0, i)).collect();
    assert_eq!(coins, vec![[1.0, 2.0], [1.0, 4.0], [3.0, 4.0], [3.0, 2.0]]);
    for r in 1..4u16 {
        for i in 0..4 {
            assert_eq!(uv_du_sommet(uv, r * 90, i), coins[(i + r as usize) % 4]);
        }
    }
}

#[test]
fn sans_uvlock_la_texture_suit_la_geometrie() {
    let mut g = Graine(11);
    for _ in 0..400 {
        let (min, max) = g.element();
        // Des uv DÉCLARÉES, dans n'importe quel ordre : un rectangle retourné
        // est un rectangle retourné, et il doit le rester.
        let uv = [
            g.tirer(17) as f32,
            g.tirer(17) as f32,
            g.tirer(17) as f32,
            g.tirer(17) as f32,
        ];
        let rotation = g.angle();
        let (x, y) = (g.angle(), g.angle());
        let a = axes(x, y);
        for f in FACES {
            let posee = poser(f, min, max, uv, rotation, a, false);
            assert_eq!(posee.face, tourner_face(f, a));
            let (p, lo, hi) = tournes(f, min, max, a);
            for (i, q) in p.iter().enumerate() {
                let (s, t) = dans_le_plan(posee.face, *q, lo, hi);
                assert_eq!(
                    uv_au_point(&posee, s, t),
                    uv_du_sommet(uv, rotation, i),
                    "{f:?} tournée par x={x} y={y} (rotation de face {rotation}), sommet {i}"
                );
            }
        }
    }
}

#[test]
fn avec_uvlock_la_texture_reste_alignee_sur_le_monde() {
    let mut g = Graine(13);
    let mut vues = 0;
    for _ in 0..400 {
        let (min, max) = g.element();
        let (x, y) = (g.angle(), g.angle());
        let a = axes(x, y);
        for f in FACES {
            let posee = poser(f, min, max, uv_par_defaut(f, min, max), 0, a, true);
            let (p, lo, hi) = tournes(f, min, max, a);
            for q in &p {
                let (s, t) = dans_le_plan(posee.face, *q, lo, hi);
                assert_eq!(
                    uv_au_point(&posee, s, t),
                    a_plat(posee.face, *q),
                    "{f:?} tournée par x={x} y={y}, au sommet {q:?}"
                );
                vues += 1;
            }
        }
    }
    assert_eq!(vues, 400 * 6 * 4);
}

#[test]
fn uvlock_sans_rotation_garde_le_rectangle_et_retourne_l_angle_de_la_face() {
    // Le rectangle ne bouge pas. L'angle de la face, lui, se compte dans
    // l'autre sens — la formule du jeu, transcrite telle quelle : 90° devient
    // 270°. Juste pour une face non tournée (0° reste 0°), et c'est ce signe
    // même qui garde alignés sur le monde les escaliers tournés : le test
    // précédent tomberait sans lui.
    let mut g = Graine(17);
    for _ in 0..200 {
        let uv = [
            g.tirer(17) as f32,
            g.tirer(17) as f32,
            g.tirer(17) as f32,
            g.tirer(17) as f32,
        ];
        let rotation = g.angle();
        for f in FACES {
            assert_eq!(
                pour_uvlock(uv, rotation, f, IDENTITE),
                (uv, (360 - rotation) % 360),
                "{f:?}"
            );
        }
    }
}

#[test]
fn une_buche_couchee_garde_ses_fibres_le_long_de_son_axe() {
    // `oak_log[axis=x]` : le modèle de la colonne, `x: 90, y: 90`, sans
    // uvlock. Le côté nord de la colonne debout a ses fibres VERTICALES —
    // `v` court le long de Y. Couchée le long de X, la colonne montre ce
    // côté sur une face horizontale ou latérale, et les fibres doivent
    // courir le long de X : c'est `v` qui doit y varier avec X.
    let a = axes(90, 90);
    let (min, max) = ([0.0; 3], [16.0; 3]);
    let mut vues = 0;
    for f in [Face::MoinsZ, Face::PlusZ, Face::MoinsX, Face::PlusX] {
        let posee = poser(f, min, max, uv_par_defaut(f, min, max), 0, a, false);
        if posee.face.axe() == 0 {
            continue; // les bouts de la bûche regardent vers ±X
        }
        // Le long de X, dans le plan de la face tournée : l'axe 0.
        let (premier, _) = axes_du_plan(posee.face);
        assert_eq!(premier, 0, "X est le premier axe de ce plan");
        let depart = uv_au_point(&posee, 0.0, 0.5);
        let arrivee = uv_au_point(&posee, 1.0, 0.5);
        assert_eq!(
            depart[0], arrivee[0],
            "{f:?} → {:?} : u ne bouge pas le long de X",
            posee.face
        );
        assert_ne!(
            depart[1], arrivee[1],
            "{f:?} → {:?} : les fibres (v) courent le long de X",
            posee.face
        );
        vues += 1;
    }
    assert_eq!(vues, 4, "les quatre côtés de la bûche sont couchés");
}

// ── la chaîne complète : pack → catalogue → habillage ──────────────────────
//
// `poser` est juste ; encore faut-il que le catalogue lui passe la BONNE
// variante — sa rotation et son `uvlock`. Un catalogue qui oublierait
// `uvlock` dessinerait des escaliers aux planches tournées, et aucun des
// tests ci-dessus ne le verrait : ils appellent `poser` eux-mêmes.

mod pack {
    use std::fs;
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(nom: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "tf-assets-uv-{nom}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
        pub fn ecrire(&self, chemin: &str, contenu: &[u8]) {
            let p = self.0.join(chemin);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, contenu).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub fn png(cote: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, cote, cote);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[200u8; 4].repeat((cote * cote) as usize))
                .unwrap();
        }
        out
    }
}

#[test]
fn le_catalogue_donne_a_chaque_face_la_rotation_et_l_uvlock_de_sa_variante() {
    use tf_assets::catalogue::{table_rendu, Disposition};
    use tf_assets::{Atlas, Catalogue, Dossier};

    let d = pack::TempDir::new("variantes");
    // Un cube plein à six faces, tourné de quatre façons.
    d.ecrire(
        "assets/minecraft/blockstates/bloc.json",
        br#"{"variants":{
            "v=droit":{"model":"minecraft:block/cube"},
            "v=couche":{"model":"minecraft:block/cube","x":90,"y":90},
            "v=tourne":{"model":"minecraft:block/cube","y":90},
            "v=verrou":{"model":"minecraft:block/cube","y":90,"uvlock":true}}}"#,
    );
    d.ecrire(
        "assets/minecraft/models/block/cube.json",
        br#"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
            "down":{"texture":"block/t"},"up":{"texture":"block/t"},
            "north":{"texture":"block/t"},"south":{"texture":"block/t"},
            "west":{"texture":"block/t"},"east":{"texture":"block/t"}}}]}"#,
    );
    d.ecrire("assets/minecraft/textures/block/t.png", &pack::png(16));
    let src = Dossier::ouvrir(d.path()).unwrap();
    let mut cat = Catalogue::new(Disposition::Pack);
    cat.charger_bloc(&src, "minecraft:bloc").unwrap();
    cat.resoudre_modeles(&src);
    let cles = [
        "minecraft:bloc|v=droit",
        "minecraft:bloc|v=couche",
        "minecraft:bloc|v=tourne",
        "minecraft:bloc|v=verrou",
    ];
    // Les textures que le catalogue DEMANDE, comme l'application les monte.
    let voulues = tf_assets::textures_des_etats(&cat, cles.iter().map(|c| c.to_string()));
    assert!(!voulues.is_empty(), "la texture du cube est demandée");
    let atlas = Atlas::batir(&src, voulues, &|n| Disposition::Pack.chemins_texture(n));
    assert_eq!(atlas.len(), 1, "et trouvée");
    let (_, hab) = table_rendu(
        &cat,
        &atlas,
        &tf_assets::Teintes::default(),
        cles.iter().map(|c| c.to_string()),
        &|_| false,
    );
    let (min, max) = ([0.0; 3], [16.0; 3]);
    for (k, (x, y, uvlock)) in [
        (0, 0, false),
        (90, 90, false),
        (0, 90, false),
        (0, 90, true),
    ]
    .into_iter()
    .enumerate()
    {
        for f in FACES {
            let p = poser(
                f,
                min,
                max,
                uv_par_defaut(f, min, max),
                0,
                axes(x, y),
                uvlock,
            );
            let a = hab[k].cube[p.face.indice()];
            assert_eq!(
                (a.uv, a.echange),
                (p.uv, p.echange),
                "{} : la face {f:?}, arrivée en {:?}",
                cles[k],
                p.face
            );
        }
    }
    // Et les deux variantes à `y: 90` ne se confondent PAS sur le dessus :
    // sans uvlock il tourne, avec il reste droit.
    let dessus = |k: usize| {
        let a = hab[k].cube[Face::PlusY.indice()];
        (a.uv, a.echange)
    };
    assert_eq!(
        dessus(3),
        dessus(0),
        "uvlock : le dessus reste aligné sur le monde"
    );
    assert_ne!(dessus(2), dessus(0), "sans uvlock, il tourne avec le bloc");
}
