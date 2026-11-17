// The oil painting of a page, after `noise.wgsl`. It paints the page once:
// primed linen, a few layers of knife and brush strokes for the blue
// ground, then the letters loaded on with short strokes that follow them.
// Its input is the page drawn in key colours on black, which say what lies
// where:
//
//   red at `INK` alpha      the headline's letters
//   yellow at `INK` alpha   the headline's orange word
//   green at `INK` alpha    the small type and its links
//   blue                    a selection; its letters show through it
//   grey `SLAB`             the button's slab, its label in grey `LABEL`
//
// It writes the paint's colour, and in alpha its thickness: `oil_light.wgsl`
// lights that relief each frame. The layer that holds the input keeps the
// painting until the page changes, so the strokes are worked out once.
//
// Lengths are in the page's unit u. Uniforms: 0 = logical pixels per u,
// 1 = the headline's size in u, 2 = the small type's size in u, 3 = the
// button's look (0 at rest, 1 under the pointer, 2 pressed); 4..7 = the
// headline's top and bottom, the small type's top and bottom in u; 8 = the
// small type's right edge in u.

const INK: f32 = 0.6;
const SLAB: f32 = 0.2;
const LABEL: f32 = 0.1;
// Thickness is stored as `HEIGHT_FLOOR` plus its share of `MAX_HEIGHT`: no
// pixel of the painting is transparent.
const HEIGHT_FLOOR: f32 = 0.04;
const MAX_HEIGHT: f32 = 14.0;
const PI: f32 = 3.14159265;
const KNIFE: f32 = 0.0;

struct Paint {
    color: vec3<f32>,
    height: f32,
}

// One stroke of a ground coat: where it lies, its shape and its load.
struct Brush {
    centre: vec2<f32>,
    angle: f32,
    length: f32,
    width: f32,
    curvature: f32,
    seed: f32,
    tool: f32,
    color_a: i32,
    color_b: i32,
    dryness: f32,
    thickness: f32,
}

var<private> page_size: vec2<f32>;
var<private> unit: f32;
var<private> stage: vec2<f32>;
var<private> texels_per_unit: f32;
var<private> head_band: vec2<f32>;
var<private> body_band: vec2<f32>;
var<private> body_right: f32;
// The weave of the linen under the point, which every stroke catches on.
var<private> weave_here: f32;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn palette(index: i32) -> vec3<f32> {
    var colors = array<vec3<f32>, 20>(
        // 0..7 the blues of the ground
        vec3<f32>(0.09, 0.11, 0.42), vec3<f32>(0.07, 0.08, 0.30),
        vec3<f32>(0.12, 0.20, 0.58), vec3<f32>(0.16, 0.30, 0.69),
        vec3<f32>(0.25, 0.44, 0.78), vec3<f32>(0.44, 0.63, 0.86),
        vec3<f32>(0.21, 0.18, 0.52), vec3<f32>(0.05, 0.08, 0.22),
        // 8..10 the primer
        vec3<f32>(0.74, 0.66, 0.62), vec3<f32>(0.80, 0.72, 0.67), vec3<f32>(0.69, 0.62, 0.60),
        // 11..14 the whites of the letters
        vec3<f32>(0.965, 0.95, 0.895), vec3<f32>(0.985, 0.97, 0.93),
        vec3<f32>(0.985, 0.965, 0.76), vec3<f32>(0.91, 0.92, 0.93),
        // 15..19 cadmium orange
        vec3<f32>(0.95, 0.36, 0.05), vec3<f32>(0.90, 0.20, 0.05), vec3<f32>(1.0, 0.55, 0.08),
        vec3<f32>(0.98, 0.43, 0.06), vec3<f32>(0.82, 0.15, 0.06),
    );
    return colors[clamp(index, 0, 19)];
}

fn pick(choices: vec4<f32>, h: f32) -> i32 {
    let at = min(i32(h * 4.0), 3);
    return i32(choices[at]);
}

// Four hashes of a point at once.
fn hash4(p: vec2<f32>) -> vec4<f32> {
    var q = fract(p.xyxy * vec4<f32>(0.1031, 0.1030, 0.0973, 0.1099));
    q += dot(q, q.wzxy + 33.33);
    return fract((q.xxyz + q.yzzw) * q.zywx);
}

fn gauss(x: f32) -> f32 {
    return exp(-x * x);
}

fn over(below: Paint, color: vec3<f32>, cover: f32, height: f32, height_cover: f32) -> Paint {
    return Paint(mix(below.color, color, cover), mix(below.height, height, height_cover));
}

// ---- the page in key colours

fn key_at(point: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(input_texture, input_sampler, point * unit / page_size, 0.0);
}

// Whether the colour is the slab or its label: their greys have equal
// channels.
fn grey(c: vec4<f32>) -> f32 {
    let spread = max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
    return (1.0 - step(0.03, spread)) * step(0.03, c.r) * (1.0 - step(0.27, c.r));
}

fn head_ink(c: vec4<f32>) -> f32 {
    return clamp(c.r / INK, 0.0, 1.0) * (1.0 - grey(c));
}

fn accent_ink(c: vec4<f32>) -> f32 {
    return clamp(min(c.r, c.g) / INK, 0.0, 1.0) * (1.0 - grey(c));
}

fn body_ink(c: vec4<f32>) -> f32 {
    return clamp((c.g - c.r) / INK, 0.0, 1.0) * (1.0 - grey(c));
}

