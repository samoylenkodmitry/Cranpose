// The floor of a swimming pool seen through moving water, after
// `noise.wgsl` and the swell's table, which `swell.rs` writes. The mesh
// carries the refraction: a point of the surface shows where it stands and
// reads the floor where its texture coordinate says, and its normal holds
// the ripples' slope. This shader lays the floor out as a mosaic of glass
// tiles, cuts the content's words into it, and lights it: the sunlight the
// swell focuses into caustics, the water's colour, the sky the surface
// mirrors, the tiled wall under the coping and the coping's stone.
//
// The content is painted in key colours: white bare floor, black ink,
// yellow the writing lane's frame, cyan its panel, magenta a selection and
// orange the caret.
//
// params: 0..1 = the stage's size in pixels; 2 = the water's clock in
// seconds; 3 = the look's size, 1 on a 1600 by 1000 stage; 4 = a tile's
// side; 5 = the waterline, the coping's lower edge; 6 = where the wall
// meets the floor; 7 = how much the lane invites typing, from 0 to 1; 8 =
// how much of the lane's first words are still there, from 0 to 1; 9 =
// where the headline ends, above which the letters are cut large.

const SUN_DIR: vec3<f32> = vec3<f32>(0.30, -0.46, 1.0);

struct Keys {
    ink: f32,
    frame: f32,
    panel: f32,
    selection: f32,
    caret: f32,
}

// What the content holds in `color`. Ink is how dark it is; what lies under
// the ink is told by the channel its colour lacks.
fn keys(color: vec4<f32>) -> Keys {
    let rgb = color.rgb / max(color.a, 1.0 / 255.0);
    let brightest = max(rgb.r, max(rgb.g, rgb.b));
    var found: Keys;
    found.ink = 1.0 - brightest;
    let ground = rgb / max(brightest, 1.0e-3);
    let missing = (vec3<f32>(1.0) - ground) * smoothstep(0.97, 0.75, found.ink);
    // The caret's orange lacks blue and most of green.
    let warm = min(missing.g, missing.b);
    found.caret = clamp(warm * 1.54, 0.0, 1.0);
    found.frame = missing.b - warm;
    found.selection = missing.g - warm;
    found.panel = missing.r;
    return found;
}

fn keys_at(point: vec2<f32>, size: vec2<f32>) -> Keys {
    return keys(content_color(clamp(point / size, vec2<f32>(0.0), vec2<f32>(1.0))));
}

// The swell's slope at a point and how fast the slope changes there.
struct SwellAt {
    slope: vec2<f32>,
    // d slope.x / dx, d slope.x / dy (= d slope.y / dx), d slope.y / dy.
    curve: vec3<f32>,
}

fn swell_at(p: vec2<f32>, clock: f32, look: f32) -> SwellAt {
    let q = p / look;
    let drift = clock * SWELL_GUST_RATE;
    let gusts = 1.0 + 0.42 * sin(q.x * 0.0061 + q.y * 0.0032 + drift)
        * sin(q.y * 0.0083 - q.x * 0.0027 - drift * 0.7 + 1.3);
    var at: SwellAt;
    at.slope = vec2<f32>(0.0);
    at.curve = vec3<f32>(0.0);
    for (var i = 0u; i < SWELL_WAVES; i = i + 1u) {
        let travel = SWELL_TRAVEL[i];
        let crest = SWELL_CREST[i];
        let phase = dot(travel.xy, q) + travel.w - travel.z * clock;
        let gain = 1.0 + (gusts - 1.0) * crest.z;
        at.slope = at.slope + crest.xy * (gain * cos(phase));
        // The sunlight is read over a few pixels of floor: the shortest
        // waves focus it less.
        let blur = exp(-36.0 * dot(travel.xy, travel.xy));
        let turn = -gain * sin(phase) * blur / look;
        at.curve = at.curve + vec3<f32>(crest.x * travel.x, crest.x * travel.y, crest.y * travel.y) * turn;
    }
    return at;
}

fn fbm(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        sum = sum + amplitude * value_noise(q);
        q = q * 2.03 + vec2<f32>(17.0, 31.0);
        amplitude = amplitude * 0.5;
    }
    return sum / 0.9375;
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(hash(p), hash(p + vec2<f32>(37.1, 11.7)));
}

