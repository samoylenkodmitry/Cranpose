@group(1) @binding(0) var hud_texture: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;

@fragment
fn fragment(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(hud_texture, hud_sampler, uv);
}
