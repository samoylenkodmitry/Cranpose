
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn rounded_rect_alpha(local_px: vec2<f32>, size_px: vec2<f32>, corner_radius_px: f32) -> f32 {
    let half = size_px * 0.5;
    let p = local_px - half;
    let d = sd_round_rect(p, half, corner_radius_px);
    return 1.0 - smoothstep(-1.0, 1.0, d);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by renderer in uniform slot 62.
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp -> pixel mapping for local effect coordinates.
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    let local_px = uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;

    let progress = clamp(get_float(2u), 0.0, 1.0);
    let feather_px = max(get_float(3u) * s, 0.001);
    let corner_radius_px = max(get_float(4u) * s, 0.0);
    let direction = get_float(5u);

    var axis_value = local_px.x;
    var axis_extent = max(size_px.x, 0.001);

    if (direction >= 0.5 && direction < 1.5) {
        axis_value = size_px.x - local_px.x;
        axis_extent = max(size_px.x, 0.001);
    } else if (direction >= 1.5 && direction < 2.5) {
        axis_value = local_px.y;
        axis_extent = max(size_px.y, 0.001);
    } else if (direction >= 2.5) {
        axis_value = size_px.y - local_px.y;
        axis_extent = max(size_px.y, 0.001);
    }

    var directional_alpha = 1.0;
    if (progress < 1.0) {
        let cut_edge = progress * axis_extent;
        directional_alpha = smoothstep(cut_edge + feather_px * 0.5, cut_edge - feather_px * 0.5, axis_value);
    }
    let shape_alpha = rounded_rect_alpha(local_px, size_px, corner_radius_px);
    let mask = directional_alpha * shape_alpha;

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * mask;
}
