//! Une caméra, et rien de plus qu'il n'en faut.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CameraGpu {
    pub vue_projection: [[f32; 4]; 4],
    pub position: [f32; 3],
    pub _pad: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub oeil: [f32; 3],
    pub cible: [f32; 3],
    /// Champ de vision vertical, en radians.
    pub fov: f32,
    pub proche: f32,
    pub loin: f32,
}

fn soustraire(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn produit_vectoriel(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normaliser(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < 1e-9 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / n, v[1] / n, v[2] / n]
}

fn produit_scalaire(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Camera {
    /// Cadre la caméra sur une boîte, vue de trois quarts.
    ///
    /// La distance se calcule depuis le RAYON de la boîte et le champ de
    /// vision : la deviner ferait sortir le build du cadre dès qu'il change de
    /// taille, et c'est précisément ce qu'une capture automatique doit éviter.
    pub fn cadrer(min: [f32; 3], max: [f32; 3], aspect: f32) -> Camera {
        let centre = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let demi = [
            (max[0] - min[0]) * 0.5,
            (max[1] - min[1]) * 0.5,
            (max[2] - min[2]) * 0.5,
        ];
        let rayon = (demi[0] * demi[0] + demi[1] * demi[1] + demi[2] * demi[2])
            .sqrt()
            .max(1.0);
        let fov = 50f32.to_radians();
        // Le champ HORIZONTAL est le plus serré quand l'image est haute : on
        // prend le plus contraignant des deux, sinon un build large déborde.
        let fov_utile = if aspect < 1.0 {
            2.0 * ((fov * 0.5).tan() * aspect).atan()
        } else {
            fov
        };
        let distance = rayon / (fov_utile * 0.5).sin() * 1.15;
        let dir = normaliser([0.62, 0.45, 0.65]);
        Camera {
            oeil: [
                centre[0] + dir[0] * distance,
                centre[1] + dir[1] * distance,
                centre[2] + dir[2] * distance,
            ],
            cible: centre,
            fov,
            proche: (distance * 0.01).max(0.05),
            loin: distance * 4.0 + rayon * 4.0,
        }
    }

    pub fn gpu(&self, aspect: f32) -> CameraGpu {
        let avant = normaliser(soustraire(self.cible, self.oeil));
        let droite = normaliser(produit_vectoriel(avant, [0.0, 1.0, 0.0]));
        let haut = produit_vectoriel(droite, avant);

        // Vue : la base inversée, appliquée à l'œil.
        let vue = [
            [droite[0], haut[0], -avant[0], 0.0],
            [droite[1], haut[1], -avant[1], 0.0],
            [droite[2], haut[2], -avant[2], 0.0],
            [
                -produit_scalaire(droite, self.oeil),
                -produit_scalaire(haut, self.oeil),
                produit_scalaire(avant, self.oeil),
                1.0,
            ],
        ];

        // Projection en profondeur 0..1 — la convention de wgpu, pas celle
        // d'OpenGL. Se tromper de convention donne une image où TOUT est
        // derrière le plan proche, donc noire, sans la moindre erreur.
        let f = 1.0 / (self.fov * 0.5).tan();
        let a = self.loin / (self.proche - self.loin);
        let proj = [
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, a, -1.0],
            [0.0, 0.0, a * self.proche, 0.0],
        ];

        let mut vp = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += vue[i][k] * proj[k][j];
                }
                vp[i][j] = s;
            }
        }
        CameraGpu {
            vue_projection: vp,
            position: self.oeil,
            _pad: 0.0,
        }
    }
}