// A selection is blue behind the letters; the letters on it let some blue
// through, so selected ink shows blue too.
fn selected(c: vec4<f32>) -> f32 {
    return smoothstep(0.2, 0.35, c.b) * (1.0 - grey(c));
}

fn label_ink(c: vec4<f32>) -> f32 {
    return grey(c) * clamp((SLAB - c.r) / (SLAB - LABEL), 0.0, 1.0);
}

// ---- the canvas

// Plain weave linen. x: height of the thread on top, y: 1 on a warp thread.
fn linen(p: vec2<f32>) -> vec2<f32> {
    let pitch = 3.3;
    let q = p + 1.1 * vec2<f32>(value_noise(p / 47.0) - 0.5, value_noise(p / 39.0 + 30.0) - 0.5);
    let cell = floor(q / pitch);
    let f = fract(q / pitch) - 0.5;
    let thread = vec2<f32>(hash(vec2<f32>(cell.x, 3.0)), hash(vec2<f32>(cell.y, 9.0)));
    let warp_width = 0.40 + 0.13 * thread.x + 0.1 * (value_noise(vec2<f32>(cell.x * 3.7, q.y / 60.0)) - 0.5);
    let weft_width = 0.40 + 0.13 * thread.y + 0.1 * (value_noise(vec2<f32>(q.x / 60.0, cell.y * 3.7)) - 0.5);
    let over_under = (abs(cell.x + cell.y) % 2.0) * 2.0 - 1.0;
    let warp = sqrt(max(0.0, 1.0 - f.x * f.x / (warp_width * warp_width)))
        * (0.62 + 0.38 * over_under * cos(f.y * PI));
    let weft = sqrt(max(0.0, 1.0 - f.y * f.y / (weft_width * weft_width)))
        * (0.62 - 0.38 * over_under * cos(f.x * PI));
    return vec2<f32>(max(warp, weft), step(weft, warp));
}

fn canvas(p: vec2<f32>) -> Paint {
    let weave = linen(p);
    weave_here = weave.x;
    let shows = smoothstep(0.6, 1.05, texels_per_unit);
    var color = mix(vec3<f32>(0.56, 0.50, 0.42), vec3<f32>(0.75, 0.69, 0.60), mix(0.6, weave.x, shows));
    color *= 0.93 + 0.14 * value_noise(p / vec2<f32>(3.0, 46.0) + weave.y * 40.0);
    color = mix(color, color * vec3<f32>(0.98, 0.96, 0.93), weave.y);
    return Paint(color, weave.x * 0.8 * shows + 0.2 * value_noise(p / 9.0));
}

// Paint as a loaded brush leaves it. x: ridges across the stroke, y: the
// coarse clumps alone.
fn bristle(across_in: f32, along: f32, seed: f32) -> vec2<f32> {
    let across = across_in + 2.2 * (value_noise(vec2<f32>(along / 48.0, seed * 3.1)) - 0.5)
        + 0.9 * (value_noise(vec2<f32>(along / 17.0, seed * 5.7)) - 0.5);
    let shift = seed * 61.7;
    let clump = value_noise(vec2<f32>(across / 8.5 + shift, along / 150.0 + seed));
    let hair = value_noise(vec2<f32>(across / 3.1 - shift, along / 74.0));
    let fine = value_noise(vec2<f32>(across / 1.45 + shift * 2.0, along / 40.0));
    let crest = 1.0 - abs(2.0 * hair - 1.0);
    let show_fine = smoothstep(0.9, 1.7, texels_per_unit);
    let ridges = 0.42 * clump + 0.24 * hair + 0.2 * crest + 0.14 * mix(0.5, fine, show_fine);
    return vec2<f32>(clamp(ridges, 0.0, 1.0), clump);
}

// ---- the ground

// The strokes swirl round a point far below the picture, and lie down
// level near the small type and toward the bottom edge.
fn flow(p: vec2<f32>) -> f32 {
    let swirl = vec2<f32>(stage.x * 0.30, stage.y * 2.15);
    let angle = atan2(p.y - swirl.y, p.x - swirl.x) + PI * 0.5
        + 0.42 * sin(p.x / 260.0 + p.y / 190.0 + 1.2) + 0.25 * sin(p.x / 150.0 - p.y / 120.0);
    let near_bottom = clamp((stage.y - 40.0 - p.y) / 170.0, 0.0, 1.0);
    let near_type = min(1.0, abs(p.y - (body_band.x - 10.0)) / 130.0 + 0.12);
    let wrapped = atan2(sin(angle), cos(angle));
    return wrapped * (0.18 + 0.82 * near_bottom * near_type);
}

fn behind_small_type(p: vec2<f32>) -> bool {
    return (p.y > body_band.x - 44.0) & (p.y < body_band.y + 40.0)
        & (p.x < max(stage.x - 60.0, body_right + 60.0));
}

// How far inside the ragged margin the paint keeps from the canvas's
// edges a point is: the linen shows at the border.
fn inside_margin(p: vec2<f32>, seed: f32) -> f32 {
    let side = 30.0 + 26.0 * value_noise(vec2<f32>(p.y / 90.0, seed));
    let top = 22.0 + 16.0 * value_noise(vec2<f32>(p.x / 120.0, seed + 3.0));
    let bottom = 46.0 + 26.0 * value_noise(vec2<f32>(p.x / 140.0, seed + 7.0));
    return min(min(p.x - side, stage.x - side - p.x), min(p.y - top, stage.y - bottom - p.y));
}

