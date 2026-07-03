
struct OffsetUniforms {
    offset: vec2<f32>, // in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> params: OffsetUniforms;

@fragment
fn offset_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let shifted_uv = input.uv - params.offset / max(tex_size, vec2<f32>(1.0));
    let inside =
        shifted_uv.x >= 0.0 && shifted_uv.x <= 1.0 && shifted_uv.y >= 0.0 && shifted_uv.y <= 1.0;
    let clamped_uv = clamp(shifted_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(input_texture, input_sampler, clamped_uv)
        * select(0.0, 1.0, inside);
}
