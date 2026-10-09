# Renderer architecture

The WGPU renderer turns a `RenderGraph` into ordered GPU passes. The graph
stores draw runs, primitives and layer nodes. The collector builds a
`LayerScene` for each root and child layer. See
[`graph.rs`](../crates/cranpose-render/common/src/graph.rs),
[`collect.rs`](../crates/cranpose-render/wgpu/src/collect.rs) and
[`render.rs`](../crates/cranpose-render/wgpu/src/render.rs).

## Frame flow

`frame.rs` resolves flat child surfaces before each parent pass. A group of
flat child surfaces shares shelf-packed atlas passes. Backdrop effects run in
z-ordered stages. Each stage captures page content beneath its effects, then
draws resolved composites in painter order. See
[`frame.rs`](../crates/cranpose-render/wgpu/src/frame.rs) and the
[`backdrop batching test`](../crates/cranpose-render/wgpu/tests/backdrop_pass_batching.rs).

The resolver caps layer depth at 128 and reports overflow once per renderer.
Runtime effects accept up to three substrate declarations. See
[`render_effect.rs`](../crates/cranpose-ui-graphics/src/render_effect.rs),
[`frame.rs`](../crates/cranpose-render/wgpu/src/frame.rs) and
[`frame tests`](../crates/cranpose-render/wgpu/src/tests/frame_tests.rs).

## Surface cache and opaque prefixes

`LayerCache` retains keyed surfaces under a 96 MiB byte budget and a 4096
entry cap. The transient texture pool reuses recent frame targets under a
32–256 MiB working-set budget and a 64-texture cap. The layer cache counts
shared texture allocations once toward its byte budget. See
[`layer_cache.rs`](../crates/cranpose-render/wgpu/src/layer_cache.rs) and
[`frame_graph.rs`](../crates/cranpose-render/wgpu/src/frame_graph.rs).

A full-page opaque solid first operation sets the pass clear when its z
position comes before every composite and in-place child. The renderer
caches eligible opaque gradient prefixes after a repeated frame. Admission
draws the prefix into the page and copies its texels into a retained texture.
Later frames composite the retained region at the prefix's original z
position. The cache key covers fill data, placement, scale, format and page
bounds. Source checks require opaque, whole-pixel geometry. See
[`opaque_prefix.rs`](../crates/cranpose-render/wgpu/src/opaque_prefix.rs),
[`frame.rs`](../crates/cranpose-render/wgpu/src/frame.rs) and the
[`opaque prefix cache test`](../crates/cranpose-render/wgpu/tests/opaque_prefix_cache.rs).

## Uploads

On a GPU that reads mapped buffers at full speed
(`MAPPABLE_PRIMARY_BUFFERS`), the CPU writes frame uniforms and geometry, the
frame's run arena, stored run tables and retained glyph runs into mapped
buffers. An animated frame then records no buffer copy and no queue write.
Each buffer maps again once the GPU finishes the frames that read it. A
changed stored run is written into a spare version of its tables, which takes
the 4 KiB chunks changed since the update it holds. Glyph chunks take new
runs only in the frame that maps them. Other GPUs copy through a staging belt,
in order between the passes. See
[`frame_graph.rs`](../crates/cranpose-render/wgpu/src/frame_graph.rs),
[`run_store.rs`](../crates/cranpose-render/wgpu/src/run_store.rs) and
[`glyph_run_arena.rs`](../crates/cranpose-render/wgpu/src/glyph_run_arena.rs).

## Pipeline compilation

Native WGPU renderers can compile on two worker lanes: demanded pipelines and
requested warm-ups. Each lane processes jobs in queue order. Vulkan can use a
general shape pipeline while a demanded specialization builds. The shape
scheduler keeps at most two demanded jobs outstanding. Other backends build demanded shape pipelines at
first use. Web builds pipelines inline. See
[`pipeline_compiler.rs`](../crates/cranpose-render/wgpu/src/pipeline_compiler.rs),
[`shape_pipelines.rs`](../crates/cranpose-render/wgpu/src/shape_pipelines.rs),
[`pipeline compiler tests`](../crates/cranpose-render/wgpu/src/tests/pipeline_compiler_tests.rs)
and [`shape readiness tests`](../crates/cranpose-render/wgpu/tests/shape_pipeline_readiness.rs).

## Pixel contracts and performance evidence

Renderer tests compare optimized output with reference output across layer
transforms, cache hits, atlas placement and backdrop effects. See
[`render_contract.rs`](../crates/cranpose-render/common/src/render_contract.rs),
[`animated layer transform tests`](../crates/cranpose-render/wgpu/tests/animated_layer_transform.rs),
[`surface atlas tests`](../crates/cranpose-render/wgpu/tests/surface_atlas.rs) and
[`backdrop atlas parity tests`](../crates/cranpose-render/wgpu/tests/backdrop_atlas_parity.rs).

Dated physical-device results and revision details live in the
[mobile performance evidence index](mobile_watch_performance.md) and
[frame-cost report](frame_cost_attribution.md). Each result applies to its
listed source revision, app and measurement window.
