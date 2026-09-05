
struct BlurUniforms {
    direction_and_radius: vec4<f32>,      // direction.xy, radius.xy in source texels
    texture_size_and_tile_mode: vec4<f32>,// sampled texture size.xy, tile_mode, unused
    source_region: vec4<f32>,             // x, y, width, height in source texels; zero = whole
    dest_region: vec4<f32>,               // x, y, width, height in destination pixels; zero = whole
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

// The source texels one destination pixel of the downsample stands for on
// each axis. A pipeline constant, so the block's fetch loops unroll.
override BLUR_BLOCK: i32 = 2;

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

// The texture value at a region-local coordinate under the tile mode:
// mirrored or repeated into [0, 1], or held to the region's edge.
fn tiled_sample(uv: vec2<f32>) -> vec4<f32> {
    let tile_mode = blur.texture_size_and_tile_mode.z;
    if (tile_mode >= 1.5 && tile_mode < 2.5) {
        // Mirror: ... 0->1, 1->0, repeat.
        let wrap_x = uv.x - floor(uv.x / 2.0) * 2.0;
        let wrap_y = uv.y - floor(uv.y / 2.0) * 2.0;
        let mirrored_uv = vec2<f32>(
            select(wrap_x, 2.0 - wrap_x, wrap_x > 1.0),
            select(wrap_y, 2.0 - wrap_y, wrap_y > 1.0),
        );
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(mirrored_uv), 0.0);
    }
    if (tile_mode >= 0.5 && tile_mode < 1.5) {
        // Repeated: wrap to [0,1).
        let repeated_uv = vec2<f32>(uv.x - floor(uv.x), uv.y - floor(uv.y));
        return textureSampleLevel(input_texture, input_sampler, region_texture_uv(repeated_uv), 0.0);
    }
    // Clamp and decal: hold to the region's edge; decal drops the taps
    // outside through their weights.
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(input_texture, input_sampler, region_texture_uv(clamped_uv), 0.0);
}

// A tap's weight under the tile mode: zero outside the region for decal.
fn tap_weight(uv: vec2<f32>, weight: f32) -> f32 {
    let decal = blur.texture_size_and_tile_mode.z >= 2.5;
    return select(weight, weight * inside_unit_bounds(uv), decal);
}

// The fragment's place in its destination region, in [0, 1]: the whole
// target unless the uniform names a region of it. One region-local unit
// spans one source region.
fn region_local(input: VertexOutput) -> vec2<f32> {
    let dest = blur.dest_region;
    if (dest.z > 0.5 && dest.w > 0.5) {
        return (input.position.xy - dest.xy) / dest.zw;
    }
    return input.uv;
}

// The downsample: each destination pixel is the average of the block of
// source texels it stands for. The pixel's centre is the block's centre, a
// texel corner for an even block, so the fetches at every other corner
// across the block read each of its texels once through the bilinear
// filter.
@fragment
fn blur_downsample_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let local = region_local(input);
    let texel = 1.0 / max(source_region().zw, vec2<f32>(1.0, 1.0));
    let fetches = max(BLUR_BLOCK / 2, 1);
    var sum = vec4<f32>(0.0);
    for (var y: i32 = 0; y < fetches; y = y + 1) {
        for (var x: i32 = 0; x < fetches; x = x + 1) {
            let corner = vec2<f32>(f32(2 * x + 1 - fetches), f32(2 * y + 1 - fetches));
            sum = sum + tiled_sample(local + corner * texel);
        }
    }
    return sum / f32(fetches * fetches);
}

// One axis of the separable kernel over a source whose texels are the
// destination's pixels, or coarser: a step is one source texel, so a pass
// reading the downscaled scratch back up to full size steps by the scratch
// texel, and the radius counts those texels.
@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let local = region_local(input);
    let source_size = max(source_region().zw, vec2<f32>(1.0, 1.0));
    let step = blur.direction_and_radius.xy / source_size;
    let dir = blur.direction_and_radius.xy;
    let radius = max(dot(dir, blur.direction_and_radius.zw), 0.0);
    let sigma = max(radius * 0.5, 0.001);

    let tap_count = min(i32(ceil(radius)), 32);

    var color = tiled_sample(local) * tap_weight(local, 1.0);
    var total_weight = 1.0;
    if (tap_count <= 0) {
        return color;
    }

    let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);

    // The taps at i and i + 1 on one side become one bilinear fetch between
    // them, placed where the filter hands each its Gaussian weight, so the
    // kernel keeps every weight and costs half the fetches. A tap the decal
    // mode drops leaves the fetch on its partner alone and keeps its weight
    // in the total, as the transparent texel it reads would: the kernel
    // fades out past the region instead of renormalising to what is left.
    // The trip count comes off the uniform buffer, so control flow stays
    // uniform and the loop shrinks with the radius. Sampling is explicit-LOD
    // (the sources are mipless offscreens), which frees the taps from
    // derivative uniformity.
    for (var i: i32 = 1; i <= tap_count; i = i + 2) {
        let fi = f32(i);
        let fj = fi + 1.0;
        let w1 = exp(-(fi * fi) * inv_2sigma2);
        let w2 = select(0.0, exp(-(fj * fj) * inv_2sigma2), i + 1 <= tap_count);
        total_weight = total_weight + 2.0 * (w1 + w2);
        for (var side: f32 = -1.0; side <= 1.0; side = side + 2.0) {
            let e1 = tap_weight(local + step * (fi * side), w1);
            let e2 = tap_weight(local + step * (fj * side), w2);
            let e = e1 + e2;
            if (e > 0.0) {
                let offset = (fi * e1 + fj * e2) / e;
                color = color + tiled_sample(local + step * (offset * side)) * e;
            }
        }
    }

    return color / max(total_weight, 0.00001);
}
