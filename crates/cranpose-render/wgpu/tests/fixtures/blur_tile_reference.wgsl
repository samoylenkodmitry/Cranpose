struct BlurRegion {
    source_region: vec4<f32>,
    dest_region: vec4<f32>,
    quad: vec4<f32>,
}

struct BlurUniforms {
    direction_and_radius: vec4<f32>,
    texture_size_and_tile_mode: vec4<f32>,
    pairs: array<vec4<f32>, 16>,
    kernel: vec4<f32>,
    target_size: vec4<f32>,
    regions: array<BlurRegion, 16>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) source_region: vec4<f32>,
    @location(2) @interpolate(flat) dest_region: vec4<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

override BLUR_BLOCK: i32 = 2;

override BLUR_TILE_MODE: u32 = 0u;

@vertex
fn blur_vs(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let region = blur.regions[instance_index];
    let target_size = max(blur.target_size.xy, vec2<f32>(1.0, 1.0));
    let corner = vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
    let pixel = region.quad.xy + corner * region.quad.zw;
    var out: VertexOutput;
    out.position = vec4<f32>(
        pixel.x / target_size.x * 2.0 - 1.0,
        1.0 - pixel.y / target_size.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = pixel / target_size;
    out.source_region = region.source_region;
    out.dest_region = region.dest_region;
    return out;
}

fn inside_unit_bounds(uv: vec2<f32>) -> f32 {
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    return select(0.0, 1.0, inside);
}

fn source_region(raw: vec4<f32>) -> vec4<f32> {
    let region = raw;
    if (region.z > 0.5 && region.w > 0.5) {
        return region;
    }
    return vec4<f32>(0.0, 0.0, blur.texture_size_and_tile_mode.xy);
}

fn region_texture_uv(raw: vec4<f32>, local: vec2<f32>) -> vec2<f32> {
    let region = source_region(raw);
    let texture_size = max(blur.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let half_texel = 0.5 / max(region.zw, vec2<f32>(1.0, 1.0));
    let held = clamp(local, half_texel, vec2<f32>(1.0, 1.0) - half_texel);
    return (region.xy + held * region.zw) / texture_size;
}

fn tiled_sample(raw: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    let tile_mode = blur.texture_size_and_tile_mode.z;
    if (tile_mode >= 1.5 && tile_mode < 2.5) {

        let wrap_x = uv.x - floor(uv.x / 2.0) * 2.0;
        let wrap_y = uv.y - floor(uv.y / 2.0) * 2.0;
        let mirrored_uv = vec2<f32>(
            select(wrap_x, 2.0 - wrap_x, wrap_x > 1.0),
            select(wrap_y, 2.0 - wrap_y, wrap_y > 1.0),
        );
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(raw, mirrored_uv), 0.0);
    }
    if (tile_mode >= 0.5 && tile_mode < 1.5) {

        let repeated_uv = vec2<f32>(uv.x - floor(uv.x), uv.y - floor(uv.y));
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(raw, repeated_uv), 0.0);
    }

    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(input_texture, input_sampler, region_texture_uv(raw, clamped_uv), 0.0);
}

fn tap_weight(uv: vec2<f32>, weight: f32) -> f32 {
    return select(weight, weight * inside_unit_bounds(uv), (blur.texture_size_and_tile_mode.z == 3.0));
}

fn region_local(input: VertexOutput) -> vec2<f32> {
    let dest = input.dest_region;
    if (dest.z > 0.5 && dest.w > 0.5) {
        return (input.position.xy - dest.xy) / dest.zw;
    }
    return input.uv;
}

@fragment
fn blur_downsample_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let local = region_local(input);
    let texel = 1.0 / max(source_region(input.source_region).zw, vec2<f32>(1.0, 1.0));
    let fetches = max(BLUR_BLOCK / 2, 1);
    var sum = vec4<f32>(0.0);
    for (var y: i32 = 0; y < fetches; y = y + 1) {
        for (var x: i32 = 0; x < fetches; x = x + 1) {
            let corner = vec2<f32>(f32(2 * x + 1 - fetches), f32(2 * y + 1 - fetches));
            sum = sum + tiled_sample(input.source_region, local + corner * texel);
        }
    }
    return sum / f32(fetches * fetches);
}

@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let local = region_local(input);
    let source_size = max(source_region(input.source_region).zw, vec2<f32>(1.0, 1.0));
    let step = blur.direction_and_radius.xy / source_size;
    let pair_count = i32(blur.kernel.x);

    var color = tiled_sample(input.source_region, local) * tap_weight(local, 1.0);
    if (pair_count <= 0) {
        return color;
    }

    for (var k: i32 = 0; k < pair_count; k = k + 1) {
        let pair = blur.pairs[k];
        let fi = f32(2 * k + 1);
        let fj = fi + 1.0;
        for (var side: f32 = -1.0; side <= 1.0; side = side + 2.0) {
            var offset = pair.z;
            var e = pair.w;
            if ((blur.texture_size_and_tile_mode.z == 3.0)) {
                let e1 = tap_weight(local + step * (fi * side), pair.x);
                let e2 = tap_weight(local + step * (fj * side), pair.y);
                e = e1 + e2;
                offset = select(0.0, (fi * e1 + fj * e2) / e, e > 0.0);
            }
            if (e > 0.0) {
                color = color + tiled_sample(input.source_region, local + step * (offset * side)) * e;
            }
        }
    }

    return color / max(blur.kernel.y, 0.00001);
}
