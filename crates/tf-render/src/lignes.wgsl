// Le quadrillage : des segments en coordonnées MONDE, une couleur par sommet.
//
// Pas d'atlas, pas d'instanciation, pas de masquage : ce calque ne dessine
// que ce qu'on lui donne. C'est voulu — un repère qui se met à raisonner
// devient un repère qui peut se tromper.

struct Camera {
    vue_projection: mat4x4<f32>,
    position: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct Entree {
    @location(0) position: vec3<f32>,
    @location(1) couleur: u32,
};

struct Sortie {
    @builtin(position) clip: vec4<f32>,
    @location(0) couleur: vec4<f32>,
};

@vertex
fn vs(e: Entree) -> Sortie {
    var s: Sortie;
    s.clip = camera.vue_projection * vec4<f32>(e.position, 1.0);
    // Déballage RGBA8, rouge en poids faible — la même convention que
    // `rgba()` côté Rust. Deux conventions inverses donneraient un
    // quadrillage bleu là où on a demandé du rouge, sans la moindre erreur.
    s.couleur = vec4<f32>(
        f32((e.couleur >> 0u) & 255u) / 255.0,
        f32((e.couleur >> 8u) & 255u) / 255.0,
        f32((e.couleur >> 16u) & 255u) / 255.0,
        f32((e.couleur >> 24u) & 255u) / 255.0,
    );
    return s;
}

@fragment
fn fs(s: Sortie) -> @location(0) vec4<f32> {
    return s.couleur;
}
