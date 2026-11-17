// A sheet of cotton paper in the sun on a dark table, after `noise.wgsl`.
// The content is the sheet's print: the headline, the chosen chip and the
// button are pressed into the paper, small type is printed flat, a red
// accent is pressed in as a plate and the slider's thumb is a raised disc
// of red wax. The relief is read from the content around each point, so it
// follows the words wherever they lay out. The scene's sun lights the
// paper; the leaves of a tree over it, which the camera never sees, shade
// it through the scene's shadows.
//
// The content is painted in key colours on a white ground: black ink,
// green accent, yellow blind (pressed without ink), cyan wax and magenta a
// selection.
//
// params: 0..1 = the stage's size in pixels; 2 = the stage's unit, 1 on a
// 1600 by 1000 stage; 3 = how far in from the stage's edges the sheet
// lies; 4 = where the headline ends, below which type is printed flat; 5 =
// how far new words are pressed in, from 0 to 1; 6 = how wet the fresh ink
// is; 7 = 1 when the script starts on the right; 8 = how strong the sun is.

// On the sheet, away from the sun: shadows fall this way.
const SUN: vec2<f32> = vec2<f32>(0.8, 0.6);
// A relief h high throws a shadow h * SUN_COT long.
const SUN_COT: f32 = 2.0;
const PI: f32 = 3.14159265;

struct Print {
    ink: f32,
    accent: f32,
    blind: f32,
    wax: f32,
    selection: f32,
}

// The print in `color`. Ink is how dark it is; what lies under the ink is
// told by the channels its colour lacks: green lacks red and blue, yellow
// blue, cyan red and magenta green.
fn print_of(color: vec4<f32>) -> Print {
    let rgb = color.rgb / max(color.a, 1.0 / 255.0);
    let brightest = max(rgb.r, max(rgb.g, rgb.b));
    let ground = rgb / max(brightest, 1.0e-3);
    var print: Print;
    print.ink = 1.0 - brightest;
    let missing = (vec3<f32>(1.0) - ground) * smoothstep(0.97, 0.75, print.ink);
    print.accent = min(missing.r, missing.b);
    print.blind = missing.b - print.accent;
    print.wax = missing.r - print.accent;
    print.selection = missing.g;
    return print;
}

fn print_at(point: vec2<f32>, size: vec2<f32>) -> Print {
    return print_of(content_color(point / size));
}

// The height of the printed sheet at `point`, in pixels, below zero where
// it is pressed in: inked headline type, the plates of colour and the
// blind outlines go down, wax stands up.
fn relief_at(point: vec2<f32>, size: vec2<f32>, headline_end: f32, depth: f32) -> f32 {
    let print = print_at(point, size);
    let headline = step(point.y, headline_end);
    let pressed = print.ink * headline + print.accent * 1.4 + print.blind * (1.0 - print.accent) * 0.9;
    return -depth * pressed + depth * 2.6 * print.wax;
}

fn noise0(p: vec2<f32>) -> f32 {
    return value_noise(p) - 0.5;
}

// The torn (deckle) edge: distance inside the sheet in pixels.
fn deckle(p: vec2<f32>, sheet: vec4<f32>, u: f32) -> f32 {
    let q = min(p - sheet.xy, sheet.zw - p);
    let d = min(q.x, q.y);
    if (d > 34.0 * u) {
        return d;
    }
    let ragged = noise0(p / (120.0 * u)) * 8.0 + noise0(p / (31.0 * u) + vec2<f32>(7.0)) * 4.5
        + noise0(p / (9.0 * u) + vec2<f32>(3.0)) * 2.2 + noise0(p / (2.6 * u)) * 1.6;
    return d + ragged * u * 0.62;
}

