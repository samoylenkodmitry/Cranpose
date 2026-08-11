
// Shared structs
//
// Everything the fragment shader needs from ShapeData rides here as a flat
// varying instead of being re-fetched from the uniform array per fragment.
// The vertex shader runs six times per shape; the fragment shader runs once
// per covered pixel — thousands of times more in an overdraw-heavy scene —
// and a dynamically indexed uniform array cannot be promoted to registers,
// so every one of those fragment fetches was a real memory load on the
// GPU's load/store pipe. Flat varyings move that traffic to the (otherwise
// idle) varying interpolator. Only the gradient-stop array is still fetched
// per fragment, and solid brushes never touch it.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_pos: vec2<f32>,
    @location(3) @interpolate(flat) rect: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    @location(5) @interpolate(flat) gradient_params: vec4<f32>,
    @location(6) @interpolate(flat) clip_rect: vec4<f32>,
    @location(7) @interpolate(flat) stroke_params: vec4<f32>,
    @location(8) @interpolate(flat) arc_params: vec4<f32>,
    @location(9) @interpolate(flat) brush: vec4<u32>,
}

struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Per-batch similarity transform: rotate by the angle whose (cos, sin) is
// `rot` and scale by `scale`, both about `center`, all in device pixels.
// Replayed shape batches (cached slots re-drawn under an accumulated
// rotation/breathing transform) set a real value; every freshly converted
// batch binds a shared identity, which is bit-exact — multiplying by 1.0 and
// adding 0.0 leaves every coordinate untouched.
//
// Only the quad corners are transformed. Every SDF below evaluates in
// rect-local space reconstructed from `uv` (see `rect_pos` in `fs_main`), and
// uv interpolation across an affinely transformed quad reproduces that local
// space exactly, so radii, stroke widths, arc trig and gradient params all
// stay valid untouched. The one deliberate approximation: the smoothstep
// anti-aliasing half-width is a capture-space half pixel, so it reads as
// `scale` screen pixels — the breathing transforms this rides stay within a
// few percent of 1.
// `paint_select` = 1.0 makes the vertex stage read each shape's color from
// the retained paint buffer instead of `ShapeData.color`. Replayed batches
// set it (their recolors land in the slot's paint buffer; the captured
// ShapeData is immutable); every fresh batch leaves it 0.0. Only the
// storage-mode source rewrite declares the paint array and reads the flag —
// in this base text the field is inert padding, which keeps the uniform
// variant valid for WebGL.
struct SimilarityTransform {
    center: vec2<f32>,
    rot: vec2<f32>,
    scale: f32,
    paint_select: f32,
    _pad1: vec2<f32>,
}

@group(1) @binding(2)
var<uniform> similarity: SimilarityTransform;

