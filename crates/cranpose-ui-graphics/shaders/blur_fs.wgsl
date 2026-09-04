
struct BlurUniforms {
    direction_and_radius: vec4<f32>,      // direction.xy, radius.xy in destination pixels
    texture_size_and_tile_mode: vec4<f32>,// texture_size.xy, tile_mode, unused
    source_region: vec4<f32>,             // x, y, width, height in source texels; zero = whole
    dest_region: vec4<f32>,               // x, y, width, height in destination pixels; zero = whole
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

fn inside_unit_bounds(uv: vec2<f32>) -> f32 {
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    return select(0.0, 1.0, inside);
}

// The source region in texels: the whole texture unless the uniform names
// a packed region of it.
fn source_region() -> vec4<f32> {
    let region = blur.source_region;
    if (region.z > 0.5 && region.w > 0.5) {
        return region;
    }
    return vec4<f32>(0.0, 0.0, blur.texture_size_and_tile_mode.xy);
}

// A region-local coordinate in [0, 1] mapped onto the texture, held to the
// region's texel centers so a bilinear tap never reads beside the region:
// regions are packed edge to edge, and the edge reads as a dedicated
// texture's clamp-to-edge would.
fn region_texture_uv(local: vec2<f32>) -> vec2<f32> {
    let region = source_region();
    let texture_size = max(blur.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let half_texel = 0.5 / max(region.zw, vec2<f32>(1.0, 1.0));
    let held = clamp(local, half_texel, vec2<f32>(1.0, 1.0) - half_texel);
    return (region.xy + held * region.zw) / texture_size;
}

fn sample_with_tile_mode(uv: vec2<f32>) -> vec4<f32> {
    let tile_mode = blur.texture_size_and_tile_mode.z;
    if (tile_mode >= 2.5) {
        // Decal: out-of-bounds samples are transparent.
        let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(clamped_uv), 0.0)
            * inside_unit_bounds(uv);
    }

    if (tile_mode >= 1.5) {
        // Mirror: ... 0->1, 1->0, repeat.
        let wrap_x = uv.x - floor(uv.x / 2.0) * 2.0;
        let wrap_y = uv.y - floor(uv.y / 2.0) * 2.0;
        let mirrored_uv = vec2<f32>(
            select(wrap_x, 2.0 - wrap_x, wrap_x > 1.0),
            select(wrap_y, 2.0 - wrap_y, wrap_y > 1.0),
        );
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(mirrored_uv), 0.0);
    }

    if (tile_mode >= 0.5) {
        // Repeated: wrap to [0,1).
        let repeated_uv = vec2<f32>(uv.x - floor(uv.x), uv.y - floor(uv.y));
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(repeated_uv), 0.0);
    }

    // Clamp: sample nearest edge texel outside bounds.
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(input_texture, input_sampler, region_texture_uv(clamped_uv), 0.0);
}

@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    // The fragment's place in its destination region, in [0, 1]: the whole
    // target unless the uniform names a region of it. One region-local unit
    // spans one source region, so a step of one destination pixel is one
    // source region width over the destination width, which is the
    // downscale the caller chose for a wide blur.
    var local = input.uv;
    var dest_size = max(blur.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let dest = blur.dest_region;
    if (dest.z > 0.5 && dest.w > 0.5) {
        local = (input.position.xy - dest.xy) / dest.zw;
        dest_size = dest.zw;
    }
    let pixel_size = 1.0 / dest_size;
    let dir = blur.direction_and_radius.xy;
    // Use the radius component matching the direction.
    let radius = max(dot(dir, blur.direction_and_radius.zw), 0.0);
    let sigma = max(radius * 0.5, 0.001);

    // Number of taps on each side (capped for shader cost stability).
    let tap_count = min(i32(ceil(radius)), 32);

    if (tap_count <= 0) {
        return sample_with_tile_mode(local);
    }

    let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);
    var color = vec4<f32>(0.0);
    var total_weight = 0.0;

    // The trip count comes off the uniform buffer, so control flow stays
    // uniform and the loop shrinks with the radius: a radius-6 blur runs 13
    // iterations, not a fixed 65. Sampling is explicit-LOD (the sources are
    // mipless offscreens), which frees the taps from derivative uniformity.
    for (var i: i32 = -tap_count; i <= tap_count; i = i + 1) {
        let fi = f32(i);
        let weight = exp(-(fi * fi) * inv_2sigma2);
        let offset = dir * fi * pixel_size;
        color = color + sample_with_tile_mode(local + offset) * weight;
        total_weight = total_weight + weight;
    }

    return color / max(total_weight, 0.00001);
}
