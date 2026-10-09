struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

// A quad whose `uv` spans the effect's rect. The pass's viewport stays
// inside its target, as Safari's WebGPU requires, so u[55] (slots 220..224)
// places the rect in it: the clip-space scale minus one in xy and the
// offset in zw. Zero places it over the whole viewport.
@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let corner = vec2<f32>(f32(i32(vertex_index & 1u) * 2 - 1), f32(i32(vertex_index >> 1u) * 2 - 1));
    let placement = u[55];
    output.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    output.position = vec4<f32>(corner * (1.0 + placement.xy) + placement.zw, 0.0, 1.0);
    return output;
}
