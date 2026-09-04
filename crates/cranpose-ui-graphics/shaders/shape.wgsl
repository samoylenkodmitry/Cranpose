// Shared structs
//
// Everything the fragment shader needs from a record rides here as a flat
// varying instead of being re-fetched per fragment. The vertex shader runs
// six times per record; the fragment shader runs once per covered pixel —
// thousands of times more in an overdraw-heavy scene — and a dynamically
// indexed array cannot be promoted to registers, so every one of those
// fragment fetches was a real memory load. Flat varyings move that traffic
// to the (otherwise idle) varying interpolator. Only the gradient-stop walk
// for ramps longer than four stops is still fetched per fragment, and solid
// brushes never touch it.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_pos: vec4<f32>,
    @location(3) @interpolate(flat) rect: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    @location(5) @interpolate(flat) gradient_params: vec4<f32>,
    @location(6) @interpolate(flat) clip_rect: vec4<f32>,
    @location(7) @interpolate(flat) stroke_params: vec4<f32>,
    @location(8) @interpolate(flat) arc_params: vec4<f32>,
    @location(9) @interpolate(flat) brush: vec4<u32>,
    @location(10) @interpolate(flat) stop_offsets: vec4<f32>,
    @location(11) @interpolate(flat) stop_color0: vec4<f32>,
    @location(12) @interpolate(flat) stop_color1: vec4<f32>,
    @location(13) @interpolate(flat) stop_color2: vec4<f32>,
    @location(14) @interpolate(flat) stop_color3: vec4<f32>,
}

// Where a recording lands on the device: the logical offset of its record
// space (the layer origin with the rigid snap delta folded in), the root
// scale, the clip, the dither origin and the paint every record takes.
//
//   flags bit 0  canonicalize: the placement is rigidly snapped, so every
//                device coordinate rounds to the 1/16 px grid
//   flags bit 1  clip: `clip` holds a device clip rect
//   flags bit 2  color filter: `color_matrix`/`color_offset` apply
//   flags bit 3  painted: the layer's alpha or filter applies, so a solid
//                colour is quantized to 8-bit sRGB first, as the CPU
//                brush resolution did; an unpainted colour passes as is
struct Placement {
    offset: vec2<f32>,
    root_scale: f32,
    flags: u32,
    clip: vec4<f32>,
    dither_origin: vec2<f32>,
    alpha: f32,
    reserved: f32,
    color_matrix: mat4x4<f32>,
    color_offset: vec4<f32>,
}

struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
    placement: Placement,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// One recorded shape, byte for byte what the draw scope wrote: local
// space, the app's own units, nothing resolved. `flags` packs the record
// kind (bits 0-1: rect, round rect, arc), the stroke bit (2), the stroke
// cap (3-4) and join (5-6), the blend mode (8-15), the arc band's cap
// (16-17), and the arc facts: degenerate (18), loose rect (19), banded (20).
// `placement` names the record's placement in the arena tier and is unused
// in the store tier.
struct ShapeRecord {
    rect: vec4<f32>,
    radii: vec4<f32>,
    color: vec4<f32>,
    stroke_width: f32,
    flags: u32,
    brush: u32,
    placement: u32,
    arc: vec4<f32>,
    arc_band: vec4<f32>,
    arc_normalized: vec4<f32>,
}

struct BrushRecord {
    kind: u32,
    tile_mode: u32,
    stop_start: u32,
    stop_count: u32,
    params: vec4<f32>,
    explicit_start: u32,
    explicit_len: u32,
    reserved: vec2<u32>,
}

struct GradientStop {
    color: vec4<f32>,
    position: vec4<f32>,
}

// The uniform-buffer form is the WebGL floor: 16 KB bindings, so a chunk of
// 128 records, 256 brushes, 256 stops and 64 placements, drawn as quads.
// Native pipelines rewrite these five declarations to unbounded storage
// arrays — see `shape_shader_source`, which string-replaces the exact
// literals.
@group(1) @binding(0)
var<uniform> records: array<ShapeRecord, 128>;

