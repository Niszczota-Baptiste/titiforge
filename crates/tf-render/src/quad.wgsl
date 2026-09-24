// Un quad par INSTANCE. Six indices, aucun sommet en mémoire : la géométrie se
// déduit de la face et de la taille. Une muraille de 64 × 40 blocs tient donc
// en 32 octets.

struct Camera {
    vue_projection: mat4x4<f32>,
    position: vec3<f32>,
    _pad: f32,
};

struct Origine {
    position: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var echantillonneur: sampler;
// Les origines ont leur PROPRE groupe : la table grandit quand des sections
// arrivent, et la relier ne doit pas obliger à relier l'atlas avec elle.
@group(1) @binding(0) var<storage, read> origines: array<Origine>;

struct Instance {
    // `x | y<<5 | z<<10 | (l−1)<<15 | (h−1)<<19 | face<<23`, en BLOCS et
    // LOCAL à la section. Seize octets par quad au lieu de trente-deux : sur
    // une région bâtie, l'arène passe de 132 Mo à 66. La fenêtre de résidence
    // est plafonnée en octets, donc c'est autant de monde en plus.
    @location(0) geo: u32,
    @location(1) couche: u32,
    // Un FACTEUR par canal, pas une couleur : 1 sur une face non teintée.
    @location(2) teinte: vec4<f32>,
    @location(3) section: u32,
};

struct Sortie {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) couche: u32,
    @location(2) ombre: f32,
    @location(3) monde: vec3<f32>,
    @location(4) teinte: vec3<f32>,
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

    let face = (inst.geo >> 23u) & 7u;
    // **Un TROU de l'arène** (`InstanceQuad::VIDE`) : aucune face n'a un rang
    // au-delà de 5. On rend un quad dégénéré — six sommets au même point,
    // aucun fragment. C'est ce qui permet de libérer une place sans recopier
    // le tableau, et de garder UN appel de dessin.
    if (face > 5u) {
        var vide: Sortie;
        vide.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        vide.uv = vec2<f32>(0.0);
        vide.couche = 0u;
        vide.ombre = 0.0;
        vide.monde = vec3<f32>(0.0);
        vide.teinte = vec3<f32>(0.0);
        return vide;
    }
    // Blocs → seizièmes, l'unité du mailleur et des modèles.
    let bloc = vec3<f32>(
        f32(inst.geo & 31u),
        f32((inst.geo >> 5u) & 31u),
        f32((inst.geo >> 10u) & 31u),
    );
    let taille = vec2<f32>(
        f32(((inst.geo >> 15u) & 15u) + 1u) * 16.0,
        f32(((inst.geo >> 19u) & 15u) + 1u) * 16.0,
    );
    let p = origines[inst.section].position.xyz + bloc * 16.0 + coin(face, taille, u, v);

    var out: Sortie;
    // Seizièmes → blocs.
    let monde = p / 16.0;
    out.clip = cam.vue_projection * vec4<f32>(monde, 1.0);
    out.monde = monde;
    // La texture se RÉPÈTE par bloc : un quad de 4 blocs montre quatre fois
    // sa texture. C'est pour ça que l'atlas est un TABLEAU — sur une planche,
    // la répétition mordrait sur la tuile voisine.
    out.uv = vec2<f32>(u * taille.x, v * taille.y) / 16.0;
    out.couche = inst.couche;
    out.ombre = ombre_de(face);
    out.teinte = inst.teinte.rgb;
    return out;
}

@fragment
fn fs(e: Sortie) -> @location(0) vec4<f32> {
    let c = textureSample(atlas, echantillonneur, e.uv, i32(e.couche));
    if c.a < 0.5 {
        discard;
    }
    // Deux facteurs, et un seul de chaque : l'OMBRAGE, qui dépend de la face,
    // et la TEINTE, qui dépend du bloc. La teinte vaut 1 partout où le modèle
    // ne déclare pas de `tintindex` ; là où il en déclare, elle porte déjà la
    // compensation du gris de la tuile, calculée une fois côté processeur.
    // L'appliquer aussi à une face non teintée assombrirait tout le build.
    return vec4<f32>(c.rgb * e.ombre * e.teinte, 1.0);
}
