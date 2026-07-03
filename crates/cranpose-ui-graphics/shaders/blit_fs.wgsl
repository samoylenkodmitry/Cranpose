
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
struct BlitUniforms {
    alpha: vec4<f32>,
    mask_rect: vec4<f32>,    // x, y, width, height in destination pixels
    mask_radii: vec4<f32>,   // top_left, top_right, bottom_left, bottom_right
    mask_enabled: vec4<f32>, // x > 0 => apply rounded mask
    sampling: vec4<f32>,     // x = 0 => linear, x = 1 => 4x box resolve
    dest_viewport: vec4<f32>, // x, y, width, height in destination pixels
    source_viewport: vec4<f32>, // x, y, width, height in source pixels
    resolve_span: vec4<f32>, // x, y = exact source pixels covered by one destination pixel
}
@group(1) @binding(0) var<uniform> blit: BlitUniforms;