fn lay(below: Paint, p: vec2<f32>, b: Brush) -> Paint {
    let d = vec2<f32>(cos(b.angle), sin(b.angle));
    let q = p - b.centre;
    let along = dot(q, d);
    let x = dot(q, vec2<f32>(-d.y, d.x)) - 0.5 * b.curvature * along * along;
    let s = along + 0.5 * b.length;
    if ((abs(x) > 0.62 * b.width) | (s < -10.0) | (s > b.length + 10.0)) {
        return below;
    }
    let width = b.width;
    let len = b.length;
    let seed = b.seed;
    let is_knife = 1.0 - step(0.5, b.tool);
    let across = abs(x) / (width * 0.5);
    let along_share = s / len;
    let hair = bristle(x, s, seed);
    let weave = weave_here;

    // The two long edges wander each on its own, with rough stretches.
    let side = step(0.0, x);
    let s_warped = s + 26.0 * (value_noise(vec2<f32>(s / 41.0, seed * 5.0 + side)) - 0.5);
    let wander = mix(16.0, 60.0, hash(vec2<f32>(seed * 311.0, side + 3.0)));
    let rough = smoothstep(0.5, 0.85, value_noise(vec2<f32>(s / 57.0 + side * 7.0, seed * 29.0)));
    let edge = across
        + 0.17 * (value_noise(vec2<f32>(s_warped / wander, seed * 9.0 + side * 4.0)) - 0.5)
        + rough * 0.05 * (value_noise(vec2<f32>(s_warped / 8.3, seed * 19.0)) - 0.5);
    let edge_softness = min(0.12, 3.2 / width);
    var cover = 1.0 - smoothstep(0.95 - edge_softness, 0.95, edge - 0.05);

    // A ragged start, bristle by bristle.
    let teeth = 9.0 * value_noise(vec2<f32>(x / 7.0, seed * 31.0));
    let from_start = s - 5.0 - teeth - 5.0 * hair.x;
    cover *= smoothstep(0.0, 1.6, from_start);

    // Running dry: the paint catches on the tops of the weave, then stops.
    let catches = weave * (0.15 + 1.1 * value_noise(p / 7.3 + 3.0));
    var dry = smoothstep(
        1.0 - b.dryness,
        1.02,
        along_share + (value_noise(vec2<f32>(x / 6.0, seed * 13.0)) - 0.5) * 0.42 * b.dryness
            + (hair.y - 0.5) * 0.24,
    );
    let end_rag = min(0.13 * len, 34.0) * value_noise(vec2<f32>(x / 3.4 + 5.0, seed * 23.0));
    dry = max(dry, 1.3 * smoothstep(len - 12.0, len - 1.0, s + end_rag + 7.0 * (hair.x - 0.5)));
    let load = hair.x * 0.8 + catches * 0.22;
    cover *= smoothstep(dry - 0.10, dry + 0.07, load + 0.12 + step(dry, 0.001));
    // The ground keeps inside its ragged margin.
    cover *= smoothstep(-6.0, 10.0, inside_margin(p, 4.0) + 14.0 * (hair.x - 0.5));
    if (cover < 0.004) {
        return below;
    }

    // Bristle furrows (a knife leaves it smoother), a ridge pushed out
    // along the edge, a lip where the stroke began.
    let furrows = mix(1.0, 0.34, is_knife);
    let ridge = gauss((edge - 0.80) / 0.085)
        * (0.12 + 1.5 * pow(value_noise(vec2<f32>(s_warped / 58.0, seed * 41.0 + side * 3.0)), 1.6))
        * mix(1.0, side * 1.5, is_knife);
    let lip = gauss((from_start - 5.0) / 5.0) * (1.0 - across * across);
    let height = b.thickness
        * (0.50 + furrows * 1.35 * (hair.x - 0.5) + 0.62 * ridge + 0.5 * lip) * (1.0 - 0.55 * dry)
        + 0.5 * b.thickness * (1.0 - dry) * (value_noise(p / 31.0) - 0.5);
    let covering = cover * mix(0.82, 0.4, dry) * min(1.0, 0.35 + b.thickness * 0.35);

    // The two colours on the brush lie beside each other in streaks.
    let streak = smoothstep(
        0.36,
        0.66,
        value_noise(vec2<f32>(x / 5.2 + seed * 77.0, s / 210.0 + seed)) * 0.6
            + 0.4 * value_noise(vec2<f32>(x / 2.2 - seed * 33.0, s / 120.0)),
    );
    var color = mix(palette(b.color_a), palette(b.color_b), streak * mix(0.75, 0.5, is_knife));
    color *= 0.93 + 0.16 * hair.x;
    return over(below, color, cover, max(height, 0.04), covering);
}

