// The finish of the backrooms' surfaces: yellow wallpaper, damp carpet and
// ceiling tiles, lit by the room's fluorescent lamps, one over the middle
// of each unit, and lost in haze far from the eye. Content, as the entity's
// writing or a thing in a room, takes the same light. UNIT and HEIGHT come
// before this file, from plan.rs. Room coordinates: the floor at y = 0, y
// down.
//
// A surface's values:
//   0      what it shows: 0 wallpaper, 1 carpet, 2 ceiling tiles, 3 its
//          content, 4 the dark behind a doorway too deep to open
//   1-3    where the mesh's (0, 0, 0) lies in the room
//   4-6    the mesh's x in the room
//   7-9    its y
//   10-12  the eye, in the room
//   13     the camcorder's frame
//   14, 15 the room's width and depth
//   16     a number the room's lamps flicker by
//   17     how its lamps burn: 0 lit, 1 dim, 2 dark
//   18     how far its lamps are on, from 0 to 1
//   19-21  the mesh's z in the room

const WALLPAPER: u32 = 0u;
const CARPET: u32 = 1u;
const TILES: u32 = 2u;
const CONTENT: u32 = 3u;
const VOID: u32 = 4u;
const DIM: u32 = 1u;
const DARK: u32 = 2u;
// The distance at which the haze hides a third of what lies behind it.
const HAZE: f32 = 1500.0;
const LAMP_REACH: f32 = 230.0;
// The light every point takes, so no room is black: a little that bounces
// in from the rooms around, and the red emergency light that comes on as
// the lamps' power goes.
const FILL: vec3<f32> = vec3<f32>(0.05, 0.047, 0.04);
const EMERGENCY: vec3<f32> = vec3<f32>(0.26, 0.04, 0.025);
// How far a lamp's light reaches along the floor: it fades to nothing there,
// so the lamps of the 3 by 3 units around a point are all that light it.
const LAMP_WINDOW: f32 = UNIT * 1.5;

fn mix32(value: u32) -> u32 {
    var x = value;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    return x ^ (x >> 16u);
}

fn hash3(a: i32, b: i32, salt: u32) -> u32 {
    return mix32((bitcast<u32>(a) * 0x9e3779b1u) ^ mix32(bitcast<u32>(b) ^ salt));
}

