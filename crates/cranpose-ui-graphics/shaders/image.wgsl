
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

// An image of a layer drawn in place under a rounded clip: the clip's
// device rect and corner radius ride beside the quad.
struct RoundedVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_bounds: vec4<f32>,
    @location(4) clip_rect: vec4<f32>,
    @location(5) clip_radius: f32,
}

struct RoundedVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_bounds: vec4<f32>,
    @location(3) @interpolate(flat) clip_rect: vec4<f32>,
    @location(4) @interpolate(flat) clip_radius: f32,
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

fn placed_vertex(
    position: vec2<f32>,
    color: vec4<f32>,
    uv: vec2<f32>,
    uv_bounds: vec4<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    let placed = vec2<f32>(
        uniforms.transform.x * position.x + uniforms.transform.y * position.y,
        uniforms.transform.z * position.x + uniforms.transform.w * position.y,
    ) + uniforms.translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, batch_depth(), 1.0);
    output.color = color;
    output.uv = uv;
    output.uv_bounds = uv_bounds;
    return output;
}

@vertex
fn image_vs_main(input: VertexInput) -> VertexOutput {
    return placed_vertex(input.position, input.color, input.uv, input.uv_bounds);
}

@vertex
fn image_rounded_vs_main(input: RoundedVertexInput) -> RoundedVertexOutput {
    let placed = placed_vertex(input.position, input.color, input.uv, input.uv_bounds);
    return RoundedVertexOutput(
        placed.clip_position,
        placed.color,
        placed.uv,
        placed.uv_bounds,
        input.clip_rect,
        input.clip_radius,
    );
}

fn image_color(color: vec4<f32>, uv: vec2<f32>, uv_bounds: vec4<f32>) -> vec4<f32> {
    let clamped = clamp(uv, uv_bounds.xy, uv_bounds.zw);
    return textureSampleLevel(image_texture, image_sampler, clamped, 0.0) * color;
}

fn mask_color(color: vec4<f32>, uv: vec2<f32>, uv_bounds: vec4<f32>) -> vec4<f32> {
    let clamped = clamp(uv, uv_bounds.xy, uv_bounds.zw);
    let alpha = textureSampleLevel(image_texture, image_sampler, clamped, 0.0).r;
    return vec4<f32>(1.0, 1.0, 1.0, alpha) * color;
}

// `color` under a rounded clip, taking the coverage the blit's mask gives a
// surface composited through the same clip. Images blend with straight
// alpha, so the coverage scales the alpha alone.
fn rounded_clipped(color: vec4<f32>, input: RoundedVertexOutput) -> vec4<f32> {
    let position = input.clip_position.xy + uniforms.viewport_offset;
    let half_size = input.clip_rect.zw * 0.5;
    let local_pos = position - (input.clip_rect.xy + half_size);
    let dist = sdf_rounded_rect(local_pos, half_size, vec4<f32>(input.clip_radius));
    return vec4<f32>(color.rgb, color.a * (1.0 - smoothstep(-0.5, 0.5, dist)));
}

@fragment
fn image_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return image_color(input.color, input.uv, input.uv_bounds);
}

@fragment
fn image_mask_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return mask_color(input.color, input.uv, input.uv_bounds);
}

@fragment
fn image_rounded_fs_main(input: RoundedVertexOutput) -> @location(0) vec4<f32> {
    return rounded_clipped(image_color(input.color, input.uv, input.uv_bounds), input);
}

@fragment
fn image_mask_rounded_fs_main(input: RoundedVertexOutput) -> @location(0) vec4<f32> {
    return rounded_clipped(mask_color(input.color, input.uv, input.uv_bounds), input);
}