// A stroke of a coat whose strokes stand one to a cell of `cell` u, in the
// cell `at`. `coat` picks the coat's tools and colours.
fn coat_brush(at: vec2<f32>, cell: vec2<f32>, coat: i32) -> Brush {
    let key = at + vec2<f32>(f32(coat) * 101.0, f32(coat) * 37.0);
    let first = hash4(key);
    let h = hash4(key + 17.0);
    let g = hash4(key + 41.0);
    // Within half a cell of its middle: a point sees every stroke that
    // reaches it among the nine cells round its own.
    let centre = (at + 0.5 + (first.xy - 0.5) * 0.5) * cell;
    let half_turn = select(0.0, PI, h.w < 0.45);
    let scale = min(1.0, stage.x / 900.0);
    var b: Brush;
    b.centre = centre;
    b.seed = first.z;
    b.curvature = (g.x - 0.5) * 0.0044;
    b.tool = 1.0;
    switch coat {
        case 0: {
            // Primer, scrubbed on thin and level.
            b.angle = select(0.0, PI, h.x < 0.5) + (h.y - 0.5) * 0.14;
            b.length = mix(300.0, 470.0, h.z) * scale;
            b.width = mix(70.0, 110.0, g.y);
            b.curvature = 0.0;
            b.color_a = pick(vec4<f32>(8.0, 9.0, 10.0, 9.0), g.z);
            b.color_b = pick(vec4<f32>(8.0, 9.0, 8.0, 9.0), g.w);
            b.dryness = mix(0.2, 0.4, h.x);
            b.thickness = mix(0.45, 0.75, h.y);
        }
        case 1: {
            // A lean, dark underpainting.
            b.angle = flow(centre) + (h.x - 0.5) + half_turn;
            b.length = mix(200.0, 330.0, h.z) * scale;
            b.width = mix(60.0, 96.0, g.y);
            b.curvature *= 0.55;
            b.color_a = pick(vec4<f32>(0.0, 1.0, 6.0, 7.0), g.z);
            b.color_b = pick(vec4<f32>(0.0, 2.0, 6.0, 0.0), g.w);
            b.dryness = mix(0.3, 0.6, h.x);
            b.thickness = mix(0.8, 1.2, h.y);
        }
        case 2: {
            // Solid dark sweeps under the small type, so it reads.
            b.angle = (h.x - 0.5) * 0.06 + select(0.0, PI, h.y < 0.5);
            b.length = mix(0.42, 0.56, h.z) * (body_right + 70.0);
            b.width = mix(62.0, 84.0, g.y);
            b.curvature = 0.0;
            b.color_a = pick(vec4<f32>(0.0, 1.0, 0.0, 2.0), g.z);
            b.color_b = pick(vec4<f32>(0.0, 2.0, 6.0, 0.0), g.w);
            b.dryness = 0.12;
            b.thickness = mix(1.0, 1.4, h.y);
        }
        case 5: {
            // Last accents above the type: shorter, thicker, brighter.
            b.angle = flow(centre) + (h.x - 0.5) * 0.6 + select(0.0, PI, h.w < 0.5);
            b.length = mix(150.0, 300.0, h.z) * scale;
            b.width = mix(34.0, 62.0, g.y) * max(0.75, scale);
            b.tool = step(0.35, g.x);
            b.color_a = pick(vec4<f32>(4.0, 3.0, 4.0, 2.0), g.z);
            b.color_b = pick(vec4<f32>(5.0, 4.0, 3.0, 2.0), g.w);
            b.dryness = mix(0.4, 0.8, h.x);
            b.thickness = mix(2.4, 3.8, h.y);
            if (behind_small_type(centre) | (g.x < 0.3) | (centre.y > body_band.x - 120.0)) {
                b.width = 0.0;
            }
        }
        default: {
            // The body of the ground: lighter toward the lamp's corner,
            // thick and lively away from the type, calm and dark behind it.
            let quiet = behind_small_type(centre);
            b.angle = select(flow(centre) + (h.x - 0.5) * 0.56, (h.x - 0.5) * 0.2, quiet)
                + select(0.0, PI, h.w < 0.42);
            let light = clamp(
                1.05 - centre.x / stage.x * 0.75 - centre.y / stage.y * 0.75 + (g.x - 0.5) * 0.7,
                0.0,
                1.0,
            );
            b.tool = select(1.0, KNIFE, g.y < select(0.24, 0.6, quiet));
            b.length = select(mix(190.0, 330.0, h.z), mix(260.0, 330.0, h.z), quiet) * scale;
            b.width = select(mix(44.0, 80.0, g.z), mix(50.0, 100.0, g.z), b.tool < 0.5)
                * max(0.7, scale);
            if (quiet) {
                b.color_a = pick(vec4<f32>(0.0, 0.0, 2.0, 1.0), g.w);
                b.color_b = pick(vec4<f32>(0.0, 2.0, 6.0, 0.0), h.y);
            } else if (light > 0.62) {
                b.color_a = pick(vec4<f32>(3.0, 3.0, 4.0, 2.0), g.w);
                b.color_b = pick(vec4<f32>(4.0, 5.0, 3.0, 4.0), h.y);
            } else if (light > 0.3) {
                b.color_a = pick(vec4<f32>(2.0, 2.0, 3.0, 0.0), g.w);
                b.color_b = pick(vec4<f32>(3.0, 2.0, 6.0, 4.0), h.y);
            } else {
                b.color_a = pick(vec4<f32>(0.0, 2.0, 6.0, 1.0), g.w);
                b.color_b = pick(vec4<f32>(2.0, 3.0, 6.0, 2.0), h.y);
            }
            b.dryness = mix(0.25, 0.72, h.x);
            b.thickness = select(mix(1.7, 2.9, h.y), mix(1.0, 1.5, h.y), quiet);
        }
    }
    return b;
}

