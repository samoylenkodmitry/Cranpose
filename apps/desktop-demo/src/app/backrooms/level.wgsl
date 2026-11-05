// The backrooms, drawn ray by ray inside a box that follows the eye. The
// maze's constants come before this file, from maze.rs, whose hashes these
// mirror. Coordinates are the box's: the scene's, less the corner of the
// maze cell `origin`, so they stay small however far the eye walks.
//
// The box's values:
//   0      the camcorder's frames so far
//   1, 2   the maze cell at the box's origin
//   3      how far a blackout dims every lamp, from 0 to 1
//   4, 5   where the figure stands on the floor
//   6      1 while the figure stands there
//   7      how far the eye sees, in scene units

const FAR: f32 = 1.0e9;
// How many cells a ray crosses at most.
const MOST_CELLS: i32 = 28;
// The distance at which the haze hides a third of what lies behind it.
const HAZE: f32 = 2200.0;
const LAMP_REACH: f32 = 260.0;
// How far a lamp's light reaches along the floor: it fades to nothing there,
// so the lamps of the 3 by 3 cells around a point are all that light it.
const LAMP_WINDOW: f32 = CELL * 1.5;

const NOTHING: u32 = 0u;
const WALL: u32 = 1u;
const FLOOR: u32 = 2u;
const CEILING_TILES: u32 = 3u;
const FIGURE: u32 = 4u;

