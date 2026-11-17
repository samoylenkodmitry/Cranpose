// A plastered wall painted pink, after `noise.wgsl`: the paint shades from
// its upper left to its lower right, a key light from the upper left rakes
// the plaster's relief, and one soft band of extra light runs across.
//
// params: 0..1 = the wall's size in logical pixels; 2 = pixels to the
// unit; 4..6 = the paint at the top, 8..10 at the bottom, both linear.

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

fn relief(p: vec2<f32>) -> f32 {
    return value_noise(p * 6.0) + value_noise(p * 13.0 + vec2<f32>(3.0)) * 0.5
        + value_noise(p * 31.0) * 0.22 + value_noise(p * 74.0) * 0.10
        + value_noise(p * 170.0) * 0.06;
}

fn srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let unit = shaded_param(2u);
    let q = surface.uv * size;
    let p = q / unit / 760.0;
    let texel = 1.0 / 1024.0;
    let h = relief(p);
    let slope = vec2<f32>(relief(p + vec2<f32>(texel, 0.0)) - h, relief(p + vec2<f32>(0.0, texel)) - h) / texel * 0.02;
    let c = surface.uv;
    var paint = mix(param3(4u), param3(8u), smoothstep(-0.1, 1.15, c.y * 0.75 + c.x * 0.35));
    paint *= 0.965 + 0.035 * h;
    var key = 1.13 - 0.31 * length((c - vec2<f32>(0.20, 0.02)) * vec2<f32>(0.72, 0.95));
    key += dot(slope, vec2<f32>(-0.55, -0.8)) * 0.09;
    let band = smoothstep(0.0, 0.9, 1.0 - abs(c.x * 0.9 + c.y * 0.42 - 0.52));
    key *= 0.93 + 0.09 * band;
    let lit = paint * key;
    // The same soft shoulder as the chocolate's, so the shadows it lays on
    // the wall match the wall.
    let shouldered = lit / (1.0 + max(lit - vec3<f32>(0.92), vec3<f32>(0.0)) * 0.75);
    let grain = (hash(q * 1.7) - 0.5) / 255.0;
    return vec4<f32>(clamp(srgb(shouldered) + vec3<f32>(grain), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0) * surface.color.a;
}