// A coat of strokes over `below`, one stroke to a cell of `cell` u, the
// grid moved by `shift` cells. The cells are painted in rows from the top,
// so every point sees the strokes over it in the same order.
fn coat(below: Paint, p: vec2<f32>, cell: vec2<f32>, shift: vec2<f32>, coat_index: i32) -> Paint {
    var paint = below;
    let base = floor(p / cell - shift);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let b = coat_brush(base + vec2<f32>(f32(i), f32(j)) + shift, cell, coat_index);
            if (b.width > 0.0) {
                paint = lay(paint, p, b);
            }
        }
    }
    return paint;
}

fn ground(p: vec2<f32>) -> Paint {
    var paint = canvas(p);
    paint = coat(paint, p, vec2<f32>(190.0, 62.0), vec2<f32>(0.0), 0);
    paint = coat(paint, p, vec2<f32>(150.0, 150.0), vec2<f32>(0.0), 1);
    paint = coat(paint, p, vec2<f32>(150.0, 150.0), vec2<f32>(0.5), 1);
    if ((p.y > body_band.x - 70.0) & (p.y < body_band.y + 70.0) & (p.x < body_right + 140.0)) {
        paint = coat(paint, p, vec2<f32>((body_right + 70.0) / 3.0, 40.0), vec2<f32>(0.0), 2);
    }
    paint = coat(paint, p, vec2<f32>(150.0, 150.0), vec2<f32>(0.0), 3);
    paint = coat(paint, p, vec2<f32>(150.0, 150.0), vec2<f32>(0.5), 4);
    paint = coat(paint, p, vec2<f32>(150.0, 150.0), vec2<f32>(0.25, 0.75), 6);
    paint = coat(paint, p, vec2<f32>(190.0, 140.0), vec2<f32>(0.0), 5);
    return paint;
}

// ---- the headline

// The headline around a point.
struct HeadField {
    // How much of the headline lies here, its edge softened a little.
    letter: f32,
    // How deep inside a letter the point lies: highest along the middle of
    // a stem.
    depth: f32,
    // How much of the headline lies near.
    halo: f32,
    // The way a brush follows the letters here.
    direction: vec2<f32>,
}

fn head_field(p: vec2<f32>, here: vec4<f32>, em: f32) -> HeadField {
    let centre = head_ink(here);
    var near = 0.0;
    var halo = 0.0;
    var moments = vec4<f32>(0.0);
    var sums = vec2<f32>(0.0);
    for (var i = 0; i < 12; i++) {
        let a = f32(i) * PI / 6.0 + 0.13;
        let o = vec2<f32>(cos(a), sin(a));
        let ink = head_ink(key_at(p + o * em * 0.2));
        halo += ink;
        moments += ink * vec4<f32>(o.x * o.x, o.y * o.y, o.x * o.y, 0.0);
        sums += ink * o;
    }
    if (halo + centre < 0.001) {
        return HeadField(0.0, 0.0, 0.0, vec2<f32>(0.0, 1.0));
    }
    // A ring halfway in steadies the way the brush follows and tells how
    // deep in a letter the point lies.
    var inner = 0.0;
    var weight = halo;
    for (var i = 0; i < 6; i++) {
        let a = f32(i) * PI / 3.0 + 0.4;
        let o = vec2<f32>(cos(a), sin(a)) * 0.55;
        let ink = head_ink(key_at(p + o * em * 0.2));
        inner += ink;
        moments += ink * vec4<f32>(o.x * o.x, o.y * o.y, o.x * o.y, 0.0);
        sums += ink * o;
    }
    weight = max(weight + inner, 1.0e-3);
    for (var i = 0; i < 4; i++) {
        let a = f32(i) * PI * 0.5 + 0.4;
        near += head_ink(key_at(p + vec2<f32>(cos(a), sin(a)) * em * 0.025));
    }
    let mean = sums / weight;
    let xx = moments.x / weight - mean.x * mean.x;
    let yy = moments.y / weight - mean.y * mean.y;
    let xy = moments.z / weight - mean.x * mean.y;
    let strength = sqrt((xx - yy) * (xx - yy) + 4.0 * xy * xy);
    let angle = 0.5 * atan2(2.0 * xy, xx - yy);
    // The field has no sign: the brush runs down a letter, so the way that
    // points down is taken, and a stroke keeps one way along a stem.
    var direction = normalize(mix(vec2<f32>(0.0, 1.0), vec2<f32>(cos(angle), sin(angle)), smoothstep(0.01, 0.08, strength)));
    direction = select(direction, -direction, direction.y < 0.0);
    let letter = 0.5 * centre + 0.125 * near;
    let depth = (centre + near + inner) / 11.0;
    return HeadField(letter, depth, halo / 12.0, direction);
}

