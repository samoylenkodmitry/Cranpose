# Renderer Architecture

How a `RenderGraph` becomes pixels in the WGPU renderer. The contract every
rule serves is [liquid_scroll_pixel_stability.md](liquid_scroll_pixel_stability.md):
byte identity against a never-caching render of the same build, on Metal,
Linux and the acceptance GPU (Adreno 702). Every rule names the test that
fails without it. Evidence for every number: legs, reports and one-off
fixtures under `/tmp/cranpose-mobile-watch-60fps/` (the shared root), the
named tests in `crates/cranpose-render/wgpu/tests/`, and the commits.

## Budget

Pixel Watch 3 (Adreno 702, 454², 8-bit direct-surface root), showcase
full-scroll route, pass timing, hot plateau at 43 C, span 31.4 ms (28 fps);
cool plateau 26 ms (35 fps); 60 fps needs 16.7 ms.

| item | ms | what it is |
|---|---|---|
| Layer Pass 1 | 11.6 | the cards' glass: rim band ~6, interior ~6; their content ~1 |
| Layer Passes 2-4 | ~10 | header stages: glass 3.5, the translucent header gradient rect 2.2, gradient-blur composite ~1, chips and text ~3 |
| blur chain | 6.0 | 2.21 downsample + horizontal + vertical chains per frame at scratch size, 13 paired taps |
| Layer Pass 0 | 2.4 | page below the first glass: card shadows 1.2, stars, list |
| captures, pass floors | ≤2 | seven copies of 0.33 MP fit in the 0.7 ms outside pass rows; a populated pass floors at ~0.1 ms |

Mate 20 X (Mali): showcase 37-40 fps at 32-34 C; Orbit on the 60 Hz cap.

Price list, watch: a blended full-page quad 2.0 ms (~10 ns per fragment,
even at one tap; probe row below), a copy command ~free, a page of radial
gradient with dither ~5 ms, glass 54-89 ns per shaded fragment, a blur tap
~10 ns per texel; passes are not the cost, fragments × taps are. Mali does
not price the blended quad above the copy.

## Shape of a frame

```text
applier state
  -> RenderGraph { root: LayerNode }                     scene_builder.rs (common)
  -> LayerScene per isolated layer                       collect.rs
       flat z-ordered ops | isolated children at z | backdrop effects at z
  -> resolve: every backdrop effect and isolated child   frame.rs
       gets a texture, in z order, from a capture
  -> compose: one pass into the target                   draw_pass.rs
       ops in z order, resolved textures blitted where they sit
```

The swapchain pass never reads itself: a backdrop effect resolves from a
capture of the ops beneath it, an isolated child renders into its own
texture through the same steps, the final pass draws direct ops and
composites the resolved textures.

## Rules and their contracts

- **Direct or isolated** (`collect_child`, `child_placement`): a layer is
  direct when its transform is a translation, it needs no group alpha,
  blend mode, render effect or explicit offscreen, and its rounded clip
  admits every op (`content_admits_rounded_clip`). Isolated children under
  uniform scale plus translation render on the parent's pixel grid with the
  1/16-pixel phase in the cache key; other transforms composite
  projectively. Contract `effect_semantics.rs`. A rounded clip becomes a
  composite mask (`grid_rounded_mask`) under any uniform scale plus
  translation, its radii scaled with the child, on the child's surface and
  on its backdrop alike: a glass shader draws its optical band past its
  silhouette and relies on the mask to cut it, so a scaled glass button
  without one shows the band up to its scissor as a square. Contract
  `scaled_glass_child_mask.rs`.
- **Nesting** (`MAX_RESOLVE_DEPTH`, 128): every isolated layer resolves its
  children recursively, about 4.5 KB of stack per level in release and 12 KB
  in debug; the present thread runs on 8 MiB. A layer nested deeper than the
  limit draws nothing and is reported once; the rest of the frame draws.
  Contract `nested_rotated_relayout.rs`.
