
struct VertexInput {
    @location(0) position: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
}

struct ProjectiveBlitUniforms {
    viewport: vec2<f32>,
    source_size: vec2<f32>,
    inverse_row0: vec4<f32>,
    inverse_row1: vec4<f32>,
    inverse_row2: vec4<f32>,
    alpha: vec4<f32>,
    sampling: vec4<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blit: ProjectiveBlitUniforms;
