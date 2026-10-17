// A control's stage drawn as a card's effect, seen from its design camera
// tilted toward the pointer. Follows `controls_stage.wgsl`.

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let unit = min(size_px.y, size_px.x / max(get_float(19u), 0.1));
    let local_px = input.uv * tex_size - effect_rect.xy;
    let offset = (local_px - size_px * 0.5) / unit;
    let screen = vec2<f32>(offset.x, -offset.y);

    let yaw = get_float(12u) + get_float(5u);
    let pitch = clamp(get_float(13u) + get_float(6u), 0.14, 1.30);
    let distance = get_float(14u);
    let focal = get_float(15u);
    let look_at = vec3<f32>(0.0, get_float(16u), 0.0);
    let camera = look_at
        + vec3<f32>(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * distance;
    let fwd = normalize(look_at - camera);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(right, fwd);
    let rd = normalize(screen.x * right + screen.y * up + focal * fwd);

    let trace = trace_stage(camera, rd, 0.5 / (focal * unit), vec2<f32>(0.02, MAX_DIST));
    let base = textureSampleLevel(input_texture, input_sampler, input.uv, 0.0);
    return trace.color + base * (1.0 - trace.color.a);
}
