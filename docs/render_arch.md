# Renderer Architecture

How a `RenderGraph` becomes pixels in the WGPU renderer, as it is now. The
pixel-stability contract every rule serves is
[liquid_scroll_pixel_stability.md](liquid_scroll_pixel_stability.md). Each
rule names the test that fails without it; a rule without a test is not a
rule. Numbers live in the last three sections only.

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

The swapchain pass never reads itself. A backdrop effect resolves from a
capture (the ops beneath it, clipped to its device rect, on a texture) and
filters that into its result; an isolated child renders into its own
texture through the same steps recursively; the final pass draws direct
ops and composites the resolved textures.

## Direct or isolated

`collect_child` (`child_placement`) makes a layer **direct** when its
transform is a translation, it needs no group alpha, blend mode, render
effect or explicit offscreen, and its rounded clip admits every op inside
it (`content_admits_rounded_clip`: device bounds plus one pixel stay out of
the corner squares); direct ops join the parent's flat scene with the
translation applied. Everything else is **isolated**: a `ChildLayer`
composited at its z with alpha, blend, transform and rounded mask, its
content collected into its own `LayerScene`. The root goes through the
same function.

An isolated child under a uniform scale plus translation renders on the
parent's pixel grid (surface = device rect snapped outward, the fractional
remainder as target offset, a one-to-one nearest composite); `LayerCache`
keys carry the 1/16-pixel device phase. Any other transform composites
through the projective path. `effect_semantics.rs` pins both against the
same box drawn directly.

## Rigid motion

A translated content context carries one `SnapAnchor` for its subtree: one
device-pixel delta per frame, the subtree moves as one raster. Text
rasterizes at a canonical device origin and re-rasterizes only when its
device phase changes; images and shapes translate their quads; a
gradient's ordered dither is keyed on the device position relative to the
anchor's snapped origin (`ShapeData.dither_origin`). Outside a context a
layer snaps when its own primitives are pixel-sensitive
(`layer_needs_rigid_snap`); an isolated child also snaps when a
translating descendant draws text or images
(`layer_has_pixel_sensitive_subtree`). There is no supersampled
"motion-stable capture". Contract in `effect_semantics.rs`: after undoing
the rounded translation the local picture is byte-identical (translated
text, thin shapes, static alpha surface, translated gradient).

## Stages

A tiler pays per pass whatever the pass draws, and a capture that redraws
the page under every glass pays the page's fill per glass; so the page is
drawn progressively and backdrops resolve in batches (`ResolveStages`,
`run_stages` in `frame.rs`).

- **Membership.** An effect joins the stage after every queued effect
  below it whose composite lies under its capture; every capture of a
  stage reads only composites of earlier stages.
- **Strata.** A layer's page (`LayerPass`) is drawn in strata: before a
  stage captures, the page is flushed up to the stage's last glass
  (`flush_page`). What a flush would draw is released in z order
  (`LayerPass::release`): an op touching a blocker below it waits, a
  composite draws outside the blockers it overlaps with the covered parts
  pending, whatever waits blocks in turn. Blockers are every backdrop
  still waiting to capture, this stage's and every later one's, and a
  stage's blockers drop once it has captured
  (`a_cold_frame_draws_every_row_text_over_its_glass`, red before the
  blockers covered later stages).
- **Captures.** Shelf-packed edge to edge into an atlas. A region of the
  layer's own page is a `copy_texture_to_texture` of the page (`Page::copy`,
  no pass: `a_capture_of_the_page_is_a_copy_that_records_no_pass`). What
  is below a region's z and not yet on the page (a deferred op, a pending
  composite part) is drawn over the copies in one scissored fix-up pass,
  recorded only when some region needs it (`segment_draws_anything`,
  `capture_fixup_passes`; `a_capture_adds_no_shape_fill_of_its_own`). An
  isolated child reading its backdrop draws its regions from the parent's
  page (`PageBase`, re-based by the child's translation or projected
  through its transform). The atlas is never cleared: every reader holds
  its taps to its region's texel centres (`region_map` in
  `liquid_glass.wgsl`, `gradient_blur.wgsl`), so a neighbour's or a pooled
  texture's texels are never read. A region draws only what reaches into
  it (`capture_culling.rs`).