@group(1) @binding(1)
var<uniform> brushes: array<BrushRecord, 256>;

@group(1) @binding(2)
var<uniform> gradient_stops: array<GradientStop, 256>;

@group(1) @binding(3)
var<uniform> band_records: array<vec4<u32>, 4>;

@group(1) @binding(4)
var<uniform> placements: array<Placement, 64>;

const RECORD_KIND_ROUND_RECT: u32 = 1u;
const RECORD_KIND_ARC: u32 = 2u;
const RECORD_STROKED: u32 = 4u;
const RECORD_CAP_SHIFT: u32 = 3u;
const RECORD_JOIN_SHIFT: u32 = 5u;
const RECORD_BAND_CAP_SHIFT: u32 = 16u;
const RECORD_ARC_DEGENERATE: u32 = 262144u;
const RECORD_ARC_BANDED: u32 = 1048576u;

const BRUSH_LINEAR: u32 = 1u;
const BRUSH_RADIAL: u32 = 2u;
const BRUSH_SWEEP: u32 = 3u;

const PLACEMENT_CANONICALIZE: u32 = 1u;
const PLACEMENT_CLIPPED: u32 = 2u;
const PLACEMENT_FILTERED: u32 = 4u;
const PLACEMENT_PAINTED: u32 = 8u;

// A band's slack beyond its ring, in device pixels, so every pixel the
// fragment stage anti-aliases lies inside the strip.
const BAND_MARGIN: f32 = 1.0;
const BAND_ANGULAR_PAD: f32 = 0.05;
const INFINITE_GRADIENT_POINT: f32 = 1.0e30;
const PI: f32 = 3.14159265358979;

// Which tier a pipeline draws from. The store tier draws one recording from
// its retained buffers under the placement in `uniforms`, and hands wide
// arcs to `vs_band`; the arena tier draws many small recordings copied
// into one buffer, each record naming its placement, every record a quad.
override TIER_ARENA: bool = false;
// Whether band pipelines draw the banded arcs of this tier, so the quad
// entry point collapses them; false on the uniform floor, which has no
// band table and draws every record as its quad.
override SHAPE_BANDS: bool = true;
// The strip segments of a band pipeline; unused by the quad entry points.
override BAND_SEGMENTS: u32 = 8u;
// A band pipeline shades only inside the record's rect, the quad path's
// raster extent, so the two paths cover the same pixels.
override SHAPE_BAND: bool = false;

fn record_placement(record: ShapeRecord) -> Placement {
    if (TIER_ARENA) {
        return placements[record.placement];
    }
    return uniforms.placement;
}

// The device coordinate the CPU would have written: rounded to the 1/16
// device pixel grid when the placement is snapped, so a rigidly moving
// subtree lands on the same sub-pixel phase every frame.
fn canonical(value: f32) -> f32 {
    return sign(value) * floor(abs(value) * 16.0 + 0.5) / 16.0;
}

fn device_coordinate(value: f32, canonicalize: bool) -> f32 {
    return select(value, canonical(value), canonicalize);
}

struct RecordGeometry {
    // The device rect the record rasterizes: the stroke's outer half
    // included, canonicalized under a snapped placement.
    rect: vec4<f32>,
    canonicalize: bool,
    scale: f32,
}

