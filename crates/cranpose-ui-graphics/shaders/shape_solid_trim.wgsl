// Trimmed-varying twins of the solid draw path — `CRANPOSE_SOLID_TRIM_VARYINGS`
// opt-in (`debug.cranpose.solid_trim` on Android). This file is never compiled
// alone: `shape_shader_source` appends it to `shape.wgsl` only when the trim
// is on, so the default build's shader modules stay byte-identical to the
// text that ships today — that is the kill switch.
//
// Why: `gradient_params` and `brush` exist only to feed `fs_main`'s gradient
// branches, and a batch reaches the solid pipelines exactly when its gradient
// stop count is zero. The full interface therefore interpolates eight flat
// scalars per fragment that the fragment stage never reads — parameter-store
// space and fragment input bandwidth spent on precisely the overdraw-heavy
// scenes the solid path exists for. These entries drop them.
//
// LOCATION DISCIPLINE — the rule this file exists to encode: the surviving
// varyings KEEP the location indices they hold in `VertexOutput`, and
// locations 5 and 9 stay VACANT. The first attempt at this trim (16a5d312,
// reverted in 371dd06a) renumbered the survivors densely (clip_rect 6 -> 5,
// stroke_params 7 -> 6, arc_params 8 -> 7) and a watch died with it on; the
// crash was never diagnosed and the renumbering was suspect #1. With the
// indices preserved, every pipeline in the app agrees on which location
// carries which semantic, so a driver that keys its interpolant setup by
// location sees one layout everywhere. WebGPU permits the gaps. Do not
// densify them.
//
// SUBSTITUTION CONTRACTS — this text is appended BEFORE `shape_shader_source`
// runs its rewrites and BEFORE `with_content_z`, so both exact-text
// substitutions must land here the way they land in `vs_main`. (Neither
// literal may appear in a comment with its trailing semicolon, or the
// rewrite would edit the comment and the landing counts would drift.)
// * the `output.color = shape.color` assignment — the storage rewrite
//   injects the retained-paint select through that line. A solid entry it
//   missed would freeze every recolor on the retained slots it draws (the
//   substitution test counts five landings).
// * the clip-position tail `(x, y, 0.0, 1.0)` — the display-clip depth
//   variants rewrite that tail to z 0.5; the trimmed depth pipelines cull
//   against the occluder like every other content pipeline.

// The solid pipelines' inter-stage interface: `VertexOutput` minus
// `gradient_params` (its location 5 left vacant) and `brush` (its location 9
// left vacant). See LOCATION DISCIPLINE above before touching an index.
struct VertexOutputSolid {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_pos: vec2<f32>,
    @location(3) @interpolate(flat) rect: vec4<f32>,
    @location(4) @interpolate(flat) radii: vec4<f32>,
    // location 5 is `gradient_params`' slot in `VertexOutput` — VACANT.
    @location(6) @interpolate(flat) clip_rect: vec4<f32>,
    @location(7) @interpolate(flat) stroke_params: vec4<f32>,
    @location(8) @interpolate(flat) arc_params: vec4<f32>,
    // location 9 is `brush`'s slot in `VertexOutput` — VACANT.
}

// BIT-EXACTNESS REQUIREMENT: every expression below is copied verbatim from
// `vs_main`; only the two dropped varying writes are gone. Dropping unused
// outputs cannot change the interpolation of the ones that remain, so the
// solid pipelines rasterize the same fragments with the same inputs and
// `solid_fs_parity` holds the trimmed-vs-full bar at ZERO differing bytes.
@vertex
fn vs_solid(@builtin(vertex_index) vertex_idx: u32) -> VertexOutputSolid {
    var output: VertexOutputSolid;

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
    output.clip_rect = shape.clip_rect;
    output.stroke_params = shape.stroke_params;
    output.arc_params = shape.arc_params;

    return output;
}

// `vs_shape_instanced` minus the two dropped varying writes; the same
// verbatim-copy rule as `vs_solid`. Storage mode only, like its full-fat
// twin; in the uniform variant it is dead code, which keeps the appended
// text valid there too.
@vertex
fn vs_solid_instanced(
    @builtin(vertex_index) corner_idx: u32,
    @builtin(instance_index) instance_idx: u32,
) -> VertexOutputSolid {
    var output: VertexOutputSolid;

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
    output.clip_rect = shape.clip_rect;
    output.stroke_params = shape.stroke_params;
    output.arc_params = shape.arc_params;

    return output;
}

// `fs_solid` behind the trimmed interface. The coverage pass MUST stay the
// shared `shape_coverage_alpha` — never a copy: its doc comment records a
// line-for-line duplicate measuring 2/255 apart on Vulkan. The full
// `VertexOutput` it takes is rebuilt member by member from the trimmed
// input; the two dropped members stay zero-initialized and are dead —
// `shape_coverage_alpha` reads neither — so the coverage arithmetic runs the
// one shared body over bit-identical inputs.
@fragment
fn fs_solid_trim(input: VertexOutputSolid) -> @location(0) vec4<f32> {
    var full: VertexOutput;
    full.clip_position = input.clip_position;
    full.color = input.color;
    full.uv = input.uv;
    full.world_pos = input.world_pos;
    full.rect = input.rect;
    full.radii = input.radii;
    full.clip_rect = input.clip_rect;
    full.stroke_params = input.stroke_params;
    full.arc_params = input.arc_params;
    let alpha = shape_coverage_alpha(full);
    let color = input.color;
    return vec4<f32>(color.rgb, color.a * alpha);
}