fn box_sdf(q: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let b = abs(q) - half_size;
    return length(max(b, vec2<f32>(0.0))) + min(max(b.x, b.y), 0.0);
}

fn tone(c: vec3<f32>) -> vec3<f32> {
    return clamp((c * (vec3<f32>(1.0) + c * 0.16)) / (vec3<f32>(1.0) + c * 0.72) * 1.32, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, c <= vec3<f32>(0.0031308));
}

// Metres of water as light takes them, and as the water's colour does.
fn depth_metres(p: vec2<f32>, size: vec2<f32>, wall_foot: f32) -> f32 {
    let t = clamp(0.38 * p.x / size.x + 0.62 * (p.y - wall_foot) / (size.y - wall_foot), -0.05, 1.1);
    return 0.8 + 1.35 * t;
}

fn water_column(p: vec2<f32>, size: vec2<f32>, wall_foot: f32) -> f32 {
    let t = clamp(0.36 * p.x / size.x + 0.64 * (p.y - wall_foot) / (size.y - wall_foot), -0.05, 1.1);
    return 0.5 + 1.1 * t + 2.7 * t * t;
}

// Slabs of sandy limestone with veins, grain, pores and dark joints.
fn coping_stone(p: vec2<f32>, look: f32, wet: f32) -> vec3<f32> {
    let slab_width = 430.0 * look;
    let along = p.x + slab_width * 0.37;
    let slab = floor(along / slab_width);
    let in_slab = along - slab * slab_width;
    let to_joint = min(in_slab, slab_width - in_slab);
    let r = p / look + slab * vec2<f32>(71.0, 133.0);
    let veins = fbm(r * vec2<f32>(0.011, 0.021) + vec2<f32>(3.0));
    let cloud = fbm(r * 0.16 + vec2<f32>(9.0));
    let grain = value_noise(r * 0.95) * 0.55 + value_noise(r * 1.9 + vec2<f32>(5.0)) * 0.45 - 0.5;
    let pore_cell = floor(r / 7.0);
    let to_pore = r - (pore_cell + vec2<f32>(0.2) + 0.6 * hash2(pore_cell)) * 7.0;
    let pore_seed = hash(pore_cell + vec2<f32>(4.0));
    let pore_radius = pore_seed * pore_seed * pore_seed * 1.7;
    let pore = step(0.55, pore_seed)
        * (1.0 - smoothstep(pore_radius - 0.7, pore_radius + 0.4, length(to_pore * vec2<f32>(1.0, 1.25))));
    var c = mix(vec3<f32>(0.50, 0.462, 0.40), vec3<f32>(0.66, 0.635, 0.585), smoothstep(0.25, 0.75, veins));
    c = c * (0.90 + 0.20 * cloud) * (1.0 + 0.30 * grain) * (0.93 + 0.12 * hash(vec2<f32>(slab, 3.0)));
    c = c * (1.0 - 0.5 * pore);
    c = mix(c, c * 0.42 * vec3<f32>(0.96, 1.0, 1.04), wet);
    return c * (1.0 - 0.72 * (1.0 - smoothstep(0.4, 2.2, to_joint / look)));
}