// The dark rim piled against the outside of the headline: strokes that
// follow its outline in bands, darkest next to the letters.
fn rim(below: Paint, p: vec2<f32>, field: HeadField) -> Paint {
    let d = field.direction;
    let wobble = value_noise(p / 31.0) - 0.5;
    let band = field.halo * 6.0 + wobble * 0.8;
    let hair = bristle(band * 9.0, dot(p, d) * 0.6, 0.37);
    let outside = 1.0 - smoothstep(0.25, 0.45, field.letter);
    let cover = smoothstep(0.02, 0.12, field.halo + 0.06 * wobble + 0.04 * (hair.x - 0.5)) * outside;
    if (cover < 0.004) {
        return below;
    }
    let next_to_letters = smoothstep(0.18, 0.4, field.halo);
    let echo = smoothstep(0.35, 0.65, fract(band));
    var color = mix(palette(select(0, 6, wobble > 0.0)), palette(2), echo * 0.5 * (1.0 - next_to_letters));
    color = mix(color, palette(select(7, 1, wobble > 0.1)), next_to_letters);
    color *= 0.92 + 0.16 * hair.x;
    let height = 1.6 + 1.1 * (hair.x - 0.5) + 0.9 * gauss((field.halo - 0.3) / 0.12) + 0.4 * echo;
    return over(below, color, cover * 0.97, height, cover * 0.85);
}

// The letters themselves: first laid in flat, then strokes over them that
// follow the letters and stop at a slightly different place each. The
// paint stands in a smooth bead, highest along the middle of a stem.
fn letters(below: Paint, p: vec2<f32>, here: vec4<f32>, field: HeadField, em: f32, dark: f32) -> Paint {
    let letter = field.letter;
    if (letter < 0.05) {
        return below;
    }
    let accent = smoothstep(0.1, 0.4, accent_ink(here) / max(head_ink(here), 0.05)) * step(0.05, head_ink(here))
        + smoothstep(0.2, 0.5, accent_ink(key_at(p + vec2<f32>(0.0, em * 0.04)))) * (1.0 - step(0.05, head_ink(here)));
    let dome = smoothstep(0.3, 0.95, field.depth);
    let amount = value_noise(p / 37.0 + 11.0);

    // The underlayer: white, or orange in the orange word, or dark.
    var paint = below;
    let white = mix(vec3<f32>(0.84, 0.87, 0.93), vec3<f32>(0.97, 0.95, 0.88), value_noise(p / 23.0));
    let orange = vec3<f32>(1.0, 0.50, 0.08);
    let laid = mix(mix(white, white * orange, accent), vec3<f32>(0.05, 0.06, 0.20), dark);
    let glyph = smoothstep(0.25, 0.5, letter);
    let bead = (3.0 + 3.2 * dome + 1.2 * amount) * smoothstep(0.25, 0.55, letter);
    paint = over(paint, laid, glyph, bead, glyph * 0.95);

    // Strokes along the letters, in two coats.
    let d = field.direction;
    let n = vec2<f32>(-d.y, d.x);
    for (var c = 0; c < 2; c++) {
        let cell = em * 0.45;
        let shift = vec2<f32>(f32(c) * 0.5, f32(c) * 0.33);
        let base = floor(p / cell - shift);
        for (var j = -1; j <= 1; j++) {
            for (var i = -1; i <= 1; i++) {
                let at = base + vec2<f32>(f32(i), f32(j)) + shift;
                let key = at + vec2<f32>(f32(c) * 53.0, 7.0);
                let shape = hash4(key);
                let load = hash4(key + 23.0);
                let centre = (at + 0.5 + (shape.xy - 0.5) * 0.5) * cell;
                let width = em * mix(0.13, 0.2, shape.z);
                let len = em * mix(0.6, 1.1, shape.w);
                let seed = load.x;
                let q = p - centre;
                let x = dot(q, n);
                let s = dot(q, d) + len * 0.5;
                if ((abs(x) > width * 0.6) | (s < 0.0) | (s > len)) {
                    continue;
                }
                let hair = bristle(x * 0.8, s, seed);
                let across = abs(x) / (width * 0.5);
                let edge = across + 0.15 * (value_noise(vec2<f32>(s / 14.0, seed * 9.0)) - 0.5);
                var cover = 1.0 - smoothstep(0.6, 0.98, edge);
                let from_start = s - 2.0 - 5.0 * value_noise(vec2<f32>(x / 4.0, seed * 31.0));
                cover *= smoothstep(0.0, 4.0, from_start);
                let to_end = len - s - 8.0 * hair.x - 4.0 * value_noise(vec2<f32>(x / 2.0, seed * 11.0));
                cover *= smoothstep(0.0, 5.0, to_end);
                let ink_level = max(0.27, 0.30 + 0.2 * (value_noise(vec2<f32>(x / 3.0, seed * 7.0 + s / 60.0)) - 0.5));
                cover *= smoothstep(ink_level - 0.05, ink_level + 0.05, letter);
                if (cover < 0.004) {
                    continue;
                }
                let ridge = gauss((edge - 0.7) / 0.15);
                let tail = gauss((to_end - 4.0) / 4.0);
                var height = bead + 1.3 * (hair.x - 0.5) + 0.7 * ridge * dome + 0.6 * tail;

                let streak = smoothstep(0.45, 0.78, value_noise(vec2<f32>(x / 2.6 + seed * 77.0, s / 80.0)));
                let tint = value_noise(vec2<f32>(x / 2.1 - seed * 33.0, s / 130.0 + 3.0));
                let first = 11 + i32(load.y * 2.0);
                let second = select(11 + i32(load.z * 4.0), select(12, 13, load.z < 0.3), c == 1);
                var color = mix(palette(first), palette(second), streak * 0.6);
                // White picks up the wet blue under it near the edge of a
                // letter, on a dirty brush.
                let dirty = step(0.62, hash(vec2<f32>(seed * 377.0, 1.0)));
                let blue = smoothstep(0.60, 0.82, tint + 0.2 * (1.0 - dome))
                    * (0.25 + 0.75 * dirty) * (1.0 - 0.7 * s / len);
                color = mix(color, vec3<f32>(0.42, 0.52, 0.82), blue * 0.6);
                color *= 0.95 + 0.07 * hair.x;
                // The orange word: three cadmiums, a yellow streak, a
                // vermilion one.
                let tube = hash(vec2<f32>(seed * 617.0, 7.0));
                var cadmium = select(select(vec3<f32>(0.95, 0.36, 0.05), vec3<f32>(1.0, 0.58, 0.10), tube < 0.8), vec3<f32>(0.98, 0.47, 0.05), tube < 0.45);
                cadmium = mix(cadmium, vec3<f32>(1.0, 0.74, 0.24), streak * 0.62);
                cadmium = mix(cadmium, vec3<f32>(0.86, 0.22, 0.06), smoothstep(0.62, 0.84, tint) * 0.6);
                color = mix(color, cadmium * (0.93 + 0.10 * hair.x), accent);
                // Selected letters are painted again in dark blues.
                let brushed = value_noise(vec2<f32>(x / 2.3, s / 40.0));
                color = mix(color, mix(vec3<f32>(0.03, 0.035, 0.13), vec3<f32>(0.07, 0.085, 0.27), brushed), dark);
                height = mix(height, 3.2 * dome + 2.0 * brushed + 1.2, dark * 0.7);
                paint = over(paint, color, cover, max(height, 0.1), cover * 0.9);
            }
        }
    }
    return paint;
}

