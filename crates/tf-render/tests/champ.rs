//! **Les lignes du calque sont découpées au champ de la caméra**, côté
//! processeur, avant de partir au GPU.
//!
//! Le GPU découpe au plan proche, et c'est exact ; mais l'extrémité découpée
//! d'un segment qui passe DERRIÈRE la caméra peut tomber des milliers de fois
//! la largeur de l'écran plus loin, et un rastériseur n'a pas à y garder sa
//! précision : sous llvmpipe, l'arête d'une `.mca` qui passait derrière la
//! caméra se dessinait en escalier de marches de 64 pixels.
//!
//! Deux propriétés, vérifiées sur des segments tirés d'une graine : tout ce
//! qui reste est DANS le champ, et tout ce qui était visible est RESTÉ.

use tf_render::{rgba, Camera, Lignes};

/// La caméra des tests : à l'origine, regardant +Z. Son plan lointain est
/// PLUS PRÈS que le bord des tirages (± 200), pour que les six plans servent.
fn camera() -> Camera {
    Camera {
        oeil: [0.0, 0.0, 0.0],
        cible: [0.0, 0.0, 1.0],
        fov: 60f32.to_radians(),
        proche: 0.1,
        loin: 150.0,
    }
}

fn vp() -> [[f32; 4]; 4] {
    camera().gpu(1.5).vue_projection
}

/// Les coordonnées normalisées d'un point, et s'il est devant la caméra.
fn ndc(p: [f32; 3]) -> Option<[f32; 3]> {
    let m = vp();
    let q = [p[0], p[1], p[2], 1.0];
    let mut c = [0.0f32; 4];
    for (j, cj) in c.iter_mut().enumerate() {
        *cj = (0..4).map(|i| q[i] * m[i][j]).sum();
    }
    (c[3] > 0.0).then(|| [c[0] / c[3], c[1] / c[3], c[2] / c[3]])
}

/// Dans le champ, à `marge` près : au bord de l'écran en coordonnées
/// normalisées, mais entre les deux plans en PROFONDEUR — la caméra regarde
/// +Z depuis l'origine, la profondeur est donc `z`. La profondeur normalisée,
/// elle, est hyperbolique : tout ce qui est au-delà du plan lointain y tient
/// dans un millième, et une marge d'un millième n'en verrait rien.
fn dedans(p: [f32; 3], marge: f32) -> bool {
    let c = camera();
    ndc(p).is_some_and(|n| {
        n[0].abs() <= 1.0 + marge
            && n[1].abs() <= 1.0 + marge
            && p[2] >= c.proche * (1.0 - marge)
            && p[2] <= c.loin * (1.0 + marge)
    })
}

fn un(a: [f32; 3], b: [f32; 3]) -> Lignes {
    let mut l = Lignes::new();
    l.segment(a, b, rgba(255, 90, 90, 230));
    l
}

#[test]
fn un_segment_dans_le_champ_ressort_tel_quel() {
    let l = un([-1.0, 0.5, 10.0], [2.0, -0.5, 30.0]);
    assert_eq!(l.dans_le_champ(&vp()), l);
}

#[test]
fn un_segment_tout_entier_derriere_la_camera_disparait() {
    let l = un([-5.0, 0.0, -1.0], [5.0, 3.0, -50.0]);
    assert!(l.dans_le_champ(&vp()).is_empty());
}

/// **Le cas de l'escalier** : une arête qui part devant la caméra et passe
/// DERRIÈRE elle. Ce qui reste est le morceau visible, qui s'arrête au BORD
/// de l'écran et pas au plan proche, où l'extrémité découpée tombait des
/// centaines de largeurs d'écran plus loin — et il reste sur la droite
/// d'origine.
#[test]
fn un_segment_qui_passe_derriere_la_camera_ne_garde_que_ce_qu_on_voit() {
    let (a, b) = ([0.0, -1.0, 50.0], [10.0, -1.0, -50.0]);
    // Sans découpe, le point où le segment traverse le plan proche est très
    // loin hors de l'écran : c'est lui que le rastériseur recevait.
    let t = (50.0 - 0.1) / 100.0;
    let proche = [a[0] + t * (b[0] - a[0]), -1.0, 0.1];
    assert!(ndc(proche).unwrap()[0].abs() > 30.0, "{:?}", ndc(proche));

    let l = un(a, b).dans_le_champ(&vp());
    assert_eq!(l.len(), 1);
    assert_eq!(l.sommets[0].position, a, "le début était dedans");
    let fin = l.sommets[1].position;
    let n = ndc(fin).unwrap();
    assert!((n[0].abs() - 1.0).abs() < 1e-4, "arrêté au bord : {n:?}");
    assert!(dedans(fin, 1e-4));
    // Sur la droite d'origine.
    let t = (fin[0] - a[0]) / (b[0] - a[0]);
    assert!(
        (fin[2] - (a[2] + t * (b[2] - a[2]))).abs() < 1e-3,
        "{fin:?}"
    );
}

