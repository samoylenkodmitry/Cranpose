
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
//
// `stroke_params.y` packs three 2-bit fields so the struct stays at seven
// vec4-sized slots (112 bytes) instead of eight:
//
//   bits 0-1  shape kind : 0 = fill, 1 = stroked rect/round-rect, 2 = arc band
//   bits 2-3  stroke cap : 0 = butt, 1 = round, 2 = square   (arcs only)
//   bits 4-5  stroke join: 0 = miter, 1 = round, 2 = bevel   (rects only)
//
// Angle convention for arcs: radians, 0 = +X, increasing CLOCKWISE on screen
// (y-down device space) — the same convention the sweep-gradient branch below
// gets from atan2(dy, dx).
struct ShapeData {
    rect: vec4<f32>,            // x, y, width, height
    radii: vec4<f32>,           // top_left, top_right, bottom_left, bottom_right
    gradient_params: vec4<f32>, // linear: start.xy,end.xy; radial: center.xy,radius,unused
    clip_rect: vec4<f32>,       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    stroke_params: vec4<f32>,   // stroke width, packed flags, arc outer radius, arc inner radius
    arc_params: vec4<f32>,      // arc center.xy, start_angle, sweep_angle
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
// ShapeData is 112 bytes now (clip_rect + stroke/arc params), so 146 shapes =
// 16352 bytes, the most that fits the 16KB floor. Native pipelines rewrite
// both array lengths from the real device limits — see `shape_shader_source`,
// which string-replaces these exact literals.
@group(1) @binding(0)
var<uniform> shape_data: array<ShapeData, 146>;

@group(1) @binding(1)
var<uniform> gradient_stops: array<GradientStop, 256>;

const SHAPE_KIND_FILL: u32 = 0u;
const SHAPE_KIND_STROKE: u32 = 1u;
const SHAPE_KIND_ARC: u32 = 2u;

const STROKE_CAP_BUTT: u32 = 0u;
const STROKE_CAP_SQUARE: u32 = 2u;

const STROKE_JOIN_MITER: u32 = 0u;
const STROKE_JOIN_ROUND: u32 = 1u;
const STROKE_JOIN_BEVEL: u32 = 2u;

const TAU: f32 = 6.28318530717959;
const INV_SQRT2: f32 = 0.70710678118655;

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

// Signed distance to the outline of a rounded rect, stroked with a centered
// stroke of width `stroke_params.x`.
//
// `half_size` is the *inflated* quad the renderer emitted (geometry plus half
// the stroke width on every side), so the geometric box is recovered by
// shrinking it back. Modelling the stroke as "inside the outer offset, outside
// the inner offset" — rather than abs(sdf) - hw — is what makes the join style
// expressible: abs(sdf) - hw always produces a ROUND outer corner.
fn sdf_stroked_rounded_rect(
    p: vec2<f32>,
    half_size: vec2<f32>,
    radii: vec4<f32>,
    half_width: f32,
    join: u32,
) -> f32 {
    let hw = max(half_width, 0.0);
    let hw2 = vec2<f32>(hw, hw);
    let geom = max(half_size - hw2, vec2<f32>(0.0, 0.0));

    // Round join: the true parallel offset of every corner, sharp ones
    // included, is an arc of radius `hw`.
    var outer_radii = radii + vec4<f32>(hw, hw, hw, hw);
    if (join != STROKE_JOIN_ROUND) {
        // Miter/bevel: a corner that is already rounded (radius > 0) has no
        // join at all and keeps the true offset; a square corner keeps a zero
        // radius so it stays square.
        outer_radii = outer_radii * step(vec4<f32>(0.0001, 0.0001, 0.0001, 0.0001), radii);
    }
    let inner_radii = max(radii - vec4<f32>(hw, hw, hw, hw), vec4<f32>(0.0, 0.0, 0.0, 0.0));

    let outer = sdf_rounded_rect(p, geom + hw2, outer_radii);
    let inner = sdf_rounded_rect(p, max(geom - hw2, vec2<f32>(0.0, 0.0)), inner_radii);
    var dist = max(outer, -inner);

    if (join == STROKE_JOIN_BEVEL) {
        // The bevel joins the ends of the two offset edges, which sit at
        // (geom.x + hw, geom.y) and (geom.x, geom.y + hw): the line
        // |x| + |y| = geom.x + geom.y + hw. Exact for square corners.
        //
        // For a corner whose radius is small but non-zero (radius < hw/sqrt(2))
        // this chamfer also shaves the rounded corner slightly — an
        // approximation, and a visually irrelevant one at that scale.
        let chamfer = (abs(p.x) + abs(p.y) - (geom.x + geom.y + hw)) * INV_SQRT2;
        dist = max(dist, chamfer);
    }
    return dist;
}

// Signed distance to a circular band (`inner`..`outer` radius) limited to an
// angular sweep — the shared shape behind stroked arcs and filled annular
// sectors.
//
// Built on the analytic arc SDF (Inigo Quilez's sdArc), which natively yields
// ROUND ends; butt and square ends come from clipping against the two radial
// half-planes.
fn sdf_arc_band(
    p: vec2<f32>,
    center: vec2<f32>,
    inner: f32,
    outer: f32,
    start_angle: f32,
    sweep_angle: f32,
    cap: u32,
) -> f32 {
    let ra = (outer + inner) * 0.5;
    let rb = max((outer - inner) * 0.5, 0.0);
    let sweep = clamp(sweep_angle, 0.0, TAU);
    let half_sweep = sweep * 0.5;
    let mid = start_angle + half_sweep;

    // Rotate into the frame the arc SDF expects: the band straddles +Y and is
    // symmetric about it.
    let sm = sin(mid);
    let cm = cos(mid);
    let d = p - center;
    var q = vec2<f32>(-sm * d.x + cm * d.y, cm * d.x + sm * d.y);
    q.x = abs(q.x);

    // sin() of a half sweep in [0, PI] is non-negative in exact math; the max()
    // pins a full turn (half_sweep == PI) to exactly (0, -1) instead of the
    // tiny negative float sin(PI) actually returns, which would otherwise open
    // a hairline seam across a closed ring.
    let sc = vec2<f32>(max(sin(half_sweep), 0.0), cos(half_sweep));

    var dist: f32;
    if (sc.y * q.x > sc.x * q.y) {
        dist = length(q - sc * ra) - rb;
    } else {
        dist = abs(length(q) - ra) - rb;
    }

    // Signed distance to the radial boundary plane, positive outside the wedge.
    // `sc` is unit length, so this is a true distance and antialiases cleanly.
    let plane = sc.y * q.x - sc.x * q.y;
    if (cap == STROKE_CAP_BUTT) {
        dist = max(dist, plane);
    } else if (cap == STROKE_CAP_SQUARE) {
        // Project the flat end half a stroke width along the tangent.
        dist = max(dist, plane - rb);
    }
    return dist;
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

    // Packed stroke/arc flags (see the ShapeData comment). Fills leave
    // stroke_params zeroed, so kind 0 keeps the original code path byte for
    // byte — and, crucially, stroked and arc shapes stay on this same pipeline
    // and blend state, so they batch together with fills instead of splitting
    // the batch.
    let flags = u32(max(shape.stroke_params.y, 0.0));
    let shape_kind = flags & 3u;
    let stroke_cap = (flags >> 2u) & 3u;
    let stroke_join = (flags >> 4u) & 3u;

    let has_radii = (shape.radii[0] > 0.0 || shape.radii[1] > 0.0 ||
                     shape.radii[2] > 0.0 || shape.radii[3] > 0.0);
    var alpha: f32;
    if (shape_kind == SHAPE_KIND_ARC) {
        let dist = sdf_arc_band(
            rect_pos,
            shape.arc_params.xy,
            shape.stroke_params.w,
            shape.stroke_params.z,
            shape.arc_params.z,
            shape.arc_params.w,
            stroke_cap,
        );
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else if (shape_kind == SHAPE_KIND_STROKE) {
        let dist = sdf_stroked_rounded_rect(
            local_pos,
            half_size,
            shape.radii,
            shape.stroke_params.x * 0.5,
            stroke_join,
        );
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else if (has_radii) {
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