// Sun glints: small fast facets on the swell; one shows where a facet
// mirrors the sun.
fn glitter(p: vec2<f32>, swell: vec2<f32>, clock: f32, look: f32) -> f32 {
    let q = vec2<f32>(0.55, 1.25) * p / look + swell * 90.0;
    let facets = vec2<f32>(0.35, 0.94) * cos(dot(q, vec2<f32>(0.10, 0.28)) + clock * 3.1)
        + vec2<f32>(-0.42, 0.91) * cos(dot(q, vec2<f32>(-0.17, 0.33)) + clock * 3.8 + 1.0)
        + vec2<f32>(0.2, -0.98) * cos(dot(q, vec2<f32>(0.05, -0.47)) + clock * 4.4 + 2.0)
        + vec2<f32>(-0.7, 0.7) * cos(dot(q, vec2<f32>(0.29, 0.22)) + clock * 5.0 + 4.0);
    let off_mirror = swell * 1.1 + 0.026 * facets - vec2<f32>(0.030, -0.055);
    let e = dot(off_mirror, off_mirror);
    return 1.9 * exp(-e / 0.00009) + 0.34 * exp(-e / 0.0009) + 0.035;
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let clock = shaded_param(2u);
    let look = shaded_param(3u);
    let tile = shaded_param(4u);
    let waterline = shaded_param(5u);
    let wall_foot = shaded_param(6u);
    let invite = shaded_param(7u);
    let first_words = shaded_param(8u);
    let headline_end = shaded_param(9u);
    let aa = 0.6;

    // Where the point shows, and the floor point seen there.
    let p = surface.position.xy * size;
    let c = surface.uv * size;
    let shift = c - p;
    let ripple = surface.normal.xy / max(-surface.normal.z, 0.2);
    let swell = swell_at(p, clock, look);
    let on_floor = clamp((p.y - waterline) / (wall_foot - waterline), 0.0, 1.0);

    // ---- the tile under the floor point
    let grid = c / tile;
    let tile_id = floor(grid);
    let in_tile = (grid - tile_id - vec2<f32>(0.5)) * tile;
    let rnd_a = hash(tile_id + vec2<f32>(17.0));
    let rnd_b = hash(tile_id * 1.7 + vec2<f32>(5.0));
    let tile_tone = (value_noise(tile_id * 0.16) * 0.65 + value_noise(tile_id * 0.45 + vec2<f32>(9.0)) * 0.35) * 0.9;
    let jitter = fract(vec2<f32>(rnd_a * 57.31, rnd_b * 91.73));

    // What the tile lies in: four reads along its middle row.
    let middle = (tile_id + vec2<f32>(0.5)) * tile;
    var selected = 0.0;
    var frame = 0.0;
    var panel = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let found = keys_at(middle + vec2<f32>(tile * (f32(i) * 0.25 - 0.375), 0.0), size);
        selected = max(selected, found.selection);
        frame = frame + found.frame * 0.25;
        panel = max(panel, found.panel);
    }
    selected = step(0.4, selected);
    frame = step(0.45, frame);
    let lane = step(0.5, panel) * (1.0 - frame);
    let big_type = max(step(c.y, headline_end), lane);

    // Letters are cut tile by tile: every piece sits a little off and a
    // little turned, the small type less.
    let slip = (jitter - vec2<f32>(0.5)) * mix(0.45, 1.8, big_type) * look;
    let turn = vec2<f32>(-in_tile.y, in_tile.x) * (rnd_a - 0.5) * mix(0.02, 0.10, big_type);
    let text_at = c + slip + turn;

    // Ink. Where the water bends hard, red and blue are read a little apart.
    let here = keys_at(text_at, size);
    var ink = vec3<f32>(here.ink);
    let fringe = shift * 0.05;
    if (dot(fringe, fringe) > 0.16) {
        ink.r = keys_at(text_at - fringe, size).ink;
        ink.b = keys_at(text_at + fringe, size).ink;
    }

    // ---- mosaic: a rounded square of glass per tile, grout between
    let grout_half = tile * 0.034;
    let corner = tile * 0.10;
    let q = in_tile + (jitter - vec2<f32>(0.5)) * tile * 0.022;
    let tile_sdf = box_sdf(q, vec2<f32>(tile * 0.5 - grout_half - corner)) - corner;
    let on_tile = clamp(0.5 - tile_sdf / aa, 0.0, 1.0);
    let rim = smoothstep(-tile * 0.13, 0.0, tile_sdf) * on_tile;
    let outward = sign(q) * step(abs(q.yx), abs(q.xy));
    let tilt = (jitter - vec2<f32>(0.5)) * 0.16 + outward * rim * 0.55
        + 0.04 * sin(in_tile.yx * (vec2<f32>(0.35) + 0.25 * jitter) / look + jitter * 6.0);

    var glass = mix(vec3<f32>(0.50, 0.80, 0.78), vec3<f32>(0.69, 0.89, 0.86), rnd_a);
    glass = mix(glass, vec3<f32>(0.88, 0.93, 0.89), step(0.80, rnd_b) * 0.85);
    glass = mix(glass, vec3<f32>(0.28, 0.64, 0.70), step(rnd_b, 0.14) * 0.8);
    glass = glass * (0.84 + 0.33 * tile_tone + 0.06 * (rnd_a - 0.5));
    var grout = vec3<f32>(0.93, 0.95, 0.91);

    var ink_glass = mix(vec3<f32>(0.004, 0.012, 0.060), vec3<f32>(0.010, 0.034, 0.150), smoothstep(0.25, 1.0, rnd_b))
        * (0.75 + 0.5 * rnd_a);
    ink_glass = mix(ink_glass, vec3<f32>(0.006, 0.060, 0.20), step(0.90, rnd_a));

    // In the lane: white tiles, grey grout, paler ink while the first words
    // are still there.
    let white = mix(vec3<f32>(0.90, 0.925, 0.91), vec3<f32>(0.99, 0.985, 0.96), rnd_a) * (0.93 + 0.07 * step(0.2, rnd_b));
    glass = mix(glass, white, lane);
    grout = mix(grout, vec3<f32>(0.60, 0.69, 0.70), lane);
    ink_glass = mix(ink_glass, mix(ink_glass, vec3<f32>(0.018, 0.145, 0.33) * (0.8 + 0.4 * rnd_a), first_words), lane);

    // Selected tiles are gold; the frame is yellow and brightens while the
    // lane invites typing.
    let gold = mix(vec3<f32>(1.1, 0.43, 0.004), vec3<f32>(1.15, 0.60, 0.02), rnd_a);
    let yellow = mix(vec3<f32>(1.25, 0.60, 0.010), vec3<f32>(1.28, 0.70, 0.04), rnd_a) * (0.94 + 0.30 * invite);
    var albedo = mix(glass, gold, selected);
    grout = mix(grout, grout * vec3<f32>(1.05, 0.82, 0.45), selected);
    albedo = mix(albedo, yellow, frame);
    grout = mix(grout, vec3<f32>(0.95, 0.80, 0.48), frame);

    // Some big letter tiles have a chipped edge that shows the tile beneath.
    var chip = 0.0;
    if (ink.g > 0.01 && big_type > 0.5 && tile_sdf > -4.0 * look) {
        let bite = tile_sdf + 2.6 * look - 9.0 * look * (1.0 - value_noise(c * 0.13 / look + vec2<f32>(rnd_a * 9.0)));
        chip = step(0.89, fract(rnd_a * 31.7 + rnd_b * 7.3)) * smoothstep(0.0, 1.6 * look, bite);
    }
    albedo = mix(albedo, ink_glass, ink * (1.0 - 0.85 * chip));
    grout = mix(grout, ink_glass * 2.2 + vec3<f32>(0.02, 0.05, 0.075), ink * mix(0.86, 0.74, big_type));
    albedo = mix(grout, albedo, on_tile);

    // The caret: a strip of lit orange glass, wider than the content's.
    var caret = here.caret;
    if (lane > 0.5) {
        let near = vec2<f32>(2.5 * look, 0.0);
        let far = vec2<f32>(5.0 * look, 0.0);
        caret = max(max(caret, keys_at(c - near, size).caret), max(keys_at(c + near, size).caret,
            max(keys_at(c - far, size).caret, keys_at(c + far, size).caret)));
    }
    let orange = mix(vec3<f32>(0.95, 0.20, 0.004), vec3<f32>(1.0, 0.33, 0.02), rnd_a);
    albedo = mix(albedo, mix(vec3<f32>(0.50, 0.10, 0.01), orange, on_tile), caret);

    // ---- the wall under the coping: courses of tiles seen steeply, the
    // top course dark blue
    let wall_seen = clamp((p.y + swell.slope.y * 26.0 * on_floor - waterline) / (wall_foot - waterline), 0.0, 1.0);
    let wall_mask = clamp((1.0 - wall_seen) * (wall_foot - waterline), 0.0, 1.0);
    if (wall_mask > 0.0) {
        let course = wall_seen * 3.2;
        let wall_id = vec2<f32>(tile_id.x, floor(course));
        let in_wall_tile = vec2<f32>(in_tile.x / tile, fract(course) - 0.5);
        let face = (1.0 - smoothstep(0.43, 0.49, abs(in_wall_tile.x))) * (1.0 - smoothstep(0.40, 0.48, abs(in_wall_tile.y)));
        var wall_glass = mix(vec3<f32>(0.52, 0.80, 0.80), vec3<f32>(0.75, 0.90, 0.88), hash(wall_id + vec2<f32>(71.0)));
        wall_glass = mix(wall_glass, vec3<f32>(0.02, 0.06, 0.20), step(wall_id.y, 0.5) * 0.92);
        albedo = mix(albedo, mix(vec3<f32>(0.93, 0.95, 0.91) * 0.7, wall_glass, face) * mix(0.62, 0.84, wall_seen), wall_mask);
    }

    // ---- light on the floor: the sky's, and the sun's that the swell
    // focuses. A patch of floor gets as much sun as the patch of surface it
    // came through, over its own area.
    let depth = depth_metres(p, size, wall_foot);
    let spread = 250.0 * look * depth * on_floor;
    let det = (1.0 + spread * swell.curve.x) * (1.0 + spread * swell.curve.z) - spread * spread * swell.curve.y * swell.curve.y;
    let focus = min(inverseSqrt(det * det + 0.012), 5.0) * 0.75;
    var caustics = vec3<f32>(pow(focus, 1.08), focus, pow(focus, 0.92)) * (1.0 - wall_mask);
    // The panel is sunk: the frame above it and to its right shades it.
    if (lane > 0.5) {
        let above = keys_at(c - vec2<f32>(0.0, 7.0 * look), size).frame + keys_at(c - vec2<f32>(0.0, 15.0 * look), size).frame;
        let right = keys_at(c + vec2<f32>(4.0 * look, 0.0), size).frame + keys_at(c + vec2<f32>(8.0 * look, 0.0), size).frame;
        caustics = caustics * (1.0 - 0.35 * max(above, right));
    }
    let sun_dir = normalize(SUN_DIR);
    let facing = clamp(dot(normalize(vec3<f32>(tilt, 1.0)), sun_dir) / sun_dir.z, 0.0, 2.0);
    var lit = vec3<f32>(0.34, 0.50, 0.55) + vec3<f32>(1.0, 0.965, 0.90) * 0.76 * caustics * facing;
    lit = mix(lit, vec3<f32>(0.34, 0.44, 0.50) * (0.8 + 0.5 * on_floor) + vec3<f32>(0.10, 0.12, 0.11), wall_mask);

    // ---- water: red goes first. Gold, the frame, the lane and the caret
    // keep their colour, as if lit from within.
    let column = water_column(p, size, wall_foot);
    let absorbed = exp(-vec3<f32>(0.60, 0.112, 0.022) * column * (0.55 + 0.45 * on_floor) * 2.0);
    let keep = max(max(max(frame, selected), lane) * (1.0 - 0.6 * ink.g), caret);
    let tint = mix(absorbed, exp(log(absorbed) * mix(0.12, 0.22, lane * (1.0 - selected))), keep);
    var col = albedo * lit * tint + vec3<f32>(0.0, 0.026, 0.05) * (1.0 - exp(-column * 0.34));
    col = col + absorbed * caustics * pow(max(dot(tilt, vec2<f32>(0.53, -0.85)), 0.0), 1.4) * mix(0.35, 0.16, ink.g) * on_tile;
    col = col + tint * gold * selected * on_tile * pow(max(caustics.g - 1.0, 0.0), 1.2) * 0.10;
    col = col + yellow * frame * tint * invite * 0.22 * on_tile;
    col = mix(col, col + vec3<f32>(0.62, 0.055, 0.0) * (0.25 + 0.55 * on_tile), caret);

    // The coping's shadow on the water.
    let under_lip = p.y - waterline - 120.0 * swell.slope.y;
    col = col * (1.0 - 0.55 * exp(-max(under_lip, 0.0) / (9.0 * look)));

    // ---- the surface: the sky by Fresnel, lit and shaded wave faces, and
    // glitter near the sun's reflection
    let steep = swell.slope * 2.2 + ripple * 1.25;
    let normal = normalize(vec3<f32>(-steep, 1.0));
    let to_eye = normalize(vec3<f32>((vec2<f32>(size.x * 0.5, size.y * 1.25) - p) / (size.y * 2.1), 1.0));
    let mirrored = reflect(-to_eye, normal);
    let grazing = 1.0 - max(dot(normal, to_eye), 0.0);
    let fresnel = 0.02 + 0.98 * grazing * grazing * grazing * grazing * grazing;
    let sky = mix(vec3<f32>(0.63, 0.90, 1.2), vec3<f32>(0.10, 0.32, 0.86), sqrt(clamp(mirrored.z, 0.0, 1.0)));
    col = col * (1.0 - fresnel) + sky * fresnel * 0.8;
    let swell_face = dot(swell.slope, vec2<f32>(0.45, -0.89));
    col = col * (1.0 + 0.9 * swell_face) + vec3<f32>(0.5, 0.8, 0.9) * 0.05 * smoothstep(0.03, 0.16, swell_face);
    if (dot(ripple, ripple) > 1.0e-5) {
        var face = dot(ripple, vec2<f32>(0.50, -0.866));
        face = face / (abs(face) + 0.10);
        col = col * (1.0 - 0.60 * max(-face, 0.0));
        let sunny = max(face, 0.0);
        col = col + vec3<f32>(0.60, 0.90, 1.0) * (0.10 * sunny + 0.34 * sunny * sunny * sunny + 0.8 * pow(sunny, 9.0));
    }
    let from_sun = (p - vec2<f32>(size.x * 0.90, waterline + 0.17 * size.y)) / vec2<f32>(0.20 * size.x + 60.0, 0.24 * size.y);
    let near_sun = exp(-dot(from_sun, from_sun) * 1.4);
    if (near_sun > 0.03) {
        col = col + vec3<f32>(1.0, 0.985, 0.95) * near_sun * (1.0 - ink.g) * glitter(p, swell.slope, clock, look);
    }
    var shown = tone(col);

    // ---- the coping: a dry lit top and a wet rounded front
    let waterline_now = waterline - 1.0 + 2.2 * look * (swell.slope.y * 9.0 + 0.3 * sin(p.x * 0.021 / look + clock * 0.7));
    if (p.y < waterline + 8.0 * look) {
        let stone_cover = clamp(waterline_now - p.y + 0.5, 0.0, 1.0);
        let above_water = waterline - p.y;
        let front = 12.0 * look;
        let wet_line = front
            + 7.0 * look * (value_noise(vec2<f32>(p.x / (90.0 * look), clock * 0.12)) + 0.5 * value_noise(vec2<f32>(p.x / (23.0 * look), 4.0)))
            + 24.0 * swell.slope.y * look;
        let wet = 1.0 - smoothstep(-4.0 * look, 4.0 * look, above_water - wet_line + 3.0 * look * (value_noise(p / (3.0 * look)) - 0.5));
        var stone = coping_stone(p, look, wet);
        let round_off = clamp((front - above_water) / front, 0.0, 1.0);
        let sun_on_stone = mix(1.22, 0.30, smoothstep(0.0, 0.55, round_off));
        stone = stone * (vec3<f32>(1.0, 0.97, 0.91) * sun_on_stone + vec3<f32>(0.055, 0.075, 0.11));
        let sheen = exp(-pow((above_water - front * 0.80) / (1.3 * look), 2.0))
            * (0.35 + 0.65 * value_noise(vec2<f32>(p.x / (19.0 * look), clock * 0.3)));
        stone = stone + vec3<f32>(0.75, 0.9, 1.0) * 0.56 * sheen * wet;
        stone = stone + vec3<f32>(0.30, 0.62, 0.66) * 0.20 * smoothstep(0.45, 1.0, round_off);
        shown = mix(shown, tone(stone), stone_cover);
        shown = shown + vec3<f32>(0.8, 0.9, 0.9) * 0.28 * exp(-abs(p.y - waterline_now - 1.6 * look) / (0.9 * look))
            * (1.0 - stone_cover) * (0.4 + 0.6 * value_noise(vec2<f32>(p.x / 22.0, clock)));
    }
    return vec4<f32>(to_srgb(shown), 1.0);
}
