// The lamp room's back wall, after `runtime_shader_prelude.wgsl` and
// `noise.wgsl`: a thick plaster plate, and every letter of the content a
// hole cut through it with a lamp behind. The content draws its letters in
// key colours: the headline red, the paragraph blue, a selection green. A
// selection knocks a ragged opening out of the plate and glazes it with
// cathedral glass; selected letters are clear glass.
//
// The plate's light comes from its own holes. Three blurred copies of the
// content, the renderer's substrates, spill their light over the plaster
// at three reaches and rake its relief from the side the light comes from;
// the light is smeared toward the lamp's image for the shafts in the dusty
// air. A hole shows the inner wall of the cut, dark at the lip and blazing
// at the back, and past it the lamp box. Holes and glass are drawn
// translucent, under half opaque: the scene's shadow map takes them as
// open, so the scene's light passes them onto the floor.
//
// Uniforms: 0..1 = the plate's size in logical pixels; 2 = pixels to the
// unit; 3 = device pixels to the logical pixel; 4..6 = the lamp's colour,
// its strength included, linear; 7 = how far the lamp has warmed up, from
// 0 to 1; 8..9 = the lamp's image on the wall; 10..11 = the point of the
// wall straight ahead of the eye, 12 = the eye's distance from the wall,
// 13 = the plate's thickness; 14 = the headline's bottom edge; 16..18 =
// the substrates' blur radii in logical pixels; 20..21 = where the eye sees
// the lamp through the wall.

const BARE_LAMP: vec3<f32> = vec3<f32>(1.0, 0.565, 0.225);
const NIGHT: vec3<f32> = vec3<f32>(0.010, 0.016, 0.030);
const HOLE_ALPHA: f32 = 0.45;
const PANE_SIZE: vec2<f32> = vec2<f32>(68.0, 54.0);
const WALL_TILE: f32 = 620.0;

fn param(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn read(region: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(input_texture));
    let whole = vec4<f32>(0.0, 0.0, dims);
    let r = select(whole, region, region.z > 0.0);
    let texel = clamp(r.xy + uv * r.zw, r.xy + vec2<f32>(0.5), r.xy + r.zw - vec2<f32>(0.5));
    return textureSampleLevel(input_texture, input_sampler, texel / dims, 0.0);
}

fn blurred(which: u32, uv: vec2<f32>) -> vec4<f32> {
    var region = u[58];
    if (which == 1u) {
        region = u[57];
    } else if (which == 2u) {
        region = u[56];
    }
    if (region.z <= 0.0) {
        region = u[59];
    }
    return read(region, uv);
}

fn letters_of(texel: vec4<f32>) -> f32 {
    return min(texel.r + texel.b, 1.0);
}

// The light a texel of the content lets through: the bare lamp through a
// letter, a glow of coloured glass through a selection.
fn emission(texel: vec4<f32>) -> vec3<f32> {
    return BARE_LAMP * letters_of(texel) + vec3<f32>(0.55, 0.42, 1.25) * texel.g * 0.8;
}

fn chamber(c: vec2<f32>, size: vec2<f32>) -> f32 {
    let hot = vec2<f32>(param(8u), param(9u));
    let r = length(c - hot) / size.y;
    return 0.17 + 1.15 / (1.0 + r * r * 5.5);
}

