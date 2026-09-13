fn circular_displacement_profile(depth: f32, height: f32) -> f32 {
    let elevation = clamp(1.0 - depth / max(height, 0.001), 0.0, 1.0);
    return 1.0 - sqrt(max(1.0 - elevation * elevation, 0.0));
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
