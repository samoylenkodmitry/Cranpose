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
  projectively. Contract `effect_semantics.rs`.
- **Rigid motion**: a translated context carries one `SnapAnchor`, one
  device-pixel delta per frame; text re-rasterizes only on a phase change,
  quads translate, a gradient's dither is keyed on the position relative to
  the anchor (`ShapeData.dither_origin`). No supersampled capture. Contract
  `effect_semantics.rs` (byte identity after undoing the translation).
- **Stages** (`ResolveStages`, `run_stages`): the page is drawn in strata
  and backdrops resolve in batches; an effect joins the stage after every
  effect below it under its capture; blockers are every backdrop still
  waiting to capture. Captures are shelf-packed copies of the page into an
  atlas that is never cleared (every reader holds its taps to its region:
  `region_map`); what is not yet on the page is drawn in one scissored
  fix-up pass. Substrates (`SubstrateSpec`, `MAX_SUBSTRATES`) are rendered
  beside the atlas, one downsample + horizontal + vertical per stage at
  scratch size packed to the blurred regions; a declaration is capture
  geometry and must not follow a runtime value. Every glass is shaded once.
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
  `AllocationLedger`): retained child layers (`raster_cache.rs`), blurred
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
  partial one is composited over the clear (`AdmissionCost::Copy`).
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
- **Diagnostics**: `RenderStatsSnapshot` (passes, pass and copy pixels,
  bytes, cache traffic, `shader_pixels`, `glass_rasterized_pixels`,
  `blur_pixels`, `shape_fill_pixels_by_class`), `CRANPOSE_GPU_PASS_TIMING`
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