- **Rigid motion**: a translated context carries one `SnapAnchor`, one
  device-pixel delta per frame; text re-rasterizes only on a phase change,
  quads translate, a gradient's dither is keyed on the position relative to
  the anchor (`ShapeData.dither_origin`). No supersampled capture. Contract
  `effect_semantics.rs` (byte identity after undoing the translation).
- **Animated scale** (`LayerMotion`, `animated_raster_scale`): an isolated,
  cacheable child whose scale changed since the previous frame over the same
  content rasterizes at the next 2^(1/8) step at or above its scale, and
  its composite applies the rest of the transform. A scale animation then
  redraws a surface only when it crosses a step. The first frame the scale
  holds redraws it at its exact scale, so a still frame is byte-identical
  to a fresh renderer's. Raster scales key the cache exactly
  (`RasterScale`): a raster drawn at one scale never serves another. A
  child's source surface is kept on first sight; after a kept surface
  nothing read back, the next one is kept only once its key repeats
  (`AdmissionGate::rendered`). A layer isolated only by its scale or
  rotation is as cacheable as one isolated for alpha or a clip
  (`layer_cache_policy`). Contract `animated_layer_transform.rs`, opaque
  and translucent.
- **Drawn in place** (`ChildLayer::in_place`, `flush_parts`,
  `SegmentTransform`): an isolated layer that only turns and moves -- no
  alpha, blend, effect, backdrop, rounded clip, offscreen strategy, image,
  clip that would cut a text or a child, or content that blends other than
  source-over -- may draw its content straight into its parent's pass as
  segments under its transform, between the parent's ops at its z, its own
  such children composed in. It does whenever its surface is neither cached
  nor admitted this frame (`draws_in_place`), and its source gate
  (`AdmissionGate::drawn_in_place`) admits a surface only once the content
  has held still for `IN_PLACE_PATIENCE` frames: content that holds still
  then composites its cached surface, which costs the GPU less than drawing
  it again, while content that changes, or only pauses at the turn of its
  motion, never renders a surface at all. The viewport uniform carries the transform; only shape pipelines
  built `SHAPE_TRANSFORMED` read it, growing each quad half a pixel so a
  turned edge anti-aliases on both sides and mapping each fragment back to
  evaluate its distance field, so untransformed pipelines compile the
  arithmetic they always had. Glyphs under a transform sample the atlas
  filtered, a scissor is the bounds of what it cuts, and a retained glyph
  run adds its raster origin on the GPU as the shared path adds it on the
  CPU, so the two draw one picture. Forty nested turned levels laid out
  again every frame draw in one pass with no surface. Contracts
  `in_place_layers.rs`, `nested_rotated_relayout.rs`,
  `rotated_grid_relayout.rs`.
- **Surfaces drawn together** (`resolve_flat_children`,
  `render_surface_atlas`): before a layer draws, every child surface it must
  draw this frame whose content is flat (no nested surface, backdrop,
  effect or blurred shadow) and that the cache does not keep is drawn into
  one shelf-packed atlas per raster scale, in one pass, each child
  scissored to its region; a grid of cells laid out again every frame costs
  one pass, not one per cell. A flat child the cache admits is copied out of
  the atlas into its retained texture, never drawn in a pass of its own, and
  a node's new source surface supersedes its old one when it draws other
  content (`draws_other_content`): a cell laid out again replaces its
  surface, while a tile animating its scale keeps its rasters of the scale
  steps it passes through. A projective composite filters in its shader
  (`projective_blit_main.wgsl`) with weights from the position within the
  surface and taps held to the surface's texels, so a surface in an atlas
  composites byte for byte as one in a texture of its own. Contract
  `rotated_grid_relayout.rs`.
