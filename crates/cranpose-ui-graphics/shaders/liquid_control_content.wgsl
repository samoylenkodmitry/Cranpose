override CONTROL_FOREGROUND: bool = false;

fn source_uv(position: vec2<f32>) -> vec2<f32> {
    let origin = u[62].xy;
    let extent = u[62].zw;
    return clamp(origin + position / u[0].xy * extent,
        origin + vec2<f32>(0.5), origin + extent - vec2<f32>(0.5))
        / vec2<f32>(textureDimensions(input_texture));
}

fn capsule_field(position: vec2<f32>) -> vec3<f32> {
    let local = (position - u[0].zw) / u[1].zw;
    let half_size = u[1].xy * 0.5;
    let radius = min(half_size.x, half_size.y);
    let nearest = clamp(local, -half_size + vec2<f32>(radius), half_size - vec2<f32>(radius));
    let delta = local - nearest;
    let length = length(delta);
    return vec3<f32>(delta / max(length, 0.001), length - radius);
}

fn content_warp(position: vec2<f32>, field: vec3<f32>, reach: f32, depth: f32) -> vec2<f32> {
    return position - field.xy * u[1].zw * reach
        * circular_displacement_profile(-field.z, depth);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let scale = u[62].zw / u[0].xy;
    var position = input.uv * u[0].xy;
    if CONTROL_FOREGROUND {
        position = (input.uv * vec2<f32>(textureDimensions(input_texture)) - u[62].xy) / scale;
    }
    let field = capsule_field(position);
    let pixel_scale = min(u[62].z / u[0].x, u[62].w / u[0].y);
    let coverage = clamp(0.5 - (field.z - select(0.0, 2.0 / 3.0, CONTROL_FOREGROUND)) * pixel_scale, 0.0, 1.0);
    let source = textureSampleLevel(input_texture, input_sampler, source_uv(position), 0.0);
    if coverage <= 0.0 {
        return select(source, vec4<f32>(0.0), CONTROL_FOREGROUND);
    }
    let activity = u[2].x;
    if CONTROL_FOREGROUND {
        let angle = -0.2617993878;
        let axis = vec2<f32>(field.y * cos(angle) + field.x * sin(angle),
            field.x * cos(angle) - field.y * sin(angle)) * u[1].zw;
        let spectral_reach = 2.3157895 * activity;
        let footprint = abs(dot(axis / u[1].zw, field.xy)) * spectral_reach;
        let presence = clamp(1.0 + field.z / 8.8, 0.0, 1.0)
            * clamp(-field.z / max(footprint, 0.001), 0.0, 1.0);
        var dispersed = source.rgb;
        if presence > 0.0 {
            dispersed = vec3<f32>(0.0);
            for (var band = -3; band <= 3; band += 1) {
                let offset = axis * (f32(band) / 3.0 * spectral_reach);
                let weights = vec3<f32>(max(f32(band), 0.0) / 6.0,
                    max(3.0 - abs(f32(band)), 0.0) / 9.0,
                    max(-f32(band), 0.0) / 6.0);
                var sample = source.rgb;
                if band != 0 {
                    sample = textureSampleLevel(input_texture, input_sampler, source_uv(position + offset), 0.0).rgb;
                }
                dispersed += sample * weights;
            }
        }
        var rgb = mix(source.rgb, dispersed, presence);
        let light = select(0.0, clamp(1.0 + 0.75 * field.z, 0.0, 1.0), -field.z < 1.0)
            * abs(field.x) * min(activity, 1.0);
        let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let reflected = clamp((rgb - vec3<f32>(luma)) * 1.1765
            + vec3<f32>(luma * 0.9118 + 0.1471), vec3<f32>(0.0), vec3<f32>(1.0));
        rgb = mix(rgb, reflected, light);
        return vec4<f32>(rgb, source.a) * coverage;
    }
    if -field.z >= 11.2 {
        return source;
    }
    let surface_position = content_warp(position, field, 8.8 * activity, 7.04);
    let portal_position = content_warp(surface_position, capsule_field(surface_position), 17.5 * activity, 11.2);
    let transmitted = textureSampleLevel(input_texture, input_sampler, source_uv(portal_position), 0.0);
    return mix(source, transmitted, coverage);
}
