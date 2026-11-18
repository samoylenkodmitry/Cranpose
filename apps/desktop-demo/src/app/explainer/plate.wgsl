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
// at the back, and past it the lamp box. An opening's alpha says how open
// the plate is there, for the floor, which shows the same picture and
// reads the light that comes through.
//
// The picture is the whole stage; the floor hides its band below the wall
// and reads it (see `floor_band`).
//
// Uniforms: 0..1 = the wall's size in logical pixels; 2 = pixels to the
// unit; 3 = device pixels to the logical pixel; 4..6 = the lamp's colour,
// its strength included, linear; 7 = how far the lamp has warmed up, from
// 0 to 1; 8..9 = the lamp's image on the wall; 10..11 = the point of the
// wall straight ahead of the eye, 12 = the eye's distance from the wall,
// 13 = the plate's thickness; 14 = the headline's bottom edge; 15 = how
// opaque a bare opening is, 22 = an opening of glass; 16..18 = the
// substrates' blur radii in logical pixels; 19 = the stage's height;
// 20..21 = where the eye sees the lamp through the wall.

const BARE_LAMP: vec3<f32> = vec3<f32>(1.0, 0.565, 0.225);
const GLASS_GAIN: vec3<f32> = vec3<f32>(3.2, 4.0, 5.6);
// The light the panes let through on average: each colour of the palette
// by its share, at the panes' average thickness.
const GLASS_LIGHT: vec3<f32> = vec3<f32>(0.089, 0.050, 0.451) * GLASS_GAIN;
const CLEAR_GLASS: vec3<f32> = vec3<f32>(0.75, 0.72, 0.78);
// A selected letter seen from the room: bare white light.
const CLEAR_LETTER: vec3<f32> = vec3<f32>(1.0, 0.95, 0.88) * 4.4;
const NIGHT: vec3<f32> = vec3<f32>(0.010, 0.016, 0.030);
const PANE_SIZE: vec2<f32> = vec2<f32>(68.0, 54.0);
const GOLDEN_ANGLE: f32 = 2.399963;
// Below this blurred selection the plate stands whole.
const MIN_SPREAD: f32 = 0.012;
// The plaster's slope against the reference's baked relief.
const RELIEF: f32 = 18.0;
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
// letter; through a selection, the stained glass's light between the
// letters and clear glass in them. Letters cover about a quarter of a
// selection, so its key colour covers the rest.
fn emission(texel: vec4<f32>) -> vec3<f32> {
    let letters = letters_of(texel);
    let selected = min(texel.g / 0.75, 1.0);
    let glazed = GLASS_LIGHT * (1.0 - letters) + CLEAR_GLASS * letters;
    return BARE_LAMP * letters * (1.0 - selected) + glazed * selected;
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

// How strongly a pane absorbs red, green and blue: blue, deep blue and
// violet glass, and one pane in eight gold.
fn pane_colour(pick: f32) -> vec3<f32> {
    if (pick < 0.40) {
        return vec3<f32>(4.2, 2.6, 0.40);
    }
    if (pick < 0.70) {
        return vec3<f32>(4.0, 3.5, 0.62);
    }
    if (pick < 0.88) {
        return vec3<f32>(2.4, 3.8, 0.95);
    }
    return vec3<f32>(0.42, 1.35, 4.4);
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

// How much of the pixel at `c` a lead line covers. The lines vary a
// little in width.
fn lead_cover(pane: Pane, c: vec2<f32>, unit: f32, aa: f32) -> f32 {
    let width = 1.0 + 0.7 * value_noise(c / unit * 0.06);
    return 1.0 - smoothstep(width - aa, width + aa, pane.lead);
}

// The light the glass lets through at `c` (rgb) and how much of the pixel
// is lead (a). The thickness varies from pane to pane, with drawn streaks,
// and swells toward the lead.
fn glass_at(pane: Pane, c: vec2<f32>, unit: f32, aa: f32) -> vec4<f32> {
    let q = c / unit;
    let turn = pane.seed * 6.0;
    let streak = vec2<f32>(cos(turn) * q.x - sin(turn) * q.y, sin(turn) * q.x + cos(turn) * q.y);
    var thick = 0.98 + 0.40 * pane.seed;
    thick += 0.60 * (value_noise(streak * vec2<f32>(0.030, 0.12)) - 0.5);
    thick += 0.40 * (value_noise(streak * vec2<f32>(0.10, 0.30) + vec2<f32>(9.0)) - 0.5);
    thick += 0.25 * smoothstep(7.0, 0.0, pane.lead);
    return vec4<f32>(exp(-pane.absorb * max(thick, 0.35)), lead_cover(pane, c, unit, aa));
}

// The opening a selection knocks out of the plate. `spread` is the blurred
// selection; cut at a level that differs from pane to pane, the opening
// follows the leading. x = how much of the pixel is open, y = a wider rim
// round it, z = how far inside the cut the pixel lies.
fn opening_at(spread: f32, pane: Pane, c: vec2<f32>, unit: f32) -> vec3<f32> {
    let level = 0.035 + 0.30 * pane.seed + 0.05 * value_noise(c / unit * 0.07) + 0.12 * smoothstep(6.0, 0.0, pane.lead);
    return vec3<f32>(smoothstep(level - 0.03, level + 0.03, spread), smoothstep(level * 0.45, level, spread), spread - level);
}

// The stained glass seen in an opening at `c`, before the lamp's colour:
// the pane lit by the lamp box (`box`), seeded and rippled; its lead lit
// along the upper left edge; darker right at the cut (`cut` from the
// opening) and round the clear letters (`near_letters`, blurred).
fn stained(pane: Pane, c: vec2<f32>, unit: f32, aa: f32, relief: vec2<f32>, cut: f32, near_letters: f32, box: f32) -> vec3<f32> {
    let glass = glass_at(pane, c, unit, aa);
    let lit = c - vec2<f32>(1.1, 1.4) * unit;
    let lead_lit = lead_cover(pane_at(lit, unit), lit, unit, 0.9);
    let seeds = smoothstep(0.62, 0.95, value_noise(c / unit * vec2<f32>(0.34, 0.42) + relief * 0.8)) * 0.28;
    let ripple = 1.0 + 0.3 * relief.x + 0.2 * relief.y;
    var light = glass.rgb * (1.05 + seeds) * ripple * (1.15 * box + 0.55) * 0.74;
    let lead = vec3<f32>(0.012, 0.011, 0.013) + vec3<f32>(0.10, 0.09, 0.085) * max(0.0, lead_lit - glass.a) * 0.6;
    light = mix(light, lead, glass.a);
    light *= mix(0.30, 1.0, smoothstep(0.0, 0.12, cut));
    return light * (1.0 - 0.72 * smoothstep(0.06, 0.5, near_letters));
}

// The glow of the holes over the plaster at `uv`, from the content's three
// blurred copies.
fn glow_at(uv: vec2<f32>) -> vec3<f32> {
    return emission(blurred(0u, uv)) * 0.12 + emission(blurred(1u, uv)) * 0.5 + emission(blurred(2u, uv)) * 0.45;
}

// The light in the dusty air at `p`, the lamp's colour included. Along a
// ray from the eye the lamp shows through the holes between the pixel and
// the lamp's image, so the light in the air is the holes' light smeared
// toward that image.
fn haze_at(p: vec2<f32>, content: vec2<f32>, wall: vec2<f32>, unit: f32, lamp: vec3<f32>) -> vec3<f32> {
    let image = vec2<f32>(param(20u), param(21u));
    var haze = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 16; i++) {
        let shrink = exp(-(f32(i) + 0.5) * 0.16);
        let w = pow(shrink, 1.9);
        haze += emission(blurred(1u, (image + (p - image) * shrink) / content)) * w;
        total += w;
    }
    let r = length(p - image) / wall.y;
    let dust = 0.5 + mix(value_noise(p / (unit * 210.0)), value_noise(p / (unit * 91.0) + vec2<f32>(4.7)), 0.4);
    haze = haze / total * (0.30 + 1.0 / (1.0 + r * r * 1.6)) * dust * lamp * 1.6;
    return haze * haze / (haze + vec3<f32>(0.035));
}

// Below the wall the picture holds what the floor adds to the lamp's
// light there, which the floor reads at its own point of the screen: the
// air in front of it, and the lit wall mirrored in its polish. The mirror
// is a soft lobe that widens away from the wall, taller than wide, and
// strongest where the eye grazes the floor.
fn floor_band(p: vec2<f32>, content: vec2<f32>, wall: vec2<f32>, unit: f32, lamp: vec3<f32>) -> vec4<f32> {
    let haze = haze_at(p, content, wall, unit, lamp);
    let below = p.y - wall.y;
    let mirrored = vec2<f32>(p.x, wall.y - below);
    let lobe = vec2<f32>(5.0 * unit + below * 0.30, 4.0 * unit + below * 0.62);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 12; i++) {
        let f = (f32(i) + 0.5) / 12.0;
        let a = f32(i) * GOLDEN_ANGLE;
        let w = exp(-f * 2.4);
        let q = mirrored + sqrt(f) * vec2<f32>(cos(a), sin(a)) * lobe;
        sum += emission(blurred(1u, q / content)) * chamber(q, wall) * w;
        total += w;
    }
    let ahead = vec2<f32>(param(10u), param(11u));
    let distance = param(12u);
    let eye_height = wall.y - ahead.y;
    let away = distance / ((p.y - ahead.y) / eye_height);
    let side = p.x - ahead.x;
    let cosine = eye_height / sqrt(eye_height * eye_height + away * away + side * side * 0.3);
    let fresnel = 0.10 + 0.90 * pow(1.0 - cosine, 3.4);
    let mirror = (sum / total * 3.0 + glow_at(mirrored / content) * 0.30) * fresnel;
    return vec4<f32>(to_srgb(aces((haze * (1.0 + 0.6 * fresnel) + mirror * lamp * 0.8) * 1.05)), 1.0);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let content = vec2<f32>(param(0u), param(19u));
    let size = vec2<f32>(param(0u), param(1u));
    let unit = param(2u);
    let px = 1.0 / max(param(3u), 0.5);
    let uv = input.uv;
    let p = uv * content;
    let lamp = vec3<f32>(param(4u), param(5u), param(6u));
    let warm = param(7u);
    if (p.y > size.y) {
        return floor_band(p, content, size, unit, lamp);
    }

    let k = read(u[59], uv);
    let near = blurred(0u, uv);
    let mid = blurred(1u, uv);
    let ink = letters_of(k);

    // The glow of the holes over the plaster, and which way it brightens.
    let glow = glow_at(uv);
    let reach = param(17u) * 0.7 / content;
    let level = vec3<f32>(0.35, 0.4, 0.25);
    let brighter = vec2<f32>(
        dot(emission(blurred(1u, uv + vec2<f32>(reach.x, 0.0))), level) - dot(emission(blurred(1u, uv - vec2<f32>(reach.x, 0.0))), level),
        dot(emission(blurred(1u, uv + vec2<f32>(0.0, reach.y))), level) - dot(emission(blurred(1u, uv - vec2<f32>(0.0, reach.y))), level),
    ) / (dot(emission(mid), level) * 0.55 + 0.004);
    let glow_slope = brighter / max(1.0, length(brighter));
    let haze = haze_at(p, content, size, unit, lamp);

    // The plaster, lit by the glow raking across its relief.
    let surface = plaster(p / unit);
    let albedo = 0.30 + 0.20 * (surface.z - 0.4);
    let raking = max(0.0, 1.0 + dot(surface.xy, glow_slope) * 0.55 + (surface.z - 0.4) * 0.5);
    let light = glow * (chamber(p, size) + 0.12) * raking * 1.55;
    var wall = albedo * (light * lamp + NIGHT);

    // The selection's opening, ragged along the leading.
    let spread = near.g * 0.6 + mid.g * 0.4;
    var pane: Pane;
    var opening = vec3<f32>(0.0);
    if (spread > MIN_SPREAD) {
        pane = pane_at(p, unit);
        opening = opening_at(spread, pane, p, unit);
    }
    // The lip of the plate round every opening stays dark.
    let rim = max(letters_of(near), opening.y * 0.6);
    wall *= 1.0 - 0.80 * smoothstep(0.02, 0.42, rim) * (1.0 - ink);
    let wall_color = to_srgb(aces((wall + haze) * 1.05));

    let clear_letters = ink * smoothstep(0.15, 0.4, near.g);
    let glass_share = opening.x * (1.0 - ink);
    let hole = max(opening.x, ink);
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
            clear *= smoothstep(0.08, 0.78, letters_of(read(u[59], (p + shift * f32(i) / 8.0) / content)));
            deep += clear / 8.0;
        }
        let behind = p + shift * 2.2;
        let chamber_light = chamber(behind, size);
        let lamp_box = vec3<f32>(1.0, 0.60, 0.255) * (3.2 * chamber_light + 5.0 * bulb(behind, size));
        let facing = 0.62 + 0.38 * (1.0 + shift.y / (length(shift) + 0.001));
        var inner = mix(vec3<f32>(0.10, 0.024, 0.003), vec3<f32>(1.5, 0.50, 0.10), deep * deep * deep);
        inner *= (chamber_light * 1.15 + 0.12) * facing * mix(1.5, 1.0, heading);
        var seen = mix(inner, lamp_box, clear);
        if (opening.x > 0.002 || clear_letters > 0.0) {
            if (spread <= MIN_SPREAD) {
                pane = pane_at(p, unit);
            }
            let glass = stained(pane, p, unit, 0.7 * px / unit, surface.xy * RELIEF, opening.z, letters_of(near), chamber(p, size));
            seen = mix(seen, glass, min(1.0, glass_share / hole));
            seen = mix(seen, CLEAR_LETTER, clear_letters);
        }
        hole_color = to_srgb(aces((seen * lamp * mix(1.5, 1.0, warm) + haze * 0.6) * 1.05));
    }

    // The alpha tells the floor how open the plate is and how much of the
    // opening is glass.
    let hole_alpha = param(15u);
    let alpha = 1.0 - hole * (1.0 - hole_alpha) - glass_share * (hole_alpha - param(22u));
    return vec4<f32>(mix(wall_color, hole_color, hole), alpha);
}