- **Stages** (`ResolveStages`, `run_stages`): the page is drawn in strata
  and backdrops resolve in batches; an effect joins the stage after every
  effect below it under its capture; blockers are every backdrop still
  waiting to capture. Captures are shelf-packed copies of the page into an
  atlas that is never cleared (every reader holds its taps to its region:
  `region_map`); what is not yet on the page is drawn in one scissored
  fix-up pass. Substrates (`SubstrateSpec`, `MAX_SUBSTRATES`) are rendered
  beside the atlas, one downsample + horizontal + vertical per stage at
  scratch size packed to the blurred regions; a mean's row and column
  reductions ride in the downsample and horizontal passes of a stage that
  blurs and its texel is carried in the vertical pass, and every result
  with a slot in the atlas is drawn there instead of copied
  (`encode_blur_atlas_passes`, exact: the same draws, other targets). A
  chained substrate's source is copied beside its substrates, never
  blitted. A child page's transparent clear waits for its first draw: a
  capture of an untouched page skips the identity blit and a child
  reading it through its base clears it first (`start_page`). A
  declaration is capture geometry and must not follow a runtime value.
  Every glass is shaded once.
  `plan_stage` lays every stage out before any member is served from the
  cache and each key hashes its own placement. Contracts
  `backdrop_pass_batching.rs` (one full-screen pass, one copy per glass,
  one blur triple per stage, never a pass per glass),
  `backdrop_atlas_parity.rs`, `blur_reference.rs`, `capture_culling.rs`,
  `a_cold_frame_draws_every_row_text_over_its_glass`,
  `a_capture_of_the_page_is_a_copy_that_records_no_pass`,
  `a_shader_reading_its_place_in_the_atlas_is_re_rendered_when_a_neighbour_moves_it`.
- **What a material declares**: `set_output_support` bounds the composite's
  scissor, `set_sample_domain` lets feeding passes leave the rest
  unwritten; both are hashed. The capture never shrinks to the support (a
  moved capture shifts texel coordinates and substrate phase by ULPs), so
  exact reductions keep every origin and size and cut with scissors.
  Contracts `glass_output_support.rs`, `effect_sample_domain.rs`.
- **Fill-shaped geometry**: a record draws as its bounding quad; an arc or
  stroked circle is banded when `band_pays`; strips instance over the
  largest class's index pattern. Band rasterization is not ULP-exact across
  vertex-layout changes, so shader edits are judged by the frozen reference
  below. Contracts `run_geometry.rs`, `band_fill.rs`, `arc_tessellation.rs`.
- **Uploads, passes, pools**: one `ViewportUniformRing` per frame; shape
  runs of `STORE_RUN_MIN_RECORDS` keep retained buffers keyed by
  `DrawCommandId`, smaller runs and shadows use per-pass arena chunks
  (WebGL has only the arena); all queue writes live in `frame_graph.rs`
  (`render_contract.rs`); transients come from `TransientTexturePool`,
  evicted by age, pinned ones skipped until the cache retires them. Shape
  pipelines are one per (blend mode, vertex stage, `ShapeVariant`): a
  variant fixes the shape kind and brush kind of its batch and picks
  `fs_solid` (7 locations), `fs_gradient_fill` (11) or `fs_main` (15);
  every entry ends in the one `fragment` function. Contract
  `shape_variant_parity.rs` (zero bytes; a wrong varying or fixed brush
  fails by 10^5 bytes).
- **Caches** (`LayerCache`, 96 MB LRU, bytes per texture through an
  `AllocationLedger`; the byte budget bounds it, the 4096-entry cap only
  guards the index): retained child layers (`raster_cache.rs`), blurred
  shadows composited as bands, backdrops keyed by node, effect, capture
  size, layout signature and a hash of everything the capture reads
  (`capture_hash.rs`). A backdrop is pinned the first frame its key is seen
  (`AdmissionCost::Pin`, no pass, no ratchet, budget to the longest-held
  keys) and lives exactly as long as its key; a hit replays the composite
  kind at the current placement, never a copied result (a copied result
  rounded twice, 64107979). The opaque prefix (`opaque_prefix.rs`): a
  page's first op that is a plain opaque rect is admitted on its second
  frame by a split first pass and a same-format copy back; on later frames
  a page-covering prefix is copied into the page and the pass loads it, a
  partial one is composited over the clear (`AdmissionCost::Copy`). A
  render effect over a retained child surface is a pure function of the
  surface's content and the effect, so its output is admitted on the
  second frame the same output is wanted (`effect_over_surface`,
  `LayerRasterCacheKey::layer_effect`, `AdmissionCost::Copy`) and read back
  while both hold; an animated effect over still content is drawn afresh.
  Contract `layer_effect_cache.rs`.
