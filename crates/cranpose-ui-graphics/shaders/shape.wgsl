
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
