
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

// The prefix of the viewport uniform the shape stage documents: the
// segment's transform into its target, the identity unless a layer is
// drawn in place, and the origin a retained run's vertices sit at, which is
// added before the transform exactly as the shared path adds it on the CPU.
struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
    transform: vec4<f32>,
    translation: vec2<f32>,
    reserved: vec2<f32>,
    inverse: vec4<f32>,
    origin: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var glyph_texture: texture_2d<f32>;

@group(1) @binding(1)
var glyph_sampler: sampler;

@vertex
fn glyph_atlas_vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let position = input.position + uniforms.origin;
    let placed = vec2<f32>(
        uniforms.transform.x * position.x + uniforms.transform.y * position.y,
        uniforms.transform.z * position.x + uniforms.transform.w * position.y,
    ) + uniforms.translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.uv_bounds = input.uv_bounds;
    return output;
}

@fragment
fn glyph_atlas_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
    let coverage = textureSample(glyph_texture, glyph_sampler, uv).r;
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