fn bulb(c: vec2<f32>, size: vec2<f32>) -> f32 {
    let r = length((c - vec2<f32>(param(8u), param(9u))) / size.y);
    return exp(-r * r * 30.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

// Plaster, trowelled: height and its slope at `p` in units.
fn plaster(p: vec2<f32>) -> vec3<f32> {
    let q = p / WALL_TILE;
    let h = value_noise(q * 6.0) * 0.5 + value_noise(q * 23.0 + vec2<f32>(5.0)) * 0.25 + value_noise(q * 90.0) * 0.12;
    let e = 1.0 / 512.0;
    let hx = value_noise((q + vec2<f32>(e, 0.0)) * 6.0) * 0.5 + value_noise((q + vec2<f32>(e, 0.0)) * 23.0 + vec2<f32>(5.0)) * 0.25 + value_noise((q + vec2<f32>(e, 0.0)) * 90.0) * 0.12;
    let hy = value_noise((q + vec2<f32>(0.0, e)) * 6.0) * 0.5 + value_noise((q + vec2<f32>(0.0, e)) * 23.0 + vec2<f32>(5.0)) * 0.25 + value_noise((q + vec2<f32>(0.0, e)) * 90.0) * 0.12;
    return vec3<f32>((vec2<f32>(hx, hy) - h) / e * 0.004, h);
}

// The pane of leaded glass at `c`: x = how strongly it absorbs red, y
// green, z blue; w = how far it is to the nearest lead line, in units.
fn pane_centre(cell: vec2<f32>) -> vec2<f32> {
    return 0.1 + 0.8 * vec2<f32>(hash(cell + vec2<f32>(3.0)), hash(cell + vec2<f32>(11.7, 3.0)));
}

// How strongly a pane absorbs red, green and blue: mostly blue and violet
// glass, some red, some gold.
fn pane_colour(pick: f32) -> vec3<f32> {
    if (pick < 0.40) {
        return vec3<f32>(4.2, 2.6, 0.40);
    }
    if (pick < 0.70) {
        return vec3<f32>(4.0, 3.5, 0.62);
    }
    if (pick < 0.79) {
        return vec3<f32>(2.4, 3.8, 0.95);
    }
    if (pick < 0.88) {
        return vec3<f32>(0.85, 5.6, 4.0);
    }
    if (pick < 0.95) {
        return vec3<f32>(0.42, 1.35, 4.4);
    }
    return vec3<f32>(3.0, 1.45, 2.1);
}

struct Pane {
    absorb: vec3<f32>,
    lead: f32,
    seed: f32,
}

fn pane_at(c: vec2<f32>, unit: f32) -> Pane {
    var q = c / unit;
    q += 5.0 * vec2<f32>(sin(q.y * 0.043 + q.x * 0.011), sin(q.x * 0.037 + 1.7));
    q /= PANE_SIZE;
    let base = floor(q);
    let f = q - base;
    var near_cell = vec2<f32>(0.0);
    var near_vec = vec2<f32>(0.0);
    var near_d = 9.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let cell = vec2<f32>(f32(x), f32(y));
            let r = cell + pane_centre(base + cell) - f;
            let d = dot(r, r);
            if (d < near_d) {
                near_d = d;
                near_vec = r;
                near_cell = cell;
            }
        }
    }
    var lead = 9.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let cell = near_cell + vec2<f32>(f32(x), f32(y));
            let r = cell + pane_centre(base + cell) - f;
            let across = r - near_vec;
            if (dot(across, across) > 1.0e-4) {
                lead = min(lead, dot(0.5 * (near_vec + r) * PANE_SIZE, normalize(across / PANE_SIZE)));
            }
        }
    }
    let id = base + near_cell;
    var pane: Pane;
    pane.absorb = pane_colour(hash(id + vec2<f32>(41.0)));
    pane.lead = lead;
    pane.seed = hash(id + vec2<f32>(7.3));
    return pane;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(param(0u), param(1u));
    let unit = param(2u);
    let px = 1.0 / max(param(3u), 0.5);
    let uv = input.uv;
    let p = uv * size;
    let lamp = vec3<f32>(param(4u), param(5u), param(6u));
    let warm = param(7u);

    let k = read(u[59], uv);
    let near = blurred(0u, uv);
    let mid = blurred(1u, uv);
    let far = blurred(2u, uv);
    let ink = letters_of(k);
    let hole = max(ink, 0.0);

    // The glow of the holes over the plaster, and which way it brightens.
    let glow = emission(near) * 0.12 + emission(mid) * 0.5 + emission(far) * 0.45;
    let reach = param(17u) * 0.7 / size;
    let level = vec3<f32>(0.35, 0.4, 0.25);
    let brighter = vec2<f32>(
        dot(emission(blurred(1u, uv + vec2<f32>(reach.x, 0.0))), level) - dot(emission(blurred(1u, uv - vec2<f32>(reach.x, 0.0))), level),
        dot(emission(blurred(1u, uv + vec2<f32>(0.0, reach.y))), level) - dot(emission(blurred(1u, uv - vec2<f32>(0.0, reach.y))), level),
    ) / (dot(emission(mid), level) * 0.55 + 0.004);
    let glow_slope = brighter / max(1.0, length(brighter));

    // Shafts in the dusty air. Along a ray from the eye the lamp shows
    // through the holes between the pixel and the lamp's image, so the
    // light in the air is the holes' light smeared toward that image.
    let image = vec2<f32>(param(20u), param(21u));
    var haze = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 16; i++) {
        let shrink = exp(-(f32(i) + 0.5) * 0.16);
        let w = pow(shrink, 1.9);
        haze += emission(blurred(1u, (image + (p - image) * shrink) / size)) * w;
        total += w;
    }
    let r = length(p - image) / size.y;
    let dust = 0.5 + mix(value_noise(p / (unit * 210.0)), value_noise(p / (unit * 91.0) + vec2<f32>(4.7)), 0.4);
    haze = haze / total * (0.30 + 1.0 / (1.0 + r * r * 1.6)) * dust * lamp * 1.6;
    haze = haze * haze / (haze + vec3<f32>(0.035));

    // The plaster, lit by the glow raking across its relief.
    let surface = plaster(p / unit);
    let albedo = 0.30 + 0.20 * (surface.z - 0.4);
    let raking = max(0.0, 1.0 + dot(surface.xy, glow_slope) * 0.55 + (surface.z - 0.4) * 0.5);
    let light = glow * (chamber(p, size) + 0.12) * raking * 1.55;
    var wall = albedo * (light * lamp + NIGHT);
    // The lip of the plate round every opening stays dark.
    let rim = max(letters_of(near), near.g * 0.6);
    wall *= 1.0 - 0.80 * smoothstep(0.02, 0.42, rim) * (1.0 - ink);
    let wall_color = to_srgb(aces((wall + haze) * 1.05));

    // The selection's opening, ragged along the leading.
    var opening = 0.0;
    var glass = vec3<f32>(0.0);
    let spread = near.g;
    if (spread > 0.012) {
        let pane = pane_at(p, unit);
        let cut = 0.035 + 0.30 * pane.seed + 0.12 * smoothstep(6.0, 0.0, pane.lead);
        opening = smoothstep(cut - 0.03, cut + 0.03, spread) * (1.0 - ink);
        let lead = 1.0 - smoothstep(0.8, 1.6, pane.lead);
        let thick = 0.98 + 0.40 * pane.seed + 0.5 * (value_noise(p / unit * vec2<f32>(0.03, 0.12)) - 0.5);
        let through = exp(-pane.absorb * max(thick, 0.35));
        glass = mix(through * (1.15 * chamber(p, size) + 0.55) * 1.4, vec3<f32>(0.012, 0.011, 0.013), lead);
    }

    var hole_color = vec3<f32>(0.0);
    if (hole > 0.002) {
        // The plate is thick: on the side away from the eye the inner wall
        // of the cut shows, dark at the lip and blazing at the back.
        let heading = 1.0 - smoothstep(0.0, unit * 12.0, p.y - param(14u));
        let eye = vec2<f32>(param(10u), param(11u));
        let shift = (p - eye) * param(13u) / param(12u) * mix(0.27, 1.0, heading) * vec2<f32>(1.0, 0.72);
        var clear = 1.0;
        var deep = 0.0;
        for (var i = 1; i <= 8; i++) {
            clear *= smoothstep(0.08, 0.78, letters_of(read(u[59], (p + shift * f32(i) / 8.0) / size)));
            deep += clear / 8.0;
        }
        let behind = p + shift * 2.2;
        let chamber_light = chamber(behind, size);
        let lamp_box = vec3<f32>(1.0, 0.60, 0.255) * (3.2 * chamber_light + 5.0 * bulb(behind, size));
        let facing = 0.62 + 0.38 * (1.0 + shift.y / (length(shift) + 0.001));
        var inner = mix(vec3<f32>(0.10, 0.024, 0.003), vec3<f32>(1.5, 0.50, 0.10), deep * deep * deep);
        inner *= (chamber_light * 1.15 + 0.12) * facing * mix(1.5, 1.0, heading);
        var seen = mix(inner, lamp_box, clear);
        // Selected letters are clear glass: bare white light.
        seen = mix(seen, vec3<f32>(1.0, 0.95, 0.88) * 4.4, smoothstep(0.2, 0.6, k.g + near.g * 0.5) * step(0.5, ink));
        hole_color = to_srgb(aces((seen * lamp * mix(1.5, 1.0, warm) + haze * 0.6) * 1.05));
    }
    let glass_color = to_srgb(aces((glass * lamp + haze * 0.4) * 1.05));

    // Translucent holes and glass let the scene's light through the plate.
    let open = max(hole, opening);
    let through_color = mix(glass_color, hole_color, hole / max(open, 0.001));
    let alpha = mix(1.0, HOLE_ALPHA, open);
    return vec4<f32>(mix(wall_color, through_color, open), alpha);
}