- **Nothing drawn, nothing composited** (`composites_nothing`): a child
  that draws nothing whose render effect keeps a transparent source
  transparent (`RenderEffect::preserves_transparency`: blurs, offsets, and
  shaders that declare it, such as a glass content mask) resolves its
  backdrop and no more, since source-over of a transparent source leaves
  the page as it is. Contract `transparent_child.rs`; the liquid tab bar's
  empty surface and lens layers and its unlit lighting
  (`tab_lighting_rest_identity.rs`) cost the frame nothing on that account.
  Reference toggles `CRANPOSE_NO_BACKDROP_CACHE`, `CRANPOSE_NO_FILL_CACHE`.
  Contracts `glass_layer_cache.rs`, `backdrop_atlas_parity.rs`,
  `opaque_prefix_cache.rs` (byte identity at three scales, covering and
  partial, copy count asserted), `layer_cache.rs` unit tests.
- **The glass material**: `liquid_glass.wgsl` is specialized per material
  (`specialize_liquid_glass`): each `LIQUID_GLASS_SPECIALIZATIONS` entry
  names a bool override, the slots it guards and when the feature is
  inactive; a raised flag substitutes the value the uniform holds
  (`fixed_or`), so a fold is exact by construction. A glass draws as two
  pipelines, interior and rim (`GLASS_RIM_DRAW`), the interior skipping the
  rim's terms; `glass_split.rs` scissors the rim to four bands around a
  hole inset by the rim's reach plus the corner's tangent share and the
  interior to one inset rect, on the same quad. Contracts
  `glass_reference_shader.rs` (every scene byte-identical to the frozen
  `tests/fixtures/liquid_glass_reference.wgsl`; a deliberate picture change
  re-freezes it in the same commit), `glass_specialization_parity.rs`
  (folded vs general, split vs whole, at zero bytes), the liquid crate's
  flag-table unit tests, `glass_split.rs` unit tests (the hole lies where
  the rim draw discards).
