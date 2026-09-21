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
    let brut = vec3<f32>(
        f32((e.couleur >> 0u) & 255u) / 255.0,
        f32((e.couleur >> 8u) & 255u) / 255.0,
        f32((e.couleur >> 16u) & 255u) / 255.0,
    );
    // **La couleur est donnée en sRGB, la cible est en sRGB, et entre les deux
    // le pipeline est LINÉAIRE.** Sans conversion, la valeur passe pour du
    // linéaire et se fait réencoder à l'écriture : (120, 220, 140) sort à
    // (184, 240, 196), un vert qui se lit BLANC. C'est le piège des couleurs
    // de sommet d'`ExeWorldEdit`, mot pour mot, dans un autre moteur.
    //
    // Et il ne se voit pas sur des primaires pures : 0 et 255 sont des points
    // FIXES de la conversion. Un test qui n'essaie que du rouge, du vert et du
    // bleu saturés passe des deux côtés.
    s.couleur = vec4<f32>(
        srgb_vers_lineaire(brut),
        f32((e.couleur >> 24u) & 255u) / 255.0,
    );
    return s;
}

fn srgb_vers_lineaire(c: vec3<f32>) -> vec3<f32> {
    let bas = c / 12.92;
    let haut = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(haut, bas, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(s: Sortie) -> @location(0) vec4<f32> {
    return s.couleur;
}
