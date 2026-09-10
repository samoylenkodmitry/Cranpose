const TAPS: i32 = 4;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let local = (input.uv * tex_size - effect_rect.xy) / size_px;

    let spine_at_left = get_float(0u);
    let bend = clamp(get_float(1u), 0.0, 1.0);
    let blur_px = get_float(2u);
    let shade = clamp(get_float(3u), 0.0, 1.0);
    let sheen = get_float(4u);

    var away = local.x;
    if (spine_at_left < 0.5) {
        away = 1.0 - local.x;
    }
    away = clamp(away, 0.0, 1.0);

    let radius = blur_px * bend * pow(away, 1.4);
    let step = vec2<f32>(radius / (f32(TAPS) * max(tex_size.x, 1.0)), 0.0);
    let low = (effect_rect.xy + vec2<f32>(0.5)) / tex_size;
    let high = (effect_rect.xy + effect_rect.zw - vec2<f32>(0.5)) / tex_size;

    var sum = textureSample(input_texture, input_sampler, input.uv);
    var weight = 1.0;
    for (var i = 1; i <= TAPS; i = i + 1) {
        let offset = step * f32(i);
        let falloff = exp(-2.2 * f32(i * i) / f32(TAPS * TAPS));
        sum = sum
            + textureSample(input_texture, input_sampler, clamp(input.uv + offset, low, high))
                * falloff
            + textureSample(input_texture, input_sampler, clamp(input.uv - offset, low, high))
                * falloff;
        weight = weight + 2.0 * falloff;
    }
    var colour = sum / weight;

    let crease = 1.0 - away;
    let darken = 1.0 - shade * (0.12 + 0.88 * crease * crease);
    colour = vec4<f32>(colour.rgb * darken, colour.a);

    let band = exp(-pow((away - 0.86) / 0.16, 2.0));
    colour = vec4<f32>(colour.rgb + vec3<f32>(sheen * band * bend * colour.a), colour.a);

    return colour;
}