- **Pipeline compilation** (`pipeline_compiler.rs`, `lazy_resource.rs`,
  `shader_cache.rs`): the driver compiles a glass pipeline in ~100 ms and a
  fixed effect pipeline in ~70 ms (Metal, no disk cache), so no frame waits
  for one it can avoid. One background thread compiles in queue order; a
  `LazyGpuResource` is one shared cell, so a frame arriving mid-compile
  waits for that compile instead of starting another. `GpuRenderer::new`
  queues what the first frame draws first (glyph, image, output and the
  fixed effect pipelines) and the general pipelines of the shipped runtime
  shaders last, since one liquid glass compile is a second on Mali and
  nothing draws it before the first glass screen. The six general shape
  pipelines stay synchronous in `ShapePipelines::new`: the creating thread
  is idle until the first frame, so building them there overlaps the
  compiler's work, where queuing them ahead on the one compiler thread put
  the Mate 20 X's cold first frame at 286-755 ms against 245-294 ms.
  Specialized shape pipelines (Vulkan only) queue on the compiler thread
  behind at most two pending keys, the general shape pipeline drawing until
  each lands. Shaders a widget crate assembles at runtime reach the queue
  through `WgpuRenderer::warm_shaders`, each `ShaderWarmUp` naming its
  `ShaderTarget` (`Page` composites with premultiplied source-over, `Layer`
  renders into the layer with replace) and keeping its overrides, so a mask
  pass warms the pipeline it draws with; every platform registers
  `cranpose_liquid::shader_warm_ups()` (tab lighting, the two vibrancy
  passes) before `init_gpu`, and the list applies again at every later
  `init_gpu` (`shader_warm_ups.rs`). A runtime shader that
  declares its specialization exact (`set_specialization_exact`; liquid
  glass does) has its specializations (override set, interior and rim)
  compiled in the background while its general pipeline draws in their
  place: a fold substitutes the value the uniform holds, so the pictures
  are the same bytes (`glass_specialization_parity.rs`, which also proves
  the first frame draws through the fallback and later frames land on the
  same bytes; Metal's fast-math compile moved one lens-rim pixel by one
  unit between the two, so reference captures settle first). An override
  that picks a different picture, such as vibrancy's mask pass, compiles
  inside the frame that first draws it, once per install. Per-material folds are on for Android only
  (`glass_material_folds_enabled`), so on Android the compiler keeps one
  pipeline pair per material off the present thread, while the desktop
  pays only the fixed set and the general glass pipelines.
  `shader_pipeline_fallback_draws` counts such draws; captures that
  assert per-draw statistics settle on zero first (`settled_capture`).
  `CRANPOSE_BACKGROUND_PIPELINES=0` compiles everything at first use.
  Across launches a shader compiles once per install and once more when
  its source changes; every launch still creates the pipeline objects from
  a cache, off the render thread. Android keeps the driver's compiled
  pipelines in the `pipeline_disk_cache` blob (Pixel Watch 3: ~20 ms per
  glass pipeline from the blob, 650-990 ms cold). Mesa on Linux serializes
  nothing into that blob (Intel ANV 26.2: a 96 B header, the same from
  unpatched CI runs) and keeps its own `mesa_shader_cache` instead, which
  brings a relaunch to ~25 ms per glass pipeline. Metal offers wgpu no
  cache (`PIPELINE_CACHE` is Vulkan-only in wgpu 29 and 30) but macOS
  caches compiled pipelines itself: relaunching an unchanged binary
  compiled the liquid page's 44 pipelines in 25 ms against 731 ms on the
  first launch (2026-09-14). DX12 recompiles HLSL each launch and only the
  driver caches the DXIL to ISA step; browsers cache for the web. A blob
  that stays at 96 B is a driver that serializes nothing, not a broken
  cache.
- **Diagnostics**: `RenderStatsSnapshot` (passes, pass and copy pixels,
  bytes, cache traffic, `shader_pixels`, `glass_rasterized_pixels`,
  `blur_pixels`, `shape_fill_pixels_by_class`,
  `shader_pipeline_fallback_draws`), `CRANPOSE_GPU_PASS_TIMING`
  (the watch has timestamps, Mali does not), `CRANPOSE_GPU_STAGE_DIAG`,
  `CRANPOSE_ABLATE` (`stages`, `glass`, `substrates`, `blur`, `text`,
  `shape`, `shape_fill`, `glass_dispersion`, `glass_refraction`: bounds by
  removal in the same binary), `CRANPOSE_PROBE_PASSES` and
  `CRANPOSE_PROBE_DRAW_PASSES`. Android maps `debug.cranpose.<name>` to
  these in `android_frame_telemetry.rs`; a switch absent from that table
  measures the control.
- **Validation bar**: `just fmt`, `just clippy`, `just test`, `just robot`,
  the pre-commit diff gates. A change touching placement, crispness, pass
  counts or bytes ships with a test proven red by breaking it; a
  performance change ships with a correctness test that fails when the
  optimization is wrong and an A B A B then B A B A device comparison, no
  cooling, every leg kept, thermal crossings labelled and never voided.
  Debug toggles are process-global: a test raises one only while it holds
  the GPU lock.

## The attachment's blend arithmetic