const OPEN_SIDE: u32 = 0u;
const WALL_SIDE: u32 = 1u;
const DOOR_SIDE: u32 = 2u;

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
fn unit(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// Smooth noise from 0 to 1 over a lattice of unit squares.
fn value_noise(p: vec2<f32>, salt: u32) -> f32 {
    let cell = floor(p);
    let f = p - cell;
    let s = f * f * (3.0 - 2.0 * f);
    let i = vec2<i32>(cell);
    let a = unit(hash3(i.x, i.y, salt));
    let b = unit(hash3(i.x + 1, i.y, salt));
    let c = unit(hash3(i.x, i.y + 1, salt));
    let d = unit(hash3(i.x + 1, i.y + 1, salt));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

struct Side {
    kind: u32,
    // A doorway's middle, from the side's start.
    middle: f32,
}

// Side::on in maze.rs.
fn side_on(line: i32, along: i32, salt: u32) -> Side {
    var side = Side(OPEN_SIDE, 0.0);
    let phase = i32(hash3(line, 0, salt) & RUN_MASK);
    if (hash3(line, (along + phase) >> RUN_SHIFT, salt ^ RUN_SALT) >= WALL_RUN) {
        return side;
    }
    let pick = hash3(line, along, salt ^ PICK_SALT);
    if (pick < GAP) {
        return side;
    }
    if (pick < GAP_OR_DOOR) {
        let shift = f32(pick & 0xffffu) * (2.0 / 65535.0) - 1.0;
        side.kind = DOOR_SIDE;
        side.middle = CELL * 0.5 + DOOR_SHIFT * shift;
        return side;
    }
    side.kind = WALL_SIDE;
    return side;
}

// Cell::pick in maze.rs.
fn cell_pick(g: vec2<i32>) -> u32 {
    return hash3(g.x, g.y, CELL_SALT);
}

fn has_pillar(g: vec2<i32>) -> bool {
    return cell_pick(g) < PILLAR;
}

// How bright a cell's lamp shines now, from 0 for none or a dead one to 1
// (Cell::lamp in maze.rs, with the flicker).
fn lamp_level(g: vec2<i32>, frame: i32) -> f32 {
    let cell = cell_pick(g);
    if (cell < PILLAR) {
        return 0.0;
    }
    let pick = (cell << 16u) | (cell >> 16u);
    var flickering = false;
    if (hash3(g.x >> 2u, g.y >> 2u, DARK_SALT) < DARK_BLOCK) {
        if (pick >= FLICKERING_IN_DARK) {
            return 0.0;
        }
        flickering = true;
    } else if (pick < DEAD) {
        return 0.0;
    } else if (pick < DEAD_OR_FLICKERING) {
        flickering = true;
    }
    if (!flickering) {
        return 1.0;
    }
    // A flickering tube holds steady for a while, then stutters.
    if (hash3(g.x, g.y ^ (frame >> 5u), CELL_SALT ^ 0x9e3779b9u) < 0x80000000u) {
        return 1.0;
    }
    let stutter = unit(hash3(g.x + frame, g.y, CELL_SALT ^ 0x7f4a7c15u));
    return select(0.08, 1.0, stutter < 0.55);
}

struct Hit {
    t: f32,
    normal: vec3<f32>,
    surface: u32,
}

// Where a ray meets the wall of a side as it crosses the wall's thickness,
// the ray being in the wall from `span.x` to `span.y`; `face` is 1 where
// it meets the wall's face or end, 3 a doorway's jamb and 4 its lintel,
// and 0 where it passes through the doorway.
struct Meet {
    t: f32,
    face: u32,
}

// The part of [earliest, latest] in which `o + t d`, along one axis, lies
// between `low` and `high`; empty when x > y.
fn span(o: f32, d: f32, low: f32, high: f32, earliest: f32, latest: f32) -> vec2<f32> {
    if (abs(d) < 1.0e-7) {
        if (o >= low && o <= high) {
            return vec2<f32>(earliest, latest);
        }
        return vec2<f32>(1.0, 0.0);
    }
    let a = (low - o) / d;
    let b = (high - o) / d;
    return vec2<f32>(max(min(a, b), earliest), min(max(a, b), latest));
}

// The meeting of a ray with a side's wall, whose thickness it crosses over
// `inside`; `along` and `up` give the ray's start and direction along the
// side and down, and `start` is where the side starts along it.
fn meet_side(side: Side, inside: vec2<f32>, along: vec2<f32>, up: vec2<f32>, start: f32) -> Meet {
    if (side.kind == WALL_SIDE) {
        return Meet(inside.x, 1u);
    }
    let middle = start + side.middle;
    let at_along = along.x + along.y * inside.x;
    let at_up = up.x + up.y * inside.x;
    if (abs(at_along - middle) > DOOR_HALF_WIDTH || at_up < -DOOR_HEIGHT) {
        return Meet(inside.x, 1u);
    }
    // In the doorway: the jamb or the lintel the ray leaves it by, if it
    // leaves before it is through the wall.
    var jamb = FAR;
    if (along.y > 1.0e-7) {
        jamb = (middle + DOOR_HALF_WIDTH - along.x) / along.y;
    } else if (along.y < -1.0e-7) {
        jamb = (middle - DOOR_HALF_WIDTH - along.x) / along.y;
    }
    var lintel = FAR;
    if (up.y < -1.0e-7) {
        lintel = (-DOOR_HEIGHT - up.x) / up.y;
    }
    let leaves = min(jamb, lintel);
    if (leaves >= inside.y) {
        return Meet(FAR, 0u);
    }
    return Meet(leaves, select(4u, 3u, jamb < lintel));
}

// The normal of a side's face that a meeting found: the wall's face, which
// looks `facing`, the face of the cell the ray came in by, a jamb, which
// looks against the ray along the side, or the lintel, which looks down.
fn side_normal(face: u32, facing: vec3<f32>, entry: vec3<f32>, jamb: vec3<f32>, entered: bool) -> vec3<f32> {
    if (face == 3u) {
        return jamb;
    }
    if (face == 4u) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return select(facing, entry, entered);
}

// A thin figure that stands at `at` on the floor, facing the eye: a body,
// two long arms and a head.
fn meet_figure(o: vec3<f32>, d: vec3<f32>, at: vec2<f32>) -> f32 {
    let toward = normalize(o.xz - at + vec2<f32>(1.0e-3, 0.0));
    let aside = vec2<f32>(-toward.y, toward.x);
    var t = meet_column(o, d, at, 16.0, 190.0, 0.0);
    t = min(t, meet_column(o, d, at + aside * 23.0, 4.5, 178.0, 70.0));
    t = min(t, meet_column(o, d, at - aside * 23.0, 4.5, 178.0, 70.0));
    let head = o - vec3<f32>(at.x, -206.0, at.y);
    let b = dot(head, d);
    let h = b * b - dot(head, head) + 14.0 * 14.0;
    if (h > 0.0) {
        let near = -b - sqrt(h);
        if (near > 0.0) {
            t = min(t, near);
        }
    }
    return t;
}

// An upright column of `radius` around `at`, from `bottom` to `top` above
// the floor.
fn meet_column(o: vec3<f32>, d: vec3<f32>, at: vec2<f32>, radius: f32, top: f32, bottom: f32) -> f32 {
    let off = o.xz - at;
    let a = dot(d.xz, d.xz);
    let b = dot(off, d.xz);
    let h = b * b - a * (dot(off, off) - radius * radius);
    if (h <= 0.0 || a < 1.0e-8) {
        return FAR;
    }
    let near = (-b - sqrt(h)) / a;
    let y = o.y + d.y * near;
    if (near <= 0.0 || y < -top || y > -bottom) {
        return FAR;
    }
    return near;
}

// The first thing a ray from `o` along `d` meets, within `sight`.
fn trace(o: vec3<f32>, d: vec3<f32>, origin: vec2<i32>, sight: f32) -> Hit {
    var hit = Hit(sight, vec3<f32>(0.0), NOTHING);
    if (d.y > 1.0e-5) {
        let t = -o.y / d.y;
        if (t < hit.t) {
            hit = Hit(t, vec3<f32>(0.0, -1.0, 0.0), FLOOR);
        }
    } else if (d.y < -1.0e-5) {
        let t = (-CEILING - o.y) / d.y;
        if (t < hit.t) {
            hit = Hit(t, vec3<f32>(0.0, 1.0, 0.0), CEILING_TILES);
        }
    }
    if (volume_param(6u) > 0.5) {
        let t = meet_figure(o, d, vec2<f32>(volume_param(4u), volume_param(5u)));
        if (t < hit.t) {
            hit = Hit(t, vec3<f32>(0.0), FIGURE);
        }
    }
    var cell = vec2<i32>(floor(o.xz / CELL));
    let forward = d.xz >= vec2<f32>(0.0);
    let stepping = select(vec2<i32>(-1), vec2<i32>(1), forward);
    let safe = select(d.xz, select(vec2<f32>(-1.0e-7), vec2<f32>(1.0e-7), forward), abs(d.xz) < vec2<f32>(1.0e-7));
    let inverse = 1.0 / safe;
    var next = (vec2<f32>(cell + select(vec2<i32>(0), vec2<i32>(1), forward)) * CELL - o.xz) * inverse;
    let delta = abs(vec2<f32>(CELL) * inverse);
    var t_in = 0.0;
    var entry = vec3<f32>(0.0);
    var entered_across_x = false;
    // Which way the jambs of a doorway face: against the ray along its side.
    let jamb_x = vec3<f32>(-sign(safe.x), 0.0, 0.0);
    let jamb_z = vec3<f32>(0.0, 0.0, -sign(safe.y));
    for (var crossed = 0; crossed < MOST_CELLS; crossed++) {
        if (t_in >= hit.t) {
            break;
        }
        let t_out = min(min(next.x, next.y), hit.t);
        let g = cell + origin;
        let low = vec2<f32>(cell) * CELL;
        let high = low + vec2<f32>(CELL);
        var near = Meet(FAR, 0u);
        var normal = vec3<f32>(0.0);
        // The side ahead across x, whose whole wall the ray may cross, and
        // the end of the wall behind, which a ray that came in by a side
        // across z near the corner may meet at once.
        let ahead_x = select(low.x, high.x, forward.x);
        let wall_x = span(o.x, safe.x, ahead_x - WALL_HALF, ahead_x + WALL_HALF, t_in, FAR);
        if (wall_x.x <= wall_x.y && wall_x.x <= t_out) {
            let side = side_on(g.x + select(0, 1, forward.x), g.y, SIDE_ACROSS_X);
            if (side.kind != OPEN_SIDE) {
                let met = meet_side(side, wall_x, vec2<f32>(o.z, d.z), vec2<f32>(o.y, d.y), low.y);
                if (met.t < near.t) {
                    near = met;
                    let facing = vec3<f32>(-f32(stepping.x), 0.0, 0.0);
                    normal = side_normal(met.face, facing, entry, jamb_z, met.t <= t_in);
                }
            }
        }
        let ahead_z = select(low.y, high.y, forward.y);
        let wall_z = span(o.z, safe.y, ahead_z - WALL_HALF, ahead_z + WALL_HALF, t_in, FAR);
        if (wall_z.x <= wall_z.y && wall_z.x <= t_out) {
            let side = side_on(g.y + select(0, 1, forward.y), g.x, SIDE_ACROSS_Z);
            if (side.kind != OPEN_SIDE) {
                let met = meet_side(side, wall_z, vec2<f32>(o.x, d.x), vec2<f32>(o.y, d.y), low.x);
                if (met.t < near.t) {
                    near = met;
                    let facing = vec3<f32>(0.0, 0.0, -f32(stepping.y));
                    normal = side_normal(met.face, facing, entry, jamb_x, met.t <= t_in);
                }
            }
        }
        if (crossed > 0) {
            // The wall end behind, on the side the ray did not come in by.
            let at = o + d * t_in;
            if (entered_across_x) {
                let behind = select(high.y, low.y, forward.y);
                if (abs(at.z - behind) <= WALL_HALF && t_in < near.t) {
                    let side = side_on(g.y + select(1, 0, forward.y), g.x, SIDE_ACROSS_Z);
                    if (side.kind != OPEN_SIDE) {
                        near = Meet(t_in, 1u);
                        normal = entry;
                    }
                }
            } else {
                let behind = select(high.x, low.x, forward.x);
                if (abs(at.x - behind) <= WALL_HALF && t_in < near.t) {
                    let side = side_on(g.x + select(1, 0, forward.x), g.y, SIDE_ACROSS_X);
                    if (side.kind != OPEN_SIDE) {
                        near = Meet(t_in, 1u);
                        normal = entry;
                    }
                }
            }
        }
        // A pillar in the middle of the cell.
        let middle = low + vec2<f32>(CELL * 0.5);
        let pillar_x = span(o.x, safe.x, middle.x - PILLAR_HALF, middle.x + PILLAR_HALF, t_in, t_out);
        let pillar_z = span(o.z, safe.y, middle.y - PILLAR_HALF, middle.y + PILLAR_HALF, t_in, t_out);
        let enters = max(pillar_x.x, pillar_z.x);
        if (enters <= min(pillar_x.y, pillar_z.y) && enters < near.t && has_pillar(g)) {
            near = Meet(enters, 1u);
            normal = select(vec3<f32>(0.0, 0.0, -f32(stepping.y)), vec3<f32>(-f32(stepping.x), 0.0, 0.0), pillar_x.x >= pillar_z.x);
        }
        if (near.t < hit.t) {
            hit = Hit(near.t, normal, WALL);
            break;
        }
        if (next.x < next.y) {
            t_in = next.x;
            next.x += delta.x;
            cell.x += stepping.x;
            entry = vec3<f32>(-f32(stepping.x), 0.0, 0.0);
            entered_across_x = true;
        } else {
            t_in = next.y;
            next.y += delta.y;
            cell.y += stepping.y;
            entry = vec3<f32>(0.0, 0.0, -f32(stepping.y));
            entered_across_x = false;
        }
    }
    return hit;
}

// The light of a lamp that hangs over `middle` on a point `p` that faces
// `n`: the light that falls on it straight, and in y the lamp's share of
// the light that bounces about. `reach` is the lamp's level, faded with
// the distance across the floor.
fn lamp_light(p: vec3<f32>, n: vec3<f32>, middle: vec2<f32>, reach: f32) -> vec2<f32> {
    let to_lamp = vec3<f32>(middle.x, -CEILING + 4.0, middle.y) - p;
    let distance_squared = dot(to_lamp, to_lamp);
    let facing = max(dot(n, to_lamp) * inverseSqrt(distance_squared), 0.0);
    let straight = 1.35 * (0.12 + 0.88 * facing) / (1.0 + distance_squared / (LAMP_REACH * LAMP_REACH));
    return vec2<f32>(straight, 1.0) * reach;
}

// How much a side's wall darkens a point `distance` from it, `across` being
// how far the point's surface turns from the wall: a surface in the wall's
// own plane takes none of it.
fn shade_by(side: Side, distance: f32, across: f32) -> f32 {
    return 1.0 - select(0.32, 0.0, side.kind == OPEN_SIDE) * across * exp(-max(distance, 0.0) / 34.0);
}

// The light on a point `p` that faces `n`: the lamps of the 3 by 3 cells
// around the cell it faces into, each fading to nothing within
// `LAMP_WINDOW`, so the light runs on unbroken from cell to cell; some
// light that bounces about; and the darkness that gathers where the walls
// meet the floor, the ceiling and each other. A wall turned away from a
// lamp takes little of its light, but the floor near a wall takes the light
// of a lamp behind it.
fn lighting(p: vec3<f32>, n: vec3<f32>, origin: vec2<i32>, frame: i32, dim: f32) -> f32 {
    let local = vec2<i32>(floor((p.xz + n.xz * 2.0) / CELL));
    let g = local + origin;
    let low = vec2<f32>(local) * CELL;
    var lamps = vec2<f32>(0.0);
    for (var dz = -1; dz <= 1; dz++) {
        for (var dx = -1; dx <= 1; dx++) {
            let middle = low + (vec2<f32>(f32(dx), f32(dz)) + 0.5) * CELL;
            let across = middle - p.xz;
            let fade = max(1.0 - dot(across, across) / (LAMP_WINDOW * LAMP_WINDOW), 0.0);
            // The hashes only for a lamp whose light reaches.
            if (fade > 0.0) {
                let reach = fade * fade * lamp_level(g + vec2<i32>(dx, dz), frame);
                if (reach > 0.0) {
                    lamps += lamp_light(p, n, middle, reach);
                }
            }
        }
    }
    let light = lamps.x + 0.2 * lamps.y;
    let west = side_on(g.x, g.y, SIDE_ACROSS_X);
    let east = side_on(g.x + 1, g.y, SIDE_ACROSS_X);
    let south = side_on(g.y, g.x, SIDE_ACROSS_Z);
    let north = side_on(g.y + 1, g.x, SIDE_ACROSS_Z);
    let across_x = 1.0 - abs(n.x);
    let across_z = 1.0 - abs(n.z);
    let upright = 1.0 - abs(n.y);
    var gloom = shade_by(west, p.x - low.x - WALL_HALF, across_x);
    gloom *= shade_by(east, low.x + CELL - WALL_HALF - p.x, across_x);
    gloom *= shade_by(south, p.z - low.y - WALL_HALF, across_z);
    gloom *= shade_by(north, low.y + CELL - WALL_HALF - p.z, across_z);
    gloom *= 1.0 - 0.35 * upright * exp(p.y / 22.0);
    gloom *= 1.0 - 0.2 * upright * exp(-(p.y + CEILING) / 18.0);
    return light * gloom * (1.0 - dim) + 0.03;
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
    let motif = vec2<f32>(along - (round(along / 36.0 + 0.5 * (row % 2.0)) - 0.5 * (row % 2.0)) * 36.0, height - row * 44.0);
    let diamond = abs(motif.x) / 4.5 + abs(motif.y) / 6.5;
    colour *= 1.0 - 0.06 * fine * smoothstep(1.0 + edge / 4.0, 1.0 - edge / 4.0, diamond);
    colour *= 0.94 + 0.12 * value_noise(vec2<f32>(along / 520.0, height / 400.0), 0x3c6ef372u);
    let damp = value_noise(vec2<f32>(along / 75.0, height / 120.0), 0xa54ff53au);
    let low = smoothstep(190.0, 15.0, height);
    let stain = smoothstep(0.58, 0.74, damp + 0.2 * low - 0.1) * low;
    let rim = smoothstep(0.55, 0.6, damp + 0.2 * low - 0.1) - smoothstep(0.6, 0.66, damp + 0.2 * low - 0.1);
    colour = mix(colour, vec3<f32>(0.60, 0.48, 0.24), stain * 0.75);
    colour *= 1.0 - 0.18 * rim * low;
    let drip = value_noise(vec2<f32>(along / 8.0, height / 240.0), 0x510e527fu);
    colour *= 1.0 - 0.14 * smoothstep(0.7, 0.9, drip) * smoothstep(110.0, 270.0, height) * fine;
    return colour * (1.0 - 0.25 * exp(-height / 14.0));
}

// Damp mustard carpet.
fn carpet(at: vec2<f32>, pixel: f32) -> vec3<f32> {
    let fine = clamp(1.0 - pixel / 5.0, 0.0, 1.0);
    let fibre = unit(hash3(i32(floor(at.x / 2.5)), i32(floor(at.y / 2.5)), 0x9b05688cu)) - 0.5;
    var colour = vec3<f32>(0.55, 0.47, 0.27) * (1.0 + 0.2 * fibre * fine);
    let damp = smoothstep(0.56, 0.82, value_noise(at / 170.0, 0x1f83d9abu));
    colour = mix(colour, vec3<f32>(0.33, 0.27, 0.15), damp * 0.75);
    return colour * (0.9 + 0.2 * value_noise(at / 650.0, 0x5be0cd19u));
}

// The ceiling: tiles in a grid, some stained and one now and then missing,
// and the cell's lamp, lit, flickering or dead. Returns the colour the
// light falls on in rgb and the light the lamp gives off itself in a.
fn ceiling(p: vec3<f32>, at: vec2<f32>, origin: vec2<i32>, pixel: f32, frame: i32, dim: f32) -> vec4<f32> {
    let local = vec2<i32>(floor(p.xz / CELL));
    let g = local + origin;
    let from_middle = p.xz - (vec2<f32>(local) + 0.5) * CELL;
    let to_panel = abs(from_middle) - vec2<f32>(25.0, 75.0);
    if (!has_pillar(g) && max(to_panel.x, to_panel.y) < 0.0) {
        let level = lamp_level(g, frame) * (1.0 - dim);
        if (max(to_panel.x, to_panel.y) > -3.0) {
            return vec4<f32>(0.62, 0.61, 0.56, 0.0);
        }
        let lens = 0.92 + 0.08 * step(0.5, fract(from_middle.y / 6.0)) * clamp(1.0 - pixel / 3.0, 0.0, 1.0);
        let glow = vec3<f32>(1.0, 0.98, 0.9) * (0.3 + 1.6 * level) * lens;
        return vec4<f32>(glow, 1.0);
    }
    let tile_at = (at + vec2<f32>(25.0)) / 50.0;
    let tile = floor(tile_at);
    let within = (fract(tile_at) - 0.5) * 50.0;
    let seam = smoothstep(25.0 - 1.2 - pixel, 25.0 - 1.2 + pixel, max(abs(within.x), abs(within.y)));
    let pick = hash3(i32(tile.x), i32(tile.y), 0x6a09e667u);
    if (pick < 0x01800000u) {
        return vec4<f32>(vec3<f32>(0.015) * (1.0 - seam) + vec3<f32>(0.5, 0.49, 0.42) * seam, 0.0);
    }
    let speckle = unit(hash3(i32(floor(at.x / 1.5)), i32(floor(at.y / 1.5)), 0xbb67ae85u)) - 0.5;
    var colour = vec3<f32>(0.70, 0.68, 0.57) * (1.0 + 0.1 * speckle * clamp(1.0 - pixel / 2.0, 0.0, 1.0));
    if (pick < 0x14000000u) {
        let ring = length(within - (vec2<f32>(unit(pick * 747796405u), unit(pick * 2891336453u)) - 0.5) * 20.0);
        let stain = smoothstep(17.0, 9.0, ring + 6.0 * value_noise(within / 6.0, pick));
        colour = mix(colour, vec3<f32>(0.62, 0.50, 0.30), stain * 0.6);
    }
    colour = mix(colour, vec3<f32>(0.56, 0.55, 0.47), seam);
    // Light the lamp spills on the tiles around it.
    let spill = exp(-max(max(to_panel.x, to_panel.y), 0.0) / 38.0) * lamp_level(g, frame) * (1.0 - dim);
    return vec4<f32>(colour, 0.0) + vec4<f32>(colour * spill * 0.55, 0.0);
}

fn volume(eye: vec3<f32>, direction: vec3<f32>, entry: f32, exit: f32, footprint: f32) -> VolumeHit {
    let frame = i32(volume_param(0u));
    let origin = vec2<i32>(i32(volume_param(1u)), i32(volume_param(2u)));
    let dim = volume_param(3u);
    let sight = min(exit, volume_param(7u));
    let hit = trace(eye, direction, origin, sight);
    let p = eye + direction * hit.t;
    let at = p.xz + vec2<f32>(origin) * CELL;
    let pixel = hit.t * footprint;
    var colour = vec3<f32>(0.0);
    if (hit.surface == WALL) {
        var shown = vec3<f32>(0.76, 0.70, 0.46);
        if (hit.normal.y < 0.5) {
            shown = wallpaper(select(at.x, at.y, abs(hit.normal.x) > 0.5), -p.y, pixel);
        }
        colour = shown * lighting(p, hit.normal, origin, frame, dim);
    } else if (hit.surface == FLOOR) {
        colour = carpet(at, pixel) * lighting(p, hit.normal, origin, frame, dim);
    } else if (hit.surface == CEILING_TILES) {
        let tiles = ceiling(p, at, origin, pixel, frame, dim);
        if (tiles.a > 0.5) {
            colour = tiles.rgb;
        } else {
            colour = tiles.rgb * lighting(p, hit.normal, origin, frame, dim);
        }
    } else if (hit.surface == FIGURE) {
        colour = vec3<f32>(0.012, 0.010, 0.008);
    }
    let far = volume_param(7u);
    let haze = max(1.0 - exp(-hit.t / HAZE), smoothstep(far * 0.7, far, hit.t));
    colour = mix(colour, vec3<f32>(0.09, 0.075, 0.035), haze);
    colour = (1.0 - exp(-colour * 1.6)) * 1.1;
    let grain = unit(hash3(i32(direction.x * 4096.0) ^ (frame * 7919), i32(direction.y * 4096.0) + i32(direction.z * 4096.0) * 4099, 0x2545f491u)) - 0.5;
    colour += vec3<f32>(grain * 0.075);
    var out: VolumeHit;
    out.color = vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
    out.at = p;
    return out;
}
