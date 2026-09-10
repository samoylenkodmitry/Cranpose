const TAPS: i32 = 12;
/// How wide the blur is allowed to grow, in pixels of the half's own width.
const MAX_RADIUS: f32 = 110.0;
/// How much of the blur runs up and down as well as across.
const UPRIGHT_SHARE: f32 = 0.42;
/// How much of the blur the picture carries right at the crease, where it is
/// least off square to the reader.
const BLUR_FLOOR: f32 = 0.62;
/// How much light the outer edge of the folding half keeps, against the crease
/// which keeps none.
const CREASE_SHARE: f32 = 0.42;

const MODE_WALLPAPER: f32 = 0.5;
const MODE_TURNED: f32 = 1.5;
const MODE_PAINTED: f32 = 2.5;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn dune(p: vec2<f32>, crest: f32, near: vec3<f32>, far: vec3<f32>, colour: vec3<f32>) -> vec3<f32> {
    let mask = smoothstep(crest - 0.004, crest + 0.004, p.y);
    let depth = clamp((p.y - crest) * 2.4, 0.0, 1.0);
    return mix(colour, mix(near, far, depth), mask);
}

/// The lock screen behind the glass: dusk over dunes, drawn from the half's
/// place in the whole open screen so the picture crosses the crease.
fn wallpaper(p: vec2<f32>) -> vec3<f32> {
    let horizon = 0.54;
    let sky = clamp(p.y / horizon, 0.0, 1.0);
    var colour = mix(
        vec3<f32>(0.07, 0.12, 0.21),
        vec3<f32>(0.93, 0.77, 0.57),
        pow(sky, 2.2),
    );

    let sun = vec2<f32>(0.66, horizon - 0.02);
    colour = colour + vec3<f32>(1.0, 0.80, 0.55) * exp(-distance(p, sun) * 6.5) * 0.55;

    let ridge = horizon - 0.10
        + 0.055 * sin(p.x * 7.3 + 0.6)
        + 0.030 * sin(p.x * 15.1 + 2.4)
        + 0.014 * sin(p.x * 31.7 + 5.0);
    let ridge_mask = smoothstep(ridge - 0.004, ridge + 0.004, p.y);
    let haze = clamp((p.y - ridge) * 5.0, 0.0, 1.0);
    colour = mix(
        colour,
        mix(vec3<f32>(0.33, 0.28, 0.28), vec3<f32>(0.15, 0.13, 0.15), haze),
        ridge_mask * 0.94,
    );

    colour = dune(
        p,
        0.64 + 0.10 * sin(p.x * 2.1 + 1.2) + 0.03 * sin(p.x * 5.3),
        vec3<f32>(0.82, 0.68, 0.48),
        vec3<f32>(0.52, 0.42, 0.30),
        colour,
    );
    colour = dune(
        p,
        0.84 + 0.17 * sin(p.x * 1.3 + 3.4),
        vec3<f32>(0.60, 0.47, 0.33),
        vec3<f32>(0.28, 0.22, 0.17),
        colour,
    );

    let corner = distance(p, vec2<f32>(0.5, 0.5));
    return colour * (1.0 - 0.38 * corner * corner);
}

/// Blur the picture at `uv`, across and a little up and down. A blur that only
/// runs across leaves every horizontal stroke of a glyph standing, which reads
/// as banding rather than as something out of focus.
fn softened(uv: vec2<f32>, radius: f32, tex_size: vec2<f32>, low: vec2<f32>, high: vec2<f32>)
    -> vec4<f32> {
    let step = vec2<f32>(radius / (f32(TAPS) * max(tex_size.x, 1.0)), 0.0);
    var sum = vec4<f32>(0.0);
    for (var row = -1; row <= 1; row = row + 1) {
        let lift = vec2<f32>(0.0, f32(row) * radius * UPRIGHT_SHARE / max(tex_size.y, 1.0));
        var row_weight = 1.0;
        if (row != 0) {
            row_weight = 0.62;
        }
        sum = sum + textureSample(input_texture, input_sampler, clamp(uv + lift, low, high))
            * row_weight;
        for (var i = 1; i <= TAPS; i = i + 1) {
            let offset = step * f32(i) + lift;
            let falloff = exp(-2.2 * f32(i * i) / f32(TAPS * TAPS)) * row_weight;
            sum = sum
                + textureSample(input_texture, input_sampler, clamp(uv + offset, low, high))
                    * falloff
                + textureSample(input_texture, input_sampler, clamp(uv - offset + lift * 2.0, low, high))
                    * falloff;
        }
    }
    return sum;
}