// The paper's tooth: soft clouds of pulp at four scales.
fn tooth_at(p: vec2<f32>) -> f32 {
    let warp = vec2<f32>(value_noise(p * 0.02), value_noise(p * 0.02 + vec2<f32>(9.5))) * 12.0;
    let flocks = value_noise((p + warp) * 0.033);
    return flocks * flocks * 1.25 + value_noise((p - warp) * 0.08) * 0.85 + value_noise(p * 0.19) * 0.5
        + value_noise(p * 0.41) * 0.22;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let u = shaded_param(2u);
    let inset = shaded_param(3u);
    let headline_end = shaded_param(4u);
    let pressed_in = shaded_param(5u);
    let wet_ink = shaded_param(6u);
    let rtl = shaded_param(7u);
    let sun_strength = shaded_param(8u);
    let p = surface.uv * size;
    let centred = (p - 0.5 * size) / sqrt(size.x * size.y);
    let depth = max(0.7, u) * 1.2;

    // ---- paper: the tooth and its slope, and stray fibres
    let tooth = tooth_at(p) / 2.8;
    let grain_slope = vec2<f32>(tooth_at(p + vec2<f32>(1.0, 0.0)) - tooth_at(p - vec2<f32>(1.0, 0.0)),
        tooth_at(p + vec2<f32>(0.0, 1.0)) - tooth_at(p - vec2<f32>(0.0, 1.0))) * 0.16;
    let fibre = smoothstep(0.55, 0.8,
        value_noise(vec2<f32>(p.x * 0.16 + p.y * 0.05, p.y * 0.9) + vec2<f32>(40.0))
            * value_noise(p.yx * vec2<f32>(0.7, 0.12) + vec2<f32>(11.0)));

    // ---- the sheet's gentle waves from being handled
    let wave_a = centred.x * 3.1 + centred.y * 2.0 + 0.7 * sin(centred.y * 3.7 + 1.0);
    let wave_b = centred.x * -2.2 + centred.y * 4.6 + 0.6 * sin(centred.x * 4.1);
    let wave_c = centred.x * 6.8 + centred.y * 1.3 + 2.2;
    let wave_slope = (cos(wave_a) * vec2<f32>(3.1, 2.0) * 0.5 + cos(wave_b) * vec2<f32>(-2.2, 4.6) * 0.35
        + cos(wave_c) * vec2<f32>(6.8, 1.3) * 0.16) * 0.018;

    // ---- new words are pressed in from the side the script starts on,
    // down the sheet, the high grain taking ink first
    let from_start = mix(p.x, size.x - p.x, rtl) / size.x;
    let delay = 0.30 * (0.7 * p.y / size.y + 0.3 * from_start);
    let t = clamp(pressed_in * 1.32 - delay, 0.0, 1.0);
    let press = 1.0 - pow(1.0 - clamp(t / 0.42, 0.0, 1.0), 3.0) + 0.16 * sin(PI * smoothstep(0.35, 1.0, t));
    let grain = clamp(tooth * 1.15 - 0.1 + fibre * 0.25, 0.0, 1.0);
    let ink_in = smoothstep(grain * 0.5, grain * 0.5 + 0.5, (t - 0.1) / 0.62);
    let wet = max(wet_ink, sin(PI * clamp((t - 0.2) / 0.8, 0.0, 1.0)) * step(pressed_in, 0.999));

    // ---- the words, and the relief they press: its slope from reads
    // round the point, near for the walls of the letters and far for the
    // paper giving way round them
    let headline = step(p.y, headline_end);
    let jitter = (vec2<f32>(value_noise(p * 0.7), value_noise(p * 0.7 + vec2<f32>(5.0))) - vec2<f32>(0.5)) * 0.8 * headline;
    let here = print_at(p + jitter, size);
    let near = 1.3 * u;
    let far = 5.0 * u;
    let x = vec2<f32>(1.0, 0.0);
    let y = vec2<f32>(0.0, 1.0);
    let relief_here = relief_at(p, size, headline_end, depth);
    let near_slope = vec2<f32>(
        relief_at(p + x * near, size, headline_end, depth) - relief_at(p - x * near, size, headline_end, depth),
        relief_at(p + y * near, size, headline_end, depth) - relief_at(p - y * near, size, headline_end, depth)) / (2.0 * near);
    let far_slope = vec2<f32>(
        relief_at(p + x * far, size, headline_end, depth) - relief_at(p - x * far, size, headline_end, depth),
        relief_at(p + y * far, size, headline_end, depth) - relief_at(p - y * far, size, headline_end, depth)) / (2.0 * far);
    let relief_slope = (near_slope * 0.8 + far_slope * 0.5) * press;
    // The walls toward the sun shade what lies below them.
    var relief_sun = 1.0;
    for (var i = 1; i <= 3; i = i + 1) {
        let reach = f32(i * i) * 1.1 * u;
        let above = (relief_at(p - SUN * reach, size, headline_end, depth) - relief_here) * press - reach / SUN_COT;
        relief_sun = min(relief_sun, 1.0 - smoothstep(-0.12, 0.6, above));
    }
    let occlusion = 1.0 - 0.25 * smoothstep(0.0, depth, -relief_here * press);

    let ink = here.ink * ink_in;
    let accent = here.accent * ink_in;
    let wax = here.wax;

    // ---- the sheet's edge, and the dark table it lies on
    let sheet = vec4<f32>(inset, inset * 0.9, size.x - inset, size.y - inset * 0.9);
    let inside = deckle(p, sheet, u);
    let on_sheet = smoothstep(-1.4, 1.2, inside / u + (fibre - 0.25) * 2.0);
    var table_sun = 1.0;
    var edge_occlusion = 1.0;
    var edge_slope = vec2<f32>(0.0);
    if (inside < 26.0 * u) {
        let gradient = vec2<f32>(deckle(p + x, sheet, u) - inside, deckle(p + y, sheet, u) - inside);
        edge_slope = gradient * exp(-max(inside, 0.0) / (3.2 * u)) * 0.34 * on_sheet;
        let towards_sun = deckle(p - SUN * 7.0 * u, sheet, u);
        table_sun = mix(smoothstep(3.0 * u, -5.0 * u, towards_sun), 1.0, on_sheet);
        edge_occlusion = mix(1.0 - 0.45 * exp(inside / (6.0 * u)), 1.0, on_sheet);
    }

    // Ink fills the grain; wax hides it.
    var slope = grain_slope * (1.0 - 0.72 * clamp(ink + accent, 0.0, 1.0)) * (1.0 - wax)
        + wave_slope + relief_slope * on_sheet + edge_slope;
    slope = slope + mix(vec2<f32>(0.02, -0.015), vec2<f32>(0.0), on_sheet);

    // ---- light: a low sun, and the sky
    var facing = (1.0 + dot(slope, SUN) * SUN_COT) / sqrt(1.0 + dot(slope, slope));
    facing = max(facing, 0.0);
    facing = facing / (1.0 + 0.16 * max(facing - 1.0, 0.0));
    let falloff = 1.0 - 0.14 * dot(centred, SUN) - 0.16 * dot(centred, centred);
    let sun_colour = vec3<f32>(0.86, 0.715, 0.535) * falloff * sun_strength;
    let sky_colour = vec3<f32>(0.285, 0.325, 0.415) * (1.0 - 0.22 * dot(centred, vec2<f32>(0.7, 0.5)) - 0.10 * dot(centred, centred));
    let ambient = clamp(1.0 + 0.55 * dot(slope, vec2<f32>(0.75, 0.55)), 0.25, 1.7) * occlusion * edge_occlusion;
    // The leaves' shadow, which the scene casts, is a touch green.
    let leaves = surface.lit;
    let sun = sun_colour * leaves * relief_sun * table_sun;

    // ---- materials
    var paper = vec3<f32>(0.925, 0.905, 0.865) * (1.0 + (tooth - 0.5) * 0.05 - fibre * 0.035);
    let table = vec3<f32>(0.205, 0.212, 0.165) * (1.0 + (value_noise(p * 0.004) - 0.5) * 0.35 + (tooth - 0.5) * 0.16);
    paper = mix(table, paper, on_sheet);

    // The pool above laps over the top edge: the paper ahead of it drank
    // some.
    let su = clamp(u, 0.6, 1.5);
    let front = 84.0 * su + 15.0 * su * (value_noise(vec2<f32>(p.x / (58.0 * su), 3.1)) - 0.5)
        + 7.0 * su * (value_noise(p / (9.0 * su)) - 0.5);
    var damp = 1.0 - smoothstep(front - 34.0 * su, front + 12.0 * su, p.y);
    damp = damp * damp * (3.0 - 2.0 * damp);
    let from_front = (p.y - front) / (5.0 * su);
    paper = paper * mix(vec3<f32>(1.0), vec3<f32>(0.50, 0.525, 0.53), damp) * (1.0 - exp(-from_front * from_front) * 0.05);

    // A selection is a leaf green wash, brushed on with a wobbly edge.
    if (here.selection > 0.002) {
        let streaks = value_noise(vec2<f32>(p.x / (110.0 * u), p.y / (1.9 * u))) * 0.6
            + value_noise(vec2<f32>(p.x / (60.0 * u) + 4.0, p.y / (4.2 * u))) * 0.4 - 0.5;
        let amount = here.selection + streaks * 0.22 + (tooth - 0.5) * 0.16;
        let wash = smoothstep(0.30, 0.42, amount);
        let rim = wash * (1.0 - smoothstep(0.38, 0.60, amount));
        let pooling = 0.50 + 0.55 * streaks + 0.55 * (tooth - 0.5) + 0.62 * (value_noise(p / (52.0 * u) + vec2<f32>(2.0)) - 0.5);
        paper = paper * exp(-(wash * max(0.15, pooling) + rim * 0.34) * vec3<f32>(1.02, 0.56, 1.75));
    }

    // Ink sits thinner on the high points of the tooth.
    let ink_cover = clamp(ink, 0.0, 1.0);
    let accent_cover = clamp(accent, 0.0, 1.0);
    let solid = clamp(1.0 - 0.34 * smoothstep(0.45, 0.95, tooth), 0.0, 1.0);
    let ink_colour = mix(vec3<f32>(0.017, 0.031, 0.024), vec3<f32>(0.006, 0.017, 0.012), solid);
    let accent_colour = vec3<f32>(0.735, 0.085, 0.022) * mix(1.14, 0.94, solid);
    var albedo = mix(paper, accent_colour, accent_cover * (0.97 - 0.06 * (1.0 - solid)));
    albedo = mix(albedo, vec3<f32>(0.70, 0.070, 0.022), wax);
    albedo = mix(albedo, ink_colour, ink_cover * (0.985 - 0.05 * (1.0 - solid)));

    var sun_lit = albedo * sun * facing;
    var shade_lit = albedo * (sky_colour + vec3<f32>(0.040, 0.032, 0.022)) * ambient;

    // The ink film shines a little, more while wet.
    let to_sun = normalize(vec3<f32>(-SUN * SUN_COT, 1.0));
    let halfway = normalize(to_sun + vec3<f32>(0.0, 0.0, 1.0));
    let gloss_slope = slope + grain_slope * 0.5;
    let gloss_normal = normalize(vec3<f32>(-gloss_slope, 1.0));
    let nh = max(dot(gloss_normal, halfway), 0.0);
    let film = max(ink_cover, accent_cover * 0.5);
    sun_lit = sun_lit + sun * film * (max(pow(nh, 18.0) - 0.055, 0.0) * (0.20 + 0.30 * wet) + 0.012 * wet);
    shade_lit = shade_lit + sky_colour * film * (0.007 + 0.035 * wet) * max(0.0, 1.0 + dot(gloss_slope, vec2<f32>(1.2, 0.9)));
    // Colour and bare paper in a plate stay bright in shade.
    shade_lit = shade_lit * (1.0 + accent_cover * 0.34 + here.blind * accent_cover * 0.62);
    shade_lit = shade_lit + sky_colour * damp * 0.035 * max(0.0, 1.0 + dot(gloss_slope, vec2<f32>(1.6, 1.1)));

    // Wax: a tight highlight and a broad one.
    let wax_normal = normalize(vec3<f32>(-(relief_slope + wave_slope), 1.0));
    let wax_nh = max(dot(wax_normal, halfway), 0.0);
    sun_lit = sun_lit + wax * sun * (pow(wax_nh, 70.0) * 1.3 + pow(wax_nh, 9.0) * 0.10);
    shade_lit = shade_lit + wax * sky_colour * 0.10 * smoothstep(0.2, 0.9, dot(wax_normal, normalize(vec3<f32>(-0.5, -0.45, 0.75))));

    // Exposure with a soft shoulder; the shade under leaves is a touch
    // greener.
    let shadow = 1.0 - leaves;
    var c = (sun_lit + shade_lit * (vec3<f32>(1.0) + vec3<f32>(-0.012, 0.022, 0.0) * shadow)) * 0.94;
    let over = max(c - vec3<f32>(0.76), vec3<f32>(0.0));
    c = min(c, vec3<f32>(0.76)) + over / (vec3<f32>(1.0) + over * 1.9);
    return vec4<f32>(to_srgb(c), 1.0);
}
