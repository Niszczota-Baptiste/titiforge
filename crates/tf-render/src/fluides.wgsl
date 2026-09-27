// La passe des FLUIDES : une instance par face, six sommets, aucun en mémoire.
//
// Deux différences avec un quad glouton, et ce sont celles du jeu
// (`LiquidBlockRenderer`) :
//
// - le haut d'une face n'est pas plat : les quatre coins du dessus ont chacun
//   leur hauteur, et le haut d'un côté suit les deux coins qu'il touche ;
// - un côté est RENTRÉ d'un millième dans sa case, un dessus BAISSÉ d'autant,
//   un dessous LEVÉ d'autant — assez pour ne pas se battre avec la face d'un
//   bloc voisin, trop peu pour se voir.
//
// La même instance sert deux passes : la lave (opaque, avec la profondeur) et
// l'eau (translucide, sans écrire la profondeur, après tout le reste). Chaque
// passe écrase en triangle dégénéré ce qui n'est pas à elle.

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
// La table des origines est PARTAGÉE avec les deux autres passes.
@group(1) @binding(0) var<storage, read> origines: array<Origine>;

struct Instance {
    @location(0) geo: u32,
    @location(1) hauteurs: u32,
    @location(2) couche_angle: u32,
    @location(3) teinte: vec4<f32>,
    @location(4) section: u32,
};

struct Sortie {
    @builtin(position) clip: vec4<f32>,
    // En BLOCS le long du plan de la face : le fragment en tire la case et
    // sa position dedans, pour répéter la texture case par case.
    @location(0) plan: vec2<f32>,
    @location(1) @interpolate(flat) couche: u32,
    @location(2) ombre: f32,
    @location(3) teinte: vec3<f32>,
    // 0 : répétée par case (dessus immobile, dessous). 1 : un côté (courant
    // ou voile). 2 : un dessus qui court — uv déjà tournées au sommet.
    @location(4) @interpolate(flat) genre_uv: u32,
    @location(5) uv: vec2<f32>,
};

// L'ombrage du jeu pour un fluide (`getShade`) : le même que pour un bloc.
fn ombre_de(face: u32) -> f32 {
    switch face {
        case 0u, 1u: { return 0.6; }   // ±X
        case 2u:     { return 0.5; }   // −Y
        case 3u:     { return 1.0; }   // +Y
        default:     { return 0.8; }   // ±Z
    }
}

const EPS: f32 = 0.001;

fn vide() -> Sortie {
    var s: Sortie;
    s.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    s.plan = vec2<f32>(0.0);
    s.couche = 0u;
    s.ombre = 0.0;
    s.teinte = vec3<f32>(0.0);
    s.genre_uv = 0u;
    s.uv = vec2<f32>(0.0);
    return s;
}

fn hauteur(inst: Instance, k: u32) -> f32 {
    return f32((inst.hauteurs >> (8u * k)) & 255u) / 255.0;
}