// A hash as a number from 0 up to 1.
fn unit_of(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// Smooth noise from 0 to 1 over a lattice of unit squares.
fn value_noise(p: vec2<f32>, salt: u32) -> f32 {
    let cell = floor(p);
    let f = p - cell;
    let s = f * f * (3.0 - 2.0 * f);
    let i = vec2<i32>(cell);
    let a = unit_of(hash3(i.x, i.y, salt));
    let b = unit_of(hash3(i.x + 1, i.y, salt));
    let c = unit_of(hash3(i.x, i.y + 1, salt));
    let d = unit_of(hash3(i.x + 1, i.y + 1, salt));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

// How bright the lamp over unit `cell` of a room of `size` shines now,
// from 0 for a dead one to 1. The lamp over the room's middle unit never
// dies, so every room has some light.
fn lamp_level(cell: vec2<i32>, size: vec2<f32>, seed: u32, mood: u32, frame: i32) -> f32 {
    let pick = unit_of(hash3(cell.x, cell.y, seed));
    var dead = 0.08;
    var flickering = 0.15;
    var level = 1.0;
    if (mood == DIM) {
        dead = 0.4;
        flickering = 0.6;
        level = 0.6;
    } else if (mood == DARK) {
        dead = 0.6;
        flickering = 1.0;
    }
    let kept = all(cell == vec2<i32>(floor(size / (2.0 * UNIT))));
    if (pick < dead && !kept) {
        return 0.0;
    }
    if (pick >= flickering) {
        return level;
    }
    // A flickering tube holds steady for a while, then stutters.
    if (hash3(cell.x, cell.y ^ (frame >> 5u), seed ^ 0x9e3779b9u) < 0x80000000u) {
        return level;
    }
    let stutter = unit_of(hash3(cell.x + frame, cell.y, seed ^ 0x7f4a7c15u));
    return level * select(0.08, 1.0, stutter < 0.55);
}

// The light of a lamp over `middle` on a point `p` that faces `n`: the
// light that falls on it straight, and in y the share that bounces about.
// `reach` is the lamp's level, faded with the distance across the floor.
fn lamp_light(p: vec3<f32>, n: vec3<f32>, middle: vec2<f32>, reach: f32) -> vec2<f32> {
    let to_lamp = vec3<f32>(middle.x, -HEIGHT + 4.0, middle.y) - p;
    let distance_squared = dot(to_lamp, to_lamp);
    let facing = max(dot(n, to_lamp) * inverseSqrt(distance_squared), 0.0);
    let straight = 1.35 * (0.12 + 0.88 * facing) / (1.0 + distance_squared / (LAMP_REACH * LAMP_REACH));
    return vec2<f32>(straight, 1.0) * reach;
}

// The light on a point `p` that faces `n` in a room of `size`: the lamps of
// the 3 by 3 units around it, each fading to nothing within LAMP_WINDOW, so
// the light runs on unbroken; some light that bounces about; and the
// darkness that gathers where the walls meet the floor, the ceiling and each
// other.
fn lighting(p: vec3<f32>, n: vec3<f32>, size: vec2<f32>, seed: u32, mood: u32, frame: i32) -> f32 {
    let units = vec2<i32>(round(size / UNIT));
    let here = vec2<i32>(floor(p.xz / UNIT));
    var lamps = vec2<f32>(0.0);
    for (var dz = -1; dz <= 1; dz++) {
        for (var dx = -1; dx <= 1; dx++) {
            let cell = here + vec2<i32>(dx, dz);
            let inside = cell.x >= 0 && cell.y >= 0 && cell.x < units.x && cell.y < units.y;
            let middle = (vec2<f32>(cell) + 0.5) * UNIT;
            let across = middle - p.xz;
            let fade = max(1.0 - dot(across, across) / (LAMP_WINDOW * LAMP_WINDOW), 0.0);
            // The hashes only for a lamp of the room whose light reaches.
            if (inside && fade > 0.0) {
                let reach = fade * fade * lamp_level(cell, size, seed, mood, frame);
                if (reach > 0.0) {
                    lamps += lamp_light(p, n, middle, reach);
                }
            }
        }
    }
    let to_walls = min(p.xz, size - p.xz);
    var gloom = 1.0 - 0.32 * (1.0 - abs(n.x)) * exp(-max(to_walls.x, 0.0) / 34.0);
    gloom *= 1.0 - 0.32 * (1.0 - abs(n.z)) * exp(-max(to_walls.y, 0.0) / 34.0);
    let upright = 1.0 - abs(n.y);
    gloom *= 1.0 - 0.35 * upright * exp(p.y / 22.0);
    gloom *= 1.0 - 0.2 * upright * exp(-(p.y + HEIGHT) / 18.0);
    return (lamps.x + 0.2 * lamps.y) * gloom;
}

// Yellow wallpaper with faint stripes and a small pattern, gone brown with
// damp near the floor and streaked below the ceiling. `along` runs along
// the wall and `height` up from the floor; `pixel` is how wide a pixel is
// there, which fades detail too fine to show.
fn wallpaper(along: f32, height: f32, pixel: f32) -> vec3<f32> {
    let fine = clamp(1.0 - pixel / 6.0, 0.0, 1.0);
    let edge = max(pixel, 0.75);
    let from_stripe = abs(fract(along / 36.0) - 0.5) * 36.0;
    let stripe = mix(0.5, smoothstep(9.0 + edge, 9.0 - edge, from_stripe), fine);
    var colour = vec3<f32>(0.80, 0.72, 0.41) * (0.965 + 0.07 * stripe);
    let row = round(height / 44.0);
    let shift = 0.5 * (row % 2.0);
    let motif = vec2<f32>(along - (round(along / 36.0 + shift) - shift) * 36.0, height - row * 44.0);
    let diamond = abs(motif.x) / 4.5 + abs(motif.y) / 6.5;
    colour *= 1.0 - 0.06 * fine * smoothstep(1.0 + edge / 4.0, 1.0 - edge / 4.0, diamond);
    colour *= 0.94 + 0.12 * value_noise(vec2<f32>(along / 520.0, height / 400.0), 0x3c6ef372u);
    let damp = value_noise(vec2<f32>(along / 75.0, height / 120.0), 0xa54ff53au);
    let low = smoothstep(190.0, 15.0, height);
    let wet = damp + 0.2 * low - 0.1;
    let stain = smoothstep(0.58, 0.74, wet) * low;
    let rim = smoothstep(0.55, 0.6, wet) - smoothstep(0.6, 0.66, wet);
    colour = mix(colour, vec3<f32>(0.60, 0.48, 0.24), stain * 0.75);
    colour *= 1.0 - 0.18 * rim * low;
    let drip = value_noise(vec2<f32>(along / 8.0, height / 240.0), 0x510e527fu);
    colour *= 1.0 - 0.14 * smoothstep(0.7, 0.9, drip) * smoothstep(110.0, 270.0, height) * fine;
    return colour * (1.0 - 0.25 * exp(-height / 14.0));
}

// Damp mustard carpet.
fn carpet(at: vec2<f32>, pixel: f32) -> vec3<f32> {
    let fine = clamp(1.0 - pixel / 5.0, 0.0, 1.0);
    let fibre = unit_of(hash3(i32(floor(at.x / 2.5)), i32(floor(at.y / 2.5)), 0x9b05688cu)) - 0.5;
    var colour = vec3<f32>(0.55, 0.47, 0.27) * (1.0 + 0.2 * fibre * fine);
    let damp = smoothstep(0.56, 0.82, value_noise(at / 170.0, 0x1f83d9abu));
    colour = mix(colour, vec3<f32>(0.33, 0.27, 0.15), damp * 0.75);
    return colour * (0.9 + 0.2 * value_noise(at / 650.0, 0x5be0cd19u));
}

// The ceiling: tiles in a grid, some stained and one now and then missing,
// and over the middle of each unit its lamp, lit, flickering or dead.
// Returns the colour the light falls on in rgb, and in a 1 where the lamp
// gives off light of its own.
fn tiles(at: vec2<f32>, size: vec2<f32>, pixel: f32, seed: u32, mood: u32, frame: i32, power: f32) -> vec4<f32> {
    let cell = vec2<i32>(floor(at / UNIT));
    let from_middle = at - (vec2<f32>(cell) + 0.5) * UNIT;
    let to_panel = abs(from_middle) - vec2<f32>(25.0, 75.0);
    let level = lamp_level(cell, size, seed, mood, frame) * power;
    if (max(to_panel.x, to_panel.y) < 0.0) {
        if (max(to_panel.x, to_panel.y) > -3.0) {
            return vec4<f32>(0.62, 0.61, 0.56, 0.0);
        }
        let lens = 0.92 + 0.08 * step(0.5, fract(from_middle.y / 6.0)) * clamp(1.0 - pixel / 3.0, 0.0, 1.0);
        return vec4<f32>(vec3<f32>(1.0, 0.98, 0.9) * (0.3 + 1.6 * level) * lens, 1.0);
    }
    let tile_at = (at + vec2<f32>(25.0)) / 50.0;
    let tile = floor(tile_at);
    let within = (fract(tile_at) - 0.5) * 50.0;
    let seam = smoothstep(25.0 - 1.2 - pixel, 25.0 - 1.2 + pixel, max(abs(within.x), abs(within.y)));
    let pick = hash3(i32(tile.x), i32(tile.y), seed ^ 0x6a09e667u);
    if (pick < 0x01800000u) {
        return vec4<f32>(vec3<f32>(0.015) * (1.0 - seam) + vec3<f32>(0.5, 0.49, 0.42) * seam, 0.0);
    }
    let speckle = unit_of(hash3(i32(floor(at.x / 1.5)), i32(floor(at.y / 1.5)), 0xbb67ae85u)) - 0.5;
    var colour = vec3<f32>(0.70, 0.68, 0.57) * (1.0 + 0.1 * speckle * clamp(1.0 - pixel / 2.0, 0.0, 1.0));
    if (pick < 0x14000000u) {
        let ring = length(within - (vec2<f32>(unit_of(pick * 747796405u), unit_of(pick * 2891336453u)) - 0.5) * 20.0);
        let stain = smoothstep(17.0, 9.0, ring + 6.0 * value_noise(within / 6.0, pick));
        colour = mix(colour, vec3<f32>(0.62, 0.50, 0.30), stain * 0.6);
    }
    colour = mix(colour, vec3<f32>(0.56, 0.55, 0.47), seam);
    // The light a lamp spills on the tiles around it.
    let spill = exp(-max(max(to_panel.x, to_panel.y), 0.0) / 38.0) * level;
    return vec4<f32>(colour * (1.0 + spill * 0.55), 0.0);
}

// A param vector of three, from `first`.
fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let kind = u32(shaded_param(0u));
    let x = param3(4u);
    let y = param3(7u);
    let z = param3(19u);
    let eye = param3(10u);
    let frame = i32(shaded_param(13u));
    let size = vec2<f32>(shaded_param(14u), shaded_param(15u));
    let seed = u32(shaded_param(16u));
    let mood = u32(shaded_param(17u));
    let power = shaded_param(18u);
    let at = surface.position;
    let p = param3(1u) + x * at.x + y * at.y + z * at.z;
    // The side of the surface the eye sees: the normal turns the way the
    // inverse of the map's transpose turns it, which for square axes scales
    // each by one over its length squared.
    let m = surface.normal;
    let n = normalize(x * (m.x / dot(x, x)) + y * (m.y / dot(y, y)) + z * (m.z / dot(z, z)));
    let distance = length(p - eye);
    // A pixel's width there, for a camera that shows 1.2 radians across
    // about 700 pixels.
    let pixel = distance * 0.0017;
    let shadow = mix(0.6, 1.0, surface.lit);
    let fill = FILL + EMERGENCY * (1.0 - power);
    var colour = vec3<f32>(0.0);
    var alpha = 1.0;
    if (kind == CARPET) {
        let light = lighting(p, n, size, seed, mood, frame) * power * shadow;
        colour = carpet(p.xz, pixel) * (vec3<f32>(light) + fill);
    } else if (kind == TILES) {
        let ceiling = tiles(p.xz, size, pixel, seed, mood, frame, power);
        if (ceiling.a > 0.5) {
            colour = ceiling.rgb;
        } else {
            let light = lighting(p, n, size, seed, mood, frame) * power * shadow;
            colour = ceiling.rgb * (vec3<f32>(light) + fill);
        }
    } else if (kind != VOID) {
        var paint = vec3<f32>(0.0);
        if (kind == CONTENT) {
            alpha = surface.color.a;
            paint = surface.color.rgb / max(alpha, 1.0e-4);
        } else {
            let along = select(p.z, p.x, abs(x.x) > abs(x.z));
            paint = wallpaper(along, -p.y, pixel);
        }
        let light = lighting(p, n, size, seed, mood, frame) * power * shadow;
        colour = paint * (vec3<f32>(light) + fill);
    }
    let haze = 1.0 - exp(-distance / HAZE);
    colour = mix(colour, vec3<f32>(0.06, 0.05, 0.025), haze);
    colour = (1.0 - exp(-colour * 1.6)) * 1.1;
    // The grain hashes the ray from the eye in the room: the mesh's own
    // coordinates stretch with the mesh, so their ray holds still across
    // many pixels of a long wall.
    let ray = (p - eye) / max(distance, 1.0e-3);
    let grain = unit_of(hash3(i32(ray.x * 4096.0) ^ (frame * 7919), i32(ray.y * 4096.0) + i32(ray.z * 4096.0) * 4099, 0x2545f491u)) - 0.5;
    colour = clamp(colour + vec3<f32>(grain * 0.07), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(colour * alpha, alpha);
}
