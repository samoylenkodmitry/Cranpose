// A drop of rain that slides down a window, lit by a colored light: one body
// of clear water. Its head is round and heavy. The light gathers low in it
// in a soft core that is nearly white, its lower rim is bright and its
// upper rim dark, as in a lens. Over the head the water runs up in a wide
// tube, the wet path of the drop: the night shows through it, and a thin
// bright line runs along each of its edges. The edges wander and the tube
// swells and thins, as water on glass does. Past the tube a thinner and
// fainter track follows.
//
// The water the drop left does not slide with the drop: its shape is fixed
// to the glass. The head follows a path that wanders, the water in the tube
// drifts down slower than the head, and the older water necks and breaks
// into beads that stay where they are.
//
// The drop may be larger than the box it is drawn in: a box shows the part
// of the drop that lies in it.
//
// Uniforms, in dp of the box: 0..2 the size of the box, 2..4 the middle of
// the head, 4 the radius of the head, 5 the height of the tube over the
// head, 6 the height of the track over the tube, 7 the light (0 waits,
// 1 sounds, more in the flash of the start), 8..12 the color, 12 how far
// the glow may reach, 13 what tells this drop from another.

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec4(index: u32) -> vec4<f32> {
    return u[index / 4u];
}

// Slots 236..240: the region of the input texture that holds what lies
// behind the box, in texels; zero when that is the whole texture.
fn region_extent() -> vec4<f32> {
    let region = get_vec4(236u);
    let dims = vec2<f32>(textureDimensions(input_texture));
    if (region.z > 0.5 && region.w > 0.5) {
        return region;
    }
    return vec4<f32>(0.0, 0.0, dims.x, dims.y);
}

// Slots 252..254: the pixels the region stands for.
fn logical_extent() -> vec2<f32> {
    let logical = get_vec4(252u).xy;
    if (logical.x > 0.5 && logical.y > 0.5) {
        return logical;
    }
    return region_extent().zw;
}

