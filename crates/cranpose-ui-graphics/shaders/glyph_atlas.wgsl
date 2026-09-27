struct GlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) uv_bounds: vec4<f32>,
    @location(3) color: vec4<f32>,
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

@group(1) @binding(0)
var glyph_texture: texture_2d<f32>;

@group(1) @binding(1)
var glyph_sampler: sampler;

// Each instance is one glyph quad drawn as a four-corner triangle strip:
// corner 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right. `select`
// picks each corner's coordinates exactly, as the quad's vertices held them.
@vertex
fn glyph_atlas_vs_main(
    @builtin(vertex_index) corner: u32,
    glyph: GlyphInstance,
) -> VertexOutput {
    var output: VertexOutput;
    let right = (corner & 1u) != 0u;
    let bottom = corner >= 2u;
    let corner_position = vec2<f32>(
        select(glyph.rect.x, glyph.rect.z, right),
        select(glyph.rect.y, glyph.rect.w, bottom),
    );
    let position = corner_position + uniforms.origin;
    let placed = vec2<f32>(
        uniforms.transform.x * position.x + uniforms.transform.y * position.y,
        uniforms.transform.z * position.x + uniforms.transform.w * position.y,
    ) + uniforms.translation;
    let x = ((placed.x - uniforms.viewport_offset.x) / uniforms.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - ((placed.y - uniforms.viewport_offset.y) / uniforms.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = glyph.color;
    output.uv = vec2<f32>(
        select(glyph.uv.x, glyph.uv.z, right),
        select(glyph.uv.y, glyph.uv.w, bottom),
    );
    output.uv_bounds = glyph.uv_bounds;
    return output;
}

@fragment
fn glyph_atlas_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
    let coverage = textureSample(glyph_texture, glyph_sampler, uv).r;
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
