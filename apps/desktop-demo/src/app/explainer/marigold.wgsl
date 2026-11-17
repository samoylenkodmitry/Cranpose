// Seamless marigold paper in window light, after `noise.wgsl`: the
// content's colour with a fine grain, sparse fibres and a soft mottle,
// warmed where the sun through a window slants over it and crossed by the
// soft shade of the window's glazing bars. Red holds up in the shade, blue
// drops.
//
// params: 0..1 = the content's size in logical pixels.

fn sunlight(q_in: vec2<f32>) -> f32 {
    let q = q_in + 0.012 * (vec2<f32>(value_noise(q_in * 3.0), value_noise(q_in * 3.0 + 7.0)) - 0.5);
    let r = vec2<f32>(q.x + q.y * 0.42, q.y * 0.96 - q.x * 0.20);
    let soft = 0.035 + 0.03 * smoothstep(-0.6, 0.7, q.x);
    let d = abs(r - vec2<f32>(0.02, 0.0)) - vec2<f32>(0.80, 0.58);
    let sun_patch = 1.0 - smoothstep(-soft * 1.4, soft * 1.4, length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0));
    let bars = (1.0 - smoothstep(-soft, soft, abs(r.x + 0.52) - 0.026))
        + (1.0 - smoothstep(-soft, soft, abs(r.x - 0.56) - 0.03))
        + (1.0 - smoothstep(-soft, soft, abs(r.y + 0.08) - 0.020)) * 0.9;
    return mix(0.56, 1.0, sun_patch * (1.0 - 0.82 * clamp(bars, 0.0, 1.0)));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let alpha = surface.color.a;
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let point = surface.uv * size;
    let q = (point - size * 0.5) / size.y;
    let sun = sunlight(q);
    let falloff = 1.07 - 0.16 * dot(q, vec2<f32>(0.62, 0.78));
    let grain = (hash(floor(point * 1.5)) - 0.5) * 0.05
        + (value_noise(point * 0.9) - 0.5) * 0.05
        + (value_noise(point * 0.012) + 0.5 * value_noise(point * 0.03) - 0.75) * 0.10;
    let base = surface.color.rgb / max(alpha, 1.0 / 255.0);
    var ground = base * (1.0 + grain * 0.8);
    ground *= pow(vec3<f32>(min(sun * falloff, 1.1)), vec3<f32>(0.62, 0.98, 1.5));
    // A shadow on marigold is warm and brown.
    let in_shadow = mix(vec3<f32>(0.40, 0.28, 0.19), vec3<f32>(1.0), surface.lit);
    return vec4<f32>(ground * in_shadow * alpha, alpha);
}
