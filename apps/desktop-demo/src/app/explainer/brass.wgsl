// Polished brass, after `studio.wgsl`: the content's colour as the metal's,
// the studio mirrored in the curve with a warm tint, a fine brushed grain
// around the parts, and a hot highlight from the light.

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let alpha = surface.color.a;
    let metal = surface.color.rgb / max(alpha, 1.0 / 255.0);
    let n = surface.normal;
    let v = surface.to_eye;
    let l = surface.to_light;
    let lit = surface.lit;
    let brushed = 0.92 + 0.08 * sin(surface.uv.x * 900.0);
    let mirrored = studio(reflect(-v, n)) * brushed;
    let facing = max(dot(n, v), 0.0);
    // Metal mirrors more at a grazing angle.
    let fresnel = metal + (vec3<f32>(1.0) - metal) * pow(1.0 - facing, 5.0);
    let h = normalize(l + v);
    let highlight = pow(max(dot(n, h), 0.0), 120.0) * lit;
    let diffuse = surface.ambient + (1.0 - surface.ambient) * max(dot(n, l), 0.0) * lit;
    let color = fresnel * mirrored * 0.85 + metal * diffuse * 0.15
        + vec3<f32>(1.0, 0.95, 0.82) * highlight * 1.5;
    return vec4<f32>(color * alpha, alpha);
}
