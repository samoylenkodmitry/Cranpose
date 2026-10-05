struct GlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) uv_bounds: vec4<f32>,
    @location(3) color: vec4<f32>,
}

// A glyph of a layer drawn in place under a turn: the quad in the layer's
// device space, its turn (row-major) and the turn's translation in
// `translation.xy`. Its batch binds the identity.
struct TurnedGlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) uv_bounds: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) turn: vec4<f32>,
    @location(5) translation: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_bounds: vec4<f32>,
}

// The prefix of the viewport uniform the shape stage documents: the
// segment's transform into its target, the identity unless a layer is
// drawn in place, and the origin a retained run's vertices sit at, which is
// added before the transform exactly as the shared path adds it on the CPU.
struct Uniforms {
    viewport: vec2<f32>,
    viewport_offset: vec2<f32>,
    transform: vec4<f32>,
    translation: vec2<f32>,
    reserved: vec2<f32>,
    inverse: vec4<f32>,
    origin: vec2<f32>,
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

@group(1) @binding(0)
var glyph_texture: texture_2d<f32>;

@group(1) @binding(1)
var glyph_sampler: sampler;

// Each instance is one glyph quad drawn as a four-corner triangle strip:
// corner 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right. `select`
// picks each corner's coordinates exactly, as the quad's vertices held them.
fn glyph_vertex(
    corner: u32,
    glyph: GlyphInstance,
    turn: vec4<f32>,
    translation: vec2<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    let right = (corner & 1u) != 0u;
    let bottom = corner >= 2u;
    let corner_position = vec2<f32>(
        select(glyph.rect.x, glyph.rect.z, right),
        select(glyph.rect.y, glyph.rect.w, bottom),
    );
    let position = corner_position + uniforms.origin;
    let segment = vec2<f32>(
        uniforms.transform.x * position.x + uniforms.transform.y * position.y,
        uniforms.transform.z * position.x + uniforms.transform.w * position.y,
    ) + uniforms.translation;
    let placed = vec2<f32>(
        turn.x * segment.x + turn.y * segment.y,
        turn.z * segment.x + turn.w * segment.y,
    ) + translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, batch_depth(), 1.0);
    output.color = glyph.color;
    output.uv = vec2<f32>(
        select(glyph.uv.x, glyph.uv.z, right),
        select(glyph.uv.y, glyph.uv.w, bottom),
    );
    output.uv_bounds = glyph.uv_bounds;
    return output;
}

@vertex
fn glyph_atlas_vs_main(
    @builtin(vertex_index) corner: u32,
    glyph: GlyphInstance,
) -> VertexOutput {
    return glyph_vertex(corner, glyph, vec4<f32>(1.0, 0.0, 0.0, 1.0), vec2<f32>(0.0));
}

@vertex
fn glyph_atlas_turned_vs_main(
    @builtin(vertex_index) corner: u32,
    turned: TurnedGlyphInstance,
) -> VertexOutput {
    let glyph = GlyphInstance(turned.rect, turned.uv, turned.uv_bounds, turned.color);
    return glyph_vertex(corner, glyph, turned.turn, turned.translation.xy);
}

@fragment
fn glyph_atlas_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
    let coverage = textureSample(glyph_texture, glyph_sampler, uv).r;
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}

// A plain glyph quad on whole pixels at one texel a pixel: every fragment
// samples the center of one of the glyph's texels, so it needs no clamp
// and its bounds are not carried.
struct AlignedVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

@vertex
fn glyph_atlas_aligned_vs_main(
    @builtin(vertex_index) corner: u32,
    glyph: GlyphInstance,
) -> AlignedVertexOutput {
    let placed = glyph_vertex(corner, glyph, vec4<f32>(1.0, 0.0, 0.0, 1.0), vec2<f32>(0.0));
    var output: AlignedVertexOutput;
    output.clip_position = placed.clip_position;
    output.color = placed.color;
    output.uv = placed.uv;
    return output;
}

@fragment
fn glyph_atlas_aligned_fs_main(input: AlignedVertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(glyph_texture, glyph_sampler, input.uv).r;
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
