fn blurred_disk(distance: f32, radius: f32) -> f32 {
    let z = 0.5 * pow(distance / max(radius, 0.001), 2.0);
    let p = 0.3934693403 + z * (0.09020401043 + z * (0.007193838983
        + z * (0.0002919370927 + z * (0.000007171484581 + z * (0.0000001180411444
        + z * (1.392193893e-9 + z * (1.234065648e-11 + z * (8.520561128e-14
        + z * (4.711392277e-16 + z * (2.133168193e-18 + z * 8.053494527e-21))))))))));
    return exp(-z) * p;
}

fn circular_displacement_profile(depth: f32, height: f32) -> f32 {
    let elevation = clamp(1.0 - depth / max(height, 0.001), 0.0, 1.0);
    return 1.0 - sqrt(max(1.0 - elevation * elevation, 0.0));
}

fn gaussian_erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let polynomial = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t;
    return sign(x) * (1.0 - polynomial * exp(-x * x));
}

fn gaussian_stroke(distance: f32, width: f32, sigma: f32) -> f32 {
    let denominator = max(sigma, 0.001) * sqrt(2.0);
    return 0.5 * (gaussian_erf((distance + width * 0.5) / denominator)
        - gaussian_erf((distance - width * 0.5) / denominator));
}

fn smoothed_capsule_distance(position: vec2<f32>, half_size: vec2<f32>, smoothing: f32) -> f32 {
    let vertical = half_size.y > half_size.x;
    let h = select(half_size, half_size.yx, vertical);
    let p = abs(select(position, position.yx, vertical));
    let straight = h.x - h.y;
    if straight <= 0.001 {
        return length(p) - h.y;
    }
    let span = min(smoothing, straight);
    let x = p.x - straight;
    let ramp = max(1.0 - abs(x) / max(span, 0.001), 0.0);
    let q = vec2<f32>(max(x, 0.0) + span * ramp * ramp * 0.25, p.y);
    let q_length = length(q);
    if q_length <= 0.001 {
        return -h.y;
    }
    let gradient = q * vec2<f32>(clamp((x + span) / max(2.0 * span, 0.001), 0.0, 1.0), 1.0) / q_length;
    return (q_length - h.y) / max(length(gradient), 0.001);
}
fn floating_contact_light(rgb: vec3<f32>, distance: f32, radius: f32, strength: f32) -> vec3<f32> {
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let global = clamp((rgb - vec3<f32>(luma)) * 1.2 + vec3<f32>(luma + 0.05), vec3<f32>(0.0), vec3<f32>(1.0));
    return mix(mix(rgb, global, strength), vec3<f32>(1.0), blurred_disk(distance, radius) * strength);
}
