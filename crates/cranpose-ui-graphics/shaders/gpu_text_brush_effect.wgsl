
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

const BRUSH_LINEAR: u32 = 0u;
const BRUSH_RADIAL: u32 = 1u;
const BRUSH_SWEEP: u32 = 2u;
const TILE_CLAMP: u32 = 0u;
const TILE_REPEAT: u32 = 1u;
const TILE_MIRROR: u32 = 2u;
const TILE_DECAL: u32 = 3u;
const MAX_STOPS: u32 = 16u;
const DRAW_FILL: u32 = 0u;
const DRAW_STROKE: u32 = 1u;
const OUTLINE_SAMPLE_COUNT: u32 = 8u;

struct GradientSample {
    t: f32,
    valid: bool,
}

fn uniform_u32(value: f32) -> u32 {
    return u32(max(round(value), 0.0));
}

fn stop_color(index: u32) -> vec4<f32> {
    return u[8u + index * 2u];
}

fn stop_position(index: u32) -> f32 {
    return u[8u + index * 2u + 1u].x;
}

fn remap_gradient_t(raw_t: f32, tile_mode: u32) -> GradientSample {
    if (tile_mode == TILE_REPEAT) {
        let repeated = raw_t - floor(raw_t);
        return GradientSample(repeated, true);
    }

    if (tile_mode == TILE_MIRROR) {
        let wrapped = raw_t - floor(raw_t * 0.5) * 2.0;
        let mirrored = select(wrapped, 2.0 - wrapped, wrapped > 1.0);
        return GradientSample(clamp(mirrored, 0.0, 1.0), true);
    }

    if (tile_mode == TILE_DECAL) {
        let valid = raw_t >= 0.0 && raw_t <= 1.0;
        return GradientSample(clamp(raw_t, 0.0, 1.0), valid);
    }

    return GradientSample(clamp(raw_t, 0.0, 1.0), true);
}

fn sample_gradient(t: f32, stop_count: u32) -> vec4<f32> {
    if (stop_count == 0u) {
        return vec4<f32>(0.0);
    }
    if (stop_count == 1u) {
        return stop_color(0u);
    }

    let first = stop_color(0u);
    let first_t = stop_position(0u);
    if (t <= first_t) {
        return first;
    }

    for (var i: u32 = 0u; i + 1u < MAX_STOPS; i = i + 1u) {
        if (i + 1u >= stop_count) {
            break;
        }
        let current = stop_color(i);
        let next = stop_color(i + 1u);
        let current_t = stop_position(i);
        let next_t = stop_position(i + 1u);

        if (t <= next_t) {
            let span = max(next_t - current_t, 0.00001);
            let frac = clamp((t - current_t) / span, 0.0, 1.0);
            return mix(current, next, frac);
        }
    }

    return stop_color(stop_count - 1u);
}

fn evaluate_brush(local: vec2<f32>) -> vec4<f32> {
    let brush_type = uniform_u32(u[0].x);
    let stop_count = min(uniform_u32(u[0].y), MAX_STOPS);
    let tile_mode = uniform_u32(u[0].z);
    if (stop_count == 0u) {
        return vec4<f32>(0.0);
    }

    if (brush_type == BRUSH_LINEAR) {
        let start = u[2].xy;
        let end = u[2].zw;
        let delta = end - start;
        let denom = max(dot(delta, delta), 0.00001);
        let raw_t = dot(local - start, delta) / denom;
        let sample = remap_gradient_t(raw_t, tile_mode);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    if (brush_type == BRUSH_RADIAL) {
        let center = u[3].xy;
        let radius = max(u[3].z, 0.00001);
        let raw_t = distance(local, center) / radius;
        let sample = remap_gradient_t(raw_t, tile_mode);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    if (brush_type == BRUSH_SWEEP) {
        let center = u[4].xy;
        let delta = local - center;
        let angle = atan2(delta.y, delta.x);
        let raw_t = angle / (2.0 * 3.14159265358979) + 0.5;
        let sample = remap_gradient_t(raw_t, TILE_CLAMP);
        if (!sample.valid) {
            return vec4<f32>(0.0);
        }
        return sample_gradient(sample.t, stop_count);
    }

    return sample_gradient(0.0, stop_count);
}

fn sample_mask_alpha(uv: vec2<f32>) -> f32 {
    return textureSample(input_texture, input_sampler, uv).a;
}

fn max_dilated_alpha(uv: vec2<f32>, texel: vec2<f32>, radius_px: f32, center_alpha: f32) -> f32 {
    let radius = max(radius_px, 0.0);
    if (radius <= 0.0) {
        return center_alpha;
    }

    let half_radius = radius * 0.5;
    var max_alpha = center_alpha;
    let directions = array<vec2<f32>, 8>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.70710677, 0.70710677),
        vec2<f32>(-0.70710677, 0.70710677),
        vec2<f32>(0.70710677, -0.70710677),
        vec2<f32>(-0.70710677, -0.70710677),
    );

    for (var i: u32 = 0u; i < OUTLINE_SAMPLE_COUNT; i = i + 1u) {
        let dir = directions[i];
        max_alpha = max(max_alpha, sample_mask_alpha(uv + dir * texel * radius));
        max_alpha = max(max_alpha, sample_mask_alpha(uv + dir * texel * half_radius));
    }

    return max_alpha;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(input_texture, input_sampler, input.uv);
    let draw_mode = uniform_u32(u[5].x);
    let fill_alpha = sampled.a;
    if (draw_mode == DRAW_FILL && fill_alpha <= 0.0) {
        return vec4<f32>(0.0);
    }

    // Renderer-reserved slot 62 stores layer pixel rect: x, y, width, height.
    let layer_rect = u[62];
    let layer_origin = layer_rect.xy;
    let layer_size = max(layer_rect.zw, vec2<f32>(0.00001));
    let layer_max = layer_origin + layer_size;
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let pixel = input.uv * tex_size;
    if (pixel.x < layer_origin.x || pixel.y < layer_origin.y ||
        pixel.x > layer_max.x || pixel.y > layer_max.y) {
        return vec4<f32>(0.0);
    }

    let local_uv = (pixel - layer_origin) / layer_size;
    let logical_size = max(u[1].xy, vec2<f32>(0.00001));
    let stroke_padding_local = vec2<f32>(max(u[5].z, 0.0));
    let expanded_logical_size = max(logical_size + stroke_padding_local * 2.0, vec2<f32>(0.00001));
    let local = local_uv * expanded_logical_size - stroke_padding_local;
    let local_to_px = layer_size / expanded_logical_size;
    let local_to_px_avg = max((local_to_px.x + local_to_px.y) * 0.5, 0.00001);
    let stroke_width_local = max(u[5].y, 0.0);
    let stroke_radius_px = stroke_width_local * local_to_px_avg * 0.5;
    let texel = vec2<f32>(1.0) / max(tex_size, vec2<f32>(1.0));
    let outline_alpha = max_dilated_alpha(input.uv, texel, stroke_radius_px, fill_alpha);
    let stroke_alpha = max(outline_alpha - fill_alpha, 0.0);
    let material_alpha = select(fill_alpha, stroke_alpha, draw_mode == DRAW_STROKE);
    if (material_alpha <= 0.0) {
        return vec4<f32>(0.0);
    }

    let brush = evaluate_brush(local);
    let alpha_multiplier = clamp(u[0].w, 0.0, 1.0);
    let out_alpha = material_alpha * brush.a * alpha_multiplier;
    return vec4<f32>(brush.rgb * out_alpha, out_alpha);
}