fn record_geometry(record: ShapeRecord, placement: Placement) -> RecordGeometry {
    let kind = record.flags & 3u;
    let stroked = (record.flags & RECORD_STROKED) != 0u && kind != RECORD_KIND_ARC;
    let half_width = select(0.0, record.stroke_width * 0.5, stroked);
    let scale = placement.root_scale;
    let canonicalize = (placement.flags & PLACEMENT_CANONICALIZE) != 0u;
    let left = device_coordinate((record.rect.x - half_width + placement.offset.x) * scale, canonicalize);
    let top = device_coordinate((record.rect.y - half_width + placement.offset.y) * scale, canonicalize);
    let right = device_coordinate(
        (record.rect.x + record.rect.z + half_width + placement.offset.x) * scale, canonicalize);
    let bottom = device_coordinate(
        (record.rect.y + record.rect.w + half_width + placement.offset.y) * scale, canonicalize);
    var geometry: RecordGeometry;
    geometry.rect = vec4<f32>(left, top, right - left, bottom - top);
    geometry.canonicalize = canonicalize;
    geometry.scale = scale;
    return geometry;
}

// The colour a record paints with under its placement. An unpainted
// placement passes the colour through; a painting one quantizes it to
// 8-bit sRGB, applies the layer alpha and then the colour filter, in the
// order the CPU brush resolution applies them.
fn paint(color: vec4<f32>, placement: Placement) -> vec4<f32> {
    if ((placement.flags & PLACEMENT_PAINTED) == 0u) {
        return color;
    }
    var painted = floor(clamp(color, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5) / 255.0;
    painted.a = clamp(painted.a * placement.alpha, 0.0, 1.0);
    if ((placement.flags & PLACEMENT_FILTERED) != 0u) {
        painted = clamp(placement.color_matrix * painted + placement.color_offset,
                        vec4<f32>(0.0), vec4<f32>(1.0));
    }
    return painted;
}

// A gradient endpoint: relative to the record's device rect, with an
// infinite coordinate meaning the rect's far edge (positive) or origin.
fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if (abs(value) < INFINITE_GRADIENT_POINT) {
        return origin + value;
    }
    return select(origin, origin + extent, value > 0.0);
}

fn arc_trig(start: f32, sweep: f32) -> vec4<f32> {
    if (sweep >= TAU && start == 0.0) {
        return vec4<f32>(0.0, -1.0, 0.0, -1.0);
    }
    let half_sweep = clamp(sweep, 0.0, TAU) * 0.5;
    let mid = start + half_sweep;
    return vec4<f32>(sin(mid), cos(mid), max(sin(half_sweep), 0.0), cos(half_sweep));
}

fn resolved_radii(record: ShapeRecord, scale: f32) -> vec4<f32> {
    let limit = max(min(record.rect.z, record.rect.w) * 0.5, 0.0);
    // The record stores top-left, top-right, bottom-right, bottom-left; the
    // fragment stage reads top-left, top-right, bottom-left, bottom-right.
    let stored = vec4<f32>(record.radii.x, record.radii.y, record.radii.w, record.radii.z);
    return clamp(stored, vec4<f32>(0.0), vec4<f32>(limit)) * scale;
}

