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
    @location(2) rect_pos: vec2<f32>,
    @location(3) @interpolate(flat) shape_idx: u32,
}

struct Uniforms {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Vertex shader
@vertex
fn vs_main(input: VertexInput, @builtin(vertex_index) vertex_idx: u32) -> VertexOutput {
    var output: VertexOutput;

    // Convert from pixel coordinates to clip space
    let x = (input.position.x / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (input.position.y / uniforms.viewport.y) * 2.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.rect_pos = input.position;
    // Each shape has 4 vertices, so divide by 4 to get shape index
    output.shape_idx = vertex_idx / 4u;

    return output;
}

// Fragment shader structs and data
struct ShapeData {
    rect: vec4<f32>,            // x, y, width, height
    radii: vec4<f32>,           // top_left, top_right, bottom_left, bottom_right
    gradient_params: vec4<f32>, // center.x, center.y, radius, unused
    clip_rect: vec4<f32>,       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    brush_type: u32,            // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32,
    gradient_count: u32,
    _padding: u32,
}

struct GradientStop {
    color: vec4<f32>,
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

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let shape = shape_data[input.shape_idx];
    let rect_pos = input.rect_pos;
    
    // Apply clipping: if clip_rect has non-zero size, clip to it
    let clip_w = shape.clip_rect.z;
    let clip_h = shape.clip_rect.w;
    if (clip_w > 0.0 && clip_h > 0.0) {
        let clip_left = shape.clip_rect.x;
        let clip_top = shape.clip_rect.y;
        let clip_right = clip_left + clip_w;
        let clip_bottom = clip_top + clip_h;
        
        // Discard fragments outside clip rect
        if (rect_pos.x < clip_left || rect_pos.x > clip_right ||
            rect_pos.y < clip_top || rect_pos.y > clip_bottom) {
            discard;
        }
    }
    
    let rect_center = shape.rect.xy + shape.rect.zw * 0.5;
    let half_size = shape.rect.zw * 0.5;
    let local_pos = rect_pos - rect_center;

    // Compute SDF for rounded rectangle
    let dist = sdf_rounded_rect(local_pos, half_size, shape.radii);

    // Anti-aliasing
    let alpha = 1.0 - smoothstep(-0.5, 0.5, dist);

    if (alpha < 0.001) {
        discard;
    }

    var color = input.color;

    // Apply gradient if needed
    if (shape.brush_type == 1u) {
        // Linear gradient (top to bottom)
        let height = max(shape.rect.w, 0.00001);
        let t = clamp((rect_pos.y - shape.rect.y) / height, 0.0, 1.0);
        let count = shape.gradient_count;

        if (count <= 1u) {
            color = gradient_stops[shape.gradient_start].color;
        } else {
            let segments = count - 1u;
            let scaled = t * f32(segments);
            let idx = min(u32(scaled), segments);
            let next_idx = min(idx + 1u, segments);
            let local_t = fract(scaled);

            let c1 = gradient_stops[shape.gradient_start + idx].color;
            let c2 = gradient_stops[shape.gradient_start + next_idx].color;
            color = mix(c1, c2, local_t);
        }
    } else if (shape.brush_type == 2u) {
        // Radial gradient - use explicit center and radius from gradient_params
        let center = shape.gradient_params.xy;
        let radius = max(shape.gradient_params.z, 0.00001);
        let dist_from_center = length(rect_pos - center);
        let t = clamp(dist_from_center / radius, 0.0, 1.0);

        let count = shape.gradient_count;

        if (count <= 1u) {
            color = gradient_stops[shape.gradient_start].color;
        } else {
            let segments = count - 1u;
            let scaled = t * f32(segments);
            let idx = min(u32(scaled), segments);
            let next_idx = min(idx + 1u, segments);
            let local_t = fract(scaled);

            let c1 = gradient_stops[shape.gradient_start + idx].color;
            let c2 = gradient_stops[shape.gradient_start + next_idx].color;
            color = mix(c1, c2, local_t);
        }
    } else if (shape.brush_type == 3u) {
        // Sweep gradient - angle-based interpolation around center
        let center = shape.gradient_params.xy;
        let dx = rect_pos.x - center.x;
        let dy = rect_pos.y - center.y;
        let angle = atan2(dy, dx);
        // Map [-PI, PI] to [0, 1]
        let t = clamp(angle / (2.0 * 3.14159265358979) + 0.5, 0.0, 1.0);

        let count = shape.gradient_count;

        if (count <= 1u) {
            color = gradient_stops[shape.gradient_start].color;
        } else {
            let segments = count - 1u;
            let scaled = t * f32(segments);
            let idx = min(u32(scaled), segments);
            let next_idx = min(idx + 1u, segments);
            let local_t = fract(scaled);

            let c1 = gradient_stops[shape.gradient_start + idx].color;
            let c2 = gradient_stops[shape.gradient_start + next_idx].color;
            color = mix(c1, c2, local_t);
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
    _padding: vec2<f32>,
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
    let x = (input.position.x / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (input.position.y / uniforms.viewport.y) * 2.0;
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

#[allow(dead_code)] // Available for external use by custom shaders
/// Fullscreen quad vertex shader shared by all post-process effects.
///
/// Generates a full-screen triangle pair from vertex ID (no vertex buffer needed).
/// Output UV covers [0,1]x[0,1].
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

/// Two-pass separable Gaussian blur post-process shader.
///
/// Uniforms (via push-style uniform buffer):
/// - direction: vec2<f32> — (1,0) for horizontal, (0,1) for vertical
/// - radius: vec2<f32> — blur radius in pixels (x,y)
/// - texture_size: vec2<f32> — input texture dimensions in pixels
/// - tile_mode: f32 — 0.0 = Clamp, 1.0 = Decal
pub const BLUR_SHADER: &str = r#"
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

struct BlurUniforms {
    direction: vec2<f32>,   // (1,0) horizontal, (0,1) vertical
    radius: vec2<f32>,      // blur radius in pixels
    texture_size: vec2<f32>,
    tile_mode: f32,         // 0 = Clamp, 1 = Decal
    _padding: f32,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> blur: BlurUniforms;

@fragment
fn blur_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel_size = 1.0 / blur.texture_size;
    let dir = blur.direction;
    // Use the radius component matching the direction
    let r = dot(dir, blur.radius);
    let sigma = max(r / 3.0, 0.001);

    // Number of taps on each side (capped at 32 for performance)
    let tap_count = min(i32(ceil(r)), 32);

    if (tap_count <= 0) {
        return textureSample(input_texture, input_sampler, input.uv);
    }

    var color = vec4<f32>(0.0);
    var total_weight = 0.0;

    for (var i = -tap_count; i <= tap_count; i = i + 1) {
        let offset = f32(i);
        let weight = exp(-(offset * offset) / (2.0 * sigma * sigma));
        let sample_uv = input.uv + dir * offset * pixel_size;
        var sample = vec4<f32>(0.0);
        if (blur.tile_mode >= 0.5) {
            // Decal: out-of-bounds samples are transparent.
            if (sample_uv.x >= 0.0 && sample_uv.x <= 1.0 && sample_uv.y >= 0.0 && sample_uv.y <= 1.0) {
                sample = textureSample(input_texture, input_sampler, sample_uv);
            }
        } else {
            // Clamp: sample nearest edge texel outside bounds.
            let clamped_uv = clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0));
            sample = textureSample(input_texture, input_sampler, clamped_uv);
        }
        color = color + sample * weight;
        total_weight = total_weight + weight;
    }

    return color / total_weight;
}
"#;

/// Offset post-process shader.
///
/// Translates the source texture by the provided pixel offset. Pixels shifted
/// outside the source texture become transparent.
pub const OFFSET_SHADER: &str = r#"
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

    if (shifted_uv.x < 0.0 || shifted_uv.x > 1.0 || shifted_uv.y < 0.0 || shifted_uv.y > 1.0) {
        return vec4<f32>(0.0);
    }

    return textureSample(input_texture, input_sampler, shifted_uv);
}
"#;

/// Simple fullscreen blit shader for compositing offscreen targets to the surface.
///
/// Renders the entire offscreen texture as a fullscreen quad with premultiplied alpha blending.
/// Transparent regions contribute nothing, so only the effect-processed content
/// is composited onto the existing surface.
pub const BLIT_SHADER: &str = r#"
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

@fragment
fn blit_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(input_texture, input_sampler, input.uv);
}
"#;

#[allow(dead_code)] // Available for external use by custom shaders
/// Default vertex shader preamble for RuntimeShader effects.
///
/// RuntimeShader WGSL modules must include their own fullscreen vertex shader.
/// This constant provides the standard one they can copy or the framework
/// can prepend automatically.
pub const EFFECT_VS_PREAMBLE: &str = r#"
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
"#;