- **Substrates.** A batched shader declares up to `MAX_SUBSTRATES`
  low-frequency inputs (`SubstrateSpec::Blur { radius_px }` and averaged);
  the stage renders each into a side slot beside the atlas
  (`stage_side_regions`, `substrate_size`) and hands the shader the slot
  (`substrate_map`, `has_substrate`). A declaration expands the member's
  capture by the blur margin: it is capture geometry, so it must not
  follow a runtime value (`a_resting_glass_keeps_its_substrate_declaration`,
  `a_resting_frosted_glass_reads_no_substrate`; see Rejected).
- **Blur.** One downsample plus one horizontal and one vertical pass per
  stage over the blurred regions at the scratch size, packed to those
  regions alone (`substrate_scratch_size`, `encode_blur_atlas_passes`);
  kernel weights computed once per draw; contract `blur_reference.rs`
  (CPU reference within one step) and `backdrop_atlas_parity.rs` (a blur
  exact alone and packed).
- **Composites.** A blur blits its region through the rounded mask; a
  runtime shader with `batched_source` draws in its stratum reading its
  region, mask and alpha through reserved uniform slots. Every glass is
  shaded once (`shader_pixels`; a glass under two buttons shades its own
  pixels only, `backdrop_atlas_parity.rs`). A shader-only isolated child
  with no content of its own composites over a shared transparent input
  and costs no pass.
- **Budget.** `backdrop_pass_batching.rs`: one full-screen pass, one copy
  per glass and one blur triple per stage, one fix-up under a glass, one
  stratum per stage, never a pass per glass.
