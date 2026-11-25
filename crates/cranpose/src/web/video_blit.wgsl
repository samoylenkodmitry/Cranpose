// Cropped / rotated GPU blit for browser external video textures (WebGPU and WebGL2).
// Only invoked when clean aperture cropping or 90/180/270-degree rotation is active.

struct Transform {
    // xy: source UV at target (0, 0), zw: source UV delta across target +X
    origin_du: vec4<f32>,
    // xy: source UV delta across target +Y, zw: (1 / target_w, 1 / target_h)
    dv_inv_target: vec4<f32>,
    // xy: min safe UV inside crop, zw: max safe UV inside crop
    inside: vec4<f32>,
}

@group(0) @binding(0) var source_tex: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> transform: Transform;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn blit(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let t = at.xy * transform.dv_inv_target.zw;
    let uv = transform.origin_du.xy
        + t.x * transform.origin_du.zw
        + t.y * transform.dv_inv_target.xy;
    let clamped = clamp(uv, transform.inside.xy, transform.inside.zw);
    let rgb = textureSampleLevel(source_tex, filtering, clamped, 0.0).rgb;
    return vec4<f32>(rgb, 1.0);
}
