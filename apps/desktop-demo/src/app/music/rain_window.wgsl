// A night seen through a wet window. Clouds and the moon lie behind the
// glass. On the glass: a fine condensation that catches the moonlight, flat
// drops in three sizes and of uneven shapes, and rivulets that wander down
// it. A drop on glass is a thin lens, not a ball: the night shows through
// it, its edge toward the light is dark, and light gathers at its far rim.
//
// The window moves a little with the time it is given: now and then a drop
// runs down it in fits and starts and leaves a wet path that breaks into
// beads, light flows along the rivulets, and the points of light twinkle.
//
// Uniforms: 0..2 the size of the window in dp, 2..4 the middle of the moon,
// 4 the time in seconds.

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn hash1(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// A smooth noise from 0 to 1.
fn noise1(p: vec2<f32>) -> f32 {
    let corner = floor(p);
    let part = fract(p);
    let ease = part * part * (vec2<f32>(3.0) - 2.0 * part);
    return mix(
        mix(hash1(corner), hash1(corner + vec2<f32>(1.0, 0.0)), ease.x),
        mix(hash1(corner + vec2<f32>(0.0, 1.0)), hash1(corner + vec2<f32>(1.0, 1.0)), ease.x),
        ease.y,
    );
}

const MOON_RADIUS: f32 = 30.0;
// The color of moonlight on water.
const WET: vec3<f32> = vec3<f32>(0.62, 0.76, 1.0);

// The night behind the glass at `p`, as far out of focus as `blur`, both
// in dp.
fn night(p: vec2<f32>, size: vec2<f32>, moon: vec2<f32>, blur: f32) -> vec3<f32> {
    let soft = max(blur, 1.0);
    let down = clamp(p.y / size.y, 0.0, 1.0);
    var color = mix(vec3<f32>(0.003, 0.006, 0.016), vec3<f32>(0.008, 0.016, 0.040), down);
    let cloud = noise1(p * 0.0045 + vec2<f32>(3.1, 1.7)) * 0.6
        + noise1(p * 0.011 + vec2<f32>(7.3, 2.9)) * 0.4;
    color = color * (0.45 + 1.1 * cloud);
    // The light of the moon in the wet air, and in the clouds around it.
    let to_moon = length(p - moon);
    color = color
        + vec3<f32>(0.060, 0.115, 0.230) * exp(-to_moon / 320.0) * (0.6 + 0.8 * cloud)
        + vec3<f32>(0.150, 0.230, 0.380) * exp(-to_moon / 95.0);
    let disc = 1.0 - smoothstep(MOON_RADIUS - soft, MOON_RADIUS + soft, to_moon);
    let face = 0.9 + 0.1 * noise1((p - moon) * 0.13 + vec2<f32>(11.0, 5.0));
    return mix(color, vec3<f32>(0.94, 0.97, 1.0) * face, disc);
}

// How much moonlight reaches a place on the glass.
fn moonlight(at: vec2<f32>, moon: vec2<f32>) -> f32 {
    return 0.3 + 0.7 * exp(-length(moon - at) / 380.0);
}

// The drop of one layer that lies over `p`, if one does.
struct GlassDrop {
    // The place in the drop: its length is 1 at the edge of the drop.
    q: vec2<f32>,
    center: vec2<f32>,
    radius: f32,
    // How much of the drop covers `p`.
    cover: f32,
}

// A layer of drops: one may sit in each cell of a grid `cell` dp wide,
// `share` of the cells hold one, and its radius lies between `least` and
// `most`.
fn drop_layer(p: vec2<f32>, cell: f32, share: f32, least: f32, most: f32, seed: f32) -> GlassDrop {
    let corner = floor(p / cell);
    let id = corner + vec2<f32>(seed, seed * 1.37);
    let pick = hash2(id);
    let present = step(1.0 - share, hash1(id + vec2<f32>(4.7, 9.2)));
    let radius = mix(least, most, pick.x * pick.x);
    // A drop hangs: it is taller than wide, and heavier at its lower end.
    let tall = 1.1 + 0.55 * pick.y;
    let reach = vec2<f32>(radius, radius * tall);
    let room = max(vec2<f32>(cell) - 2.0 * reach - vec2<f32>(2.0), vec2<f32>(0.0));
    let center = (corner + vec2<f32>(0.5)) * cell
        + (hash2(id + vec2<f32>(2.3, 7.9)) - vec2<f32>(0.5)) * room;
    var q = (p - center) / reach;
    q.x = q.x / (1.0 + 0.22 * q.y);
    // No drop is round: its edge wanders.
    let turn = atan2(q.y, q.x);
    let phase = pick.x * 6.283;
    let uneven = 1.0 + 0.06 * sin(turn * 2.0 + phase) + 0.04 * sin(turn * 4.0 + phase * 2.1);
    var drop: GlassDrop;
    drop.q = q / uneven;
    drop.center = center;
    drop.radius = radius;
    drop.cover = present * (1.0 - smoothstep(0.88, 1.0, length(drop.q)));
    return drop;
}

// What a flat drop shows: the night behind it, a dark edge toward the moon,
// light at the far rim and a point of light.
fn lens(drop: GlassDrop, size: vec2<f32>, moon: vec2<f32>, time: f32) -> vec3<f32> {
    let q = drop.q;
    let from_middle = length(q);
    let outward = q / max(from_middle, 0.001);
    let toward_moon = normalize(moon - drop.center + vec2<f32>(0.001, 0.0));
    let lit = moonlight(drop.center, moon);
    // A flat lens moves the night behind it only a little.
    var within = night(drop.center + q * drop.radius * 0.45, size, moon, 1.5) * 1.12;
    let rim = smoothstep(0.62, 0.98, from_middle);
    let far_side = clamp(dot(outward, -toward_moon) * 0.75 + outward.y * 0.45, 0.0, 1.0);
    let near_side = clamp(dot(outward, toward_moon) * 0.75 - outward.y * 0.45, 0.0, 1.0);
    within = within * (1.0 - 0.7 * rim * near_side);
    within = within + WET * rim * rim * far_side * 0.95 * lit;
    let glint_at = toward_moon * 0.42 + vec2<f32>(-0.10, -0.18);
    let glint = 1.0 - smoothstep(0.07, 0.17, length(q - glint_at));
    let twinkle = 0.82 + 0.18 * sin(time * 1.7 + drop.center.x * 0.37 + drop.center.y * 0.21);
    return within + vec3<f32>(1.0) * glint * 0.8 * lit * twinkle;
}

// What a channel of clear water on the glass shows at `p`, `depth` dp
// inside its edge and at `side` across it, from -1 to 1: the night behind
// it, a thin bright line along each edge, the brighter one away from the
// moon, and a dark line inside. `glitter` is how much light the edge
// catches here.
fn channel(
    p: vec2<f32>,
    depth: f32,
    side: f32,
    wide: f32,
    glitter: f32,
    size: vec2<f32>,
    moon: vec2<f32>,
) -> vec3<f32> {
    let moon_side = sign(moon.x - p.x);
    var water = night(p - vec2<f32>(side * wide * 0.8, 0.0), size, moon, 1.5) * 0.85;
    let dark_line = smoothstep(0.9, 1.7, depth) * (1.0 - smoothstep(1.7, 3.0, depth));
    water = water * (1.0 - 0.3 * dark_line);
    let edge_line = 1.0 - smoothstep(0.0, 1.2, depth);
    let far_side = clamp(-side * moon_side, 0.0, 1.0);
    return water + WET * edge_line * (0.3 + 0.6 * far_side) * glitter * moonlight(p, moon);
}

// The width of a chain of beads at the place `turn` along it, where each
// whole number starts a new bead: 0 between two beads, up to 1 in a bead.
// A bead is round, a little heavier at its lower end, and no two are of one
// size.
fn bead_width(turn: f32, seed: f32) -> f32 {
    let size = 0.62 + 0.38 * fract(sin((floor(turn) + seed) * 127.1) * 43758.5453);
    // From -1 at the upper end of the bead to 1 at its lower end.
    let along = (fract(turn) - 0.5) * 2.0 / (0.5 + 0.28 * size);
    let hung = along - 0.22 * (1.0 - along * along);
    return size * sqrt(max(1.0 - hung * hung, 0.0));
}

// How far to its side the path of a running drop lies at the height `y`.
fn run_path(y: f32, lap: vec2<f32>) -> f32 {
    return (noise1(vec2<f32>(y * 0.015, lap.x * 5.1 + lap.y * 2.3)) - 0.5) * 34.0;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let texture_size = vec2<f32>(textureDimensions(input_texture));
    let rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size = max(vec2<f32>(get_float(0u), get_float(1u)), vec2<f32>(1.0));
    let scale = max(rect.zw / size, vec2<f32>(0.001));
    let p = (input.uv * texture_size - rect.xy) / scale;
    let moon = vec2<f32>(get_float(2u), get_float(3u));
    let time = get_float(4u);
    let lit = moonlight(p, moon);

    var color = night(p, size, moon, 3.0);
    // Fine lines where water ran down the glass before.
    let ran = smoothstep(0.7, 0.95, noise1(vec2<f32>(p.x * 0.6, p.y * 0.012 + 2.0)))
        * (0.5 + 0.5 * noise1(vec2<f32>(p.x * 0.6 + 40.0, p.y * 0.05)));
    color = color + WET * ran * 0.05 * lit;

    // A rivulet may run in each lane of the glass. Its axis wanders, its
    // width swells and thins, and it ends in a drop.
    let lane = 104.0;
    let column = floor(p.x / lane);
    let id = vec2<f32>(column, 41.0);
    let phase = hash1(id) * 6.283;
    let wander = (noise1(vec2<f32>(p.y * 0.012 + phase, column * 3.7)) - 0.5) * 34.0
        + sin(p.y * 0.05 + phase) * 2.2;
    let axis = (column + 0.5) * lane + (hash1(id + vec2<f32>(8.0, 0.0)) - 0.5) * lane * 0.5 + wander;
    let top = hash1(id + vec2<f32>(12.0, 0.0)) * size.y * 0.4 - 60.0;
    let bottom = top + (0.5 + 0.6 * hash1(id + vec2<f32>(15.0, 0.0))) * size.y;
    let girth = 1.7 + 2.2 * hash1(id + vec2<f32>(19.0, 0.0));
    let swell = 0.5 + 0.5 * noise1(vec2<f32>(p.y * 0.07 + phase * 3.0, column * 1.3 + 5.0));
    let half_width = girth * swell;
    let end_radius = girth * 1.8;
    let end_at = vec2<f32>(axis, bottom - end_radius);
    let d_line = max(abs(p.x - axis) - half_width, p.y - end_at.y);
    let d_end = length((p - end_at) / vec2<f32>(1.0, 1.35)) - end_radius;
    let d_rivulet = min(d_line, d_end);
    let rivulet = (1.0 - smoothstep(-0.6, 0.4, d_rivulet))
        * smoothstep(top, top + 50.0, p.y)
        * step(0.3, hash1(id + vec2<f32>(23.0, 0.0)));
    if (rivulet > 0.0) {
        let wide = max(mix(half_width, end_radius, step(end_at.y, p.y)), 0.001);
        let side = clamp((p.x - axis) / wide, -1.0, 1.0);
        // The light on its edges flows down with the water.
        let glitter = 0.6 + 0.4 * noise1(vec2<f32>((p.y - time * 34.0) * 0.3, column * 7.1));
        color = mix(color, channel(p, -d_rivulet, side, wide, glitter, size, moon), rivulet);
    }

    // A drop may run down each wide lane of the glass, in fits and starts.
    // Each time it comes down it takes another path.
    let run_lane = 210.0;
    let run_column = floor(p.x / run_lane);
    let run_id = vec2<f32>(run_column, 67.0);
    let period = 16.0 + 24.0 * hash1(run_id + vec2<f32>(9.0, 0.0));
    let turns = time / period + hash1(run_id + vec2<f32>(13.0, 0.0));
    let lap = run_id + vec2<f32>(floor(turns) * 3.7, floor(turns));
    let part = fract(turns);
    let run = part + 0.9 * sin(part * 25.13) / 25.13;
    let run_y = run * (size.y + 260.0) - 130.0;
    let run_x = (run_column + 0.2 + 0.6 * hash1(lap + vec2<f32>(17.0, 0.0))) * run_lane;
    let run_radius = 3.2 + 2.4 * hash1(lap + vec2<f32>(21.0, 0.0));
    let running = step(0.35, hash1(lap + vec2<f32>(5.0, 0.0)));
    var runner: GlassDrop;
    runner.center = vec2<f32>(run_x + run_path(run_y, lap), run_y);
    runner.radius = run_radius;
    var run_q = (p - runner.center) / vec2<f32>(run_radius, run_radius * 1.35);
    run_q.x = run_q.x / (1.0 + 0.22 * run_q.y);
    runner.q = run_q;
    runner.cover = running * (1.0 - smoothstep(0.88, 1.0, length(run_q)));
    // The wet path over it: thinner with its age, then a chain of beads,
    // then nothing.
    let wake_length = 170.0;
    let behind = run_y - p.y;
    let wake_age = clamp(behind / wake_length, 0.0, 1.0);
    let wake_beads = mix(
        1.0,
        bead_width((p.y * 0.33 + lap.x * 2.1) / 6.2832, lap.x),
        smoothstep(0.15, 0.6, wake_age),
    );
    let wake_half = run_radius * 0.55 * (1.0 - 0.6 * wake_age) * wake_beads
        - 1.2 * (1.0 - smoothstep(0.0, 0.12, wake_beads));
    let wake_axis = run_x + run_path(p.y, lap);
    let d_wake = max(abs(p.x - wake_axis) - wake_half, max(-behind, behind - wake_length));
    let wake = running * (1.0 - smoothstep(-0.5, 0.4, d_wake)) * (1.0 - wake_age);
    if (wake > 0.0) {
        let wide = max(wake_half, 0.001);
        let side = clamp((p.x - wake_axis) / wide, -1.0, 1.0);
        color = mix(color, channel(p, -d_wake, side, wide, 0.9, size, moon), wake);
    }

    // The drops: the one that covers this place most.
    var drop = drop_layer(p, 170.0, 0.4, 4.5, 8.0, 11.0);
    let medium = drop_layer(p, 46.0, 0.38, 2.0, 4.6, 23.0);
    if (medium.cover > drop.cover) {
        drop = medium;
    }
    let small = drop_layer(p, 16.0, 0.34, 0.9, 2.2, 37.0);
    if (small.cover > drop.cover) {
        drop = small;
    }
    if (runner.cover > drop.cover) {
        drop = runner;
    }
    if (drop.cover > 0.0) {
        color = mix(color, lens(drop, size, moon, time), drop.cover);
    }

    // The condensation: points that catch the light, thicker in patches and
    // over the lower edge, where the rain of the roll lands.
    let grain = floor(p / 3.0);
    let speck_at = (grain + hash2(grain + vec2<f32>(1.3, 6.1))) * 3.0;
    let speck = 1.0 - smoothstep(0.35, 1.05, length(p - speck_at));
    // It lies in drifts: thick in some places and nearly absent in others.
    let drift = noise1(p * vec2<f32>(0.016, 0.006) + vec2<f32>(9.0, 4.0));
    let thick = 0.12 * smoothstep(0.5, 0.9, drift)
        + 0.2 * smoothstep(size.y - 150.0, size.y, p.y);
    let shine = hash1(grain + vec2<f32>(7.7, 2.2))
        * (0.8 + 0.2 * sin(time * 2.3 + grain.x * 1.9 + grain.y * 2.7));
    color = color
        + WET * speck * step(1.0 - thick, hash1(grain)) * (0.06 + 0.5 * shine * shine * shine) * lit
            * (1.0 - drop.cover);

    // The glass is darker toward its corners.
    let centered = (p / size - vec2<f32>(0.5)) * 2.0;
    color = color * (1.0 - 0.12 * dot(centered, centered));
    return vec4<f32>(color, 1.0);
}