// ---- the small type

// A hand does not hold a small brush still.
fn hand_wobble(p: vec2<f32>) -> vec2<f32> {
    let slow = vec2<f32>(value_noise(p / 6.3) - 0.5, value_noise(p / 6.3 + 50.0) - 0.5);
    let quick = vec2<f32>(value_noise(p / 2.2 + 20.0) - 0.5, value_noise(p / 2.2 + 70.0) - 0.5);
    return 0.95 * slow + 0.3 * quick;
}

fn small_type(below: Paint, p: vec2<f32>) -> Paint {
    let shifted = key_at(p + hand_wobble(p) * 0.6);
    let glyph = body_ink(shifted);
    if (glyph < 0.003) {
        return below;
    }
    let dark = selected(shifted);
    let hair = bristle(p.y * 2.0, p.x, 0.71);
    let loaded = smoothstep(0.15, 0.85, value_noise(p / vec2<f32>(52.0, 25.0) + 3.0));
    let word = value_noise(p / vec2<f32>(19.0, 11.0) + 40.0);
    let thin = smoothstep(0.40, 0.78, hair.x + 0.34 * (0.5 - word) + 0.10 * (0.5 - loaded));
    let white = mix(vec3<f32>(0.93, 0.94, 0.95), vec3<f32>(0.985, 0.975, 0.93), word);
    var color = mix(white, vec3<f32>(0.66, 0.73, 0.93), thin * 0.45) * (0.955 + 0.10 * hair.x);
    let brushed = value_noise(vec2<f32>(p.y / 2.3, p.x / 40.0));
    color = mix(color, mix(vec3<f32>(0.03, 0.035, 0.13), vec3<f32>(0.06, 0.07, 0.24), brushed * brushed), dark);
    let height = glyph * mix(0.25 + 0.75 * loaded + 0.85 * (hair.x - 0.5), 0.35 + 0.7 * brushed, dark);
    return over(below, color, glyph * mix(1.0, 0.85, thin * (1.0 - dark)), max(height, 0.03), glyph * 0.62);
}

// ---- the selection: a wet orange stroke dragged over the dry paint

fn selection_reach(p: vec2<f32>, line: f32) -> f32 {
    var reach = selected(key_at(p)) * 2.0;
    for (var k = 1; k <= 3; k++) {
        let step_x = f32(k) * 0.22 * line;
        let hit = max(selected(key_at(p - vec2<f32>(step_x, 0.0))), selected(key_at(p + vec2<f32>(step_x, 0.0))));
        reach = max(reach, hit * (2.0 - f32(k) * 0.5));
    }
    let spill = max(selected(key_at(p - vec2<f32>(0.0, 0.16 * line))), selected(key_at(p + vec2<f32>(0.0, 0.16 * line))));
    return max(reach, spill * 1.6);
}

