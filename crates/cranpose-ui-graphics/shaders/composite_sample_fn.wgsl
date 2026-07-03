
fn composite_sample_box4(
    source_pos: vec2<f32>,
    source_size: vec2<f32>,
    span_hint: vec2<f32>,
) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(input_texture));
    let inferred_footprint = vec2<f32>(
        max(abs(dpdx(source_pos.x)), abs(dpdy(source_pos.x))),
        max(abs(dpdx(source_pos.y)), abs(dpdy(source_pos.y))),
    );
    let footprint = vec2<f32>(
        select(inferred_footprint.x, span_hint.x, span_hint.x > 0.0),
        select(inferred_footprint.y, span_hint.y, span_hint.y > 0.0),
    );
    let span = max(footprint, vec2<f32>(1.0, 1.0));
    let left = source_pos - span * 0.5;
    let right = source_pos + span * 0.5;
    let start_x = i32(floor(left.x));
    let start_y = i32(floor(left.y));
    var accum = vec4<f32>(0.0);
    var total_weight = 0.0;

    for (var offset_y: i32 = 0; offset_y < 6; offset_y = offset_y + 1) {
        let texel_y = start_y + offset_y;
        let texel_top = f32(texel_y);
        let texel_bottom = texel_top + 1.0;
        let weight_y = max(0.0, min(right.y, texel_bottom) - max(left.y, texel_top));
        if (weight_y <= 0.0) {
            continue;
        }

        for (var offset_x: i32 = 0; offset_x < 6; offset_x = offset_x + 1) {
            let texel_x = start_x + offset_x;
            let texel_left = f32(texel_x);
            let texel_right = texel_left + 1.0;
            let weight_x = max(0.0, min(right.x, texel_right) - max(left.x, texel_left));
            let weight = weight_x * weight_y;
            if (weight <= 0.0) {
                continue;
            }

            total_weight = total_weight + weight;
            if (texel_x < 0 || texel_x >= dims.x || texel_y < 0 || texel_y >= dims.y) {
                continue;
            }
            accum = accum + textureLoad(input_texture, vec2<i32>(texel_x, texel_y), 0) * weight;
        }
    }

    return accum / max(total_weight, 0.00001);
}

fn composite_sample(
    source_pos: vec2<f32>,
    source_size: vec2<f32>,
    sampling_mode: f32,
    span_hint: vec2<f32>,
) -> vec4<f32> {
    let safe_source_size = max(source_size, vec2<f32>(0.00001, 0.00001));
    let uv = source_pos / safe_source_size;
    if (sampling_mode <= 0.5) {
        return textureSample(input_texture, input_sampler, uv);
    }
    return composite_sample_box4(source_pos, safe_source_size, span_hint);
}