/// Un segment qui sort par le côté s'arrête au bord — lequel, c'est la
/// caméra qui le dit : regardant +Z, +X est à GAUCHE de l'image.
#[test]
fn un_segment_qui_sort_par_le_cote_s_arrete_au_bord() {
    let l = un([0.0, 0.0, 10.0], [500.0, 0.0, 10.0]).dans_le_champ(&vp());
    assert_eq!(l.len(), 1);
    let x = ndc(l.sommets[1].position).unwrap()[0];
    assert!((x.abs() - 1.0).abs() < 1e-4, "{x}");
    assert_eq!(
        l.sommets[0].position,
        [0.0, 0.0, 10.0],
        "le début était dedans"
    );
}

/// **Un segment qui traverse l'ŒIL commence au plan proche**, pas à l'œil.
///
/// Les quatre plans des côtés se rejoignent à l'œil, et c'est tout ce qu'ils
/// retiendraient : à l'œil, la profondeur vaut zéro et la division par elle
/// n'a plus de sens. C'est le plan proche qui les en sépare.
#[test]
fn un_segment_qui_traverse_l_oeil_commence_au_plan_proche() {
    let l = un([0.0, 0.0, -5.0], [0.0, 0.0, 50.0]).dans_le_champ(&vp());
    assert_eq!(l.len(), 1);
    let debut = l.sommets[0].position;
    assert!((debut[2] - 0.1).abs() < 1e-4, "{debut:?}");
    assert!(dedans(debut, 1e-4));
    assert_eq!(l.sommets[1].position, [0.0, 0.0, 50.0]);
}

/// **Un segment qui file au-delà du plan lointain s'y arrête** — le GPU ne le
/// dessinerait pas plus loin, et ce qui part au GPU doit être ce qu'il
/// dessine.
#[test]
fn un_segment_qui_file_au_loin_s_arrete_au_plan_lointain() {
    let l = un([0.0, 0.0, 10.0], [0.0, 0.0, 5000.0]).dans_le_champ(&vp());
    assert_eq!(l.len(), 1);
    assert_eq!(l.sommets[0].position, [0.0, 0.0, 10.0]);
    let fin = l.sommets[1].position;
    assert!((fin[2] - 150.0).abs() < 1e-2, "{fin:?}");
    assert!(dedans(fin, 1e-4));
}

/// **Les deux propriétés, sur des segments tirés d'une graine** : tout ce qui
/// reste est dans le champ, et tout point du segment d'origine qui était dans
/// le champ est couvert par ce qui reste — la découpe n'enlève rien de
/// visible.
#[test]
fn la_decoupe_garde_tout_le_visible_et_rien_d_autre() {
    let mut graine = 0x9e37_79b9_7f4a_7c15u64;
    let mut tirer = |bord: f32| {
        graine ^= graine << 13;
        graine ^= graine >> 7;
        graine ^= graine << 17;
        (graine >> 11) as f32 / (1u64 << 53) as f32 * 2.0 * bord - bord
    };
    let mut gardes = 0;
    for _ in 0..2000 {
        let a = [tirer(200.0), tirer(200.0), tirer(200.0)];
        let b = [tirer(200.0), tirer(200.0), tirer(200.0)];
        let l = un(a, b).dans_le_champ(&vp());
        for s in &l.sommets {
            assert!(
                dedans(s.position, 1e-3),
                "{a:?}–{b:?} garde {:?}",
                s.position
            );
        }
        // Des points du segment d'origine, franchement dans le champ : chacun
        // doit tomber entre les extrémités gardées.
        for k in 1..64 {
            let t = k as f32 / 64.0;
            let p = [
                a[0] + t * (b[0] - a[0]),
                a[1] + t * (b[1] - a[1]),
                a[2] + t * (b[2] - a[2]),
            ];
            if !dedans(p, -1e-2) {
                continue;
            }
            let [g0, g1] = [l.sommets[0].position, l.sommets[1].position];
            // Le paramètre de p le long de (g0, g1), sur l'axe le plus long.
            let axe = (0..3)
                .max_by(|&i, &j| {
                    (g1[i] - g0[i])
                        .abs()
                        .partial_cmp(&(g1[j] - g0[j]).abs())
                        .unwrap()
                })
                .unwrap();
            let long = g1[axe] - g0[axe];
            let u = if long.abs() < 1e-6 {
                0.0
            } else {
                (p[axe] - g0[axe]) / long
            };
            assert!(
                (-1e-3..=1.0 + 1e-3).contains(&u),
                "{a:?}–{b:?} : le point visible {p:?} a été coupé ({u})"
            );
        }
        gardes += l.len();
    }
    assert!(
        gardes > 100,
        "le tirage ne met presque rien dans le champ : {gardes}"
    );
}
