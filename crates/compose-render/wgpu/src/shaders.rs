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
    }

    return vec4<f32>(color.rgb, color.a * alpha);
}
"#;