- **Layout.** `plan_stage` plans every stage over all its items before any
  is served from the cache: placements, substrate slots, side packing.
  Misses take their slots from it (`StageLayout::restrict`); each member's
  key hashes its own part (`StageLayout::signature`: placement, padded
  atlas and side sizes, substrate and blur slots). Members pack in z order;
  atlas and side sizes pad in steps of an eighth of the next power of two
  (`padded_dimension`, bounded by the packer's limit, 16-texel floor), so
  a co-member's size change rarely moves a slot. An atlas is allocated only
  when a miss lands in it
  (`a_stage_spanning_two_atlases_allocates_only_the_atlas_its_misses_land_in`).
  Proof that holds on every sampler:
  `a_shader_reading_its_place_in_the_atlas_is_re_rendered_when_a_neighbour_moves_it`
  (a probe shader paints its region and `textureDimensions`; red without
  the layout in the key). Commit cc8a420b.

## What a material declares

`RuntimeShader` carries two rects with a default of "everything":
`set_output_support` (outside it the shader writes nothing; bounds the
composite's scissor) and `set_sample_domain` (outside it the shader never
samples; lets the feeding passes leave the rest unwritten, each widened by
the reach of the pass that reads it, whole-axis under `TileMode::Repeated`).
Both enter `hash_runtime_shader`. The capture rect never shrinks to the
support: a capture that starts elsewhere moves the shader's texel
coordinates by ULPs and the substrate's block phase, so exact reductions
keep every texture's origin and size and cut work with scissors.
Contracts: `glass_output_support.rs` (a 6 px short support differs by 9
pixels), `effect_sample_domain.rs` (a far-corner fetch under a pruned blur
differs by thousands; the whole-axis rule under Repeated). The liquid
material declares its support (primary shape widened by every reach the
shader subtracts, strain applied by the affine rows) and no sample domain.

## Fill-shaped geometry

A shape record draws as its bounding quad and the shape shader decides
coverage; an arc or stroked circle is banded when `band_pays` says a strip
costs less than the quad (pixels plus `BAND_VERTEX_PIXELS` per vertex).
A band keeps the angular step of a full ring at its radius
(`ARC_RING_SEGMENTS`), takes the segments its padded sweep needs rounded up
to a power of two (`ARC_BUCKET_SEGMENTS`, 1..64), and a draw instances its
records over the strip index pattern of its largest class
(`strip_index_pattern`, `SEGMENT_WASTE_QUADS`). The vertex stage builds
the strip (`band_position`; the record carries `arc_trig` and `BandRing`);
a vertex past a record's own pins onto its last; the fragment stage
discards outside the record's rect; no band is one quad wide
(`BAND_MIN_SEGMENTS`). Contracts: `run_geometry.rs` (the strip holds every
pixel centre the SDF shades), `band_fill.rs` (a ring costs its band; a
banded ring matches two clipped halves byte for byte),
`a_translucent_quad_sharing_a_draw_with_a_wide_ring_blends_once`,
`arc_tessellation.rs` (pixels independent of tessellation; coverage against
`tests/fixtures/arc_distance_reference.wgsl`). Band rasterization is not
ULP-exact across changes of vertex layout (`band-geometry` note in
`TIME_WASTERS.md`), which is why shader edits are judged by the frozen
reference below and not by re-tessellation.

## Uploads, passes, pools

- `ViewportUniformRing`: one uniform buffer with dynamic offsets, written
  once per frame; every pass and retained glyph run claims a slot.
- Shape records draw from the run store (`run_store.rs`): a run of at
  least `STORE_RUN_MIN_RECORDS` records keeps retained buffers keyed by
  `DrawCommandId`, rewritten only on other bytes or paint; smaller runs and
  shadows go to per-pass arena chunks; the WebGL floor has only the arena.
  Image batches use per-frame slot pools (`image_slots`).
- All queue writes and encoders live in `frame_graph.rs`
  (`render_contract.rs`).
- Transient textures (captures, blur scratch, child surfaces) come from
  `TransientTexturePool`, released at the submit boundary, kept in release
  order and evicted by age (`transient_pool.rs`; the pool once never
  evicted). A texture pinned by the layer cache is skipped by
  `release_transients` and returns to the pool under its descriptor when
  the cache retires it (`take_released`).
- Shape pipelines: one per (blend mode, vertex stage, `ShapeVariant`);
  batches cut by blend mode, brush table and brush class only
  (`shape_variant_parity.rs`). A variant fixes the segment's uniform shape
  kind (`SHAPE_KIND_FIXED`) and uniform brush kind (`BRUSH_KIND_FIXED`,
  from `RecordSegment::brushes`), and picks its entry pair by what the
  batch carries: `fs_solid` (no brush, 7 locations), `fs_gradient_fill`
  (brushed fills: no colour, stroke or arc vectors, 11 locations), else
  `fs_main` (15). Every entry ends in the one `fragment` function, so a
  variant only folds branches the batch cannot take; the parity contracts
  hold at zero bytes on Metal, and a wrong varying or a wrong fixed brush
  fails them by 10^5 bytes (proved 2026-09-06, restored). Runtime-shader
  pipelines are keyed by source, overrides and paddings
  (`hash_runtime_shader`).

## Caches

- Glyph atlas, glyph mask cache, retained glyph runs; image textures.
- Blurred shape-only shadows, keyed by content and anchored placement,
  composited as up to four bands in the final pass; blurred drop shadows
  are queued as pending composites of the cached texture so captures
  replay them instead of flushing the run.
- `LayerCache` holds `Retained { texture, content }`; content is `Surface`
  (a retained child layer or an opaque prefix, drawn whole for its entry)
  or `Composite(kind)` (an admitted backdrop). Budget 96 MB, LRU by entry,
  bytes charged per texture through an `AllocationLedger` (a stage
  texture shared by its backdrops is paid once and retires once, when its
  last holder leaves). Oversized entries are refused (`fits`); a pending
  retirement is revived by a new holder (`layer_cache.rs` unit tests).
- Isolated children that read no backdrop and whose `cache_policy` is
  `Auto`: keyed by content hash, size, scale bucket and 1/16-pixel phase;
  the texture holds the content before its effect (`raster_cache.rs`).
- Backdrops: keyed by node, effect, capture size, the layout signature and
  a hash of everything the capture reads (`capture_hash.rs`, geometry
  relative to the capture; composites by what their texture holds through
  `SourceContent::Retained(hash)` / `Transient`, never by pointer). A key
  is admitted by its `AdmissionGate` the first frame it is seen: the
  admitting frame draws the composite it would have drawn anyway and pins
  its sources (the stage texture, the substrate or blur slot) with the
  composite's resolved kind. A pin costs no pass, so the gate never waits
  and never ratchets (`AdmissionCost::Pin`; the prefix gate is
  `AdmissionCost::Copy` and keeps its one-frame wait with doubling); the
  per-frame pixel budget goes to the longest-held keys first, so a key
  changing every frame cannot starve a still one. A pin lives exactly as
  long as its key is the gate's current key: the gate hands it back the
  frame the key changes or the node vanishes and the cache releases it to
  the transient pool at once (`AdmissionGate::observe` / `dead_entry`,
  `LayerCache::remove`); a re-pin costs nothing, so no pin is kept for a
  recurrence, and an animating backdrop holds one stage texture, never the
  cache's budget (the Mac count runner held 160 entries at the 96 MB
  budget when read pins were left to the LRU). A copy stays for the LRU
  and is handed back only when nothing read it. Because a stage is keyed
  after the stage below is admitted, a stacked header is fully keyed in
  its first frame. Measured on the Mac count runner (204 px, still header,
  per-item stage diag): uncached items per frame 6.00 to 4.68 in the still
  header and 0.60/0.40 to 0.43/0.17 in the scroll tail, with the cache at
  3 to 7 entries (4 MB) against 7 to 32 before and no steady-state
  allocation; before, 129 of 300 stage-0 misses carried the previous
  frame's key and every stage-1 and stage-2 item was unkeyed behind a
  transient; after, no miss at any stage carries the previous frame's key,
  and the remaining stage-1 and stage-2 keys change every frame with the
  content drawn between the stages. A hit replays that kind at the backdrop's
  current dest, scissor, mask and layer rect (`replayed_kind`): same
  texels, same shader, same uniforms, so the bytes are the never-caching
  renderer's. No result is ever copied (a copied result rounded twice and
  landed a level off; commit 64107979). `CRANPOSE_NO_BACKDROP_CACHE` is the
  tests' never-caching reference. Contracts, all at exact zero against
  that reference: `glass_layer_cache.rs` (still scene misses nothing and
  runs no blur; animated overlay misses itself only; rigid scroll keeps
  the rows; a change beneath a still glass reaches the pixels, cool,
  after, settled and two warm frames), `backdrop_atlas_parity.rs` (nine
  glasses settled, one leaving, one joining, order rotated; independent
  glasses admitted over frames).
