//! WGSL shaders for 2D rendering with GPU acceleration.

pub const SHADER: &str = r#"
// Shared structs
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_pos: vec2<f32>,
    @location(3) @interpolate(flat) shape_idx: u32,
}

struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Vertex shader
@vertex
fn vs_main(input: VertexInput, @builtin(vertex_index) vertex_idx: u32) -> VertexOutput {
    var output: VertexOutput;

    // Convert from pixel coordinates to clip space (viewport_offset shifts the origin
    // so that a sub-region of the viewport maps to the full NDC range)
    let x = ((input.position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((input.position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.world_pos = input.position;
    // Each shape has 4 vertices, so divide by 4 to get shape index
    output.shape_idx = vertex_idx / 4u;

    return output;
}

// Fragment shader structs and data
struct ShapeData {
    rect: vec4<f32>,            // x, y, width, height
    radii: vec4<f32>,           // top_left, top_right, bottom_left, bottom_right
    gradient_params: vec4<f32>, // linear: start.xy,end.xy; radial: center.xy,radius,unused
    clip_rect: vec4<f32>,       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    brush_type: u32,            // 0=solid, 1=linear_gradient, 2=radial_gradient, 3=sweep
    gradient_start: u32,
    gradient_count: u32,
    gradient_tile_mode: u32,    // 0=Clamp, 1=Repeated, 2=Mirror, 3=Decal
}

struct GradientStop {
    color: vec4<f32>,
    position: vec4<f32>,
}

// Use uniform buffers for WebGL compatibility
// Note: WebGL has a minimum uniform buffer size of 16KB
// ShapeData is 80 bytes now (with clip_rect), so ~200 shapes = 16KB
@group(1) @binding(0)
var<uniform> shape_data: array<ShapeData, 200>;

@group(1) @binding(1)
var<uniform> gradient_stops: array<GradientStop, 256>;

fn sdf_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    var radius = r.x;
    if (p.x > 0.0) {
        radius = r.y;
    }
    if (p.y > 0.0) {
        if (p.x > 0.0) {
            radius = r.w;
        } else {
            radius = r.z;
        }
    }
    let q = abs(p) - b + radius;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - radius;
}

struct GradientSample {
    t: f32,
    valid: bool,
}

fn remap_gradient_t(raw_t: f32, tile_mode: u32) -> GradientSample {
    if (tile_mode == 3u) {
        if (raw_t < 0.0 || raw_t > 1.0) {
            return GradientSample(0.0, false);
        }
        return GradientSample(raw_t, true);
    }
    if (tile_mode == 1u) {
        let wrapped = raw_t - floor(raw_t);
        return GradientSample(wrapped, true);
    }
    if (tile_mode == 2u) {
        let wrapped = raw_t - floor(raw_t / 2.0) * 2.0;
        if (wrapped <= 1.0) {
            return GradientSample(wrapped, true);
        }
        return GradientSample(2.0 - wrapped, true);
    }
    return GradientSample(clamp(raw_t, 0.0, 1.0), true);
}

fn sample_gradient(shape: ShapeData, t: f32) -> vec4<f32> {
    let count = shape.gradient_count;
    if (count == 0u) {
        return vec4<f32>(0.0);
    }
    if (count == 1u) {
        return gradient_stops[shape.gradient_start].color;
    }

    let clamped = clamp(t, 0.0, 1.0);
    let first = gradient_stops[shape.gradient_start];
    if (clamped <= first.position.x) {
        return first.color;
    }

    var i: u32 = 0u;
    loop {
        if (i + 1u >= count) {
            break;
        }
        let current = gradient_stops[shape.gradient_start + i];
        let next = gradient_stops[shape.gradient_start + i + 1u];
        if (clamped <= next.position.x) {
            let denom = max(next.position.x - current.position.x, 0.00001);
            let local_t = clamp((clamped - current.position.x) / denom, 0.0, 1.0);
            return mix(current.color, next.color, local_t);
        }
        i = i + 1u;
    }

    return gradient_stops[shape.gradient_start + count - 1u].color;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let shape = shape_data[input.shape_idx];
    let world_pos = input.world_pos;
    // Local layer-space pixel coordinate derived from uv, independent of
    // world-space quad deformation (rotation/perspective).
    let rect_pos = shape.rect.xy + input.uv * shape.rect.zw;
    
    // Apply clipping: if clip_rect has non-zero size, clip to it
    let clip_w = shape.clip_rect.z;
    let clip_h = shape.clip_rect.w;
    if (clip_w > 0.0 && clip_h > 0.0) {
        let clip_left = shape.clip_rect.x;
        let clip_top = shape.clip_rect.y;
        let clip_right = clip_left + clip_w;
        let clip_bottom = clip_top + clip_h;
        
        // Discard fragments outside clip rect
        if (world_pos.x < clip_left || world_pos.x > clip_right ||
            world_pos.y < clip_top || world_pos.y > clip_bottom) {
            discard;
        }
    }
    
    let rect_center = shape.rect.xy + shape.rect.zw * 0.5;
    let half_size = shape.rect.zw * 0.5;
    let local_pos = rect_pos - rect_center;

    let has_radii = (shape.radii[0] > 0.0 || shape.radii[1] > 0.0 ||
                     shape.radii[2] > 0.0 || shape.radii[3] > 0.0);
    var alpha: f32;
    if (has_radii) {
        // Rounded rect: SDF + smoothstep for curved edges
        let dist = sdf_rounded_rect(local_pos, half_size, shape.radii);
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else {
        // Non-rounded rect: analytical box coverage.
        // Computes the exact fraction of each pixel covered by the rect,
        // producing constant visual weight (sum of alpha) regardless of
        // sub-pixel position. This prevents thin shapes (underlines, borders)
        // from changing apparent thickness during scroll.
        let cov_x = clamp(half_size.x + 0.5 - abs(local_pos.x), 0.0, 1.0);
        let cov_y = clamp(half_size.y + 0.5 - abs(local_pos.y), 0.0, 1.0);
        alpha = cov_x * cov_y;
    }

    if (alpha < 0.001) {
        discard;
    }

    var color = input.color;

    // Apply gradient if needed
    if (shape.brush_type == 1u) {
        // Linear gradient projected from start.xy to end.xy
        let start = shape.gradient_params.xy;
        let end = shape.gradient_params.zw;
        let dir = end - start;
        let denom = max(dot(dir, dir), 0.00001);
        let raw_t = dot(rect_pos - start, dir) / denom;
        let sample = remap_gradient_t(raw_t, shape.gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(shape, sample.t);
        }
    } else if (shape.brush_type == 2u) {
        // Radial gradient - use explicit center and radius from gradient_params
        let center = shape.gradient_params.xy;
        let radius = max(shape.gradient_params.z, 0.00001);
        let dist_from_center = length(rect_pos - center);
        let raw_t = dist_from_center / radius;
        let sample = remap_gradient_t(raw_t, shape.gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(shape, sample.t);
        }
    } else if (shape.brush_type == 3u) {
        // Sweep gradient - angle-based interpolation around center
        let center = shape.gradient_params.xy;
        let dx = rect_pos.x - center.x;
        let dy = rect_pos.y - center.y;
        let angle = atan2(dy, dx);
        // Map [-PI, PI] to [0, 1]
        let raw_t = angle / (2.0 * 3.14159265358979) + 0.5;
        let sample = remap_gradient_t(raw_t, shape.gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(shape, sample.t);
        }
    }

    return vec4<f32>(color.rgb, color.a * alpha);
}
"#;

pub const IMAGE_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var image_texture: texture_2d<f32>;

@group(1) @binding(1)
var image_sampler: sampler;

@vertex
fn image_vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = ((input.position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((input.position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    return output;
}

@fragment
fn image_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(image_texture, image_sampler, input.uv);
    return sampled * input.color;
}
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Shared WGSL snippets for post-process effects
// ═══════════════════════════════════════════════════════════════════════════

/// Fullscreen quad vertex shader preamble shared by all post-process effects.
///
/// Declares `VertexOutput` and `fullscreen_vs` — a vertex shader that generates
/// a full-screen triangle pair from vertex ID (no vertex buffer needed).
/// Output UV covers [0,1]×[0,1].
pub const FULLSCREEN_QUAD_VS: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Generate fullscreen triangle from vertex index (0,1,2 → covers clip space)
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    // Map clip [-1,1] to UV [0,1] with Y flipped for texture coordinates
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}
"#;

/// SDF rounded-rectangle function shared by the main shape shader and blit shader.
pub const SDF_ROUNDED_RECT_FN: &str = r#"
fn sdf_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    var radius = r.x;
    if (p.x > 0.0) {
        radius = r.y;
    }
    if (p.y > 0.0) {
        if (p.x > 0.0) {
            radius = r.w;
        } else {
            radius = r.z;
        }
    }
    let q = abs(p) - b + radius;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - radius;
}
"#;

pub const COMPOSITE_SAMPLE_FN: &str = r#"
fn composite_sample_box4(
    source_pos: vec2<f32>,
    source_size: vec2<f32>,
    span_hint: vec2<f32>,
) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(input_texture));
    let inferred_footprint = vec2<f32>(
        max(abs(dpdx(source_pos.x)), abs(dpdy(source_pos.x))),
        max(abs(dpdx(source_pos.y)), abs(dpdy(source_pos.y))),
    );
    let footprint = vec2<f32>(
        select(inferred_footprint.x, span_hint.x, span_hint.x > 0.0),
        select(inferred_footprint.y, span_hint.y, span_hint.y > 0.0),
    );
    let span = max(footprint, vec2<f32>(1.0, 1.0));
    let left = source_pos - span * 0.5;
    let right = source_pos + span * 0.5;
    let start_x = i32(floor(left.x));
    let start_y = i32(floor(left.y));
    var accum = vec4<f32>(0.0);
    var total_weight = 0.0;

    for (var offset_y: i32 = 0; offset_y < 6; offset_y = offset_y + 1) {
        let texel_y = start_y + offset_y;
        let texel_top = f32(texel_y);
        let texel_bottom = texel_top + 1.0;
        let weight_y = max(0.0, min(right.y, texel_bottom) - max(left.y, texel_top));
        if (weight_y <= 0.0) {
            continue;
        }

        for (var offset_x: i32 = 0; offset_x < 6; offset_x = offset_x + 1) {
            let texel_x = start_x + offset_x;
            let texel_left = f32(texel_x);
            let texel_right = texel_left + 1.0;
            let weight_x = max(0.0, min(right.x, texel_right) - max(left.x, texel_left));
            let weight = weight_x * weight_y;
            if (weight <= 0.0) {
                continue;
            }

            total_weight = total_weight + weight;
            if (texel_x < 0 || texel_x >= dims.x || texel_y < 0 || texel_y >= dims.y) {
                continue;
            }
            accum = accum + textureLoad(input_texture, vec2<i32>(texel_x, texel_y), 0) * weight;
        }
    }

    return accum / max(total_weight, 0.00001);
}

fn composite_sample(
    source_pos: vec2<f32>,
    source_size: vec2<f32>,
    sampling_mode: f32,
    span_hint: vec2<f32>,
) -> vec4<f32> {
    let safe_source_size = max(source_size, vec2<f32>(0.00001, 0.00001));
    let uv = source_pos / safe_source_size;
    if (sampling_mode <= 0.5) {
        return textureSample(input_texture, input_sampler, uv);
    }
    return composite_sample_box4(source_pos, safe_source_size, span_hint);
}
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Composed post-process shaders
// ═══════════════════════════════════════════════════════════════════════════

/// Two-pass separable Gaussian blur post-process shader.
///
/// Uniforms (via push-style uniform buffer):
/// - direction: vec2<f32> — (1,0) for horizontal, (0,1) for vertical
/// - radius: vec2<f32> — blur radius in pixels (x,y)
/// - texture_size: vec2<f32> — input texture dimensions in pixels
/// - tile_mode: f32 — 0.0 = Clamp, 1.0 = Repeated, 2.0 = Mirror, 3.0 = Decal
pub fn blur_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        r#"
struct BlurUniforms {
    direction_and_radius: vec4<f32>,      // direction.xy, radius.xy
    texture_size_and_tile_mode: vec4<f32>,// texture_size.xy, tile_mode, unused
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

fn inside_unit_bounds(uv: vec2<f32>) -> f32 {
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    return select(0.0, 1.0, inside);
}

fn sample_with_tile_mode(uv: vec2<f32>) -> vec4<f32> {
    let tile_mode = blur.texture_size_and_tile_mode.z;
    if (tile_mode >= 2.5) {
        // Decal: out-of-bounds samples are transparent.
        let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        return textureSample(input_texture, input_sampler, clamped_uv) * inside_unit_bounds(uv);
    }

    if (tile_mode >= 1.5) {
        // Mirror: ... 0->1, 1->0, repeat.
        let wrap_x = uv.x - floor(uv.x / 2.0) * 2.0;
        let wrap_y = uv.y - floor(uv.y / 2.0) * 2.0;
        let mirrored_uv = vec2<f32>(
            select(wrap_x, 2.0 - wrap_x, wrap_x > 1.0),
            select(wrap_y, 2.0 - wrap_y, wrap_y > 1.0),
        );
        return textureSample(input_texture, input_sampler, mirrored_uv);
    }

    if (tile_mode >= 0.5) {
        // Repeated: wrap to [0,1).
        let repeated_uv = vec2<f32>(uv.x - floor(uv.x), uv.y - floor(uv.y));
        return textureSample(input_texture, input_sampler, repeated_uv);
    }

    // Clamp: sample nearest edge texel outside bounds.
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(input_texture, input_sampler, clamped_uv);
}

@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let texture_size = max(blur.texture_size_and_tile_mode.xy, vec2<f32>(1.0, 1.0));
    let pixel_size = 1.0 / texture_size;
    let dir = blur.direction_and_radius.xy;
    // Use the radius component matching the direction.
    let radius = max(dot(dir, blur.direction_and_radius.zw), 0.0);
    let sigma = max(radius * 0.5, 0.001);

    // Number of taps on each side (capped for shader cost stability).
    let tap_count = min(i32(ceil(radius)), 32);

    if (tap_count <= 0) {
        return sample_with_tile_mode(input.uv);
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
        color = color + sample_with_tile_mode(input.uv + offset) * weight;
        total_weight = total_weight + weight;
    }

    return color / max(total_weight, 0.00001);
}
"#
    )
}

