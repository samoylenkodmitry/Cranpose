
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
// drawn in place.
struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
    transform: vec4<f32>,
    translation: vec2<f32>,
    reserved: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// The batch's depth in a pass that lays opaque interiors down first: its
// place in the pass's order from `uniforms.reserved.x`, on the step the
// shape stage's records use (`record_depth`), so later interiors hide it.
const DEPTH_STEP: f32 = 1.0 / 1048576.0;

fn batch_depth() -> f32 {
    return max(1.0 - (uniforms.reserved.x + 1.0) * DEPTH_STEP, 0.0);
}

@group(1) @binding(0)
var image_texture: texture_2d<f32>;

@group(1) @binding(1)
var image_sampler: sampler;

@vertex
fn image_vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let placed = vec2<f32>(
        uniforms.transform.x * input.position.x + uniforms.transform.y * input.position.y,
        uniforms.transform.z * input.position.x + uniforms.transform.w * input.position.y,
    ) + uniforms.translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, batch_depth(), 1.0);
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

@fragment
fn image_mask_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
    let alpha = textureSample(image_texture, image_sampler, uv).r;
    return vec4<f32>(1.0, 1.0, 1.0, alpha) * input.color;
}
