// The green paper sweep, after `noise.wgsl` and `sheet_paper.wgsl`: a table
// that curves up into a wall, lit by an even sky and by a tall window whose
// bars show in its light. The light is held off the caption, so its words
// sit on calm green.
//
// The mesh is in world space, y up, z toward the viewer.
//
// params: 0 = the unit; 1..2 = x and z of the middle of the window's pool of
// light; 3 = the table's height; 4..5 = the camera's pitch, as its cosine
// and sine; 6..7 = the world's shift in view space, y and z; 8 = the eye's
// distance from the picture plane; 9 = the stage x of the optical axis;
// 10..11 = the stage x where the caption's shade ends and the light starts.

const PAPER_GREEN: vec3<f32> = vec3<f32>(0.013, 0.262, 0.122);

// 1 inside a bar of half width `half`, with soft edges `soft`.
fn bar(x: f32, half: f32, soft: f32) -> f32 {
    return smoothstep(half + soft, half - soft, abs(x));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let u = shaded_param(0u);
    let pool = vec2<f32>(shaded_param(1u), shaded_param(2u));
    let table_y = shaded_param(3u);
    let pitch = vec2<f32>(shaded_param(4u), shaded_param(5u));
    let shift = vec2<f32>(shaded_param(6u), shaded_param(7u));
    let eye_distance = shaded_param(8u);
    let axis_x = shaded_param(9u);
    let shade_ramp = vec2<f32>(shaded_param(10u), shaded_param(11u));
    let p = surface.position;
    let n = surface.normal;
    let key = surface.to_light;
    let rise = clamp(n.z, 0.0, 1.0);
    let slope = asin(rise);

    // The point as the window sees it: q lies in the plane square to the
    // light, in units.
    let d = (p - vec3<f32>(pool.x, table_y, pool.y)) / u;
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), key));
    let up = cross(key, right);
    let q = vec2<f32>(dot(d, right), dot(d, up));

    // A tall window with one upright and two cross bars, a little out of
    // true; its far end is softer.
    var w = q + vec2<f32>(-330.0, -800.0);
    w += w.yx * vec2<f32>(0.05, -0.03);
    let soft = 24.0 + 20.0 * smoothstep(-400.0, 1200.0, w.y);
    var pane = bar(w.x, 810.0, soft) * bar(w.y, 1310.0, soft);
    pane *= 1.0 - 0.9 * bar(w.x + 10.0, 17.0, soft * 0.8);
    pane *= 1.0 - 0.9 * bar(w.y + 470.0, 15.0, soft * 0.8);
    pane *= 1.0 - 0.9 * bar(w.y - 320.0, 15.0, soft * 0.8);

    // A flag keeps the window's light off the caption.
    let view_z = p.y * pitch.y + p.z * pitch.x + shift.y;
    let stage_x = axis_x + p.x * eye_distance / (eye_distance - view_z);
    let clear_share = clamp((stage_x - shade_ramp.x) / (shade_ramp.y - shade_ramp.x), 0.0, 1.0);
    let clear = clear_share * clear_share * clear_share * (clear_share * (clear_share * 6.0 - 15.0) + 10.0);
    pane *= mix(0.05, 1.0, clear);

    let hot = q - vec2<f32>(0.0, 500.0);
    let lit = surface.lit;
    let key_light = pane * max(dot(n, key), 0.0) * 1.18 * exp(-dot(hot, hot) / (2100.0 * 2100.0)) * lit;

    // The sky: a pool round the figure, less on the wall, a little bounce
    // near the light's middle, falling off far away; shadows dim it a
    // little.
    var ambient = 0.215 + 0.075 * exp(-dot(d.xz, d.xz) / (900.0 * 900.0));
    ambient *= mix(1.0, 0.84, smoothstep(0.0, 1.2, slope));
    ambient += 0.05 * exp(-dot(q, q) / (700.0 * 700.0));
    ambient *= 1.0 - 0.22 * smoothstep(500.0, 2100.0, length(d.xz));
    ambient *= 1.0 - 0.45 * (1.0 - lit) * (1.0 - rise);

    let tooth = 1.0 + (value_noise(p.xz / u * 0.9) - 0.5) * 0.045 + (value_noise(p.xy / u * 0.05) - 0.5) * 0.03;
    let light = vec3<f32>(0.96, 1.02, 1.04) * ambient + vec3<f32>(1.12, 1.02, 0.84) * key_light;
    let color = PAPER_GREEN * tooth * light * 1.34 + vec3<f32>(0.006, 0.008, 0.004) * key_light;
    return vec4<f32>(to_srgb(color), 1.0);
}
