const TAPS: i32 = 12;
/// How wide the smear is allowed to grow, in pixels of the panel's own width.
const MAX_RADIUS: f32 = 130.0;
const WALLPAPER: f32 = 0.5;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn dune(p: vec2<f32>, crest: f32, near: vec3<f32>, far: vec3<f32>, colour: vec3<f32>) -> vec3<f32> {
    let mask = smoothstep(crest - 0.004, crest + 0.004, p.y);
    let depth = clamp((p.y - crest) * 2.4, 0.0, 1.0);
    return mix(colour, mix(near, far, depth), mask);
}

/// The lock screen behind the glass: dusk over dunes, drawn from the panel's
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

    let far_crest = 0.64 + 0.10 * sin(p.x * 2.1 + 1.2) + 0.03 * sin(p.x * 5.3);
    colour = dune(
        p,
        far_crest,
        vec3<f32>(0.82, 0.68, 0.48),
        vec3<f32>(0.52, 0.42, 0.30),
        colour,
    );

    let ripple = 0.014 * sin(p.x * 74.0 + p.y * 26.0)
        * smoothstep(far_crest, far_crest + 0.06, p.y);
    colour = colour + vec3<f32>(ripple);

    let near_crest = 0.84 + 0.17 * sin(p.x * 1.3 + 3.4);
    colour = dune(
        p,
        near_crest,
        vec3<f32>(0.60, 0.47, 0.33),
        vec3<f32>(0.28, 0.22, 0.17),
        colour,
    );

    let corner = distance(p, vec2<f32>(0.5, 0.5));
    return colour * (1.0 - 0.38 * corner * corner);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let local = clamp((input.uv * tex_size - effect_rect.xy) / size_px, vec2<f32>(0.0), vec2<f32>(1.0));

    if (get_float(0u) < WALLPAPER) {
        // This panel is a window on to part of the whole open screen, so the
        // picture is drawn from where the window sits in it.
        let origin = get_float(1u);
        let span = get_float(2u);
        return vec4<f32>(wallpaper(vec2<f32>(origin + local.x * span, local.y)), 1.0);
    }

    let squeeze = clamp(get_float(1u), 0.0, 1.0);
    let blur_px = get_float(2u);
    let dim = clamp(get_float(3u), 0.0, 1.0);
    let sheen = get_float(4u);
    let hinge_at_left = get_float(5u);
    let spread = max(get_float(6u), 1.0);

    // Distance from the hinge, where the panel is still square to the reader.
    // The far edge has travelled the whole arc, so it carries the smear.
    var from_hinge = 1.0 - local.x;
    if (hinge_at_left >= 0.5) {
        from_hinge = local.x;
    }

    // The panel is folding away, so what it holds is being pressed into an
    // ever narrower band. Widen the blur by as much as the panel is being
    // pressed, so the smear stays the same width to the reader while the
    // picture under it is squeezed out of legibility.
    let radius = min(blur_px * squeeze * pow(from_hinge, 1.1) * spread, MAX_RADIUS);
    let step = vec2<f32>(radius / (f32(TAPS) * max(tex_size.x, 1.0)), 0.0);
    let low = (effect_rect.xy + vec2<f32>(0.5)) / tex_size;
    let high = (effect_rect.xy + effect_rect.zw - vec2<f32>(0.5)) / tex_size;

    var sum = textureSample(input_texture, input_sampler, input.uv);
    for (var i = 1; i <= TAPS; i = i + 1) {
        let offset = step * f32(i);
        let falloff = exp(-2.2 * f32(i * i) / f32(TAPS * TAPS));
        sum = sum
            + textureSample(input_texture, input_sampler, clamp(input.uv + offset, low, high))
                * falloff
            + textureSample(input_texture, input_sampler, clamp(input.uv - offset, low, high))
                * falloff;
    }

    // Average the colour the taps carry, not their number: dividing by the
    // alpha they bring keeps the panel's own edge crisp instead of dragging
    // the room in over it, and the panel's own cover decides what is covered.
    let here = textureSample(input_texture, input_sampler, input.uv);
    var colour = vec4<f32>(sum.rgb / max(sum.a, 1.0e-4) * here.a, here.a);

    // A panel turning away takes less and less light, and takes least of it
    // along the edge that has swung furthest from the reader.
    let shaded = 1.0 - dim * (0.18 + 0.82 * from_hinge);
    colour = vec4<f32>(colour.rgb * shaded, colour.a);

    // Glass this near edge on scatters what comes through it, so the picture
    // goes milky as it is pressed into the crease rather than simply dark.
    let veil = clamp(get_float(7u) * squeeze * from_hinge, 0.0, 1.0);
    colour = vec4<f32>(
        mix(colour.rgb, vec3<f32>(0.60, 0.61, 0.65) * colour.a, veil),
        colour.a,
    );

    // The glass catches the light in a band that sits along the crease.
    let band = exp(-pow((from_hinge - 0.08) / 0.16, 2.0));
    colour = vec4<f32>(colour.rgb + vec3<f32>(sheen * band * squeeze * colour.a), colour.a);

    return colour;
}
