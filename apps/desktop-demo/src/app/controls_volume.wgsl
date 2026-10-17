// A control's stage drawn as a volume: the unit cube about the origin,
// standing out of a wall, holds the stage, seen from wherever the scene's
// eye stands. Follows `controls_stage.wgsl`.

fn get_float(index: u32) -> f32 {
    return volume_param(index);
}

// The cube's x runs to the stage's right, its y out of the wall and its z
// down the wall; values 24 to 26 give the stage's units along each, and the
// stage's ground lies on the face toward the wall.
fn volume(eye: vec3<f32>, direction: vec3<f32>, entry: f32, exit: f32, footprint: f32) -> VolumeHit {
    let scale = vec3<f32>(get_float(24u), get_float(25u), get_float(26u));
    let ro = (eye + vec3<f32>(0.0, 0.5, 0.0)) * scale;
    let along = direction * scale;
    let stretch = max(length(along), 1e-6);
    // The ground lies on the face the ray leaves by, so the ray runs a
    // hundredth of a unit past it, which rounding cannot cut short.
    let span = vec2<f32>(entry, exit) * stretch + vec2<f32>(0.0, 0.01);
    let trace = trace_stage(ro, along / stretch, footprint * 0.5, span);
    var hit: VolumeHit;
    hit.color = trace.color;
    hit.at = eye + direction * (max(trace.distance, 0.0) / stretch);
    return hit;
}