// Vertex shader
//
// There is no vertex buffer: each shape is six unindexed vertices whose
// corner positions, color and UVs are pulled from `shape_data` by
// `vertex_index`. Corner numbering matches the quad the CPU records:
// 0 = top-left, 1 = top-right, 2 = bottom-left, 3 = bottom-right, drawn as
// triangles (0, 1, 2) and (2, 1, 3).
@vertex
fn vs_main(@builtin(vertex_index) vertex_idx: u32) -> VertexOutput {
    var output: VertexOutput;

    let shape_idx = vertex_idx / 6u;
    let slot = vertex_idx % 6u;
    var corner: u32;
    switch slot {
        case 0u: { corner = 0u; }
        case 1u, 4u: { corner = 1u; }
        case 2u, 3u: { corner = 2u; }
        default: { corner = 3u; }
    }

    let shape = shape_data[shape_idx];
    var position: vec2<f32>;
    switch corner {
        case 0u: { position = shape.quad01.xy; }
        case 1u: { position = shape.quad01.zw; }
        case 2u: { position = shape.quad23.xy; }
        default: { position = shape.quad23.zw; }
    }

    // Screen-clockwise rotation in y-down device space: matches the arc
    // convention above (start_angle increases clockwise), so a batch replayed
    // with rotation delta lands where freshly emitted arcs at
    // `start_angle + delta` would.
    let rel = position - similarity.center;
    position = similarity.center + vec2<f32>(
        rel.x * similarity.rot.x - rel.y * similarity.rot.y,
        rel.x * similarity.rot.y + rel.y * similarity.rot.x,
    ) * similarity.scale;

    // Convert from pixel coordinates to clip space (viewport_offset shifts the origin
    // so that a sub-region of the viewport maps to the full NDC range)
    let x = ((position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = shape.color;
    output.uv = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    output.world_pos = position;
    output.rect = shape.rect;
    output.radii = shape.radii;
    output.gradient_params = shape.gradient_params;
    output.clip_rect = shape.clip_rect;
    output.stroke_params = shape.stroke_params;
    output.arc_params = shape.arc_params;
    output.brush = vec4<u32>(
        shape.brush_type,
        shape.gradient_start,
        shape.gradient_count,
        shape.gradient_tile_mode,
    );

    return output;
}

// Instanced quad path for ordinary shape batches (storage mode only): one
// instance per shape, four vertices fetched through the static index buffer
// [0, 1, 2, 2, 1, 3] — the exact triangle pair `vs_main`'s six-slot expansion
// produces, same diagonal, same winding — so per-shape vertex work drops from
// six executions (and six ShapeData reads) to four. `shape_idx` comes from
// the instance index instead of `vertex_index / 6`.
//
// BIT-EXACTNESS REQUIREMENT: every expression below is copied verbatim from
// `vs_main` — identical corner mapping, identical uv derivation, identical
// transform arithmetic. That is what makes the instanced path rasterize
// bit-identically to the six-vertex path at the identity transform. Under a
// rotating similarity the backend compiler may contract the multiply-adds
// differently per entry point (the P1a lesson, measured on Metal), which is
// a single ulp of position and the tiny envelope `instanced_quad_parity`
// pins.
//
// Uniform/WebGL devices never create a pipeline with this entry point (GL
// base-instance semantics for `instance_index` under a non-zero
// first_instance are a portability hazard); in that variant it is dead code,
// which keeps the base text valid for WebGL.
@vertex
fn vs_shape_instanced(
    @builtin(vertex_index) corner_idx: u32,
    @builtin(instance_index) instance_idx: u32,
) -> VertexOutput {
    var output: VertexOutput;

    let shape_idx = instance_idx;
    let corner = corner_idx;

    let shape = shape_data[shape_idx];
    var position: vec2<f32>;
    switch corner {
        case 0u: { position = shape.quad01.xy; }
        case 1u: { position = shape.quad01.zw; }
        case 2u: { position = shape.quad23.xy; }
        default: { position = shape.quad23.zw; }
    }

    let rel = position - similarity.center;
    position = similarity.center + vec2<f32>(
        rel.x * similarity.rot.x - rel.y * similarity.rot.y,
        rel.x * similarity.rot.y + rel.y * similarity.rot.x,
    ) * similarity.scale;

    let x = ((position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = shape.color;
    output.uv = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    output.world_pos = position;
    output.rect = shape.rect;
    output.radii = shape.radii;
    output.gradient_params = shape.gradient_params;
    output.clip_rect = shape.clip_rect;
    output.stroke_params = shape.stroke_params;
    output.arc_params = shape.arc_params;
    output.brush = vec4<u32>(
        shape.brush_type,
        shape.gradient_start,
        shape.gradient_count,
        shape.gradient_tile_mode,
    );

    return output;
}

// Mesh vertex path for retained arc/ring slots (storage mode only): instead
// of expanding six ShapeData corners per shape, a mesh captured alongside the
// slot supplies positions that cover only the arc band's antialiasing
// footprint, with `uv` precomputed on the CPU from the same affine rect map
// the quad corners define. Uniform-mode devices never bind a pipeline with
// this entry point; in that variant it is dead code, which keeps the base
// text valid for WebGL.
struct MeshVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) shape_idx: u32,
}

// BIT-EXACTNESS REQUIREMENT: the transform below must stay expression-for-
// expression identical to `vs_main`. Passthrough mesh vertices carry the
// exact quad corner values, and identical arithmetic is what makes their
// clip positions — and so their rasterization — bit-identical to the legacy
// path at the identity transform (measured on Metal). Under a rotating
// similarity the backend compiler may still contract the multiply-adds
// differently per entry point, which is one ulp of position and part of the
// small measured envelope in `arc_mesh_parity`.
@vertex
fn vs_mesh(in: MeshVertexInput) -> VertexOutput {
    var output: VertexOutput;

    let shape_idx = in.shape_idx;
    let shape = shape_data[shape_idx];
    var position = in.position;

    let rel = position - similarity.center;
    position = similarity.center + vec2<f32>(
        rel.x * similarity.rot.x - rel.y * similarity.rot.y,
        rel.x * similarity.rot.y + rel.y * similarity.rot.x,
    ) * similarity.scale;

    let x = ((position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = shape.color;
    output.uv = in.uv;
    output.world_pos = position;
    output.rect = shape.rect;
    output.radii = shape.radii;
    output.gradient_params = shape.gradient_params;
    output.clip_rect = shape.clip_rect;
    output.stroke_params = shape.stroke_params;
    output.arc_params = shape.arc_params;
    output.brush = vec4<u32>(
        shape.brush_type,
        shape.gradient_start,
        shape.gradient_count,
        shape.gradient_tile_mode,
    );

    return output;
}

// Fragment shader structs and data
//
// `stroke_params.y` packs three 2-bit fields so the struct stays at ten
// vec4-sized slots (160 bytes) instead of eleven:
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
    radii: vec4<f32>,           // rects: top_left, top_right, bottom_left, bottom_right
                                // arcs: mid-angle (sin, cos), half-sweep (sin, cos)
    gradient_params: vec4<f32>, // linear: start.xy,end.xy; radial: center.xy,radius,unused
    clip_rect: vec4<f32>,       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    stroke_params: vec4<f32>,   // stroke width, packed flags, arc outer radius, arc inner radius
    arc_params: vec4<f32>,      // arc center.xy, start_angle, sweep_angle
    quad01: vec4<f32>,          // device-space quad corners 0 (xy) and 1 (zw)
    quad23: vec4<f32>,          // device-space quad corners 2 (xy) and 3 (zw)
    color: vec4<f32>,           // vertex color (solid brush color or first gradient stop)
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
// ShapeData is 160 bytes now (quad corners + color ride along), so 102 shapes =
// 16320 bytes, the most that fits the 16KB floor. Native pipelines rewrite
// both array lengths from the real device limits — see `shape_shader_source`,
// which string-replaces these exact literals.
@group(1) @binding(0)
var<uniform> shape_data: array<ShapeData, 102>;

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
//
// The two direction vectors are (sin, cos) of the sweep's midpoint angle and
// of the half sweep. They are constants of the shape, so the CPU computes
// them once per shape (see `convert_shape_into_slots`) instead of this
// shader paying four transcendentals on every fragment — in an arc-heavy
// scene that is by far the largest ALU term of the whole pipeline.
fn sdf_arc_band(
    p: vec2<f32>,
    center: vec2<f32>,
    inner: f32,
    outer: f32,
    mid_sin_cos: vec2<f32>,
    half_sin_cos: vec2<f32>,
    cap: u32,
) -> f32 {
    let ra = (outer + inner) * 0.5;
    let rb = max((outer - inner) * 0.5, 0.0);

    // Rotate into the frame the arc SDF expects: the band straddles +Y and is
    // symmetric about it.
    let sm = mid_sin_cos.x;
    let cm = mid_sin_cos.y;
    let d = p - center;
    var q = vec2<f32>(-sm * d.x + cm * d.y, cm * d.x + sm * d.y);
    q.x = abs(q.x);

    let sc = half_sin_cos;

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

// The ordered-dither offset Skia adds to a gradient, in output levels.
//
// Kept identical to `gradient_dither_offset` in `cranpose-render-common`,
// which carries the derivation and the tests; the CPU sampler bins by these
// same scene device coordinates, so the two backends dither a gradient the
// same way. Fragment `@builtin(position)` is the render-target pixel coordinate;
// scene coordinates move with content and would shift the matrix phase while
// scrolling at fractional density.
fn gradient_dither(device_pos: vec2<f32>) -> f32 {
    let x = u32(max(floor(device_pos.x), 0.0)) + 1u;
    let y = u32(max(floor(device_pos.y), 0.0)) + 1u;
    let m = ((y & 1u) << 3u) | ((x & 1u) << 2u) | (y & 2u) | ((x & 2u) >> 1u);
    return f32(m) * (1.0 / 16.0) - (15.0 / 32.0);
}

fn sample_gradient(gradient_start: u32, count: u32, t: f32) -> vec4<f32> {
    if (count == 0u) {
        return vec4<f32>(0.0);
    }
    if (count == 1u) {
        return gradient_stops[gradient_start].color;
    }

    let clamped = clamp(t, 0.0, 1.0);
    let first = gradient_stops[gradient_start];
    if (clamped <= first.position.x) {
        return first.color;
    }

    var i: u32 = 0u;
    loop {
        if (i + 1u >= count) {
            break;
        }
        let current = gradient_stops[gradient_start + i];
        let next = gradient_stops[gradient_start + i + 1u];
        if (clamped <= next.position.x) {
            let denom = max(next.position.x - current.position.x, 0.00001);
            let local_t = clamp((clamped - current.position.x) / denom, 0.0, 1.0);
            return mix(current.color, next.color, local_t);
        }
        i = i + 1u;
    }

    return gradient_stops[gradient_start + count - 1u].color;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let world_pos = input.world_pos;
    // Local layer-space pixel coordinate derived from uv, independent of
    // world-space quad deformation (rotation/perspective).
    let rect_pos = input.rect.xy + input.uv * input.rect.zw;

    // Apply clipping: if clip_rect has non-zero size, clip to it
    let clip_w = input.clip_rect.z;
    let clip_h = input.clip_rect.w;
    if (clip_w > 0.0 && clip_h > 0.0) {
        let clip_left = input.clip_rect.x;
        let clip_top = input.clip_rect.y;
        let clip_right = clip_left + clip_w;
        let clip_bottom = clip_top + clip_h;

        // Discard fragments outside clip rect
        if (world_pos.x < clip_left || world_pos.x > clip_right ||
            world_pos.y < clip_top || world_pos.y > clip_bottom) {
            discard;
        }
    }

    let rect_center = input.rect.xy + input.rect.zw * 0.5;
    let half_size = input.rect.zw * 0.5;
    let local_pos = rect_pos - rect_center;

    // Packed stroke/arc flags (see the ShapeData comment). Fills leave
    // stroke_params zeroed, so kind 0 keeps the original code path byte for
    // byte — and, crucially, stroked and arc shapes stay on this same pipeline
    // and blend state, so they batch together with fills instead of splitting
    // the batch.
    let flags = u32(max(input.stroke_params.y, 0.0));
    let shape_kind = flags & 3u;
    let stroke_cap = (flags >> 2u) & 3u;
    let stroke_join = (flags >> 4u) & 3u;

    let has_radii = (input.radii[0] > 0.0 || input.radii[1] > 0.0 ||
                     input.radii[2] > 0.0 || input.radii[3] > 0.0);
    var alpha: f32;
    if (shape_kind == SHAPE_KIND_ARC) {
        // Arcs have no corner radii, so `radii` carries the precomputed
        // (sin, cos) of the mid angle (xy) and of the half sweep (zw).
        let dist = sdf_arc_band(
            rect_pos,
            input.arc_params.xy,
            input.stroke_params.w,
            input.stroke_params.z,
            input.radii.xy,
            input.radii.zw,
            stroke_cap,
        );
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else if (shape_kind == SHAPE_KIND_STROKE) {
        let dist = sdf_stroked_rounded_rect(
            local_pos,
            half_size,
            input.radii,
            input.stroke_params.x * 0.5,
            stroke_join,
        );
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else if (has_radii) {
        // Rounded rect: SDF + smoothstep for curved edges
        let dist = sdf_rounded_rect(local_pos, half_size, input.radii);
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
    var is_gradient = false;

    // Apply gradient if needed
    let brush_type = input.brush.x;
    let gradient_start = input.brush.y;
    let gradient_count = input.brush.z;
    let gradient_tile_mode = input.brush.w;
    if (brush_type == 1u) {
        // Linear gradient projected from start.xy to end.xy
        let start = input.gradient_params.xy;
        let end = input.gradient_params.zw;
        let dir = end - start;
        let denom = max(dot(dir, dir), 0.00001);
        let raw_t = dot(rect_pos - start, dir) / denom;
        let sample = remap_gradient_t(raw_t, gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(gradient_start, gradient_count, sample.t);
            is_gradient = true;
        }
    } else if (brush_type == 2u) {
        // Radial gradient - use explicit center and radius from gradient_params
        let center = input.gradient_params.xy;
        let radius = max(input.gradient_params.z, 0.00001);
        let dist_from_center = length(rect_pos - center);
        let raw_t = dist_from_center / radius;
        let sample = remap_gradient_t(raw_t, gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(gradient_start, gradient_count, sample.t);
            is_gradient = true;
        }
    } else if (brush_type == 3u) {
        // Sweep gradient - angle-based interpolation around center
        let center = input.gradient_params.xy;
        let dx = rect_pos.x - center.x;
        let dy = rect_pos.y - center.y;
        let angle = atan2(dy, dx);
        // Map [-PI, PI] to [0, 1]
        let raw_t = angle / (2.0 * 3.14159265358979) + 0.5;
        let sample = remap_gradient_t(raw_t, gradient_tile_mode);
        if (!sample.valid) {
            color = vec4<f32>(0.0);
        } else {
            color = sample_gradient(gradient_start, gradient_count, sample.t);
            is_gradient = true;
        }
    }

    // Dither the gradient, and only the gradient — a solid brush has no ramp
    // to band, and Skia leaves it alone too, which is why solid fills already
    // land byte-for-byte on the Compose build's.
    if (is_gradient && color.a > 0.0) {
        let offset = gradient_dither(input.clip_position.xy) * (1.0 / 255.0);
        color = vec4<f32>(clamp(color.rgb + vec3<f32>(offset), vec3<f32>(0.0), vec3<f32>(1.0)),
                          color.a);
    }

    return vec4<f32>(color.rgb, color.a * alpha);
}
