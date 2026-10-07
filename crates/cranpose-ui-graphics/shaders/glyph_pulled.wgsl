// Glyphs a frame draws from retained runs, appended to `glyph_atlas.wgsl`
// where storage buffers reach the vertex stage: each instance names a glyph
// of the glyph arena, its run's origin this frame and the scissor the
// glyph is cut to.

// A glyph of a retained run as the glyph arena holds it: its color, then
// pairs of 16-bit halves, low half first: its place from the run's raster
// origin (signed), its size in pixels, and its mask's texels and their size
// in the atlas.
struct RetainedGlyph {
    color: vec4<f32>,
    origin: u32,
    size: u32,
    texel: u32,
    texel_size: u32,
}

fn signed_halves(packed: u32) -> vec2<f32> {
    return vec2<f32>(
        f32(bitcast<i32>(packed << 16u) >> 16u),
        f32(bitcast<i32>(packed) >> 16u),
    );
}

fn unsigned_halves(packed: u32) -> vec2<f32> {
    return vec2<f32>(f32(packed & 0xffffu), f32(packed >> 16u));
}

@group(2) @binding(0)
var<storage, read> retained_glyphs: array<RetainedGlyph>;

// A glyph a frame draws from a retained run: its place in the glyph arena,
// the run's raster origin this frame and the text's scissor in target
// pixels (left, top, right, bottom). Twenty bytes a glyph a frame, where a
// glyph written whole takes sixty-four.
struct PulledGlyph {
    @location(0) index: u32,
    @location(1) origin: vec2<f32>,
    @location(2) cut: vec4<u32>,
}

// The glyph at its run's origin, cut to the scissor with its atlas
// coordinates cut to match, as `GlyphInstance::clipped_to` cuts a glyph
// written whole. An edge the cut leaves keeps its coordinate exactly; a
// glyph cut away collapses to no area.
fn pulled_glyph(pulled: PulledGlyph) -> GlyphInstance {
    let stored = retained_glyphs[pulled.index];
    let glyph = retained_instance(
        signed_halves(stored.origin) + pulled.origin,
        unsigned_halves(stored.size),
        unsigned_halves(stored.texel),
        unsigned_halves(stored.texel_size),
        stored.color,
    );
    let rect = glyph.rect;
    let edges = vec4<f32>(pulled.cut)
        + vec4<f32>(uniforms.viewport_offset, uniforms.viewport_offset);
    let near = max(rect.xy, edges.xy);
    let cut = vec4<f32>(near, max(min(rect.zw, edges.zw), near));
    let span = rect.zw - rect.xy;
    let uv_span = glyph.uv.zw - glyph.uv.xy;
    let uv = glyph.uv.xyxy + (cut - rect.xyxy) / span.xyxy * uv_span.xyxy;
    return GlyphInstance(cut, select(uv, glyph.uv, cut == rect), glyph.uv_bounds, glyph.color);
}

@vertex
fn glyph_atlas_pulled_vs_main(
    @builtin(vertex_index) corner: u32,
    pulled: PulledGlyph,
) -> VertexOutput {
    return glyph_vertex(corner, pulled_glyph(pulled), vec4<f32>(1.0, 0.0, 0.0, 1.0), vec2<f32>(0.0));
}

@vertex
fn glyph_atlas_pulled_aligned_vs_main(
    @builtin(vertex_index) corner: u32,
    pulled: PulledGlyph,
) -> AlignedVertexOutput {
    return aligned_glyph_vertex(corner, pulled_glyph(pulled));
}