fn wet_stroke(below: Paint, p: vec2<f32>, line: f32, big: f32) -> Paint {
    let reach = selection_reach(p, line);
    if (reach < 0.001) {
        return below;
    }
    let row = floor(p.y / (line * 1.1));
    let seed = hash(vec2<f32>(row, 13.0));
    let hair = bristle(p.y * 1.3, p.x, seed);
    // Each row's stroke runs out at its own place.
    let end = 0.75 + 0.6 * value_noise(vec2<f32>(p.y / 9.0 + 5.0, seed * 23.0)) + 0.35 * (hair.x - 0.5)
        + 0.2 * (value_noise(vec2<f32>(p.y / 2.2, seed * 7.0)) - 0.5);
    let edge = value_noise(vec2<f32>(p.x / 41.0, seed * 9.0 + step(0.0, p.y)));
    var cover = smoothstep(end - 0.1, end + 0.1, reach) * smoothstep(0.1, 0.35, hair.x + 0.3 * edge + 0.2);
    let streak = smoothstep(0.36, 0.66, value_noise(vec2<f32>(p.y / 5.2 + seed * 77.0, p.x / 210.0)));
    let first = 15 + i32(hash(vec2<f32>(row, 3.0)) * 4.0);
    var color = mix(palette(first), palette(17), streak * 0.75) * (0.93 + 0.16 * hair.x);
    let blue = smoothstep(0.70, 0.9, value_noise(vec2<f32>(p.y / 2.9 - 4.7, p.x / 70.0 + seed * 9.0)));
    color = mix(color, vec3<f32>(0.36, 0.10, 0.20), blue * 0.3);
    let thickness = mix(2.6, 4.4, big);
    let height = thickness * (0.5 + 1.35 * (hair.x - 0.5) + 0.62 * gauss((reach - end) / 0.15));
    return over(below, color, cover, max(height, 0.1), cover * 0.82);
}

// ---- the button: a slab of cadmium laid on with a knife

fn slab(below: Paint, p: vec2<f32>, here: vec4<f32>, look: f32) -> Paint {
    var inside = grey(here) * 2.0;
    for (var k = 1; k <= 2; k++) {
        let x = f32(k) * 11.0;
        let hit = max(grey(key_at(p - vec2<f32>(x, 0.0))), grey(key_at(p + vec2<f32>(x, 0.0))));
        inside = max(inside, hit * (2.0 - f32(k) * 0.6));
    }
    let spill = max(grey(key_at(p - vec2<f32>(0.0, 6.0))), grey(key_at(p + vec2<f32>(0.0, 6.0))));
    inside = max(inside, spill * 1.3);
    if (inside < 0.001) {
        return below;
    }
    let band = floor(p.y / 13.0);
    let seed = hash(vec2<f32>(band, 61.0 + look));
    let hair = bristle(p.y, p.x, seed);
    let end = 0.6 + 0.5 * value_noise(vec2<f32>(p.y / 8.0 + 5.0, seed * 23.0)) + 0.35 * (hair.x - 0.5)
        + 0.15 * (value_noise(vec2<f32>(p.y / 2.2, seed * 7.0)) - 0.5);
    let cover = smoothstep(end - 0.08, end + 0.08, inside);
    let streak = smoothstep(0.36, 0.66, value_noise(vec2<f32>(p.y / 5.2 + seed * 77.0, p.x / 210.0)));
    let hovered = step(0.5, look) * step(look, 1.5);
    let pressed = step(1.5, look);
    var color = mix(palette(select(16, 18, seed < 0.5)), palette(select(15, 17, seed > 0.3)), streak * 0.6);
    color = mix(color, mix(palette(17), vec3<f32>(1.0, 0.70, 0.22), streak * 0.4), hovered * 0.8);
    color *= (0.93 + 0.14 * hair.x) * (1.0 - 0.1 * pressed);
    let thickness = mix(mix(6.5, 8.5, hovered), 2.2, pressed);
    let furrows = mix(0.34, 1.0, step(0.5, seed));
    let height = thickness * (0.55 + furrows * 1.1 * (hair.x - 0.5) + 0.5 * gauss((inside - end) / 0.12));
    var paint = over(below, color, cover, height, cover * 0.9);
    // The label, in dark paint over the slab.
    let label = label_ink(here);
    let brushed = value_noise(p / vec2<f32>(9.0, 1.6));
    paint = over(paint, mix(vec3<f32>(0.03, 0.035, 0.13), vec3<f32>(0.06, 0.07, 0.22), brushed), label, (6.2 + 0.5 * brushed) * 0.8 * mix(1.0, 0.4, pressed), label * 0.8);
    return paint;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let texels = vec2<f32>(textureDimensions(input_texture));
    page_size = select(texels, u[63].xy, u[63].x > 0.0);
    unit = max(get_float(0u), 0.05);
    stage = page_size / unit;
    texels_per_unit = texels.x / page_size.x * unit;
    let head_em = get_float(1u);
    let body_em = get_float(2u);
    let look = get_float(3u);
    head_band = vec2<f32>(get_float(4u), get_float(5u));
    body_band = vec2<f32>(get_float(6u), get_float(7u));
    body_right = get_float(8u);
    let p = input.uv * stage;
    let here = key_at(p);

    var paint = ground(p);
    let field = head_field(p, here, head_em);
    let head_dark = selected(here) * step(0.05, field.letter);
    if (field.halo > 0.0) {
        paint = rim(paint, p, field);
    }
    paint = slab(paint, p, here, look);
    let in_head = step(head_band.x - 0.2 * head_em, p.y) * step(p.y, head_band.y + 0.2 * head_em);
    let line = mix(body_em * 1.34, head_em * 0.98, in_head);
    paint = wet_stroke(paint, p, line, in_head);
    paint = letters(paint, p, here, field, head_em, head_dark);
    paint = small_type(paint, p);

    let dither = (hash(input.uv * texels) - 0.5) / 255.0;
    let thickness = clamp(paint.height / MAX_HEIGHT + dither, 0.0, 1.0);
    return vec4<f32>(clamp(paint.color, vec3<f32>(0.0), vec3<f32>(1.0)), HEIGHT_FLOOR + (1.0 - HEIGHT_FLOOR) * thickness);
}
