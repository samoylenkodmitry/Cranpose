
struct BlurUniforms {
    direction_and_radius: vec4<f32>,      // direction.xy, radius.xy in source texels
    texture_size_and_tile_mode: vec4<f32>,// sampled texture size.xy, tile_mode, unused
    source_region: vec4<f32>,             // x, y, width, height in source texels; zero = whole
    dest_region: vec4<f32>,               // x, y, width, height in destination pixels; zero = whole
    pairs: array<vec4<f32>, 16>,
    kernel: vec4<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

// The source texels one destination pixel of the downsample stands for on
// each axis. A pipeline constant, so the block's fetch loops unroll.
override BLUR_BLOCK: i32 = 2;

override BLUR_TILE_MODE: u32 = 0u;

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
    let tile_mode = f32(BLUR_TILE_MODE);
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
    return select(weight, weight * inside_unit_bounds(uv), BLUR_TILE_MODE == 3u);
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

// Where a fragment's taps land: its region-local coordinate and one source
// texel along each axis there, and, for the modes that hold taps to the
// region's edge, the same fragment in texture coordinates with the texel
// centres at the region's edges. Holding a tap commutes with the region's
// affine map onto the texture, so those modes offset and clamp a texture
// coordinate per tap instead of mapping each tap onto the texture.
struct KernelFrame {
    local: vec2<f32>,
    step: vec2<f32>,
    center: vec2<f32>,
    texel: vec2<f32>,
    low: vec2<f32>,
    high: vec2<f32>,
}

fn kernel_frame(input: VertexOutput) -> KernelFrame {
    let local = region_local(input);
    let region = source_region();
    let texture_size = max(blur.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let source_size = max(region.zw, vec2<f32>(1.0, 1.0));
    let half_texel = 0.5 / source_size;
    return KernelFrame(
        local,
        1.0 / source_size,
        (region.xy + local * region.zw) / texture_size,
        region.zw / (source_size * texture_size),
        (region.xy + half_texel * region.zw) / texture_size,
        (region.xy + (vec2<f32>(1.0, 1.0) - half_texel) * region.zw) / texture_size,
    );
}

// The texture `offset` source texels from the fragment, under the tile mode.
fn kernel_tap(frame: KernelFrame, offset: vec2<f32>) -> vec4<f32> {
    if (BLUR_TILE_MODE == 0u || BLUR_TILE_MODE == 3u) {
        let uv = clamp(frame.center + frame.texel * offset, frame.low, frame.high);
        return textureSampleLevel(input_texture, input_sampler, uv, 0.0);
    }
    return tiled_sample(frame.local + frame.step * offset);
}

// The downsample: each destination pixel is the average of the block of
// source texels it stands for. The pixel's centre is the block's centre, a
// texel corner for an even block, so the fetches at every other corner
// across the block read each of its texels once through the bilinear
// filter: one fetch for a block of two, four for a block of four.
@fragment
fn blur_downsample_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let frame = kernel_frame(input);
    if (BLUR_BLOCK < 4) {
        return kernel_tap(frame, vec2<f32>(0.0, 0.0));
    }
    let sum = kernel_tap(frame, vec2<f32>(-1.0, -1.0))
        + kernel_tap(frame, vec2<f32>(1.0, -1.0))
        + kernel_tap(frame, vec2<f32>(-1.0, 1.0))
        + kernel_tap(frame, vec2<f32>(1.0, 1.0));
    return sum / 4.0;
}

@fragment
fn blur_mean_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let source = source_region();
    let horizontal = blur.direction_and_radius.x > 0.5;
    let local = region_local(input);
    let count = i32(select(source.w, source.z, horizontal));
    let row = min(i32(local.y * source.w), i32(source.w) - 1);
    var sum = vec4<f32>(0.0);
    for (var index = 0; index < count; index += 1) {
        let offset = select(vec2<i32>(0, index), vec2<i32>(index, row), horizontal);
        sum += textureLoad(input_texture, vec2<i32>(source.xy) + offset, 0);
    }
    return sum / f32(max(count, 1));
}

// One side of one pair of kernel taps along the pass's axis: the bilinear
// fetch standing for both taps, or under decal the fetch for the taps the
// region keeps, weighted by what they keep. A tap the decal mode drops
// leaves the fetch on its partner alone and keeps its weight in the total,
// as the transparent texel it reads would: the kernel fades out past the
// region instead of renormalising to what is left.
fn kernel_side(frame: KernelFrame, axis: vec2<f32>, pair: vec4<f32>, inner: f32) -> vec4<f32> {
    if (BLUR_TILE_MODE != 3u) {
        return kernel_tap(frame, axis * pair.z) * pair.w;
    }
    let outer = inner + 1.0;
    let e1 = tap_weight(frame.local + frame.step * axis * inner, pair.x);
    let e2 = tap_weight(frame.local + frame.step * axis * outer, pair.y);
    let e = e1 + e2;
    if (e <= 0.0) {
        return vec4<f32>(0.0);
    }
    return kernel_tap(frame, axis * ((inner * e1 + outer * e2) / e)) * e;
}

// Both sides of pair `inner` (the pair's inner tap, in taps from the
// fragment).
fn kernel_pair(frame: KernelFrame, axis: vec2<f32>, pair: vec4<f32>, inner: f32) -> vec4<f32> {
    return kernel_side(frame, -axis, pair, inner) + kernel_side(frame, axis, pair, inner);
}

// One axis of the separable kernel over a source whose texels are the
// destination's pixels, or coarser: a step is one source texel, so a pass
// reading the downscaled scratch back up to full size steps by the scratch
// texel, and the radius counts those texels. The taps at i and i + 1 on one
// side become one bilinear fetch between them, placed where the filter
// hands each its Gaussian weight, so the kernel keeps every weight and
// costs half the fetches. The renderer writes one guarded call per pair of
// the uniform table in place of the marker below: indexed by a constant,
// the table stays in uniform registers, where a loop over it would load
// each pair from memory, and the guards shrink the work with the radius.
@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let frame = kernel_frame(input);
    let axis = blur.direction_and_radius.xy;
    let pair_count = i32(blur.kernel.x);
    var color = kernel_tap(frame, vec2<f32>(0.0, 0.0)) * tap_weight(frame.local, 1.0);
    //@BLUR_KERNEL_PAIRS
    return color / max(blur.kernel.y, 0.00001);
}