/// Offset post-process shader.
///
/// Translates the source texture by the provided pixel offset. Pixels shifted
/// outside the source texture become transparent.
pub fn offset_shader() -> String {
    format!(
        "{FULLSCREEN_QUAD_VS}{}",
        r#"
struct OffsetUniforms {
    offset: vec2<f32>, // in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> params: OffsetUniforms;

@fragment
fn offset_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let shifted_uv = input.uv - params.offset / max(tex_size, vec2<f32>(1.0));
    let inside =
        shifted_uv.x >= 0.0 && shifted_uv.x <= 1.0 && shifted_uv.y >= 0.0 && shifted_uv.y <= 1.0;
    let clamped_uv = clamp(shifted_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSample(input_texture, input_sampler, clamped_uv)
        * select(0.0, 1.0, inside);
}
"#
    )
}

/// Simple fullscreen blit shader for compositing offscreen targets to the surface.
///
/// Renders the entire offscreen texture as a fullscreen quad with premultiplied alpha blending.
/// Transparent regions contribute nothing, so only the effect-processed content
/// is composited onto the existing surface.
pub fn blit_shader() -> String {
    let mut shader = format!(
        "{FULLSCREEN_QUAD_VS}{SDF_ROUNDED_RECT_FN}{}",
        r#"
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
struct BlitUniforms {
    alpha: vec4<f32>,
    mask_rect: vec4<f32>,    // x, y, width, height in destination pixels
    mask_radii: vec4<f32>,   // top_left, top_right, bottom_left, bottom_right
    mask_enabled: vec4<f32>, // x > 0 => apply rounded mask
    sampling: vec4<f32>,     // x = 0 => linear, x = 1 => 4x box resolve
    dest_viewport: vec4<f32>, // x, y, width, height in destination pixels
    resolve_span: vec4<f32>, // x, y = exact source pixels covered by one destination pixel
}
@group(1) @binding(0) var<uniform> blit: BlitUniforms;

"#
    );
    shader.push_str(COMPOSITE_SAMPLE_FN);
    shader.push_str(
        r#"

@fragment
fn blit_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let use_dest_viewport = blit.dest_viewport.z > 0.0 && blit.dest_viewport.w > 0.0;
    let dest_pos = input.position.xy;
    var source_pos = input.uv * tex_size;
    var resolve_span = blit.resolve_span.xy;
    if use_dest_viewport {
        let viewport_max = blit.dest_viewport.xy + blit.dest_viewport.zw;
        if dest_pos.x < blit.dest_viewport.x || dest_pos.y < blit.dest_viewport.y ||
            dest_pos.x >= viewport_max.x || dest_pos.y >= viewport_max.y {
            discard;
        }
        let local_dest = dest_pos - blit.dest_viewport.xy;
        source_pos = vec2<f32>(
            local_dest.x * tex_size.x / blit.dest_viewport.z,
            local_dest.y * tex_size.y / blit.dest_viewport.w,
        );
        resolve_span = vec2<f32>(
            tex_size.x / blit.dest_viewport.z,
            tex_size.y / blit.dest_viewport.w,
        );
    }
    let sampled =
        composite_sample(source_pos, tex_size, blit.sampling.x, resolve_span) * blit.alpha.x;
    if (blit.mask_enabled.x <= 0.5) {
        return sampled;
    }

    let world_pos = dest_pos;
    let center = blit.mask_rect.xy + blit.mask_rect.zw * 0.5;
    let half_size = blit.mask_rect.zw * 0.5;
    let local_pos = world_pos - center;
    let has_radii = (blit.mask_radii[0] > 0.0 || blit.mask_radii[1] > 0.0 ||
                     blit.mask_radii[2] > 0.0 || blit.mask_radii[3] > 0.0);
    var coverage: f32;
    if (has_radii) {
        let dist = sdf_rounded_rect(local_pos, half_size, blit.mask_radii);
        coverage = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else {
        let cov_x = clamp(half_size.x + 0.5 - abs(local_pos.x), 0.0, 1.0);
        let cov_y = clamp(half_size.y + 0.5 - abs(local_pos.y), 0.0, 1.0);
        coverage = cov_x * cov_y;
    }

    if (coverage <= 0.001) {
        discard;
    }
    return sampled * coverage;
}
"#,
    );
    shader
}

pub fn projective_blit_shader() -> String {
    let mut shader = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
}

struct ProjectiveBlitUniforms {
    viewport: vec2<f32>,
    source_size: vec2<f32>,
    inverse_row0: vec4<f32>,
    inverse_row1: vec4<f32>,
    inverse_row2: vec4<f32>,
    alpha: vec4<f32>,
    sampling: vec4<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blit: ProjectiveBlitUniforms;
"#
    .to_string();
    shader.push_str(COMPOSITE_SAMPLE_FN);
    shader.push_str(
        r#"

@vertex
fn projective_blit_vs(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = (input.position.x / blit.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (input.position.y / blit.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.world_pos = input.position;
    return output;
}

@fragment
fn projective_blit_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let p = vec3<f32>(input.world_pos, 1.0);
    let denom = dot(blit.inverse_row2.xyz, p);
    if (abs(denom) <= 0.00001) {
        discard;
    }

    let source_x = dot(blit.inverse_row0.xyz, p) / denom;
    let source_y = dot(blit.inverse_row1.xyz, p) / denom;
    if (source_x < 0.0 || source_y < 0.0 || source_x > blit.source_size.x || source_y > blit.source_size.y) {
        discard;
    }

    let source_pos = vec2<f32>(source_x, source_y);
    return composite_sample(source_pos, blit.source_size, blit.sampling.x, vec2<f32>(0.0, 0.0))
        * blit.alpha.x;
}
"#
    );
    shader
}

#[cfg(test)]
mod tests {
    use naga::back::glsl;
    use naga::ShaderStage;

    fn validate_wgsl_module(source: &str) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(source)
            .map_err(|err| format!("WGSL parse error: {err}"))?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .map_err(|err| format!("WGSL validation error: {err}"))?;
        Ok(())
    }

    fn validate_glsl_portability(
        source: &str,
        entry_point: &str,
        shader_stage: ShaderStage,
    ) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(source)
            .map_err(|err| format!("WGSL parse error: {err}"))?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        let module_info = validator
            .validate(&module)
            .map_err(|err| format!("WGSL validation error: {err}"))?;
        let mut glsl_source = String::new();
        let options = glsl::Options {
            version: glsl::Version::new_gles(300),
            writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
            ..Default::default()
        };
        let pipeline_options = glsl::PipelineOptions {
            shader_stage,
            entry_point: entry_point.to_string(),
            multiview: None,
        };
        let mut writer = glsl::Writer::new(
            &mut glsl_source,
            &module,
            &module_info,
            &options,
            &pipeline_options,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .map_err(|err| format!("GL/WebGL portability validation failed: {err}"))?;
        writer
            .write()
            .map(|_| ())
            .map_err(|err| format!("GL/WebGL portability emission failed: {err}"))
    }

    #[test]
    fn blur_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::blur_shader()).is_ok());
    }

    #[test]
    fn blur_shader_validates_for_webgl() {
        let shader = super::blur_shader();
        assert!(validate_glsl_portability(&shader, "fullscreen_vs", ShaderStage::Vertex).is_ok());
        assert!(validate_glsl_portability(&shader, "blur_fs", ShaderStage::Fragment).is_ok());
    }

    #[test]
    fn offset_shader_validates_for_webgpu() {
        assert!(validate_wgsl_module(&super::offset_shader()).is_ok());
    }
}
