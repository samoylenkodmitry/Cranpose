
fn composite_sample(
    source_pos: vec2<f32>,
    source_size: vec2<f32>,
    sampling_mode: f32,
) -> vec4<f32> {
    let safe_source_size = max(source_size, vec2<f32>(0.00001, 0.00001));
    let uv = source_pos / safe_source_size;
    if (sampling_mode <= 0.5) {
        return textureSample(input_texture, input_sampler, uv);
    }
    let dims = vec2<i32>(textureDimensions(input_texture));
    let texel = clamp(vec2<i32>(floor(source_pos)), vec2<i32>(0), dims - vec2<i32>(1));
    return textureLoad(input_texture, texel, 0);
}
