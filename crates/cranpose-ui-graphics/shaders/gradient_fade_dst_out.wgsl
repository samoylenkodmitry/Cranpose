
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

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by renderer in uniform slot 62.
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp -> pixel mapping for local effect coordinates.
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));

    let local_px = uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;
    let direction = get_float(4u);

    var axis_value = local_px.x;
    var axis_scale = dp_scale.x;
    if (direction >= 0.5 && direction < 1.5) {
        axis_value = size_px.x - local_px.x;
        axis_scale = dp_scale.x;
    } else if (direction >= 1.5 && direction < 2.5) {
        axis_value = local_px.y;
        axis_scale = dp_scale.y;
    } else if (direction >= 2.5) {
        axis_value = size_px.y - local_px.y;
        axis_scale = dp_scale.y;
    }

    let start_px = get_float(2u) * axis_scale;
    let end_px = get_float(3u) * axis_scale;
    let span = max(abs(end_px - start_px), 0.001);

    var keep_alpha = 1.0;
    if (end_px >= start_px) {
        keep_alpha = clamp((axis_value - start_px) / span, 0.0, 1.0);
    } else {
        keep_alpha = clamp((start_px - axis_value) / span, 0.0, 1.0);
    }

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * keep_alpha;
}