- Opaque prefix (`opaque_prefix.rs`, commit e520addf): when a page pass's
  first op is a plain opaque rect (solid, or Clamp gradient with opaque
  stops; whole-pixel device edges computed as the vertex stage computes
  them; clip contains it; no composite at or below its z), its key is
  `LayerRasterCacheKey::prefix_snapshot` over the record, brush, stops,
  placement, snap, clip, scale bits, clear colour, format, rect and page
  origin. Admitted on the second consecutive frame: the page's first pass
  splits, the rect is drawn alone and copied back (same-format copy), and
  later frames bring the copy back ahead of the run's remaining records
  (`PassSegment::first_run_window`): a prefix that covers the page is
  copied into the page and the pass loads it, one that does not is
  composited at the same integer rect over the pass's clear. The copy
  exists because a blended full-page quad is a page of fragments (2.0 ms
  on the watch, see the probe row) while a copy command is not a pass and
  fits with the frame's captures in the span outside the pass rows.
  Exact because the bytes are opaque, same-format and unmasked, so
  src-over is the identity and the copy is the same texels. `CRANPOSE_NO_FILL_CACHE`
  (`debug.cranpose.no_fill_cache`) is the reference toggle;
  `opaque_prefix_cache.rs` requires byte identity at three scales and
  refuses every disqualifier (each requirement red-proven).

## The glass material

`liquid_glass.wgsl` is compiled per material through
`specialize_liquid_glass` (`liquid_glass.rs`): every entry of
`LIQUID_GLASS_SPECIALIZATIONS` names a `bool` override, the uniform slots
it guards and the predicate under which the feature is inactive; a raised
flag substitutes through `fixed_or` the value the uniform already holds,
so the fold is byte-exact by construction, and re-specialization clears a
flag whose feature became active (`clear_override`; commit 37bd0ce8). The
renderer draws a glass as two pipelines, interior and rim
(`GLASS_RIM_DRAW`), the interior skipping the rim's terms and fetches
under the interior guard. Flags: loupe, fold, scene shapes, wobble,
ellipse blend, strain, zoom anchor, touch, content mask, optical blur,
shadow, zoom, physical refraction, full transmission, dispersion, adaptive
frost, ink, interior guard, and rim style (`GLASS_RIM_STYLE_OFF`, commit
36dab4ae: every `Glass::regular()` and `Glass::clear()`; a lens keeps it
live). The adaptive frost declares the blur substrate whatever the
activity (see Stages).

