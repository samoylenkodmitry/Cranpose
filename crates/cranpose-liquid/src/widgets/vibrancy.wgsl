override VIBRANCY_MASK: bool = false;

fn source_uv(local: vec2<f32>) -> vec2<f32> {
    let origin = u[62].xy;
    let extent = u[62].zw;
    return clamp(origin + local * extent, origin + vec2<f32>(0.5), origin + extent - vec2<f32>(0.5))
        / vec2<f32>(textureDimensions(input_texture));
}

fn content_projection() -> vec2<f32> {
    return select(vec2<f32>(1.0), u[4].xy, u[4].xy > vec2<f32>(0.0));
}

fn content_anchor(position: vec2<f32>) -> vec2<f32> {
    let index = clamp(round((position.x - u[5].x) / max(u[5].z, 0.001)), 0.0, max(u[5].w - 1.0, 0.0));
    return u[5].xy + vec2<f32>(index * u[5].z, 0.0);
}

fn content_distance(position: vec2<f32>) -> f32 {
    let half_size = u[1].zw * 0.5 / content_projection();
    let radius = min(half_size.x, half_size.y);
    return smoothed_capsule_distance((position - u[1].xy) / content_projection(), half_size, radius * 0.6 * u[3].y);
}

fn content_displacement(position: vec2<f32>, pixel_scale: f32) -> vec2<f32> {
    let depth = -content_distance(position);
    let reach = u[3].y * u[3].z;
    if reach <= 0.0 || depth >= 14.0 * reach {
        return vec2<f32>(0.0);
    }
    let eps = 0.5 / max(pixel_scale, 0.001);
    let gradient = vec2<f32>(
        content_distance(position + vec2<f32>(eps, 0.0)) - content_distance(position - vec2<f32>(eps, 0.0)),
        content_distance(position + vec2<f32>(0.0, eps)) - content_distance(position - vec2<f32>(0.0, eps)),
    );
    let local_gradient = gradient * content_projection();
    let normal = local_gradient / max(length(local_gradient), 0.001);
    return -17.5 * reach * normal * content_projection() * circular_displacement_profile(depth, 14.0 * reach);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = source_uv(input.uv);
    let source = textureSample(input_texture, input_sampler, uv);
    let luminance = dot(source.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    var light_ink = u[3].x > 0.5;
    let region = u[58];
    if region.z > 0.0 && region.w > 0.0 {
        let mean = textureLoad(input_texture, vec2<i32>(region.xy), 0);
        light_ink = dot(mean.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) / max(mean.a, 0.001) < 0.6;
    }
    let base_gain = select(u[0].x, 0.95, light_ink);
    let base_offset = select(0.0, 0.95, light_ink);
    let base_ink = clamp(source.rgb - vec3<f32>(luminance * base_gain) + vec3<f32>(base_offset * source.a), vec3<f32>(0.0), vec3<f32>(1.0));
    if u[1].z <= 0.0 && !VIBRANCY_MASK {
        return vec4<f32>(base_ink, source.a);
    }
    let scale = u[62].zw / max(u[0].yz, vec2<f32>(0.001));
    let position = (uv * vec2<f32>(textureDimensions(input_texture)) - u[62].xy) / scale;
    let distance = content_distance(position);
    let aa = 0.5 / max(min(scale.x, scale.y), 0.001);
    let selected = (1.0 - smoothstep(-aa, aa, distance)) * u[2].a;
    if VIBRANCY_MASK {
        let warped = position + content_displacement(position, min(scale.x, scale.y));
        let anchor = content_anchor(warped);
        let projected = anchor + (warped - anchor) / max(u[0].w, 1.0);
        let projected_uv = source_uv(projected / max(u[0].yz, vec2<f32>(0.001)));
        let projected_source = textureSampleLevel(input_texture, input_sampler, projected_uv, 0.0);
        let ink = mix(source, projected_source, selected);
        return vec4<f32>(0.0, 0.0, 0.0, 1.0 - ink.a);
    }
    let accent_offset = select(0.5, 0.0, light_ink);
    let accent_ink = clamp(source.rgb * 0.5 + (u[2].rgb - vec3<f32>(accent_offset)) * source.a, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(mix(base_ink, accent_ink, selected), source.a);
}
