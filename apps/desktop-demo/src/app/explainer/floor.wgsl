// The lamp room's floor, after `noise.wgsl`: polished cast concrete in
// large slabs. The floor is a mesh of the plate's content, so it reads
// what the plate lets through: the lamp stands behind the wall, and the
// ray from a floor point to the lamp crosses the wall at one point, where
// the plate's opening and its colour are the light that arrives. The lamp
// has a size: the farther the floor point lies from the wall, the larger
// the disc of wall it sees the lamp through, and the softer the light.
// Light falls off with the distance from the lamp. Near the wall the
// floor catches the plate's glow. The plate's picture holds, below the
// wall, the air in front of the floor and the wall mirrored in its polish,
// which the floor adds at its own point of the screen.
//
// The mesh lies in the scene's own coordinates, and the plate's picture
// covers the stage. params: 0..1 = the stage's size; 2 = pixels to the
// unit; 3 = the floor line, where the wall ends; 4..6 = the lamp's
// colour, its strength included, linear; 7 = the lamp's radius; 8..10 =
// where the lamp stands; 11 = how opaque a bare opening of the plate is,
// 12 = an opening of glass; 13 = the eye's distance from the wall; 14 =
// the height of the wall's point straight ahead of the eye.

const NIGHT: vec3<f32> = vec3<f32>(0.010, 0.016, 0.030);
const FLOOR_TILE: f32 = 760.0;
const SLAB: f32 = 520.0;
const GOLDEN_ANGLE: f32 = 2.399963;
const TAPS: i32 = 24;
// The wall shows a pane lit by the lamp box behind it; the floor sees the
// lamp itself through it, this much brighter.
const GLASS_BOOST: f32 = 10.0;

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// What `aces` maps to `y`, for y below 1.
fn aces_inverse(y: vec3<f32>) -> vec3<f32> {
    let v = min(y, vec3<f32>(0.98));
    let a = 2.43 * v - 2.51;
    let b = 0.59 * v - 0.03;
    return (-b - sqrt(b * b - 4.0 * a * 0.14 * v)) / (2.0 * a);
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// The plate's picture at `at`, a point of the stage, its tone mapping
// undone.
fn picture(at: vec2<f32>, stage: vec2<f32>) -> vec4<f32> {
    let texel = content_color(at / stage);
    return vec4<f32>(aces_inverse(to_linear(texel.rgb)), texel.a);
}

// The light the plate lets through at `at`, a point of the wall: the
// colour seen in an opening there times how open the plate is, brighter
// through glass. Past the wall's edge the wall stands whole.
fn through_plate(at: vec2<f32>, stage: vec2<f32>, wall: vec2<f32>) -> vec3<f32> {
    if (any(at < vec2<f32>(0.0)) || any(at > wall)) {
        return vec3<f32>(0.0);
    }
    let seen = picture(at, stage);
    let bare = shaded_param(11u);
    let glass = shaded_param(12u);
    let open = clamp((1.0 - seen.a) / (1.0 - bare), 0.0, 1.0);
    let glazed = clamp((bare - seen.a) / (bare - glass), 0.0, 1.0);
    return seen.rgb * open * (1.0 + (GLASS_BOOST - 1.0) * glazed);
}

// Where the eye sees `position` on the stage.
fn on_stage(position: vec3<f32>, stage: vec2<f32>) -> vec2<f32> {
    let distance = shaded_param(13u);
    let shear = (shaded_param(14u) - stage.y * 0.5) / distance;
    let view = vec3<f32>(position.x - stage.x * 0.5, position.y + shear * position.z - stage.y * 0.5, position.z + distance);
    return view.xy * distance / view.z + stage * 0.5;
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let unit = shaded_param(2u);
    let floor_line = shaded_param(3u);
    let lamp = param3(4u);
    let lamp_at = param3(8u);
    let position = surface.position;
    let depth = -position.z;

    // Where the ray from here to the lamp crosses the wall, and the disc
    // of wall the lamp shows through, widened to the pixel's footprint.
    let toward = lamp_at - position;
    let crossing = position.xy + toward.xy * (depth / toward.z);
    let along = lamp_at.z / toward.z;
    let footprint = length(fwidth(crossing)) * 0.6;
    let penumbra = max(shaded_param(7u) * (1.0 - along) + 0.4, footprint);
    let wall = vec2<f32>(size.x, floor_line);
    // The taps turn by a random angle from pixel to pixel, so their pattern
    // averages out over neighbours.
    let seen_at = on_stage(position, size);
    let turn = hash(floor(seen_at * 2.0)) * 6.2831853;
    var through = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < TAPS; i++) {
        let f = (f32(i) + 0.5) / f32(TAPS);
        let a = f32(i) * GOLDEN_ANGLE + turn;
        let w = exp(-f * 1.6);
        through += through_plate(crossing + sqrt(f) * vec2<f32>(cos(a), sin(a)) * penumbra, size, wall) * w;
        total += w;
    }
    through /= total;
    let air = picture(seen_at, size).rgb;
    let distance2 = dot(toward, toward);
    let arrive = max(dot(normalize(toward), surface.normal), 0.0) * size.y * size.y * 0.62 / distance2;

    // Concrete: a cloudy grey with fine grain, cast in slabs.
    let at = vec2<f32>(position.x, depth) / unit;
    let cloud = value_noise(at / FLOOR_TILE * 6.0) * 0.6 + value_noise(at / FLOOR_TILE * 19.0) * 0.3 + value_noise(at * 0.35) * 0.1;
    let albedo = 0.13 + 0.10 * (cloud - 0.5);
    let slab = vec2<f32>((position.x - size.x * 0.5) / (unit * SLAB) + 0.5, (depth + unit * 120.0) / (unit * SLAB));
    let to_joint = abs(fract(slab) - 0.5);
    let joint_width = fwidth(slab) * 0.8 + vec2<f32>(0.002);
    let joints = smoothstep(vec2<f32>(0.5) - joint_width, vec2<f32>(0.5), to_joint);
    let joint = max(joints.x, joints.y);

    // The plate's glow on the floor near the wall, and the floor's sheen
    // where it mirrors the lit wall.
    let near_wall = exp(-depth / (size.y * 0.10));
    var color = albedo * (through * arrive * 2.4 + lamp * 0.10 * near_wall + NIGHT);
    color += lamp * 0.035 * near_wall * (0.6 + 0.4 * cloud);
    color *= 1.0 - 0.75 * joint;
    color += air * (1.0 - 0.7 * joint);
    // The skirting board's contact shadow.
    color *= mix(0.12, 1.0, smoothstep(-4.0 * unit, 22.0 * unit, depth));
    return vec4<f32>(to_srgb(aces(color * 1.05)), 1.0);
}