Contracts. `glass_reference_shader.rs` renders each scene with the frozen
`tests/fixtures/liquid_glass_reference.wgsl` and the shipped shader and
requires zero differing pixels; the fixture declares every override and
reads none of the folded ones, so it computes from the uniforms while the
shipped shader folds, and a wrong fold is red (rim style raised for a
lens: 7,227 px). Scenes: cards on the page and child paths, at 1.5x and
0.75x, lenses with blur and substrates, a lens-variant card with a resting
card, and the resting-substrate pair (a positive frost uniform with
activity 0 set on the shader and re-specialized, the control asserted to
declare its substrate before rendering). `glass_specialization_parity.rs`
compares the folded pipeline with the general one byte for byte. The
liquid crate's unit tests pin the flag table (every override declared,
every guarded slot read through its flag, the plain pane raises every
flag, re-specialization matches a fresh shader, a resting glass keeps its
substrate declaration).

## Diagnostics

- `RenderStatsSnapshot`: passes and pass pixels, copies and texels,
  transient and retained bytes, uploads, isolated renders, layer-cache
  traffic, blur passes and `blur_pixels`, composites, `shader_pixels`,
  `shape_fill_pixels_by_class`, `shape_verts`, `capture_fixup_passes`.
- `CRANPOSE_GPU_PASS_TIMING` (timestamps where the adapter has them; the
  watch does, Mali does not), `CRANPOSE_GPU_STAGE_DIAG`,
  `CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS`.
- `CRANPOSE_ABLATE` (`debug.cranpose.ablate`, a comma list of `stages`,
  `glass`, `substrates`, `blur`, `text`, `shape`, `shape_fill`,
  `glass_dispersion`, `glass_refraction`; `ablation.rs`) removes one
  kind of renderer work in the same binary;
  `shape` makes every shape fragment its vertex colour inside its clip
  (blend kept, coverage and brush removed) and `shape_fill` discards it.
  Both are bounds on the fragment path, not a subtraction: `shape` also
  writes every pixel the coverage would have discarded, and a fragment
  that only discards lets the compiler drop the varyings and vertex work
  feeding it. Read them beside the vertex and fill counts. The two glass
  names force an existing exact fold (`GLASS_DISPERSION_OFF`,
  `GLASS_PHYSICAL_REFRACTION_OFF`) onto every glass pipeline through the
  shader cache's key, so the switched material is exactly the material
  with that property at zero and nothing else (`glass_reference_shader.rs`
  proves the dispersion one at zero pixels against the dispersion-zero
  twin). The parsed set is logged on change and every 600 frames while a
  switch is on. `CRANPOSE_NO_FILL_CACHE`,
  `CRANPOSE_NO_BACKDROP_CACHE`, `CRANPOSE_NO_EFFECT_DOMAINS` are the
  reference toggles. Android maps `debug.cranpose.<name>` properties to
  these in `android_frame_telemetry.rs`.

## Validation bar

`just fmt`, `just clippy` (and the target recipes), `just test`,
`just robot`; the pre-commit hook runs the diff gates. A renderer change
that touches placement, crispness, pass counts or bytes ships with a test
that fails without it, proven red by breaking the change on purpose. A
performance change ships with a correctness test that fails when the
optimization is wrong, and with a same-route A B A B then B A B A device
comparison, no cooling, every leg retained including thermal transitions;
the exactness bar is byte identity against a never-caching render of the
same build, on Metal, Linux and the acceptance GPU (Adreno 702), whose
sampler rounds where the others do not.

## The attachment's blend arithmetic

Measured with a one-off census (a full-screen premultiplied src-over of
sub-step RGBA32F sources over a known destination, readback matched against
forty arithmetic models; source and both GPUs' logs under
`/tmp/cranpose-mobile-watch-60fps/blend-census/`): Adreno 702 converts the
source to 8 bits in f32 with round half up and then blends, exactly; Apple
M5 fits no model, its residue is exact ties resolved above f32 precision.
Both hold the identities the renderer relies on: a zero premultiplied
source leaves the attachment byte for byte, an opaque source stores as its
own conversion. So a draw is never folded into the draw beneath it: the
fused header (gradient blur plus its covering gradient rect) would be exact
on Adreno only, with a ~0.6 ms ceiling on the watch. Held.

