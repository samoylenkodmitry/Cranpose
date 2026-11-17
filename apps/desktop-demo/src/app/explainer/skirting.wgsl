// The lamp room's skirting board: dark painted wood that stands proud of
// the wall on the floor, its bevelled top catching the light that spills
// down the wall and back up from the floor.
//
// The mesh is a unit cube. params: 0..2 = the lamp's colour, its strength
// included, linear.

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let lamp = vec3<f32>(shaded_param(0u), shaded_param(1u), shaded_param(2u));
    // 0 at the board's top, 1 at the floor.
    let down = clamp(surface.position.y + 0.5, 0.0, 1.0);
    let bevel = 1.0 - smoothstep(0.0, 0.14, down);
    let nose = exp(-abs(down - 0.16) / 0.03);
    let from_floor = smoothstep(0.55, 1.0, down);
    let light = lamp * (0.02 + 0.03 * (1.0 - down) + 0.06 * from_floor + 0.22 * bevel + 0.16 * nose);
    let color = vec3<f32>(0.24, 0.20, 0.17) * light;
    return vec4<f32>(to_srgb(color), 1.0) * surface.color.a;
}
