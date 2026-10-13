// Cranpose stores the HUD with premultiplied alpha in sRGB space.
@group(0) @binding(0) var hud: texture_2d<f32>;

fn linear_from_srgb(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + 0.055) / 1.055, vec3(2.4));
    return select(high, low, color <= vec3(0.04045));
}

// For a target that stores linear colors, such as an sRGB surface.
@fragment
fn over_linear(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let stored = textureLoad(hud, vec2<i32>(position.xy), 0);
    let color = stored.rgb / max(stored.a, 1.0 / 255.0);
    return vec4(linear_from_srgb(color) * stored.a, stored.a);
}

// For a target that stores sRGB-encoded colors, which blends as Cranpose does.
@fragment
fn over_encoded(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(hud, vec2<i32>(position.xy), 0);
}
