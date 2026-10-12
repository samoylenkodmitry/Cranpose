override BLIT_UNMASKED_NEAREST: bool = false;

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
struct BlitUniforms {
    alpha: vec4<f32>,
    mask_rect: vec4<f32>,    // x, y, width, height in destination pixels
    mask_radii: vec4<f32>,   // top_left, top_right, bottom_left, bottom_right
    mask_enabled: vec4<f32>, // x > 0 => apply rounded mask
    sampling: vec4<f32>,     // x = 0 => linear, x = 1 => nearest texel
    dest_clip: vec4<f32>,    // left, top, right, bottom of the destination rect in clip space
    source_viewport: vec4<f32>, // x, y, width, height in source pixels
}
@group(1) @binding(0) var<uniform> blit: BlitUniforms;

// The quad over the destination rect, the whole target when it has none,
// with `uv` across it. The rasterizer then shades exactly the pixels whose
// centres the rect holds, and `uv` interpolates to where each one samples.
// A fragment that tested the rect and mapped its own position instead took
// Mali's compiler 12 ms longer on every blit pipeline.
@vertex
fn blit_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    var output: VertexOutput;
    output.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    output.position = vec4<f32>(
        mix(blit.dest_clip.x, blit.dest_clip.z, corner.x),
        mix(blit.dest_clip.w, blit.dest_clip.y, corner.y),
        0.0,
        1.0,
    );
    return output;
}

