
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_bounds: vec4<f32>,
}

struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var image_texture: texture_2d<f32>;

@group(1) @binding(1)
var image_sampler: sampler;

@vertex
fn image_vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = ((input.position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((input.position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.uv_bounds = input.uv_bounds;
    return output;
}

@fragment
fn image_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
    let sampled = textureSample(image_texture, image_sampler, uv);
    return sampled * input.color;
}
