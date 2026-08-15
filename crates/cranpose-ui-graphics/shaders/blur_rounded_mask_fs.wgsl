
struct BlurRoundedMaskUniforms {
    direction_and_radius: vec4<f32>,       // direction.xy, radius.xy
    texture_size_and_tile_mode: vec4<f32>, // texture_size.xy, tile_mode, unused
    effect_rect: vec4<f32>,                // x, y, width, height in effect pixels
    container_and_feather: vec4<f32>,      // container_size.xy, feather, unused
    corner_radii: vec4<f32>,               // top-left, top-right, bottom-right, bottom-left
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> params: BlurRoundedMaskUniforms;

fn inside_unit_bounds(uv: vec2<f32>) -> f32 {
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    return select(0.0, 1.0, inside);
}

fn sample_with_tile_mode(uv: vec2<f32>) -> vec4<f32> {
    let tile_mode = params.texture_size_and_tile_mode.z;
    if (tile_mode >= 2.5) {
        let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        return textureSample(input_texture, input_sampler, clamped_uv) * inside_unit_bounds(uv);
    }

    if (tile_mode >= 1.5) {
        let wrap_x = uv.x - floor(uv.x / 2.0) * 2.0;
        let wrap_y = uv.y - floor(uv.y / 2.0) * 2.0;
        let mirrored_uv = vec2<f32>(
            select(wrap_x, 2.0 - wrap_x, wrap_x > 1.0),
            select(wrap_y, 2.0 - wrap_y, wrap_y > 1.0),
        );
        return textureSample(input_texture, input_sampler, mirrored_uv);
    }

    if (tile_mode >= 0.5) {
        let repeated_uv = vec2<f32>(uv.x - floor(uv.x), uv.y - floor(uv.y));
        return textureSample(input_texture, input_sampler, repeated_uv);
    }

    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(input_texture, input_sampler, clamped_uv);
}

fn blurred_sample(uv: vec2<f32>) -> vec4<f32> {
    let input_size = vec2<f32>(
        params.texture_size_and_tile_mode.w,
        params.container_and_feather.w,
    );
    let texture_size = select(
        max(params.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0)),
        input_size,
        input_size.x >= 1.0 && input_size.y >= 1.0,
    );
    let pixel_size = 1.0 / texture_size;
    let dir = params.direction_and_radius.xy;
    let radius = max(dot(dir, params.direction_and_radius.zw), 0.0);
    let sigma = max(radius * 0.5, 0.001);
    let tap_count = min(i32(ceil(radius)), 32);

    if (tap_count <= 0) {
        return sample_with_tile_mode(uv);
    }

    let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);
    var color = vec4<f32>(0.0);
    var total_weight = 0.0;

    for (var i: i32 = -32; i <= 32; i = i + 1) {
        if (abs(i) > tap_count) {
            continue;
        }

        let fi = f32(i);
        let weight = exp(-(fi * fi) * inv_2sigma2);
        let offset = dir * fi * pixel_size;
        color = color + sample_with_tile_mode(uv + offset) * weight;
        total_weight = total_weight + weight;
    }

    return color / max(total_weight, 0.00001);
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

fn rounded_rect_alpha(
    local_px: vec2<f32>,
    size_px: vec2<f32>,
    corner_radii_px: vec4<f32>,
    feather_px: f32,
) -> f32 {
    let half = size_px * 0.5;
    let p = local_px - half;
    let d = sd_round_rect(p, half, corner_radii_px);
    let half_feather = max(feather_px * 0.5, 0.001);
    return 1.0 - smoothstep(-half_feather, half_feather, d);
}

@fragment
fn blur_rounded_mask_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = max(params.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let effect_rect = params.effect_rect;
    let container_dp = max(params.container_and_feather.xy, vec2<f32>(1.0));
    let dp_scale = effect_rect.zw / container_dp;
    let s = min(dp_scale.x, dp_scale.y);

    let local_px = input.uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;
    let corner_radii_px = max(params.corner_radii * s, vec4<f32>(0.0));
    let feather_px = max(params.container_and_feather.z * s, 0.0);
    let mask = rounded_rect_alpha(local_px, size_px, corner_radii_px, feather_px);

    return blurred_sample(input.uv) * mask;
}
