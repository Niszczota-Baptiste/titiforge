// Un quad par INSTANCE. Six indices, aucun sommet en mémoire : la géométrie se
// déduit de la face et de la taille. Une muraille de 64 × 40 blocs tient donc
// en 32 octets.

struct Camera {
    vue_projection: mat4x4<f32>,
    position: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var echantillonneur: sampler;

struct Instance {
    // Coin de plus petites coordonnées, en SEIZIÈMES de bloc, en monde.
    @location(0) position: vec3<f32>,
    // Étendue dans les deux axes du plan, en seizièmes.
    @location(1) taille: vec2<f32>,
    // 0 = −X, 1 = +X, 2 = −Y, 3 = +Y, 4 = −Z, 5 = +Z.
    // L'ordre est `axe * 2 + (positif ? 1 : 0)` — la face NÉGATIVE d'abord.
    @location(2) face: u32,
    @location(3) couche: u32,
};

struct Sortie {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) couche: u32,
    @location(2) ombre: f32,
    @location(3) monde: vec3<f32>,
};

// L'ombrage par face, dans l'ORDRE DU MAILLEUR.
//
// Dans `we-engine`, une table annonçait « −X +X +Y −Y » pour un mailleur qui
// produit « −X +X −Y +Y » : le dessus des blocs était assombri et le dessous
// éclairé à plein. Invisible sur un build gris, et sorti seulement en posant
// des textures dessus. Un test de rendu compare ici les pixels du dessus et du
// dessous d'un cube — un ordre d'indices se MESURE.
// Écrit face par face plutôt qu'en table indexée : un tableau constant ne
// s'indexe pas par une variable en WGSL, et surtout la correspondance
// face → valeur se LIT ici au lieu de se compter sur les doigts.
fn ombre_de(face: u32) -> f32 {
    switch face {
        case 0u, 1u: { return 0.6; }   // ±X — les côtés est et ouest
        case 2u:     { return 0.5; }   // −Y — le DESSOUS, le plus sombre
        case 3u:     { return 1.0; }   // +Y — le DESSUS, en pleine lumière
        default:     { return 0.8; }   // ±Z — nord et sud
    }
}

fn coin(face: u32, taille: vec2<f32>, u: f32, v: f32) -> vec3<f32> {
    let axe = face >> 1u;
    let du = taille.x * u;
    let dv = taille.y * v;
    // Les deux axes du plan, dans l'ordre CROISSANT — le même que celui de
    // `taille` côté processeur. Les échanger retournerait deux faces sur six.
    switch axe {
        case 0u: { return vec3<f32>(0.0, du, dv); }   // ±X : plan (Y, Z)
        case 1u: { return vec3<f32>(du, 0.0, dv); }   // ±Y : plan (X, Z)
        default: { return vec3<f32>(du, dv, 0.0); }   // ±Z : plan (X, Y)
    }
}

@vertex
fn vs(inst: Instance, @builtin(vertex_index) i: u32) -> Sortie {
    // Deux triangles, quatre coins : 0,1,2, 2,1,3 côté indices.
    let u = f32(i & 1u);
    let v = f32((i >> 1u) & 1u);
    let p = inst.position + coin(inst.face, inst.taille, u, v);

    var out: Sortie;
    // Seizièmes → blocs.
    let monde = p / 16.0;
    out.clip = cam.vue_projection * vec4<f32>(monde, 1.0);
    out.monde = monde;
    // La texture se RÉPÈTE par bloc : un quad de 4 blocs montre quatre fois
    // sa texture. C'est pour ça que l'atlas est un TABLEAU — sur une planche,
    // la répétition mordrait sur la tuile voisine.
    out.uv = vec2<f32>(u * inst.taille.x, v * inst.taille.y) / 16.0;
    out.couche = inst.couche;
    out.ombre = ombre_de(inst.face);
    return out;
}

@fragment
fn fs(e: Sortie) -> @location(0) vec4<f32> {
    let c = textureSample(atlas, echantillonneur, e.uv, i32(e.couche));
    // Une face texturée ne porte PLUS que l'ombrage dans sa couleur. Y ajouter
    // une teinte de bloc l'appliquerait deux fois, et tout le build sortirait
    // deux fois trop sombre.
    if c.a < 0.5 {
        discard;
    }
    return vec4<f32>(c.rgb * e.ombre, 1.0);
}