fn shape_output(
    record: ShapeRecord,
    placement: Placement,
    geometry: RecordGeometry,
    position: vec2<f32>,
    uv: vec2<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    let x = ((position.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((position.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = paint(record.color, placement);
    output.uv = uv;
    output.world_pos = vec4<f32>(position, position - placement.dither_origin);
    output.rect = geometry.rect;
    let scale = geometry.scale;
    let kind = record.flags & 3u;
    let stroked = (record.flags & RECORD_STROKED) != 0u;

    if (kind == RECORD_KIND_ARC) {
        output.radii = arc_trig(record.arc_normalized.x, record.arc_normalized.y);
        let cap = (record.flags >> RECORD_BAND_CAP_SHIFT) & 3u;
        output.stroke_params = vec4<f32>(
            0.0,
            f32(SHAPE_KIND_ARC | (cap << 2u)),
            record.arc_band.w * scale,
            record.arc_band.z * scale,
        );
        output.arc_params = vec4<f32>(
            (record.arc.x + placement.offset.x) * scale,
            (record.arc.y + placement.offset.y) * scale,
            record.arc_normalized.x,
            record.arc_normalized.y,
        );
    } else {
        if (kind == RECORD_KIND_ROUND_RECT) {
            output.radii = resolved_radii(record, scale);
        } else {
            output.radii = vec4<f32>(0.0);
        }
        if (stroked) {
            let cap = (record.flags >> RECORD_CAP_SHIFT) & 3u;
            let join = (record.flags >> RECORD_JOIN_SHIFT) & 3u;
            output.stroke_params = vec4<f32>(
                max(record.stroke_width, 0.0) * scale,
                f32(SHAPE_KIND_STROKE | (cap << 2u) | (join << 4u)),
                0.0,
                0.0,
            );
        } else {
            output.stroke_params = vec4<f32>(0.0);
        }
        output.arc_params = vec4<f32>(0.0);
    }

    if (SHAPE_CLIPPED && (placement.flags & PLACEMENT_CLIPPED) != 0u) {
        output.clip_rect = placement.clip;
    } else {
        output.clip_rect = vec4<f32>(0.0);
    }

    output.gradient_params = vec4<f32>(0.0);
    output.brush = vec4<u32>(0u);
    if (!SHAPE_SOLID && record.brush != 0u) {
        let brush = brushes[record.brush - 1u];
        let rect = geometry.rect;
        let canonicalize = geometry.canonicalize;
        let params = brush.params * scale;
        if (brush.kind == BRUSH_LINEAR) {
            output.gradient_params = vec4<f32>(
                device_coordinate(resolve_gradient_point(rect.x, rect.z, params.x), canonicalize),
                device_coordinate(resolve_gradient_point(rect.y, rect.w, params.y), canonicalize),
                device_coordinate(resolve_gradient_point(rect.x, rect.z, params.z), canonicalize),
                device_coordinate(resolve_gradient_point(rect.y, rect.w, params.w), canonicalize),
            );
        } else if (brush.kind == BRUSH_RADIAL) {
            output.gradient_params = vec4<f32>(
                device_coordinate(rect.x + params.x, canonicalize),
                device_coordinate(rect.y + params.y, canonicalize),
                max(params.z, 1.1920929e-7),
                0.0,
            );
        } else {
            output.gradient_params = vec4<f32>(
                device_coordinate(rect.x + params.x, canonicalize),
                device_coordinate(rect.y + params.y, canonicalize),
                0.0,
                0.0,
            );
        }
        output.brush = vec4<u32>(brush.kind, brush.stop_start, brush.stop_count, brush.tile_mode);
        let stops = load_inline_gradient_stops(brush.stop_start, brush.stop_count);
        output.stop_offsets = stops.offsets;
        output.stop_color0 = stops.color0;
        output.stop_color1 = stops.color1;
        output.stop_color2 = stops.color2;
        output.stop_color3 = stops.color3;
    }
    return output;
}

// Six vertices at one point: nothing rasterizes.
fn collapsed() -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return output;
}

fn quad_corner(slot: u32) -> u32 {
    switch slot {
        case 0u: { return 0u; }
        case 1u, 4u: { return 1u; }
        case 2u, 3u: { return 2u; }
        default: { return 3u; }
    }
}

// Vertex shader: quads
//
// There is no vertex buffer: each record is six unindexed vertices whose
// corner positions and UVs come from the record by `vertex_index`. Corner
// numbering: 0 = top-left, 1 = top-right, 2 = bottom-left,
// 3 = bottom-right, drawn as triangles (0, 1, 2) and (2, 1, 3). A draw
// starts at the first record of its segment times six, so the record index
// indexes the whole table. Where band pipelines draw, a banded record
// collapses here: its strip draws it.
@vertex
fn vs_record(@builtin(vertex_index) vertex_idx: u32) -> VertexOutput {
    let record = records[vertex_idx / 6u];
    if ((record.flags & RECORD_ARC_DEGENERATE) != 0u) {
        return collapsed();
    }
    if (SHAPE_BANDS && (record.flags & RECORD_ARC_BANDED) != 0u) {
        return collapsed();
    }
    let corner = quad_corner(vertex_idx % 6u);
    let uv = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    let placement = record_placement(record);
    let geometry = record_geometry(record, placement);
    let position = geometry.rect.xy + uv * geometry.rect.zw;
    return shape_output(record, placement, geometry, position, uv);
}

// Vertex shader: bands
//
// A wide arc, or a stroked circle, rasterizes as a strip of
// `BAND_SEGMENTS` quads around its ring instead of the disc's quad; the
// record's arc fields carry the ring either way. The strip covers every pixel the
// fragment stage anti-aliases: the ring is padded by `BAND_MARGIN`, the
// sweep by the padding's angle, the outer vertices ride out so the
// polygon circumscribes the padded outer circle, and the inner vertices
// sit on the padded inner circle, whose chords only ever fall inside it.
// The draw's vertex range names the bucket entries: `band_records` lists
// the banded records of the recording, bucket by bucket.
@vertex
fn vs_band(@builtin(vertex_index) vertex_idx: u32) -> VertexOutput {
    let per_band = BAND_SEGMENTS * 6u;
    let slot = vertex_idx / per_band;
    let local = vertex_idx % per_band;
    let segment = local / 6u;
    let k = local % 6u;
    let packed = band_records[slot / 4u];
    var record_index: u32;
    switch (slot % 4u) {
        case 0u: { record_index = packed.x; }
        case 1u: { record_index = packed.y; }
        case 2u: { record_index = packed.z; }
        default: { record_index = packed.w; }
    }
    let record = records[record_index];
    let placement = record_placement(record);
    let geometry = record_geometry(record, placement);
    let scale = placement.root_scale;
    let center = (record.arc.xy + placement.offset) * scale;
    let inner = record.arc_band.z * scale;
    let outer = record.arc_band.w * scale;
    let start = record.arc_normalized.x;
    let sweep = record.arc_normalized.y;
    let mid = (outer + inner) * 0.5;
    let ring_half = max((outer - inner) * 0.5, 0.0) + BAND_MARGIN;
    let outer_padded = mid + ring_half;
    let inner_padded = max(mid - ring_half, 0.0);
    var range_start = 0.0;
    var range = TAU;
    if (sweep < TAU) {
        let pad = select(PI, asin(ring_half / mid) + BAND_ANGULAR_PAD, ring_half < mid);
        let padded = sweep + pad + pad;
        if (padded < TAU) {
            range_start = start - pad;
            range = padded;
        }
    }
    let step = range / f32(BAND_SEGMENTS);
    let outer_vertex = outer_padded / cos(step * 0.5);
    var boundary = segment;
    var radius = inner_padded;
    switch k {
        case 1u: { radius = outer_vertex; }
        case 2u, 4u: { boundary = segment + 1u; radius = outer_vertex; }
        case 5u: { boundary = segment + 1u; }
        default: {}
    }
    let angle = range_start + step * f32(boundary);
    let position = center + vec2<f32>(cos(angle), sin(angle)) * radius;
    let uv = (position - geometry.rect.xy) / geometry.rect.zw;
    return shape_output(record, placement, geometry, position, uv);
}

// Fragment shader
//
// `stroke_params.y` packs three 2-bit fields:
//
//   bits 0-1  shape kind : 0 = fill, 1 = stroked rect/round-rect, 2 = arc band
//   bits 2-3  stroke cap : 0 = butt, 1 = round, 2 = square   (arcs only)
//   bits 4-5  stroke join: 0 = miter, 1 = round, 2 = bevel   (rects only)
//
// Angle convention for arcs: radians, 0 = +X, increasing CLOCKWISE on screen
// (y-down device space) — the same convention the sweep-gradient branch below
// gets from atan2(dy, dx).
const SHAPE_KIND_FILL: u32 = 0u;
const SHAPE_KIND_STROKE: u32 = 1u;
const SHAPE_KIND_ARC: u32 = 2u;

// Pipeline constants a batch fixes when every record it draws agrees: the
// shape kind (-1 keeps the per-record ladder), whether every brush is solid,
// and whether any record carries a clip. A fixed value folds the branches
// the batch cannot take out of the program; the record data stays the same,
// so the general program and every specialised one shade one record alike.
override SHAPE_KIND_FIXED: i32 = -1;
override SHAPE_SOLID: bool = false;
override SHAPE_CLIPPED: bool = true;

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
// of the half sweep. They are constants of the shape, so the vertex stage
// computes them once per vertex (`arc_trig`) instead of this shader paying
// four transcendentals on every fragment — in an arc-heavy scene that is by
// far the largest ALU term of the whole pipeline.
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
// same way. `world_pos` rather than `@builtin(position)` for exactly that
// reason — on Android the two read the same pixel anyway. The coordinate is
// the device position relative to the record's dither origin (`world_pos.zw`):
// a subtree moving rigidly by whole pixels carries its pattern with it, and a
// record outside any motion anchor keeps the origin at zero, Skia's phase.
fn gradient_dither(device_pos: vec2<f32>) -> f32 {
    let x = u32(max(floor(device_pos.x), 0.0)) + 1u;
    let y = u32(max(floor(device_pos.y), 0.0)) + 1u;
    let m = ((y & 1u) << 3u) | ((x & 1u) << 2u) | (y & 2u) | ((x & 2u) >> 1u);
    return f32(m) * (1.0 / 16.0) - (15.0 / 32.0);
}

// Gradients of up to `INLINE_GRADIENT_STOPS` stops travel to the fragment
// stage as flat varyings, read once per vertex; longer ramps keep the
// per-fragment walk over `gradient_stops`. On a tiler the walk is several
// dynamically indexed uniform loads per fragment, which is what a full-screen
// three-stop radial cost on Mali (`CRANPOSE_UNIFORM_GRADIENT_STOPS` rewrites
// this to 0 and routes every gradient back through the walk; the parity test
// holds the two at zero differing bytes).
const INLINE_GRADIENT_STOPS: u32 = 4u;

struct InlineGradientStops {
    offsets: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    color2: vec4<f32>,
    color3: vec4<f32>,
}

fn load_inline_gradient_stops(gradient_start: u32, count: u32) -> InlineGradientStops {
    var stops: InlineGradientStops;
    if (count == 0u || count > INLINE_GRADIENT_STOPS) {
        return stops;
    }
    let first = gradient_stops[gradient_start];
    stops.offsets.x = first.position.x;
    stops.color0 = first.color;
    if (count > 1u) {
        let second = gradient_stops[gradient_start + 1u];
        stops.offsets.y = second.position.x;
        stops.color1 = second.color;
    }
    if (count > 2u) {
        let third = gradient_stops[gradient_start + 2u];
        stops.offsets.z = third.position.x;
        stops.color2 = third.color;
    }
    if (count > 3u) {
        let fourth = gradient_stops[gradient_start + 3u];
        stops.offsets.w = fourth.position.x;
        stops.color3 = fourth.color;
    }
    return stops;
}

fn gradient_segment(
    from_offset: f32,
    from_color: vec4<f32>,
    to_offset: f32,
    to_color: vec4<f32>,
    clamped: f32,
) -> vec4<f32> {
    let denom = max(to_offset - from_offset, 0.00001);
    let local_t = clamp((clamped - from_offset) / denom, 0.0, 1.0);
    return mix(from_color, to_color, local_t);
}

fn sample_inline_gradient(input: VertexOutput, count: u32, t: f32) -> vec4<f32> {
    if (count == 1u) {
        return input.stop_color0;
    }
    let offsets = input.stop_offsets;
    let clamped = clamp(t, 0.0, 1.0);
    if (clamped <= offsets.x) {
        return input.stop_color0;
    }
    if (clamped <= offsets.y) {
        return gradient_segment(offsets.x, input.stop_color0, offsets.y, input.stop_color1, clamped);
    }
    if (count == 2u) {
        return input.stop_color1;
    }
    if (clamped <= offsets.z) {
        return gradient_segment(offsets.y, input.stop_color1, offsets.z, input.stop_color2, clamped);
    }
    if (count == 3u) {
        return input.stop_color2;
    }
    if (clamped <= offsets.w) {
        return gradient_segment(offsets.z, input.stop_color2, offsets.w, input.stop_color3, clamped);
    }
    return input.stop_color3;
}

fn gradient_color(input: VertexOutput, t: f32) -> vec4<f32> {
    let count = input.brush.z;
    if (count > 0u && count <= INLINE_GRADIENT_STOPS) {
        return sample_inline_gradient(input, count, t);
    }
    return sample_gradient(input.brush.y, count, t);
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
            return gradient_segment(
                current.position.x, current.color, next.position.x, next.color, clamped,
            );
        }
        i = i + 1u;
    }

    return gradient_stops[gradient_start + count - 1u].color;
}

/// The coverage half of a shape fragment: the clip test, the shape-kind
/// ladder down to an alpha, and the two discards that end a fragment before
/// anything is shaded.
///
/// One function for every specialisation, so the arithmetic exists once. An
/// earlier solid-only entry duplicated it line for line on the argument that
/// identical source in the same order makes the compiler emit identical
/// instructions; on Vulkan it did not, and the parity test measured three
/// pixels apart by up to 2/255 (the macOS CI runner, on Metal, saw none of
/// it). The pipeline constants fold branches; they never copy code.
fn shape_coverage_alpha(input: VertexOutput) -> f32 {
    let world_pos = input.world_pos.xy;
    // Local layer-space pixel coordinate derived from uv, independent of
    // world-space quad deformation (rotation/perspective).
    let rect_pos = input.rect.xy + input.uv * input.rect.zw;

    // Apply clipping: if clip_rect has non-zero size, clip to it
    let clip_w = input.clip_rect.z;
    let clip_h = input.clip_rect.w;
    if (SHAPE_CLIPPED && clip_w > 0.0 && clip_h > 0.0) {
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

    if (SHAPE_BAND) {
        // The quad path shades nothing outside the record's rect; a band's
        // strip reaches past it, so the strip stops there too.
        if (rect_pos.x < input.rect.x || rect_pos.x > input.rect.x + input.rect.z ||
            rect_pos.y < input.rect.y || rect_pos.y > input.rect.y + input.rect.w) {
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
    let shape_kind = select(flags & 3u, u32(max(SHAPE_KIND_FIXED, 0)), SHAPE_KIND_FIXED >= 0);
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

    return alpha;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = shape_coverage_alpha(input);

    // Re-derived rather than threaded out of the coverage pass: both are pure
    // functions of `input`, so the compiler folds them back together.
    let world_pos = input.world_pos.xy;
    let rect_pos = input.rect.xy + input.uv * input.rect.zw;

    var color = input.color;
    var is_gradient = false;

    // Apply gradient if needed; a solid batch fixes the brush to solid and the
    // whole ladder folds away.
    let brush_type = select(input.brush.x, 0u, SHAPE_SOLID);
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
            color = gradient_color(input, sample.t);
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
            color = gradient_color(input, sample.t);
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
            color = gradient_color(input, sample.t);
            is_gradient = true;
        }
    }

    // Dither the gradient, and only the gradient — a solid brush has no ramp
    // to band, and Skia leaves it alone too, which is why solid fills already
    // land byte-for-byte on the Compose build's.
    if (is_gradient && color.a > 0.0) {
        let offset = gradient_dither(input.world_pos.zw) * (1.0 / 255.0);
        color = vec4<f32>(clamp(color.rgb + vec3<f32>(offset), vec3<f32>(0.0), vec3<f32>(1.0)),
                          color.a);
    }

    return vec4<f32>(color.rgb, color.a * alpha);
}