One-off census (a full-screen premultiplied src-over of sub-step RGBA32F
sources over a known destination, readback matched against forty models;
source and both GPUs' logs under the shared root, `blend-census/`).
Adreno 702 converts the source to 8 bits in f32 with round half up, then
blends, exactly; Apple M5 fits no model, its residue is exact ties resolved
above f32 precision. Both leave the attachment untouched under a zero
premultiplied source and store an opaque source as its own conversion.
So no draw is folded into the draw beneath it: the fused header (gradient
blur plus its covering gradient rect) is exact on Adreno only, ceiling
~0.6 ms on the watch. Held.

## Measured

Same APK, work removed by `CRANPOSE_ABLATE`, fps switched minus base per
pair (Mate 20 X base ~41; Pixel Watch 3 base 31-43, plateaus 42-43 cool,
31 at 41-42 C, 24 at 43, 16 at the next step):

| switch off | Mate 20 X | Pixel Watch 3 |
|---|---|---|
| stages | +10.0, +9.8, +11.2, +12.5 | +12.7, +13.5, +21.8, +24.2 (cap) |
| glass as blit | +9.0, +9.5, +12.7, +10.4 | +19.9, +21.2, +20.3, +31.2 |
| substrates | +6.0, +6.1, +1.7, +5.4 | +3.4, +3.5, +3.9, +3.9 |
| header blur | +3.6, +0.7, +3.2, -0.7 | +0.6, +0.8, two legs invalid hot |
| text | -1.0, +2.6, +1.4, +0.1 | unmeasurable (the route validates by OCR) |
| shape (flat colour) | +1.8, +3.0, +1.6, +1.5 | +3.5, +2.3, +11.2 (crossing), +2.9 on 24-27 |
| shape_fill (discard) | +0.5, +3.5, +1.6, +1.5 | +1.3, +1.2, +1.9, +1.3 on 24 |
| glass_dispersion | +3.3, +0.6, -2.4, +2.5 | +1.2..+2.6 within a plateau |
| glass_refraction | -1.8, +1.7, -0.9, -0.8 | +0.8, +0.3, -0.6, -0.0 |
| shape_variants=0 on Orbit | -1.5, -0.6, -1.6, -1.0 on the cap | -10.3, -8.2, -8.2, -7.9; span 146 vs 48-58 ms |
| opaque prefix cache off | -2.37, -2.83, -1.26 (+0.12 first) | -2.51, -1.80, -2.48; Layer Pass 0 4.4-5.5 → 7.4-8.7 ms |
| probe: 8 empty Load/Store passes | not run | 0.01 ms/frame for all eight |
| probe: 8 transparent full-page blits | not run | 16.2 ms/frame = 2.0 ms per page; a tiny-draw pass 0.09 ms |

The stage pipeline is the frame on both GPUs and its material is most of
it; neither glass switch moves either GPU beyond +2.6 within a plateau, so
fetch count and refraction arithmetic are not where glass time goes. The
hot Orbit frame on the watch is the main thread (span 58.5 → 25.7 ms under
`shape_fill` at unchanged fps).

Exact levers, each against the tree without it:

| change | Mate 20 X | Pixel Watch 3 | verdict |
|---|---|---|---|
| opaque prefix e520addf | +2.3, +3.1, +2.8, +4.4 | +2.0 cool; +2.1, +2.2 hot | kept |
| rim style fold 36dab4ae | +1.2, +0.5, +0.8, +2.4 | +5.2, +5.2, +5.0 (one crossing) | kept |
| first-sight backdrop pinning 2b5533bf, 5695f378 | +6.10, +1.56, +1.57, -0.48 | -0.03, +0.59, +0.42, -0.36 | kept: bounded memory, no ratchet |
| gradient-fill shape entry 3d2bd703 | +2.94, +2.70, -1.70, +0.75 | -0.34, +0.57, +0.21, -0.42 | kept: Mali |
| glass scissor split 0a234aa6 | +0.88, +0.29, +0.33, +2.17 | +1.4 on 37, +0.92 cool; Layer Pass 1 -8% | kept |
| prefix copied into the page 1e8329c2 | -1.5, +1.6, -2.1, +0.2 | +1.85, +2.81 on 25-27; Layer Pass 0 4.7-5.9 → 2.4 ms | kept |
| rim hole to the corner tangent 37fb1db1 | +0.20, -3.14, +0.94 (one telemetry-less +6.42) | +0.53, -0.19; Layer Pass 1 unchanged | kept as correct geometry, nil |
| coincident-ray reuse 0d63a76f | -1.4, -0.2, +1.7 (one outlier) | -0.0, -0.1, -0.2 | reverted: taps are free |
| activity flag 22aece9d | +1.4, +0.7, -0.5, +1.6 | +0.1, +0.4 (two crossings) | reverted |
| default curve fold 81af46dc | -0.8, -0.9, -4.1, +1.6 | not run | reverted |
| curve as constant d82d86a8 | +2.7, +2.3, +3.3, +2.0 | not run | attribution only |
| shared channel walk (a channel whose clamped interior equals the base channel's takes the transmitted path; two `channel_lens_displacement` evaluations and two taps skipped across the face) | +0.02, +1.96, +0.38, -0.54 | +0.46 on the 28 plateau, Layer Pass 1 12.09/11.65 → 11.99 ms; the run crossed 52 → 40 → 28 from a cold start | not adopted: exact on both GPUs, nil |
| tab bar zero-output work, PR #671 (a child that composites nothing is skipped, a render effect over a retained surface is cached, unlit lighting is omitted); demo Liquid tab, present cycle p50 29 → 20-25 ms | +7.5, +5.1, +5.3, +9.9 | not run | kept: 31 → 21 passes per scroll step, byte-exact |
| stage side passes folded, PR #671 (means ride in the blur passes, substrates land in their atlas slots, a chained substrate's source is copied, a child page's clear waits for its first draw); demo Liquid tab, present cycle p50 25 → 20-25 ms | +0.9, +1.9, +1.8, +3.8 | not run | kept: 21 → 19 passes and 13 → 11 copies per scroll step, byte-exact on the second run (the first run after a rebuild draws up to 13 frames through the compile-time fallback pipelines) |

Legs: `<label>-<device>-<n>-<arm>/` under the shared root, one `report.json`
and `logcat.txt` each; pass rows by `pass_timing_from_logcat.py`.

## Rejected and held

- Folding a draw into the draw beneath it: see the blend arithmetic.
- Per-pixel branches around cache-hot fetches (7f306f6b): lost every stable
  watch pair; Adreno pays more for the branch than the fetch.
- Declaring no substrate for a resting glass (54376db3): the declaration is
  capture geometry; five Adreno pixels one level apart.
- Restricting a capture to the support: 99 pixels one level off. Scissors only.
- Skipping the frost substrate where the correction is zero: not exact.
- The refraction curve as a value-carrying constant: keys a pipeline per
  float. Attribution only.
- Page-anchored substrate grid (blur phase change): refused, picture bar.

## Next lever

The dispersion path is closed as a lever: the coincident-ray reuse (taps)
and the shared channel walk (the two extra displacement evaluations, exact
on Metal and natively on Adreno, tolerance mutant red at 7,374 px) were
both nil on both GPUs, so neither the face's repeated fetches nor its
repeated arithmetic is where glass time goes; the rim band and the frost
substrate reads remain, and they are the picture.

Every priced exact idea is under 1 ms (shadow tails are not
exactly zero, the interior scissor's corners are ~2k px, text is tiny,
pass floors sum to ~1.3 ms). The cuts that reach toward 16.7 ms change the
picture or the app: the substrate grid's phase, a cheaper glass material,
fewer nested glass layers. Open designs that stay exact: a bounded gate
carrying a material constant as a pipeline override after it repeats
(+6% on Mali by the probe), an unread substrate skipped without a
geometry change.