fn sommet(inst: Instance, i: u32) -> Sortie {
    let face = (inst.geo >> 23u) & 7u;
    if (face > 5u) {
        return vide();
    }
    // Deux triangles, et la diagonale du jeu pour un dessus : de (x0, z0) à
    // (x1, z1). Un dessus dont les coins diffèrent n'est pas plan, et l'autre
    // diagonale le plierait autrement.
    var a = vec2<u32>(0u, 0u);
    switch i {
        case 0u, 3u: { a = vec2<u32>(0u, 0u); }
        case 1u:     { a = vec2<u32>(0u, 1u); }
        case 2u, 4u: { a = vec2<u32>(1u, 1u); }
        default:     { a = vec2<u32>(1u, 0u); }
    }
    let p0 = vec3<f32>(
        f32(inst.geo & 31u),
        f32((inst.geo >> 5u) & 31u),
        f32((inst.geo >> 10u) & 31u),
    );
    // La taille dans les deux axes du plan, dans l'ordre CROISSANT des axes.
    let t = vec2<f32>(
        f32(((inst.geo >> 15u) & 15u) + 1u),
        f32(((inst.geo >> 19u) & 15u) + 1u),
    );
    let texture = (inst.geo >> 27u) & 3u;
    var p = p0;
    var plan = vec2<f32>(0.0);
    var uv = vec2<f32>(0.0);
    var genre_uv = 0u;
    let fa = vec2<f32>(a);
    if (face == 3u) {
        // Le dessus : plan (X, Z). Coins [NO, SO, SE, NE].
        var k = 0u;
        if (a.x == 0u && a.y == 1u) { k = 1u; }
        if (a.x == 1u && a.y == 1u) { k = 2u; }
        if (a.x == 1u && a.y == 0u) { k = 3u; }
        p = p0 + vec3<f32>(fa.x * t.x, hauteur(inst, k) - EPS, fa.y * t.y);
        plan = fa * t;
        if (texture == 1u) {
            // Un courant : la texture tournée du sens de l'eau, sa moitié
            // centrale, comme le jeu la prend.
            let angle = f32(inst.couche_angle >> 16u) / 65536.0 * 6.2831855;
            let c = cos(angle) * 0.25;
            let s = sin(angle) * 0.25;
            let q = 2.0 * fa - vec2<f32>(1.0);
            uv = vec2<f32>(0.5) + vec2<f32>(c * q.x + s * q.y, -s * q.x + c * q.y);
            genre_uv = 2u;
        }
    } else if (face == 2u) {
        // Le dessous : plat, levé d'un millième.
        p = p0 + vec3<f32>(fa.x * t.x, EPS, fa.y * t.y);
        plan = fa * t;
    } else {
        // Un côté. `a.x` choisit le bout le long de l'axe horizontal, `a.y`
        // le bas ou le haut. Le haut suit la hauteur du coin de ce bout, au
        // sommet de la dernière rangée.
        let axe = face >> 1u;
        let positif = (face & 1u) == 1u;
        // Pour ±X le plan est (Y, Z) : les rangées sont le PREMIER axe.
        var rangees = t.y;
        var longueur = t.x;
        if (axe == 0u) {
            rangees = t.x;
            longueur = t.y;
        }
        let haut = rangees - 1.0 + hauteur(inst, a.x);
        let y = fa.y * haut;
        let h = fa.x * longueur;
        var rentre = EPS;
        if (positif) { rentre = 1.0 - EPS; }
        if (axe == 0u) {
            p = p0 + vec3<f32>(rentre, y, h);
        } else {
            p = p0 + vec3<f32>(h, y, rentre);
        }
        plan = vec2<f32>(h, y);
        genre_uv = 1u;
    }
    let monde = origines[inst.section].position.xyz / 16.0 + p;
    var out: Sortie;
    out.clip = cam.vue_projection * vec4<f32>(monde, 1.0);
    out.plan = plan;
    out.couche = inst.couche_angle & 65535u;
    out.ombre = ombre_de(face);
    out.teinte = inst.teinte.rgb;
    out.genre_uv = genre_uv;
    out.uv = uv;
    return out;
}

// La lave : opaque, avec la profondeur. L'eau s'écrase en trou.
@vertex
fn vs_opaque(inst: Instance, @builtin(vertex_index) i: u32) -> Sortie {
    if (((inst.geo >> 26u) & 1u) == 0u) {
        return vide();
    }
    return sommet(inst, i);
}

// L'eau : translucide, après tout le reste. La lave s'écrase en trou.
@vertex
fn vs_translucide(inst: Instance, @builtin(vertex_index) i: u32) -> Sortie {
    if (((inst.geo >> 26u) & 1u) == 1u) {
        return vide();
    }
    return sommet(inst, i);
}

// **Un seul échantillonnage, en contrôle UNIFORME.** Les dérivées ne se
// prennent que là où tous les fragments d'un carré passent ensemble : on les
// prend d'abord, on choisit ensuite les coordonnées selon la face, et on
// échantillonne une fois avec des dérivées explicites.
fn texel(e: Sortie) -> vec4<f32> {
    let dx_plan = dpdx(e.plan);
    let dy_plan = dpdy(e.plan);
    let dx_uv = dpdx(e.uv);
    let dy_uv = dpdy(e.uv);
    // Répétée par case : un dessus immobile, un dessous.
    var uv = e.plan;
    var dx = dx_plan;
    var dy = dy_plan;
    if (e.genre_uv == 2u) {
        // Un dessus qui court : uv tournées au sommet.
        uv = e.uv;
        dx = dx_uv;
        dy = dy_uv;
    } else if (e.genre_uv == 1u) {
        // Un côté : la moitié de la texture de courant par case, comme le
        // jeu — u de 0 à 1/2 le long de la case, v de 1/2 en BAS jusqu'au
        // haut de l'eau. Les dérivées viennent de la coordonnée CONTINUE :
        // prises sur `fract`, elles sauteraient à chaque bord de case et
        // feraient une ligne du plus petit mip.
        uv = vec2<f32>(fract(e.plan.x) * 0.5, 0.5 - fract(e.plan.y) * 0.5);
        dx = dx_plan * 0.5;
        dy = dy_plan * 0.5;
    }
    return textureSampleGrad(atlas, echantillonneur, uv, i32(e.couche), dx, dy);
}

@fragment
fn fs_opaque(e: Sortie) -> @location(0) vec4<f32> {
    let c = texel(e);
    return vec4<f32>(c.rgb * e.ombre * e.teinte, 1.0);
}

@fragment
fn fs_translucide(e: Sortie) -> @location(0) vec4<f32> {
    let c = texel(e);
    if (c.a < 0.01) {
        discard;
    }
    return vec4<f32>(c.rgb * e.ombre * e.teinte, c.a);
}
