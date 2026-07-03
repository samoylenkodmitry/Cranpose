
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

fn corner_radius_for_point(p: vec2<f32>, radii: vec4<f32>) -> f32 {
    if (p.x < 0.0) {
        if (p.y < 0.0) {
            return radii.x;
        }
        return radii.w;
    }
    if (p.y < 0.0) {
        return radii.y;
    }
    return radii.z;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let radius = corner_radius_for_point(p, radii);
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn rounded_rect_alpha(local_px: vec2<f32>, size_px: vec2<f32>, corner_radii_px: vec4<f32>, feather_px: f32) -> f32 {
    let half = size_px * 0.5;
    let p = local_px - half;
    let d = sd_round_rect(p, half, corner_radii_px);
    let half_feather = max(feather_px * 0.5, 0.001);
    return 1.0 - smoothstep(-half_feather, half_feather, d);
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

    let corner_radii_px = max(vec4<f32>(
        get_float(3u),
        get_float(4u),
        get_float(5u),
        get_float(6u),
    ) * s, vec4<f32>(0.0));
    let feather_px = max(get_float(2u) * s, 0.0);
    let mask = rounded_rect_alpha(local_px, size_px, corner_radii_px, feather_px);

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * mask;
}