## Measured (showcase full-scroll route, 60 s legs, 40 gestures)

By removal, same APK (`CRANPOSE_ABLATE`), fps switched minus base per pair:

| switch off | Mate 20 X (base ~41) | Pixel Watch 3 (base 31-43) |
|---|---|---|
| stages | +10.0, +9.8, +11.2, +12.5 | +12.7, +13.5, +21.8, +24.2 (cap) |
| glass as blit | +9.0, +9.5, +12.7, +10.4 | +19.9, +21.2, +20.3, +31.2 |
| substrates | +6.0, +6.1, +1.7, +5.4 | +3.4, +3.5, +3.9, +3.9 |
| header blur | +3.6, +0.7, +3.2, -0.7 | +0.6, +0.8, two legs invalid hot |
| text | -1.0, +2.6, +1.4, +0.1 | unmeasurable: the route validates by OCR of text |
| shape (flat colour) | +1.8, +3.0, +1.6, +1.5 | +3.5, +2.3, +11.2 (plateau crossing), +2.9 on base 24-27 at 42-43 C |
| shape_fill (discard) | +0.5, +3.5, +1.6, +1.5 | +1.3, +1.2, +1.9, +1.3 on base 24 at 42-43 C |
| probe: 8 empty Load/Store passes | not run | 0.01 ms/frame for all eight (elided or free); fps +1.4, +0.4 |
| probe: 8 passes each blitting a transparent texel over the page | not run | 16.2 ms/frame = 2.0 ms per full-page pass; span 39.6 → 55.3, fps 24.2 → 17.8 at 42 C. The frame's own rows: a populated pass with tiny draws 0.09 ms, a blur pass ~0.4 ms, so the pass floor is ~0.1 ms and 2 ms is the blit's 0.2 MP of fragments. Pass merging is not a lever; taps × pixels is, even at one tap |
| glass scissor split (`glass_split.rs`): rim as four bands around a hole inset by the rim's reach plus the corner radius, interior as one inset scissor, same quad and interpolation | +0.88, +0.29, +0.33, +2.17 at 31-33 C | 37 fps plateau at 41-42 C: whole 37.00/37.01 vs split 38.37 (+1.4), span 24.7/24.8 → 23.6 ms, Layer Pass 1 8.85/8.88 → 8.12 ms; cool 48 plateau +0.92; the second pair (48.84 → 37.08, −11.77) crossed the 48→37 thermal step and stays as data, thermally confounded; one split leg ran 3 of 17 swipes (dropped input) and its pair is void. Exact: split vs whole zero bytes; interior inset past the rim diverges 51,345 bytes; a hole ignoring the corner radius fails the geometric invariant |
| glass_dispersion (two of four fetches) | +3.3, +0.6, -2.4, +2.5 at 39-42 C | +2.2, -10.0, -7.2, +13.4 across the 36 and 24 plateaus; +1.2..+2.6 within one |
| glass_refraction (physical refraction arithmetic) | -1.8, +1.7, -0.9, -0.8 | +0.8, +0.3, -0.6, -0.0 on base 24.4 at 42.5 C |
| shape_variants=0 on Orbit (general 15-location pipeline for solid records) | -1.5, -0.6, -1.6, -1.0 on the 60 Hz cap, no spans | -10.3, -8.2, -8.2, -7.9 on 14.6-17.1; span 146-147 ms against 48-58 at 43-45 C |
| opaque prefix cache off (debug.cranpose.no_fill_cache=1: the page's opaque prefix drawn from its records every frame instead of one blit of the cached texture) | -2.37, -2.83, -1.26 on base 37.3-37.6 at 33-34 C; first pair +0.12 | 24-27 plateau at 42.6-43.2 C: -2.51, -1.80, -2.48; Layer Pass 0 4.43-5.45 ms/frame cached vs 7.35-8.74 uncached, span 34.2-36.8 vs 38.7-39.6 ms; the first pair (+7.03) is a 17.50 fps cache leg run straight after another owner's hot legs (Layer Pass 1 15.8 ms, Layer Pass 0 11.8), thermally confounded. The prefix's records cost 2.5 blit-pages of fragments, so the cache stays: the one-tap blit is the cheaper side on both GPUs, refuting the guess that a page of blit costs what a page of gradient does |

The stage pipeline is the frame on both GPUs and its material path is
nearly all of it; the material-to-blit switch removes arithmetic and
fetches together and names the path, not which. The shape fragment path
is a tenth of the watch frame and a twentieth of the Mali one; on Adreno
the discard-only bound gains less than the flat write, so the two are
bounds on that path and not its parts. Orbit on the Mate 20 X sits on the
60 Hz cap under both (59.9 either way); on the watch at 43-45 C Orbit
gives shape +1.0, +2.0, +2.1, +2.1 and shape_fill +2.0, +2.1, +2.8, +2.3
on a 17 fps base, so discarding every shape fragment leaves 84-88% of
that frame, a residual the fps alone cannot split between the GPU's
vertex work and a main thread at 37.7 CPU ms per frame. Neither glass
switch moves either GPU beyond +2.6 within a thermal plateau: the
material's fetch count and its refraction arithmetic are not where its
time goes. Watch Showcase pass timing under `shape` (44 C, 310 MHz):
the 4 ms come out of layer passes 2, 3 and 4, and layer pass 1, the glass
cards at 22 ms of a 60 ms span, does not move. Watch Orbit pass timing
under `shape_fill` (44-45 C, 310 MHz): the one layer pass falls from
58.5 to 25.7 ms per frame and the frame rate stays at 14.5-15.0, so the
hot Orbit frame is the main thread, not the GPU; of the GPU's 58 ms, 33
are the fragment side. Exact levers, each
against the tree without it:

| change | Mate 20 X | Pixel Watch 3 |
|---|---|---|
| opaque prefix e520addf | +2.3, +3.1, +2.8, +4.4 | +2.0 cool; +2.1, +2.2 hot |
| rim style fold 36dab4ae | +1.2, +0.5, +0.8, +2.4 | -4.6 (crossing), +5.2, +5.2, +5.0 |
| activity flag 22aece9d (reverted b297137b) | +1.4, +0.7, -0.5, +1.6 | -6.4 (crossing), +0.1, +0.4, +8.4 (step) |
| default curve fold 81af46dc (reverted ebdd15ea) | -0.8, -0.9, -4.1, +1.6 | not run |
| curve as constant, probe only d82d86a8 | +2.7, +2.3, +3.3, +2.0 | not run |
| coincident-ray reuse 0d63a76f (exact on Metal and Adreno, mutants red; reverted, no return) | -4.7 (43.97 outlier base), -1.4, -0.2, +1.7 | -0.0, -0.1, -0.2, -6.1 (crossing) on base 24.2-24.5 at 42 C |
| prefix copied into the page 1e8329c2 (a page-covering cached prefix is copied into the page before its first pass loads, in place of a blended full-page composite) | -1.5, +1.6, -2.1, +0.2 on base 37-40 at 33-34 C: nil on Mali | +1.85, +2.81 on the 25-27 plateau at 42.8-43.0 C; span 34.8 → 31.3 and 37.7 → 32.0 ms, Layer Pass 0 4.7-5.9 → 2.4 ms, so the blended page quad cost what the probe said; one copy leg fell to the 18 plateau at 43.3 C (17.98 vs 25.72, Layer Pass 0 9.6 against that plateau's 11.8, kept as thermally confounded); one copy leg void, its preflight return never reached the start text |
| rim hole inset to the corner tangent 37fb1db1 (hole inset rim_high + (corner − rim_high)·(1 − 1/√2) instead of rim_high + corner; a showcase card's hole grows from an 11 px sliver to about 350×90 px) | +0.20, −3.14, +0.94 on base 37-40 at 32-34 C, plus a +6.42 leg whose in-app telemetry never arrived: nil | +0.53, −0.19 on the 27-28 plateau at 42.7-43.3 C; Layer Pass 1 11.85/12.03 → 12.12/11.62/11.64 ms; first pair crossed 35 → 27 (thermal step), last pair's control fell to 18.34 (18 plateau), both kept as confounded. Nil: the rim draw's deep-interior fragments exit early, so the rim's cost is its band. Kept as the correct inscribed rectangle under the same SDF invariant, not for speed |

Watch plateaus: 42-43 fps cool, 31 at 41-42 C, 24 at 43, 16 at the next
step; every GPU reduction moves the plateau.

Interface: `shape.wgsl` crosses 14 locations (56 components) into the
general fragment, 11 (44) into `fs_gradient_fill` and 7 (28) into
`fs_solid`; main crosses 15. wgpu-types 30 allows 16 by default and 15 on
downlevel and WebGL2. naga's GLSL of a variant declares all of its
entry's locations: the pipeline constants fold branches and never the
interface, so narrowing needs an entry of its own; the showcase's page
fills are radial and its arena fills linear, and both now take the
gradient-fill entry. Main reads its 102-shape and 256-stop uniform arrays
in the vertex stage only; neither branch reads a record in the fragment,
and WebGL2 has no storage buffer to read one from.

Showcase header, from the Mac runner at watch size (its stats line equals
the device's): the search bar, chips and button are glass folded to one
or two live features; the top bar is the gradient blur (three substrates
at 36, 18 and 9 px) whose 36 px reach captures the search bar and forces
the second stage; the button's capture over that chip forces the third.
At rest the three stage-0 items are keyed but the app's 50 ms ambient
steps change the content beneath them every one or two frames, and the
items above them are never keyed over transient composites; the scroll
cannot hit in any case. Legs and reports live under
`/tmp/cranpose-mobile-watch-60fps/` and the session scratchpads.

## Rejected and held, with the reason

- Per-pixel branches around cache-hot fetches (7f306f6b, reverted in
  1581e056): exact everywhere, lost every stable watch pair (-2.2, -1.8,
  -2.0); on Adreno a divergent branch around a fetch costs more than the
  fetch and stops the compiler issuing fetches early.
- Declaring no substrate for a resting glass (reverted in 54376db3): the
  declaration is capture geometry. A resting glass rendered with and
  without the declaration differs by five Adreno pixels one level apart
  (the first test's two arms); the predicate itself never changed a
  material-built glass, whose frost is already zero at activity 0, and
  the omission mutant on the explicit test fails the declaration
  assertion, not pixels. The exact removal keeps the geometry and skips
  only the blur passes, a planner contract not designed.
- The refraction curve as a value-carrying constant (branch
  `render/curve-probe`, d82d86a8): `refraction_curve * activity` is a
  public, animatable value, so every distinct float would key a pipeline
  (`a_value_carrying_curve_override_keys_one_pipeline_per_curve_value`).
  Attribution only.
- Activity flag 22aece9d (reverted in b297137b): exact, static (two
  `select`s on pipeline constants made the interior's plain fetch dead
  code; the interior claim needs the activity condition, 34 px on Metal
  and 257 on Adreno without it); speed mixed with one Mali pair down at
  equal temperature. Not accepted.
- Default refraction-curve fold 81af46dc (reverted in ebdd15ea): exact
  (the fold raised for a 0.62 card differs by 8,926 to 34,075 px), folds
  only `Glass::regular()`'s 0.25, three of four Mali pairs down. Not
  accepted.
- Skipping the frost substrate where the correction is zero: not exact;
  the blur is scheduled before the capture exists and any capture-side
  luma bound is broken by stars, planets and text.
- Restricting a capture to the support: 99 pixels one level off (ULP
  and substrate phase). Scissors only.

## Open designs

- **Bounded admission for a carried material constant.** A per-node gate
  that carries a uniform's value as a pipeline constant only after it
  has repeated for a run of frames, caps carried variants per shader
  source, keeps the uniform beyond the cap and drops the constant the
  frame the value moves. Worth +6% on Mali by the probe above; the same
  gate shape as backdrop admission; tests: fixture identity with a
  wrong-constant mutant plus a key-count test over an animated sequence.
- **Unread substrate skipped without a geometry change.** The planner
  keeps a declared substrate's slot and margin and renders no blur into
  it when the material cannot read it this frame.
- **Fewer pixels and passes on the watch.** Occlusion scissors for a glass
  interior covered by an opaque draw (the showcase has none); the per-stage
  blur triple is three passes per stratum. A cheaper material is a picture
  change and the application's decision.
