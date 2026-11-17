// Converts a decoded NV12 picture to RGB: a plane of luma and a plane of interleaved blue and
// red differences at half the size. The result keeps the picture's encoding, so Cranpose draws
// it as it draws any other image.

struct Conversion {
    // The luma's offset and scale, then the scale of a difference
    range: vec4<f32>,
    // The weights that give red and blue from the red and blue differences, and green from both
    weights: vec4<f32>,
}

@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var filtering: sampler;
@group(0) @binding(3) var<uniform> conversion: Conversion;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn convert(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(luma));
    let y = (textureLoad(luma, vec2<i32>(at.xy), 0).r - conversion.range.x) * conversion.range.y;
    // The differences sit at half the picture's size, so the sampler blends them
    let c = (textureSampleLevel(chroma, filtering, at.xy / size, 0.0).rg - 128.0 / 255.0)
        * conversion.range.z;
    let rgb = vec3<f32>(
        y + conversion.weights.x * c.y,
        y + conversion.weights.y * c.x + conversion.weights.z * c.y,
        y + conversion.weights.w * c.x,
    );
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
