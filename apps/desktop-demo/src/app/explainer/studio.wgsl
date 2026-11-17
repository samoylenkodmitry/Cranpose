// What the tab's polished metals mirror, shared by the silk's gold leaf and
// the brass.

// The room a polished surface mirrors: a large soft light behind the viewer
// and a window up and to the left, a bright ceiling and a dark floor.
fn studio(direction: vec3<f32>) -> vec3<f32> {
    let up = clamp(-direction.y * 0.5 + 0.5, 0.0, 1.0);
    let room = mix(vec3<f32>(0.22, 0.18, 0.14), vec3<f32>(0.95, 0.92, 0.86), smoothstep(0.2, 0.85, up));
    let behind = smoothstep(0.35, 0.95, dot(direction, normalize(vec3<f32>(-0.15, -0.25, -1.0))));
    let window = smoothstep(0.8, 0.97, dot(direction, normalize(vec3<f32>(-0.55, -0.6, -0.6))));
    return room + vec3<f32>(0.75, 0.72, 0.66) * behind + vec3<f32>(1.4, 1.32, 1.18) * window;
}