// The texture coordinate of a place in the region, from 0 to 1 across it.
fn region_uv(local_uv: vec2<f32>) -> vec2<f32> {
    let region = region_extent();
    let dims = max(vec2<f32>(textureDimensions(input_texture)), vec2<f32>(1.0));
    let half_texel = 0.5 / max(region.zw, vec2<f32>(1.0));
    let held = clamp(local_uv, half_texel, vec2<f32>(1.0) - half_texel);
    return (region.xy + held * region.zw) / dims;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// Slots 240..248: the rounded clip of the box in region pixels, zero when
// it has none.
fn composite_coverage(local_uv: vec2<f32>) -> f32 {
    let mask_rect = get_vec4(240u);
    if (mask_rect.z <= 0.5 || mask_rect.w <= 0.5) {
        return 1.0;
    }
    let radii = get_vec4(244u);
    let half_size = mask_rect.zw * 0.5;
    let local = local_uv * logical_extent() - mask_rect.xy - half_size;
    var radius = radii.x;
    if (local.x > 0.0 && local.y <= 0.0) {
        radius = radii.y;
    } else if (local.x <= 0.0 && local.y > 0.0) {
        radius = radii.z;
    } else if (local.x > 0.0 && local.y > 0.0) {
        radius = radii.w;
    }
    return 1.0 - smoothstep(-0.5, 0.5, sd_round_rect(local, half_size, radius));
}

// The smaller of two distances, with a round join `k` wide between them.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// The bead of a chain at the place `turn` along it, where each whole number
// starts a new bead. `x` is the width there: 0 between two beads, up to 1
// in a bead. `y` is the place along the bead, from -1 at its upper end to 1
// at its lower end. A bead is round, a little heavier at its lower end, and
// no two are of one size.
fn bead(turn: f32, seed: f32) -> vec2<f32> {
    let size = 0.62 + 0.38 * fract(sin((floor(turn) + seed) * 127.1) * 43758.5453);
    let along = (fract(turn) - 0.5) * 2.0 / (0.5 + 0.28 * size);
    let hung = along - 0.22 * (1.0 - along * along);
    return vec2<f32>(size * sqrt(max(1.0 - hung * hung, 0.0)), clamp(hung, -1.0, 1.0));
}

// How far to its side the path of a drop lies at the height `glass` of the
// window.
fn wander(glass: f32, phase: f32) -> f32 {
    return sin(glass * 0.021 + phase * 1.3) * 1.4 + sin(glass * 0.057 + phase * 2.9) * 0.6;
}

fn drop_fs(input: VertexOutput) -> vec4<f32> {
    let extent = logical_extent();
    let rect = get_vec4(248u);
    let box_size = max(vec2<f32>(get_float(0u), get_float(1u)), vec2<f32>(1.0));
    let dp = max(rect.z / box_size.x, 0.001);
    let p = (input.uv * extent - rect.xy) / dp;
    let head = vec2<f32>(get_float(2u), get_float(3u));
    let radius = max(get_float(4u), 0.5);
    let tube = max(get_float(5u), 0.0);
    let track = max(get_float(6u), 0.0);
    let light = max(get_float(7u), 0.0);
    let tint = get_vec4(8u).rgb;
    let room = max(get_float(12u), 1.0);
    let phase = get_float(13u) * 2.399;
    let lit = min(light, 1.0);
    let flash = clamp((light - 1.0) * 4.0, 0.0, 1.0);
    let bright = 1.0 + 0.8 * lit + 0.6 * flash;
    let pale = mix(tint, vec3<f32>(1.0), 0.6);

    // Where this place and the head lie on the window, from its top. The
    // place of the head is one number all over the drop, and it grows while
    // the drop slides: it is the drop's clock.
    let glass = input.position.y / dp;
    let up = head.y - p.y;
    let head_glass = glass + up;
    // The head follows its path, and the water over it lies where the head
    // went.
    let head_path = wander(head_glass, phase);
    let from_head = vec2<f32>(p.x - head.x - head_path, p.y - head.y);
    let across = p.x - head.x - wander(min(glass, head_glass), phase);
    let to_head = length(from_head);
    let q = from_head / radius;

    // The tube. It is widest next to the head, thinner toward its end and
    // thin along the track. Each of its edges wanders on its own, and the
    // whole of it swells and thins in long beads: all of it fixed to the
    // glass, but for the swells, which drift down slower than the head.
    let length_up = tube + track;
    let along_tube = clamp(up / max(tube, 0.001), 0.0, 1.0);
    let along_track = clamp((up - tube) / max(track, 0.001), 0.0, 1.0);
    let beads = sin((glass - 0.4 * head_glass) * 0.09 + phase * 3.3);
    let swell = 1.0 + 0.16 * beads + 0.08 * sin(glass * 0.21 + phase * 1.3);
    // Next to the head the tube widens into the head's shoulders: the two
    // make a pear.
    let taper = mix(1.0, 0.7, smoothstep(0.3, 1.0, along_tube))
        * mix(1.0, 0.5, smoothstep(0.0, 0.2, along_track) * step(tube, up))
        * mix(1.24, 1.0, smoothstep(radius * 0.3, radius * 3.0, up));
    // The older water necks and breaks into a chain of beads.
    let breaking = max(smoothstep(0.3, 0.9, along_tube), step(tube, up));
    // The beads lie closer in some places than in others.
    let turn = (glass * 0.3 + phase * 4.1 + 1.3 * sin(glass * 0.07 + phase)) / 6.2832;
    let chain = bead(turn, phase);
    // First the water between two beads thins to a stalk, then the stalk
    // pinches off, and the beads shrink while they wait.
    let stalk = 1.0 - smoothstep(0.0, 0.55, breaking);
    let necked = max(chain.x * (1.0 - 0.25 * breaking), stalk);
    let freed = smoothstep(0.5, 0.9, breaking);
    // The water does not end in a point: it thins out of sight.
    let dissolve = smoothstep(
        0.0,
        max(min(length_up * 0.45, radius * 9.0), 0.001),
        length_up - up,
    );
    // Between two beads the glass is bare: no water, and no line of it.
    let half_tube = radius * 0.78 * taper * swell * necked
        - 1.2 * (1.0 - smoothstep(0.0, 0.12, necked));
    let ripple = radius * 0.2 * taper * necked;
    let left_edge = -half_tube
        + (sin(glass * 0.11 + phase) * 0.65 + sin(glass * 0.27 + phase * 1.9) * 0.35) * ripple;
    let right_edge = half_tube
        + (sin(glass * 0.13 + phase * 2.7) * 0.65 + sin(glass * 0.24 + phase * 0.6) * 0.35) * ripple;
    let d_tube = max(max(left_edge - across, across - right_edge), max(-up, up - length_up));
    let middle = (left_edge + right_edge) * 0.5;
    let half_now = max((right_edge - left_edge) * 0.5, 0.001);
    let side = clamp((across - middle) / half_now, -1.0, 1.0);

    // The head and the tube are one body: the head runs up into the tube
    // as a pear does into its stalk.
    let d_head = to_head - radius;
    var d = d_head;
    if (length_up > 0.0) {
        d = smooth_min(d_head, d_tube, radius * 0.5);
    }
    let depth = -d;
    let within = 1.0 - smoothstep(-0.6, 0.6, d);
    let in_head = 1.0 - smoothstep(radius * 1.0, radius * 2.8, to_head);

    // What lies behind the drop: mirrored across the axis of the tube, and
    // turned over in the head.
    let seen_at = vec2<f32>(
        p.x - across + middle - (across - middle) * 0.7,
        head.y + mix(from_head.y, -from_head.y * 0.6, in_head),
    );
    let seen = textureSampleLevel(
        input_texture,
        input_sampler,
        region_uv((rect.xy + seen_at * dp) / extent),
        0.0,
    );

    // How strong the light on the water is: it weakens along the tube and
    // is faint along the track.
    var strength = mix(1.0, 0.36, along_tube);
    var tube_alpha = mix(0.92, 0.55, along_tube);
    if (up > tube) {
        strength = mix(0.36, 0.05, along_track);
        tube_alpha = mix(0.55, 0.12, along_track);
    }
    strength = strength * dissolve;
    tube_alpha = tube_alpha * dissolve;

    // The tube is clear: the night behind it and a soft band of the light
    // down its middle. A thin bright line runs along each edge, brightest
    // where the water swells, with a dark line inside it. The far wall of
    // the tube shows as a fainter line inside the left edge.
    var water = seen.rgb * 0.92 + tint * (0.18 + 0.25 * lit) * strength;
    water = water + tint * 0.17 * exp(-side * side * 3.0) * strength * bright;
    let dark_line = smoothstep(1.1, 2.1, depth) * (1.0 - smoothstep(2.1, 3.8, depth));
    water = water * (1.0 - 0.18 * dark_line);
    let edge_line = 1.0 - smoothstep(0.0, 1.6, depth);
    water = water + pale * edge_line * (0.62 + 0.42 * beads) * 1.3 * strength * bright;
    let far_wall = (1.0 - smoothstep(0.0, 0.9, abs(depth - 3.3))) * step(side, 0.0);
    water = water + pale * far_wall * 0.2 * strength * (1.0 - breaking);

    // A bead that has come free is a small drop of its own, and shows a
    // little of the volume of the head: light low in it, a brighter lower
    // rim, a darker upper rim and a point of light. All of it lies on the
    // water of the tube and replaces none of it.
    let in_bead = vec2<f32>(side * sqrt(max(1.0 - chain.y * chain.y, 0.0)), chain.y);
    let bead_rim = smoothstep(0.6, 1.0, length(in_bead));
    let to_bead_core = in_bead - vec2<f32>(0.0, 0.3);
    let bead_glint = 1.0 - smoothstep(0.10, 0.2, length(in_bead - vec2<f32>(-0.36, -0.34)));
    let volume = freed * (0.5 + 0.5 * strength);
    water = water * (1.0 - 0.22 * volume * bead_rim * clamp(-in_bead.y, 0.0, 1.0));
    water = water
        + (mix(tint, vec3<f32>(1.0), 0.45) * exp(-dot(to_bead_core, to_bead_core) * 4.5) * 0.4
            + pale * bead_rim * clamp(in_bead.y, 0.0, 1.0) * 0.3
            + vec3<f32>(1.0) * bead_glint * 0.4)
            * volume;

    // The head is full of the light. It gathers low in the head in a soft
    // core that is nearly white. The lower rim is bright and the upper rim
    // dark, and two points of light sit on the head.
    var bulb = seen.rgb * 0.45 + tint * 0.2;
    let to_core = q - vec2<f32>(0.0, 0.30);
    bulb = bulb + mix(tint, vec3<f32>(1.0, 0.99, 0.96), 0.75) * exp(-dot(to_core, to_core) * 3.2)
        * 1.1 * bright;
    let to_middle = q - vec2<f32>(0.0, 0.10);
    bulb = bulb + tint * exp(-dot(to_middle, to_middle) * 1.5) * 0.7 * bright;
    let rim = 1.0 - smoothstep(0.25, 1.5, depth);
    bulb = bulb
        * (1.0 - 0.4 * smoothstep(0.45, 0.95, length(q)) * clamp(-q.y, 0.0, 1.0) * (1.0 - rim));
    bulb = bulb
        + (tint * 0.75 + vec3<f32>(0.3)) * rim * clamp(q.y * 0.7 + 0.45, 0.0, 1.0)
            * (0.85 + 0.4 * lit);
    let glint = 1.0 - smoothstep(0.10, 0.20, length(q - vec2<f32>(-0.36, -0.30)));
    let small_glint = 1.0 - smoothstep(0.05, 0.11, length(q - vec2<f32>(0.30, 0.52)));
    bulb = bulb + vec3<f32>(1.0) * (glint * 0.95 + small_glint * 0.5);

    let body = mix(water, bulb, in_head);
    let alpha = within * mix(tube_alpha, 1.0, in_head);

    // The glow around the head falls to nothing within its room, and a
    // fainter one lies along the tube.
    let reach = radius * (1.2 + 0.5 * lit + 0.6 * flash);
    var glow = exp(-max(d_head, 0.0) / reach) * (0.26 + 0.5 * lit + 0.5 * flash)
        * (1.0 - smoothstep(room * 0.55, room, max(d_head, 0.0)));
    glow = glow + exp(-max(d_tube, 0.0) / (radius * 0.4)) * 0.07 * strength * bright
        * step(0.001, length_up)
        * (1.0 - smoothstep(room * 0.4, room * 0.9, abs(across)));
    glow = glow * (1.0 - within);

    // Colors are multiplied by their alpha. The glow adds light and covers
    // nothing.
    return vec4<f32>(body * alpha + tint * glow, alpha);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return drop_fs(input) * composite_coverage(input.uv) * get_float(254u);
}
