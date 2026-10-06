// One pass of an elevation shadow as Skia's `ShadowCircularRRectOp` draws
// it, evaluated at each pixel: the offset its mesh would interpolate there,
// turned into the penumbra falloff. `cranpose_render_common::layer_shadow`
// holds the same math on the CPU.

struct ShadowInstance {
    // The device rect this instance covers: left, top, right, bottom.
    @location(0) draw: vec4<f32>,
    // The shadow's outer edge: left, top, right, bottom.
    @location(1) bounds: vec4<f32>,
    // The outer corner radii: top-left, top-right, bottom-right, bottom-left.
    @location(2) radii: vec4<f32>,
    // The umbra inset, the ramp's coordinate at the umbra, and the hole the
    // pixels test: its inset and corner radius, the radius below zero when
    // the instance's rect already leaves the hole out.
    @location(3) ramp: vec4<f32>,
    @location(4) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) bounds: vec4<f32>,
    @location(2) @interpolate(flat) radii: vec4<f32>,
    @location(3) @interpolate(flat) ramp: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
}

// The prefix of the viewport uniform the shape stage documents: the
// segment's transform into its target, the identity unless a layer is
// drawn in place.
struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
    transform: vec4<f32>,
    translation: vec2<f32>,
    reserved: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// The batch's depth in a pass that lays opaque interiors down first: its
// place in the pass's order from `uniforms.reserved.x`, on the step the
// shape stage's records use (`record_depth`), so later interiors hide it.
const DEPTH_STEP: f32 = 1.0 / 1048576.0;

fn batch_depth() -> f32 {
    return max(1.0 - (uniforms.reserved.x + 1.0) * DEPTH_STEP, 0.0);
}

// Each instance is one rect drawn as a four-corner triangle strip: corner 0
// top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
@vertex
fn rrect_shadow_vs_main(
    @builtin(vertex_index) corner: u32,
    shadow: ShadowInstance,
) -> VertexOutput {
    var output: VertexOutput;
    let position = vec2<f32>(
        select(shadow.draw.x, shadow.draw.z, (corner & 1u) != 0u),
        select(shadow.draw.y, shadow.draw.w, corner >= 2u),
    );
    let placed = vec2<f32>(
        uniforms.transform.x * position.x + uniforms.transform.y * position.y,
        uniforms.transform.z * position.x + uniforms.transform.w * position.y,
    ) + uniforms.translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, batch_depth(), 1.0);
    output.local = position;
    output.bounds = shadow.bounds;
    output.radii = shadow.radii;
    output.ramp = shadow.ramp;
    output.color = shadow.color;
    return output;
}

// The length of the offset Skia's corner fan interpolates at `along` and
// `across`, a point's distances out from the umbra's corner toward the two
// edges: 0 at the umbra, 1 on the outer edge. The fan's triangles meet the
// edge at the umbra line, `umbra - radius` along it and at the corner.
fn fan_offset(along: f32, across: f32, umbra: f32, radius: f32) -> f32 {
    let a = max(along, across);
    let b = min(along, across);
    let outer = vec2<f32>(radius - umbra, -radius - umbra)
        * inverseSqrt(max(2.0 * (radius * radius + umbra * umbra), 1e-12));
    let straight = umbra - radius;
    let per_umbra = a / max(umbra, 1e-6);
    let toward_curve = select(0.0, b / max(straight, 1e-6), straight > 0.0);
    let on_edge = vec2<f32>(toward_curve * outer.x, toward_curve * outer.y - (per_umbra - toward_curve));
    let diagonal = umbra / min(1.41421356 * (radius - umbra) - radius, -1e-6);
    let into_curve = (a - b) / max(radius, 1e-6);
    let on_curve = into_curve * outer + vec2<f32>(per_umbra - into_curve) * diagonal;
    return length(select(on_curve, on_edge, b * umbra <= a * straight));
}

// Skia's penumbra falloff at `x`: 0 on the outer edge, 1 where the shadow
// is whole, read as its 128-texel table is.
fn shadow_falloff(x: f32) -> f32 {
    let texel = clamp((x * 128.0 - 0.5) / 127.0, 0.0, 1.0);
    let distance = 1.0 - texel;
    return max(exp(-4.0 * distance * distance) - 0.018, 0.0);
}

fn in_hole(position: vec2<f32>, bounds: vec4<f32>, inset: f32, radius: f32) -> bool {
    let center = 0.5 * (bounds.xy + bounds.zw);
    let half_size = 0.5 * (bounds.zw - bounds.xy) - vec2<f32>(inset);
    let q = abs(position - center) - (half_size - vec2<f32>(radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius < 0.0;
}

@fragment
fn rrect_shadow_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let p = input.local;
    let bounds = input.bounds;
    let umbra = input.ramp.x;
    let left = bounds.x + umbra - p.x;
    let right = p.x - (bounds.z - umbra);
    let top = bounds.y + umbra - p.y;
    let bottom = p.y - (bounds.w - umbra);
    let upper = select(input.radii.y, input.radii.x, left > right);
    let lower = select(input.radii.z, input.radii.w, left > right);
    let offset = fan_offset(
        max(max(left, right), 0.0),
        max(max(top, bottom), 0.0),
        umbra,
        select(lower, upper, top > bottom),
    );
    let hole = (input.ramp.w >= 0.0) & in_hole(p, bounds, input.ramp.z, input.ramp.w);
    let coverage = select(shadow_falloff(input.ramp.y * (1.0 - offset)), 0.0, hole);
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
