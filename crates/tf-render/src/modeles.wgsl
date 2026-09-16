// La passe de MODÈLES : une instance par FACE, une pose par bloc.
//
// La géométrie vit une seule fois par état, dans `faces`. Un bloc posé ne
// porte que seize octets. Le sommet recolle les deux.
//
// Chaque pose a un nombre de faces différent — six pour une dalle, jusqu'à 492
// pour le pire bloc du serveur — donc « n faces par instance » n'existe pas. La
// pose porte le RANG de sa première face dans le flot global, et le sommet
// retrouve sa pose par dichotomie : un seul appel de dessin, zéro octet par
// face.

struct Camera {
    vue_projection: mat4x4<f32>,
    position: vec3<f32>,
    _pad: f32,
};

// **Un `vec3<f32>` s'aligne sur seize octets en WGSL.** Les bornes sont donc
// des `vec4` des deux côtés : la correspondance avec la structure Rust se LIT,
// au lieu de reposer sur des règles de bourrage. Un test compare les tailles.
struct FaceModele {
    min: vec4<f32>,
    max: vec4<f32>,
    uv: vec4<f32>,
    face: u32,
    couche: u32,
    teinte: u32,
    cullable: u32,
};

struct Pose {
    // x | y << 8 | z << 16 | voisins_opaques << 24
    local: u32,
    section: u32,
    debut_face: u32,
    debut_modele: u32,
};

struct Origine {
    position: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var echantillonneur: sampler;
@group(1) @binding(0) var<storage, read> faces: array<FaceModele>;
@group(1) @binding(1) var<storage, read> poses: array<Pose>;
@group(1) @binding(2) var<storage, read> origines: array<Origine>;

struct Sortie {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) couche: u32,
    @location(2) ombre: f32,
    @location(3) teinte: vec3<f32>,
};

// Le même ombrage que la passe gloutonne, et pour la même raison : la face
// NÉGATIVE d'abord. Deux tables divergeraient, et un escalier sortirait éclairé
// à l'envers de la marche qu'il touche.
fn ombre_de(face: u32) -> f32 {
    switch face {
        case 0u, 1u: { return 0.6; }
        case 2u:     { return 0.5; }
        case 3u:     { return 1.0; }
        default:     { return 0.8; }
    }
}

/// La pose dont la face globale `f` fait partie.
///
/// `poses[p].debut_face` est croissant : on cherche le dernier rang dont le
/// début est ≤ f. Vingt itérations sur un million de poses.
fn pose_de(f: u32) -> u32 {
    var lo = 0u;
    var hi = arrayLength(&poses);
    loop {
        if (lo + 1u >= hi) { break; }
        let mid = lo + (hi - lo) / 2u;
        if (poses[mid].debut_face <= f) { lo = mid; } else { hi = mid; }
    }
    return lo;
}

fn decompresse(t: u32) -> vec3<f32> {
    return vec3<f32>(
        f32((t >> 0u) & 255u) / 255.0,
        f32((t >> 8u) & 255u) / 255.0,
        f32((t >> 16u) & 255u) / 255.0,
    );
}

@vertex
fn vs(@builtin(vertex_index) i: u32, @builtin(instance_index) inst: u32) -> Sortie {
    let p = poses[pose_de(inst)];
    let f = faces[p.debut_modele + (inst - p.debut_face)];

    var out: Sortie;
    out.couche = f.couche;
    out.ombre = ombre_de(f.face);
    out.teinte = decompresse(f.teinte);

    // Masquage : la face porte `cullface`, elle touche le bord du bloc (les
    // deux ont été pesés côté processeur, une fois par état), et le voisin de
    // ce côté est opaque. On rend alors un quad DÉGÉNÉRÉ — rien à dessiner,
    // et rien à brancher : un `discard` coûterait le fragment.
    let voisins = (p.local >> 24u) & 63u;
    if (f.cullable != 0u && (voisins & (1u << f.face)) != 0u) {
        out.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out.uv = vec2<f32>(0.0);
        return out;
    }

    let axe = f.face >> 1u;
    let positif = (f.face & 1u) == 1u;
    // Les deux axes du plan, dans l'ordre CROISSANT — le même que la passe
    // gloutonne. Les échanger retournerait deux faces sur six.
    var au = 0u;
    var av = 0u;
    switch axe {
        case 0u:     { au = 1u; av = 2u; }   // ±X : plan (Y, Z)
        case 1u:     { au = 0u; av = 2u; }   // ±Y : plan (X, Z)
        default:     { au = 0u; av = 1u; }   // ±Z : plan (X, Y)
    }
    let u = f32(i & 1u);
    let v = f32((i >> 1u) & 1u);

    // `var` et non `let` : WGSL n'indexe un tableau par une VARIABLE que s'il
    // est en mémoire de fonction. Un `let` est une constante, et l'indexer par
    // `axe` ne compile pas.
    var mn = array<f32, 3>(f.min.x, f.min.y, f.min.z);
    var mx = array<f32, 3>(f.max.x, f.max.y, f.max.z);
    var coin = array<f32, 3>(0.0, 0.0, 0.0);
    coin[axe] = select(mn[axe], mx[axe], positif);
    coin[au] = mix(mn[au], mx[au], u);
    coin[av] = mix(mn[av], mx[av], v);

    let bloc = vec3<f32>(
        f32((p.local >> 0u) & 255u),
        f32((p.local >> 8u) & 255u),
        f32((p.local >> 16u) & 255u),
    );
    let seiziemes = origines[p.section].position.xyz
        + bloc * 16.0
        + vec3<f32>(coin[0], coin[1], coin[2]);
    out.clip = cam.vue_projection * vec4<f32>(seiziemes / 16.0, 1.0);

    // Les uv du modèle, en seizièmes, ramenées en 0..1. Une dalle montre ainsi
    // la MOITIÉ BASSE de sa texture, et non la texture entière écrasée sur huit
    // seizièmes.
    out.uv = vec2<f32>(mix(f.uv.x, f.uv.z, u), mix(f.uv.y, f.uv.w, v)) / 16.0;
    return out;
}

@fragment
fn fs(e: Sortie) -> @location(0) vec4<f32> {
    let c = textureSample(atlas, echantillonneur, e.uv, i32(e.couche));
    // Un bloc-modèle est fait de textures TROUÉES — une grille, un feuillage,
    // un barreau de clôture. Sans le rejet, la partie transparente sortirait
    // en noir et chaque escalier porterait un carré opaque.
    if c.a < 0.5 {
        discard;
    }
    return vec4<f32>(c.rgb * e.ombre * e.teinte, 1.0);
}