/// What the fold does to the light on the half that is turning: the crease is
/// the bottom of the valley the two halves make, and takes least of it.
fn shaded(colour: vec4<f32>, from_hinge: f32, fold: f32, dark: f32, sheen: f32) -> vec4<f32> {
    let valley = CREASE_SHARE + (1.0 - CREASE_SHARE) * (1.0 - from_hinge);
    let shade = clamp(dark * pow(fold, 0.85) * valley, 0.0, 1.0);
    var lit = vec4<f32>(colour.rgb * (1.0 - shade), colour.a);
    let band = exp(-pow((from_hinge - 0.02) / 0.05, 2.0));
    return vec4<f32>(lit.rgb + vec3<f32>(sheen * band * fold * lit.a), lit.a);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let local = clamp((input.uv * tex_size - effect_rect.xy) / size_px, vec2<f32>(0.0), vec2<f32>(1.0));
    let low = (effect_rect.xy + vec2<f32>(0.5)) / tex_size;
    let high = (effect_rect.xy + effect_rect.zw - vec2<f32>(0.5)) / tex_size;
    let mode = get_float(0u);

    if (mode < MODE_WALLPAPER) {
        // This half is a window on to part of the whole open screen, so the
        // picture is drawn from where the window sits in it.
        return vec4<f32>(wallpaper(vec2<f32>(get_float(1u) + local.x * get_float(2u), local.y)), 1.0);
    }

    let fold = clamp(get_float(1u), 0.0, 1.0);
    let blur_px = get_float(2u);
    let dark = get_float(3u);
    let sheen = get_float(4u);

    if (mode < MODE_TURNED) {
        // The half is really turned, by a layer transform. All that is left to
        // do is take the reader's focus off it and put it in shadow.
        let from_hinge = 1.0 - local.x;
        let radius = min(blur_px * pow(fold, 0.7) * (BLUR_FLOOR + (1.0 - BLUR_FLOOR) * from_hinge),
            MAX_RADIUS);
        let sum = softened(input.uv, radius, tex_size, low, high);
        let here = textureSample(input_texture, input_sampler, input.uv);
        let colour = vec4<f32>(sum.rgb / max(sum.a, 1.0e-4) * here.a, here.a);
        return shaded(colour, from_hinge, fold, dark, sheen);
    }

    // Painted: the surface does not move at all. The picture on it is put
    // through the same turn the other device's half really takes -- where a
    // fragment sits in the turned half is worked back to where it sits on the
    // flat one, which skews the picture into the same trapezium -- and then
    // blurred and shadowed the same way. Nothing here is a transform.
    let cos_turn = get_float(5u);
    let sin_turn = get_float(6u);
    let camera = max(get_float(7u), 1.0);
    let half_w = max(get_float(8u), 1.0);
    let half_h = max(get_float(9u), 1.0);
    let hinge_at_left = get_float(10u);

    // How far this fragment sits from the crease, and how far above the middle
    // of the half, both in pixels on the flat surface being painted. A half
    // whose hinge is on its left reaches the other way, so the turn that
    // brings its far edge toward the reader is the opposite one.
    var across = (1.0 - local.x) * half_w;
    var toward = sin_turn;
    if (hinge_at_left >= 0.5) {
        across = local.x * half_w;
        toward = -sin_turn;
    }
    let up_screen = (local.y - 0.5) * size_px.y;

    // Work back to the point on the turned half that would land here. A point
    // `a` along the half lands `a * cos / (1 - a * sin / camera)` from the
    // crease, so the way back is this.
    let denom = camera * cos_turn + across * toward;
    if (denom < 1.0e-3) {
        return vec4<f32>(0.0);
    }
    let along = across * camera / denom;
    let nearer = camera / max(camera - along * toward, 1.0e-3);
    let up = up_screen / nearer;

    let from_hinge = along / half_w;
    if (from_hinge > 1.0 || abs(up) > half_h * 0.5) {
        return vec4<f32>(0.0);
    }

    var read_x = 1.0 - from_hinge;
    if (hinge_at_left >= 0.5) {
        read_x = from_hinge;
    }
    let read_local = vec2<f32>(read_x, 0.5 + up / size_px.y);
    let read_uv = (effect_rect.xy + read_local * size_px) / tex_size;

    let radius = min(blur_px * pow(fold, 0.7) * (BLUR_FLOOR + (1.0 - BLUR_FLOOR) * from_hinge),
        MAX_RADIUS);
    let sum = softened(read_uv, radius, tex_size, low, high);
    let here = textureSample(input_texture, input_sampler, read_uv);
    let colour = vec4<f32>(sum.rgb / max(sum.a, 1.0e-4) * here.a, here.a);
    return shaded(colour, from_hinge, fold, dark, sheen);
}
