// Writes a frame read from an sRGB texture, which decodes it, into a texture
// without the sRGB conversion, encoded again: the bytes the frame holds.

@group(0) @binding(0) var frame: texture_2d<f32>;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn encode(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let color = textureLoad(frame, vec2<i32>(at.xy), 0);
    let low = color.rgb * 12.92;
    let high = 1.055 * pow(color.rgb, vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(high, low, color.rgb <= vec3<f32>(0.0031308)), color.a);
}
