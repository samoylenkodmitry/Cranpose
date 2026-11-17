// The lamp room's floor, after `noise.wgsl`: polished cast concrete in
// large slabs. The lamp behind the wall lights it only through the holes
// of the plate: the scene's shadow map says where its light gets through,
// and the wall around the plate blocks the rest. Light falls off with the
// distance from the lamp. Near the wall the floor catches the plate's glow.
//
// The mesh lies in the scene's own coordinates. params: 0..1 = the
// stage's size; 2 = pixels to the unit; 3 = the floor line, where the
// plate ends; 4..6 = the lamp's colour, its strength included, linear;
// 8..10 = where the lamp stands.

const NIGHT: vec3<f32> = vec3<f32>(0.010, 0.016, 0.030);
// The light through a bare hole: the lamp, warmed by the box it hangs in.
const BARE_LAMP: vec3<f32> = vec3<f32>(1.0, 0.565, 0.225);
const FLOOR_TILE: f32 = 760.0;
const SLAB: f32 = 520.0;

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let unit = shaded_param(2u);
    let floor_line = shaded_param(3u);
    let lamp = param3(4u);
    let position = surface.position;
    let depth = -position.z;

    // The light reaches here only through the plate: the wall around it
    // stands whole.
    var through = surface.lit;
    let toward = surface.to_light;
    if (toward.z > 1.0e-4) {
        let crossing = position + toward * (-position.z / toward.z);
        // A little inside the plate's edge: past it the shadow map's
        // filter finds fewer casters than the wall around has.
        let margin = 6.0 * unit;
        let inside = vec4<f32>(crossing.x, size.x - crossing.x, crossing.y, floor_line - crossing.y);
        through *= smoothstep(0.0, margin, min(min(inside.x, inside.y), min(inside.z, inside.w)));
    } else {
        through = 0.0;
    }
    let to_lamp = param3(8u) - position;
    let distance2 = dot(to_lamp, to_lamp);
    let arrive = max(dot(normalize(to_lamp), surface.normal), 0.0) * size.y * size.y * 0.62 / distance2;

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
    let pool = through * arrive * 11.0;
    var color = albedo * (lamp * (BARE_LAMP * pool + 0.10 * near_wall) + NIGHT);
    color += lamp * 0.035 * near_wall * (0.6 + 0.4 * cloud);
    color *= 1.0 - 0.75 * joint;
    // The skirting board's contact shadow.
    color *= mix(0.12, 1.0, smoothstep(-4.0 * unit, 22.0 * unit, depth));
    return vec4<f32>(to_srgb(aces(color * 1.05)), 1.0) * surface.color.a;
}
