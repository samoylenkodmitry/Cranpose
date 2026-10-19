// What the controls' stage and the room's control parts share: the
// stage's lights, as a metal part mirrors them, in the stage's
// coordinates, y up out of the ground.

fn key_dir() -> vec3<f32> {
    return normalize(vec3<f32>(-0.42, 0.86, 0.30));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn environment(dir: vec3<f32>, dark: f32) -> vec3<f32> {
    let up = clamp(dir.y, -1.0, 1.0);
    let ground = mix(vec3<f32>(0.034, 0.034, 0.038), vec3<f32>(0.006, 0.006, 0.009), dark);
    let band = mix(vec3<f32>(1.18, 1.20, 1.26), vec3<f32>(0.26, 0.29, 0.38), dark);
    let sky = mix(vec3<f32>(0.210, 0.213, 0.222), vec3<f32>(0.050, 0.054, 0.068), dark);
    let ceiling = mix(vec3<f32>(0.128, 0.130, 0.138), vec3<f32>(0.030, 0.032, 0.040), dark);
    var c = mix(ground, band, smoothstep(-0.22, 0.03, up));
    c = mix(c, sky, smoothstep(0.07, 0.36, up));
    c = mix(c, ceiling, smoothstep(0.52, 0.88, up));
    let softbox = mix(vec3<f32>(5.6, 5.5, 5.3), vec3<f32>(3.6, 3.6, 3.8), dark);
    c = c + softbox * smoothstep(0.930, 0.992, dot(dir, key_dir()));
    let fill = normalize(vec3<f32>(0.74, 0.30, 0.60));
    c = c + mix(vec3<f32>(0.85, 0.88, 0.98), vec3<f32>(0.34, 0.38, 0.50), dark)
        * smoothstep(0.90, 0.995, dot(dir, fill));
    let rim = normalize(vec3<f32>(-0.82, 0.08, -0.56));
    c = c + mix(vec3<f32>(1.10, 1.12, 1.20), vec3<f32>(0.44, 0.50, 0.66), dark)
        * smoothstep(0.88, 0.998, dot(dir, rim));
    return c;
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Where the ray `r` that a part at `p` reflects meets the ground: xy = the
// ground point, z = how much of the reflection the ground takes there,
// zero where the ray does not reach it.
fn ground_reflection(p: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    if (r.y >= -0.02) {
        return vec3<f32>(0.0);
    }
    let hit = p.xz + r.xz * (p.y / max(-r.y, 1e-4));
    let fade = clamp(-r.y * 2.4, 0.0, 1.0) * clamp(2.0 - 0.7 * length(hit), 0.0, 1.0);
    return vec3<f32>(hit, clamp(fade, 0.0, 0.92));
}

// How much more a part seen along `rd` mirrors toward its rim, of normal
// `n`.
fn fresnel(n: vec3<f32>, rd: vec3<f32>) -> f32 {
    return 0.90 + 0.10 * pow(1.0 - clamp(dot(n, -rd), 0.0, 1.0), 5.0);
}
