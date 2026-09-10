const TAPS: i32 = 4;
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

    let bend = clamp(get_float(1u), 0.0, 1.0);
    let blur_px = get_float(2u);
    let dim = clamp(get_float(3u), 0.0, 1.0);
    let sheen = get_float(4u);
    let hinge_at_left = get_float(5u);
    let cos_glass = get_float(6u);
    let sin_glass = get_float(7u);
    let camera_px = max(get_float(8u), 1.0);
    let stay = clamp(get_float(9u), 0.0, 1.0);
    let cos_flat = get_float(10u);
    let sin_flat = get_float(11u);

    // Distance from the hinge, where the panel is still square to the reader.
    // The far edge has travelled the whole arc, so it carries the blur.
    var from_hinge = 1.0 - local.x;
    if (hinge_at_left >= 0.5) {
        from_hinge = local.x;
    }

    // What the glass shows is not the screen turning with it. The picture goes
    // on lying in the plane the device is held in, and the glass slides across
    // it. So take where this fragment lands once the panel is turned and
    // projected, and read the picture that belongs at that place on the plane
    // the other half is still in. Undoing the fold this way is what makes the
    // picture look like it is behind the glass instead of painted on it.
    let along = from_hinge * size_px.x;
    let nearer_glass = camera_px / max(camera_px - along * sin_glass, 1.0);
    let lands = along * cos_glass * nearer_glass;
    let along_flat = lands * camera_px / max(camera_px * cos_flat + lands * sin_flat, 1.0);
    let nearer_flat = camera_px / max(camera_px - along_flat * sin_flat, 1.0);

    var flat_local = vec2<f32>(
        1.0 - along_flat / size_px.x,
        0.5 + (local.y - 0.5) * nearer_glass / nearer_flat,
    );
    if (hinge_at_left >= 0.5) {
        flat_local.x = along_flat / size_px.x;
    }
    let read_local = clamp(mix(local, flat_local, stay), vec2<f32>(0.0), vec2<f32>(1.0));
    let read_uv = (effect_rect.xy + read_local * size_px) / tex_size;

    let radius = blur_px * bend * pow(from_hinge, 1.3);
    let step = vec2<f32>(radius / (f32(TAPS) * max(tex_size.x, 1.0)), 0.0);
    let low = (effect_rect.xy + vec2<f32>(0.5)) / tex_size;
    let high = (effect_rect.xy + effect_rect.zw - vec2<f32>(0.5)) / tex_size;

    let centre = textureSample(input_texture, input_sampler, read_uv);
    var sum = centre;
    for (var i = 1; i <= TAPS; i = i + 1) {
        let offset = step * f32(i);
        let falloff = exp(-2.2 * f32(i * i) / f32(TAPS * TAPS));
        sum = sum
            + textureSample(input_texture, input_sampler, clamp(read_uv + offset, low, high))
                * falloff
            + textureSample(input_texture, input_sampler, clamp(read_uv - offset, low, high))
                * falloff;
    }
    // The panel's own shape decides what is covered; only the picture inside it
    // is read from somewhere else. Taking the cover from this fragment and the
    // colour from the taps keeps the rounded corners and the room around the
    // panel out of it. Dividing the colour by the alpha the taps carry, rather
    // than by their number, keeps the panel's edge crisp instead of dragging
    // the room in over it.
    let here = textureSample(input_texture, input_sampler, input.uv);
    var colour = vec4<f32>(sum.rgb / max(sum.a, 1.0e-4) * here.a, here.a);

    // A panel turning away takes less and less light, and takes least of it
    // along the edge that has swung furthest from the reader.
    let shaded = 1.0 - dim * (0.18 + 0.82 * from_hinge);
    colour = vec4<f32>(colour.rgb * shaded, colour.a);

    // The glass catches the light in a band that sits along the crease.
    let band = exp(-pow((from_hinge - 0.08) / 0.16, 2.0));
    colour = vec4<f32>(colour.rgb + vec3<f32>(sheen * band * bend * colour.a), colour.a);

    return colour;
}
