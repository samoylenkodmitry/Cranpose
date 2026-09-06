# Renderer Architecture

The authoritative description of how a `RenderGraph` becomes pixels in the
WGPU renderer. The pixel-stability contract that every rule below serves is
in [liquid_scroll_pixel_stability.md](liquid_scroll_pixel_stability.md).

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
*capture*: the ops beneath it, clipped to the effect's device rect and
re-rendered into a texture, then filtered into the result. An isolated
child renders into its own texture through the same three steps
recursively. The final pass draws direct ops and composites the resolved
textures.

## Stages

A tiling GPU pays a fixed cost per render pass whatever the pass draws, and
a capture that re-draws the page under its glass pays that page's fill
again for every glass, so the resolve step batches and the page is drawn
progressively. The backdrop effects of one layer scene queue into stages
(`ResolveStages`): an effect joins the stage after every queued effect
below it whose composite lies under its capture, so every capture in a
stage reads only composites of earlier stages. A layer draws into its page
(`LayerPass`: the frame's root image, or an isolated child's surface) in
strata. Before a stage captures, every glass of the stage is registered as a
blocker by its capture rect and the page is drawn up to the stage's last
glass (`flush_page`: the ops since the last flush outside the excluded
effect ranges, the deferred ops, and every pending composite below it), so
a card's drop shadow, which sits between the stage's first glass and the
card, is on the page before the card copies and is never replayed into a
capture. What a flush would draw is released in z order
(`LayerPass::release`): anything that touches a blocker below it waits for
that blocker, an op deferred whole (`LayerPass::deferred`), a composite
drawn outside the blockers it overlaps with its covered parts kept pending
under that scissor, and whatever waits blocks in turn, so nothing above it
that overlaps it lands on the page first. The stage's glass blockers clear
when its composites join the pending list; the next flush draws them and
what waited behind them merged by z, and after the last stage the rest of
the layer follows in one more stratum. `capture_culling.rs` pins the
semantics: a shadow under a later glass of the stage records no fix-up
pass and the glass shows the page exactly; content and a shadow drawn over
a tinting glass keep their own colors over the tint. A stage runs as:

1. the captures of the stage, shelf-packed edge to edge into an atlas.
   Each region of the layer's own page is a `copy_texture_to_texture` of
   the page's texels (`Page::copy`, whole texels of a copy-compatible
   format), outside any pass. What is below a region's z and not on the
   page (a deferred op, a pending composite part behind a blocker) that
   reaches into it is drawn over the copies in one pass that loads the
   atlas, scissored to each region, and that pass is recorded only when
   some region has such a fix-up (`segment_draws_anything`,
   `capture_fixup_passes` in the stats), so nothing already on the page is
   drawn a second time (`a_capture_adds_no_shape_fill_of_its_own`) and a
   stage over a finished page costs copies and no pass
   (`a_capture_of_the_page_is_a_copy_that_records_no_pass`). An isolated
   child reading its backdrop cannot copy: its regions draw the parent's
   page under it, its own page and their fix-ups in a pass that starts
   from transparent. Nothing separates the packed regions and the atlas is
   never cleared: every reader of a region (the blur, a batched shader)
   holds its sample coordinates to the region's texel centers, so a
   neighbour's texels, or a pooled texture's stale ones, are never read
   and a region's edge reads as a dedicated texture's clamp-to-edge would;
2. one blur pass pair over every blurred region: the horizontal pass writes
   each region downscaled into a scratch texture, averaging the block of
   source texels each scratch texel stands for, the vertical pass writes
   the same downscaled slot of a result texture, and both textures are
   packed to the blurred regions alone, so no pass loads or stores the
   atlas (on the Mate 20 X the atlas-sized pair cost 9 ms of bandwidth per
   frame); a composite reads the slot through the capture's size, a blit
   by bilinear interpolation, a shader through its logical-size slot;
3. composites reading their region: a blur blits its region of the result
   through the effect's rounded mask; a runtime shader that declares
   `batched_source` draws in its stratum reading its region, applying the
   mask and alpha through its reserved uniform slots. A later stage's
   captures read the page the shader was drawn into, so every glass is
   shaded exactly once (`shader_pixels` in the stats counts it;
   `backdrop_atlas_parity.rs` pins that a glass under two glass buttons
   shades exactly its own pixels).

Effects the renderer cannot batch (an app shader that reads the whole input
texture, an offset, a chain the shader does not end) resolve one at a time
inside their stage from their own capture of the page, copied the same way.
A list of glass cards over a page therefore costs one stratum and one copy
per card; the glass buttons inside the cards read the cards' glass and form
the next stage: one more stratum, one copy per button and one blur pair for
all of them.

An isolated child that draws nothing itself and whose effect is a runtime
shader (a shader-drawn planet) is a shader composite in its stratum over a
shared transparent input of its surface size, and costs no pass; captures
above it read the page, so it is shaded once however many read it.

An isolated child that reads its backdrop renders its own page with the
parent's page beneath it (`PageBase`): re-based by the child's translation
when the child only translates at the parent's scale, projected through
the child's transform otherwise. The parent draws its strata up to the
child before the child renders, so the child's captures read what the
parent had drawn.

A shader that reads its region of the atlas maps region-local uv to the
texture once per fragment (`region_map` in `liquid_glass.wgsl` and
`gradient_blur.wgsl`) and threads that map through its sampling loops, so
no tap pays for a texture query or a uniform fetch.

`backdrop_atlas_parity.rs` pins the pixels: a glass renders the same alone
and packed beside others (a shader within one 8-bit step, the float region
mapping moving a tap by a few ulps; a blur exactly), the in-shader mask
matches the masked blit, the shader-only child matches its surface resolve,
and a shader child read by three cards shades the same pixels as one read
by one. `backdrop_pass_batching.rs` pins the budget: extra glasses in a
stage add no pass.

A capture region draws only what reaches into it: shapes, texts and images
are judged by their snapped bounds and clip against the region, composites
by their destination against the region's scissor. `capture_culling.rs`
holds a pass-through glass over a text, an image and an isolated child that
each straddle one of its edges and requires the glass to show exactly the
page beneath.

## Direct or isolated

`collect_child` decides per layer, with `child_placement`:

- **Direct** when the layer's transform is a translation, it needs no group
  alpha, blend mode, render effect or explicit offscreen, and its rounded
  clip (if any) admits every op inside it (`content_admits_rounded_clip`:
  each op's device bounds plus one pixel stay out of the corner squares).
  Direct ops join the parent's flat scene with the translation applied.
- **Isolated** otherwise: the layer becomes a `ChildLayer` composited at its
  z with alpha, blend mode, transform and the rounded mask. Its content is
  collected into its own `LayerScene` in the layer's local space.

The root goes through the same function as every child, so a root with a
shadow or a backdrop behaves like any layer would.

An isolated child whose transform is a uniform scale and a translation
renders its surface on the parent's pixel grid: the surface is the child's
device rect snapped outward in parent space, the child draws with the
fractional remainder as its target offset, and the composite is a one-to-one
nearest blit at whole pixels, so a scaling box keeps analytic edges on every
side instead of one hard and one resampled edge. `LayerCache` keys carry the
1/16-pixel device phase, so a cached surface is never reused at another
phase; `effect_semantics.rs` pins both against the same box drawn directly.
Any other transform composites the surface through the projective path.

## Rigid motion

A translated content context (a scroll container) carries one rigid
`SnapAnchor` for its whole subtree: every op inherits it, the anchor decides
one device-pixel delta per frame, and the subtree moves as one raster. Text
rasterizes at a canonical device origin under an anchor and re-rasterizes
only when its device phase changes; images and shapes translate their quads
by the same delta. A gradient's ordered dither is keyed on the device
position relative to the anchor's snapped device origin
(`ShapeData.dither_origin`), so the pattern rides with the subtree; a shape
outside any anchor keeps the origin at zero and dithers by device pixel as
Skia does. `effect_semantics.rs` pins that a translated gradient's local
picture is byte-identical after undoing the rounded translation.

Outside a translated context a layer snaps when its own primitives are
pixel-sensitive (`layer_needs_rigid_snap`: text or images, or any drawn
primitive under motion). An **isolated** child additionally snaps when any
descendant that only translates against it draws text or images
(`layer_has_pixel_sensitive_subtree`): compositing such a raster at a
fractional device offset would resample every glyph, so the composite lands
on whole pixels instead. A moving translucent card with an icon and a label
therefore steps by device pixels, exactly as Jetpack Compose's integer pixel
placement does, and its content never blurs.

An animated, unclipped translated wrapper draws directly with its anchor. The
earlier renderer rendered such wrappers into a supersampled surface and
composited it at fractional offsets ("motion-stable capture"); that
mechanism, its surface-requirement planner and its parity tests are gone.
The contract is the rigid one above, tested in
`effect_semantics.rs` by the translated text, thin-shape and static alpha
surface tests: after undoing the rounded translation, the local picture is
byte-identical.

## Fill-shaped geometry

A shape record draws as its bounding quad and the shape shader decides
coverage per fragment, so a stroked circle or an arc band would rasterize
its whole disc for a band a few pixels wide. The recorder marks an arc,
or a stroked circle, as banded when `band_pays` says its strip costs
less than the quad it would otherwise draw (the disc, for a scope-recorded
arc): the pixels each rasterizes plus what their vertices cost a tiling
GPU (`BAND_VERTEX_PIXELS`), which stores every vertex's sixteen varyings
before it shades a pixel. A band keeps the angular step a full ring takes
at its radius (`ARC_RING_SEGMENTS`) and takes only the segments its padded
sweep needs, rounded up to a power of two; the record files in the bucket
of that count (`ARC_BUCKET_SEGMENTS`, one to sixty-four). Bucketing by
radius alone gave MEGA BOSS's short bricks a full ring's segments each,
2.5 M vertices a frame, which the Pixel 9 Pro absorbed at 56 fps, the
Pixel Watch 3 presented at 4 fps and the Mate 20 X's Mali answered by
losing the device on the first frame. A record carries its band class
in its flags and a segment is one draw at its largest class's vertex
budget, records in order, cut where the budget would leave more quads
collapsed than a draw call is worth (`SEGMENT_WASTE_QUADS`). A draw
instances its records, one instance a record, over the strip index
pattern of the segment's class (`strip_index_pattern`, one small static
index buffer per class in the run store): `2 * segments + 2` shared
vertices a record, so a quad is four vertex invocations and a strip of n
quads is 2(n + 1), not six a quad. The vertex stage takes a quad
record's first four vertices as the rect's corners and draws a banded
record as its strip (`band_position`): the ring padded by one device
pixel, the sweep padded by the angle that padding subtends at the padded
inner radius, the outer vertices riding out so the polygon circumscribes
the padded circle. The record carries what the vertex stage would
otherwise derive per vertex: the fragment's trig row (`arc_trig`) and
where the padded sweep starts and how far it runs (`BandRing`), computed
once when the arc is recorded. A vertex past a record's own pins onto
its last one: the pattern shares each boundary between neighbouring
quads, so a vertex collapsed anywhere else draws a real triangle from
the record's last edge (the first indexed build did, and the parity
test caught 55 k pixels; `a_translucent_quad_sharing_a_draw_with_a_wide_ring_blends_once`
goes red on it). The fragment stage discards outside the record's rect,
the quad path's raster extent, so the two paths cover the same pixels;
no band is one quad wide (`BAND_MIN_SEGMENTS`), so the class-0 pipeline
holds only quads and folds that test out, which was worth 1 ms of the
watch's pass. `run_geometry.rs` mirrors
the strip on
the CPU for the fill estimate and for the coverage proof, which walks
every pixel center the arc SDF shades and asserts the strip holds it.
`band_fill.rs` pins the budget (a ring costs its band, not its disc) and
the pixels (a banded ring matches the ring drawn in two clipped halves,
byte for byte). On the Mate 20 X, cranorbit's MEGA BOSS arena went from
16 MP to 12 MP of shape fill and from 15.9 ms to 10.6 ms present when the
CPU mesh introduced this; the vertex-stage strip keeps the fill and costs
no CPU per record.

## Uploads and passes

- `ViewportUniformRing`: one uniform buffer with dynamic offsets. Every pass
  and every retained glyph run claims a slot; the whole ring is written once
  per frame.
- Shape records draw from the run store (`run_store.rs`): a run of at
  least `STORE_RUN_MIN_RECORDS` records with a command keeps retained
  buffers keyed by its `DrawCommandId`, written only when the recorder
  hands back other bytes (`Arc` pointer, then a byte compare) or another
  paint; smaller runs and shadows are copied into per-pass arena chunks,
  every record naming its placement, and consecutive runs share a draw.
  The uniform-buffer floor (WebGL) has only the arena, in 16 KB chunks,
  drawn as quads. Image batches write into per-frame slot pools
  (`image_slots`), trimmed after the frame to what it used.
- All queue writes and command encoders live in `frame_graph.rs`
  (`render_contract.rs` pins this).
- Transient textures for captures, blur ping-pong and child surfaces come
  from the frame recorder and are released at the submit boundary; their
  bytes are reported as `transient_texture_bytes`.

## Caches

- Glyph atlas, glyph mask cache and retained glyph runs.
- Image textures.
- Blurred shape-only shadows, keyed by content and anchored device placement,
  composited as up to four bands around the occluder inside the final pass.
- Runtime-shader pipelines, specialized on the shader's inactive features
  (`glass_specialization_parity.rs` proves the specialized pipeline matches
  the general one byte for byte).
- Shape pipelines, one per (blend mode, vertex stage, `ShapeVariant`): a
  batch whose records share a kind, carry no gradient, or carry no clip
  fixes that into the pipeline's constants (`SHAPE_KIND_FIXED`,
  `SHAPE_SOLID`, `SHAPE_CLIPPED` in `shape.wgsl`), and the fragment program
  keeps only the branches the batch can take. Batches are cut by blend
  mode, brush table and brush class (solid or gradient), and by nothing
  else: the brush class is the one cut worth a draw, since a scene's
  gradient records are few (cranorbit's arena: 1.2 of 11.5 MP, three
  draws) and a solid batch then folds its whole gradient path, while a
  cut on kind would fragment a scene that interleaves kinds record by
  record (the arena's studded bricks: 293 draws and 12 fps, measured).
  `shape_variant_parity.rs` holds every variant to the general program
  within the same bound the solid entry once had, and goes red when a
  variant's mapping is wrong. On the Mate 20 X cranorbit's MEGA BOSS
  arena presents at 61.0 / 61.0 fps (period 16.8 ms, the vsync) against
  51.6 / 51.9 with the variants off and 55.5 before the brush-class cut;
  main measured 58. Ablation put the arena's cost where the variant took
  it: flattening the coverage math alone reached 61 fps, skipping the
  4.5 MB record upload changed nothing, and drawing the bands as quads
  cost 1.7 ms.
- `LayerCache`: textures of isolated children that read no backdrop and whose
  `cache_policy` is `Auto`, keyed by content hash, size, scale bucket and the
  1/16-pixel device phase. The texture holds the child's content before its
  render effect, so an animated shader over static content re-applies the
  shader each frame and re-renders nothing (`raster_cache.rs`); a translated
  child lands on whole device pixels, so a scrolling card keeps its texture.

- Backdrop results, in the same `LayerCache`: the resolved pixels of a
  batched backdrop, keyed by the backdrop's node, its effect, its capture
  size, and a hash of everything the capture reads: every op of the parent
  segments and of its own scene that touches the capture rect, with its
  geometry taken relative to the capture (`capture_hash.rs`), and every
  composite beneath it by what its texture holds. A composite says what it
  holds through `SourceContent`: `Retained` carries the hash of the cache key
  whose pixels the texture keeps (a cached child surface, a cached shadow, a
  cached backdrop result, or a shader tail derived from one), `Transient`
  means the texture is drawn anew each frame, and a backdrop that reads a
  transient gets no key at all. Texture identity is never the pointer: an
  allocator hands a freed address back, so a pointer that matches last
  frame's proves nothing about the pixels. A key seen in two consecutive
  frames is admitted: the stage still runs for it, and its unmasked result is
  resolved into a retained texture in one extra pass; from the third frame
  the backdrop is a blit of that texture through its rounded mask, with no
  capture, blur or shader. A key seen once is only remembered, so an animated
  glass never pays for a resolve it cannot reuse. Rigid scroll keeps the
  hash: a card moving over a flat page reads the same relative content every
  frame. `glass_layer_cache.rs` pins the contract: a still glass scene misses
  nothing and runs no blur, an animated overlay misses exactly itself while
  every still row hits, a rigid scroll keeps the rows' results, and a change
  beneath a still glass reaches the pixels and matches a renderer that never
  cached.

A backdrop over content that changes every frame is resolved every frame;
nothing shortens that but a cheaper material or less content under it.

## Stats

`RenderStatsSnapshot` reports passes and pass pixels, texture copies and
their texels, transient and retained texture bytes, uploads, isolated layer
renders and pixels, layer cache traffic, blur passes, composite passes,
effect applies, the pixels the runtime shaders shade (`shader_pixels`,
the number that decides a glass frame's cost on a tiling GPU) and the
shape fill split by kind and brush (`shape_fill_pixels_by_class`, printed
after `shape_fill_px` on the `[GPU f#]` line), which is what attributes a
fill-bound frame: cranorbit's arena is 5.0 MP of plain fills, 4.0 of arc
bands, 1.2 of stroke bands and 1.2 of gradient fills per frame.
`backdrop_pass_batching.rs` pins the pass budget: a frame has one full-screen
pass, a stage adds one copy per glass and one blur pair however many glasses
it holds, a fix-up under a glass adds one pass over the copies, and a stage
adds one stratum of the page, never a pass per glass.

## Validation bar

`just fmt`, `just clippy` (and the target-specific clippy recipes), `just
test`, `just robot`. A renderer change that touches placement, crispness or
pass counts ships with a test that fails without it.

## Where a glass frame's time goes

Measured on the Mate 20 X (Mali-G76) scrolling the showcase list, one APK
per series and a `debug.cranpose.ablate` toggle that drops one thing per
run (present p50 ms, battery temperature steady). With captures as copies
but the page still flushed once per stage at the stage's lowest glass
(2026-09-04 morning, 39.5): the four page loads cleared instead 38.8, so
the strata's tile traffic is free; no copies 37.2; no fix-up passes 31.8, a
fix-up pass recorded with no draws 31.8, fix-ups without composites 32.1,
without ops 39.4, so the whole 7.5 ms was the composites replayed into the
captures: each card's own drop shadow and the previous card's, and for each
icon its card's glass shaded again; no blur 28.7, blur horizontal only
31.4, blur with one tap 33.2, so the vertical blur passes' taps are ~6 ms;
one flush for the whole layer (captures re-draw instead) 46.7, so strata
beat re-drawing by 7 ms on this GPU. Drawing the same fix-ups into the page
instead of the atlas gained only 1.5 ms: the draws were the cost, not the
pass. Flushing the page before every glass removed every fix-up but added
15 full-page passes (44 passes, pass pixels 17 to 48 MP) and gained
nothing; one flush per stage with the stage's glasses as blockers (the
design above) presents in 32.7-33.1 ms with 25 passes, against 37.9 for
the stage-lowest flush measured in the same session, and main's old
renderer at 27-28. What remains: the liquid shader ~14 ms over 2.8 MP, the
blur pass pairs ~8 ms (three vertical passes of 0.2-0.5 MP at 13-25 taps
each, written at full size), copies ~2 ms, the cached shadow bands (5.7 MP
composited per frame), the page's own fill.

## Why the frame is not 60 fps (read 2026-09-04, third pass)

Mate 20 X (Mali-G76 MP10, 1080x2143, Vulkan, Fifo, 8-bit page drawn
straight into the swapchain image), `[android-frame]` p50 in ms on the
threaded Android path: `update` is compose, layout and the scene graph
patch on the producer thread; `render` is the encode (plan, record,
finish, submit) on the present thread; `present` is the `frame.present()`
call; the collect is inside `acquire` and under 1 ms on both scenes. The
stage p50s are marginals of independently sorted arrays
(`frame_cost_attribution.md`); they are not added here, and `present` is
not GPU time.

| scene                       | fps  | period | update | render | present | uploads | GPU stats |
| --------------------------- | ---: | -----: | -----: | -----: | ------: | ------: | --------- |
| showcase scroll             | 24.2 |   40.0 |    4.5 |    7.7 |    32.0 |  0.1 MB | 5 passes, 17 copies, shader 2.8 MP, shadow bands 5.7 MP, 110 draws |
| cranorbit MEGA BOSS         | 52.4 |   18.7 |    6.9 |    9.9 |     8.1 |  4.5 MB | 1 pass, 1 draw, 11 MP shape fill |
| cranorbit MEGA BOSS on main | 58.0 |   17.2 |    6.1 |    6.4 |     1.7 |  0.3 MB | 1 pass, 29-40 draws |

**What the frame is bound by was measured, not read.** No GPU timer works
on this device: there are no timestamp queries, and the whole-frame fence
(`gpu_fence_profile frame`) reports 43 ms per frame on cranorbit, a scene
that runs at 52 fps unfenced, so it has a round-trip floor larger than the
frame (`TIME_WASTERS.md` saw the same 50 ms on two frames that differed
five-fold). `Mailbox` against `Fifo` changes nothing (showcase present
31.6 against 32.0, cranorbit 7.4 against 8.1). What does discriminate is
a CPU delay injected into the present thread between acquire and encode
(`debug.cranpose.encode_delay_ms`, `present_runtime.rs`), two alternating
rounds each:

| scene     | delay ms | period ms | fps  |
| --------- | -------: | --------: | ---: |
| showcase  |        0 |      40.0 | 23.6 / 24.1 |
| showcase  |       20 |      37.8 | 24.3 / 24.8 |
| showcase  |       40 |      59.8 | 16.8 / 16.7 |
| cranorbit |        0 |      19.4 | 51.3 / 51.1 (SurfaceFlinger) |
| cranorbit |       10 |      24.2 | 41.0 / 41.0 |
| cranorbit |       20 |      36.0 | 28.5 / 28.1 |

Twenty milliseconds of extra CPU on the showcase's encode thread cost
nothing; forty cost a frame. The present thread therefore overlaps the
GPU by a frame (present blocks until the previous frame's GPU work is
done, not this one's), the period is `max(gpu, present-thread cycle,
producer cycle)`, and the showcase is GPU-bound at ~40 ms per frame with
its CPU stages hidden under that. Cranorbit's period grows one for one
with the delay from zero, so its present-thread cycle (encode 10.9 plus
the wait for the previous frame's GPU) is already the frame, and the wait
absorbing exactly `gpu - encode` puts its GPU at ~19 ms per frame, not
the 8 ms the present column suggested. Both scenes are GPU-bound on this
device; on both the CPU is under the vsync already; and the first plan's
CPU budget, correct as work removal, was aimed beside the gate.

Cranorbit's GPU spends ~19 ms on 11 MP of shape fill and a 2.4 MP page:
~1.5 ns per pixel through `shape.wgsl`'s one fragment program for every
kind, with fifteen flat varyings per vertex, a `discard`, and SrcOver
blending on every fragment. The band meshes took the fill from 16 to
11 MP and the frame from ~24 to ~19, so the frame is fill-bound at about
a millisecond per megapixel. Main drew the same arena under 17 ms of GPU
(its present never blocked) through its replay pipeline; what it shaded
per pixel and how many pixels is not recorded, and is the first thing to
measure against.

The showcase's GPU is the glass (14 ms of position-only arithmetic over
2.8 MP), the blur pair (8 ms, the vertical pass at full size), the cached
shadow bands (5.7 MP composited per frame), the copies (2 ms), four
full-page strata (~4 ms of tile traffic) and the page's own fill and
composites; these deltas were measured under GPU-bound conditions by
ablation, so they are GPU deltas, and they sum to the ~40 ms the delay
probe puts on the GPU. Under the GL backend the same scroll runs at
33.5 fps, ten milliseconds cheaper per frame, while cranorbit gets
slower; GL is not a lever, but the one configuration difference is the
page: on Vulkan it is the swapchain image with `COPY_SRC |
TEXTURE_BINDING` usage, which on Mali can cost the image its framebuffer
compression, so every stratum, copy and glass read of it would pay
uncompressed bandwidth. That ablation (an offscreen page and one blit
against the direct root) runs before the showcase budget is trusted.

The scroll frame's 4.5 ms `update` splits (`frame_stage_ms`, p50 of the
frames that did work) into layout 0.7 and the scene phase 3.2 ms; it is
under the vsync and off the critical path on the phone, and matters on
the watch, where the sync path runs every stage on one core.

## Architecture: the recording is the GPU record (low-tier first)

The watch measurement inverted the order. On a phone the GPU is the frame
and the CPU hides under it; on a low-tier device the CPU is the frame,
and this renderer walks every primitive of a re-recorded command in five
stages (record into `DrawPrimitive`, rescan for the summary and the
layer hash, flatten into `CompositorScene`, merge and batch, convert to
`ShapeData`, tessellate, upload). Cranorbit's arena is one command of
17,600 primitives that the app re-records every frame, and so will any
game, dial or chart drawn from state. The app's recording is
Omega(primitives) and stays; the framework's obligation is the cheapest
possible record per call and no further per-primitive CPU work after it.
Everything past the record is O(commands + segments) plus one comparison
and one upload of the record bytes. Two rounds of unbiased review (sol,
2026-09-04, `codex_review3.out` and `codex_review4.out` in the session
scratchpad) shaped this section; the first withdrew a typed-tape,
similarity-span, patched-slot design as a reproduction of the 44 fps
renderer with an unsupported budget, the second corrected the record
format and the order below.

**The record.** `DrawScope` writes a fixed-size POD `ShapeRecord`
(`f32` geometry throughout, colour as `f32x4`; kind, cap, join and blend
packed in one flags word; rect, radii, stroke width; for arcs the band
inner and outer radius, the centre, and the precomputed mid-angle and
half-sweep sine/cosine the fragment stage reads today; a brush handle for
gradients; ~112 bytes) straight into the command's record vector, in the
layer's local space. Degenerate arcs are rejected at record time as
today; the tight cap-aware rect is computed at record time until the
vertex-stage band makes the quad irrelevant. Gradient brushes are
interned per command into a stop table addressed by handle. Text, images
and shadows go to a general lane and are converted to the send-safe
scene forms before the packet boundary; every shape, including a
blend-wrapped one, stays in the compact lane. Recording builds, online,
the command's coalesced segments (lane, start, count, blend, brush class:
the cuts `shape_run` makes today, never on kind), its conservative
bounds, its content summary, and an ordered fingerprint over record
bytes, lane order, general-lane content and gradient stops, so the layer
and backdrop caches keep their identity without rescanning anything.
`DrawRunNode` holds an `Arc` over POD bytes and tables plus these facts
and is never rescanned; the software renderer and the primitive
inspection tests read records through a materialising iterator that
yields today's `DrawPrimitive` exactly (a lossless format, hence `f32`).

**Placement.** One uniform per stored run, in the viewport ring (the
arena reads its placements from a table the record indexes): the
snapped translation, root scale, the clip rect, layer alpha and the
colour matrix, applied in the shader in the CPU resolution's exact paint
order (a painting layer quantizes to 8-bit sRGB, then alpha, then the
filter; an unpainted solid colour passes through, as `resolve_layer_brush`
passed it). Gradient stops are painted on the CPU at upload, few per
command. Only translated children draw direct, as
`child_placement` admits today; scaled or rotated children stay
isolated with their snapped-surface semantics. The vertex stage maps the
record through the placement and reproduces per record what
`convert_shape_into_slots` does now: every rect edge, quad coordinate,
clip and gradient coordinate canonicalised to 1/16 device pixel, the
dither origin from the anchor's snapped origin. The fragment stage
receives what it receives today and its SDF, anti-aliasing, gradient and
dither code does not change. The device-space work moves from the CPU,
once per primitive, to the vertex stage.

**The run store.** On the present side, keyed by `DrawCommandId`: one
set of GPU buffers per command of `STORE_RUN_MIN_RECORDS` records or
more, grown on demand, and the tables it last uploaded. Per frame per
run: same `Arc` as last time, nothing; else compare the bytes, equal,
nothing; else write the whole command. No patches, ranges or
generations: a rejected packet leaves the store equal to its last
upload, which `cancellation_contract.rs` pins (a cancelled packet
carrying new tables, the next presented packet drawing them, red-proven
against a store that skips the byte compare). Buffers of commands absent
for `STORE_IDLE_FRAMES` are dropped. Runs below the threshold, loose
primitives and shadows go to the frame arena instead: per pass, one
chunk of records copied with their brushes re-based and their placement
index in the record's spare word, so a page of hundreds of small commands
is a handful of draws, not hundreds of bind groups. The uniform floor
(WebGL) is the arena alone, in 16 KB chunks, quads only.

**Scene and pass.** `collect` pushes one `RunDraw` per stretch of shape
segments of a draw-run node (the command, the segment range, the
placement, the tables' `Arc`, the fingerprint, the bounds) and hands the
other lane's primitives to their item paths between them; a layer's own
primitives record into a loose `ShapeRecorder` under one placement and
close into a run when the placement changes or anything else takes a z.
Visibility is per run bounds. `merge_items`, `shape_run`, the conversion
in `prepare_shape_batch`, `band_mesh.rs`, `ShapeData` and the flat
`shapes` vector are gone. The pass draws a stored run as one draw per
segment under its (blend, kind, brush class, clip) pipeline, `6 n`
vertices from `vertex_index`, then one band draw per bucket the segment
has banded arcs in; arena runs merge into the chunk's draws.

**Bands from the vertex stage.** Arc and ring records draw as strips
generated from `vertex_index`, in segment-count buckets (a draw per
bucket per segment), with the same one-texel slack and the pixel-centre
coverage proof the CPU mesh had; see "Fill-shaped geometry". This
replaced the CPU mesh in the same landing, because removing the mesh
first would have restored the disc-sized fill measured on the Mate 20 X
(12 to 16 MP, 10.6 to 15.9 ms).

**Budget on the watch, as a bar.** Floors outside the renderer: the
app's own math plus 17,600 record writes (main's whole process_frame,
with a 4-byte tape, was 8-10 ms; the record write must stay under
~0.25 us per call on that core), reconcile 3-4 ms, drain 1-2, layout 1,
present 0.6. The renderer after the record: compare ~0.3 ms, upload
under 1 ms, collect and encode ~1 ms. Bar: p50 period at or under
16.7 ms and p90 under 20 ms over 60 s at steady temperature; floor: beat
main's 21.8 ms p50 on the same watch. If the record floor makes the vsync
unreachable, the number is reported, not estimated away. On the phone
the encode thread drops from ~11 ms to a few, which is battery, not fps.

**Not in this design.** Similarity-span inference, recolour patches and
persistent patched ranges: only if the bar is missed with everything
above landed and the numbers show the record floor is not the reason.

## Review of this design

- The first draft (typed tape, per-record similarity verify, patched
  persistent slots) reproduced the retained feed of the 44 fps renderer,
  forecast a 1 ms pass the old renderer's 18-20 ms refutes, and needed a
  producer/present commit protocol. Withdrawn.
- `Rc` cannot cross the packet boundary (`FramePacket` is `Send`), and
  `DrawPrimitive::Text` carries `Rc<str>`: the packet payload is an `Arc`
  over POD bytes and tables, text converted before the boundary.
- Present-side comparison is too late for cache identity: the fingerprint
  is produced while recording and covers stops and lane order, or a
  recoloured gradient behind an unchanged handle reuses stale stops.
- `f16` would break the exact `PartialEq` gates on `Color` and
  `CornerRadii` and the GLES 3.0 lowering; geometry and colour stay `f32`.
- An alpha-only uniform changes output: the colour matrix and the exact
  paint order ride the uniform.
- A common integer delta is not today's snapping: the vertex stage
  canonicalises every coordinate to 1/16 px as the CPU does, pinned by
  the byte-identity tests in `effect_semantics.rs`.
- A per-entry tape walked in the pass is the forbidden scan; segments are
  coalesced at record time.
- Variable segment counts cannot share one instanced draw: buckets.
- The earlier draft's own arithmetic (93 records per chunk, ~190 web
  draws) belonged to the 176-byte `ShapeData`.

## Budget, GPU ms per frame, from the delay probe and the ablation deltas

`after` is an estimate until the step's own device number replaces it;
a step whose number does not move is reverted.

| scene / item                        | now | after | step |
| ----------------------------------- | --: | ----: | ---- |
| cranorbit shape fill and page       |  19 |    13 | pipeline per kind and brush, varyings trimmed |
| cranorbit fill margin and disc quads|     |    11 | mesh margin measured, octagon discs |
| showcase liquid shader              |  14 |     6 | lens field |
| showcase blur pairs                 |   8 |     2 | scratch-scale vertical pass |
| showcase shadow bands               |   3 |   1.5 | visible extent |
| showcase copies, strata, fill, composites | 15 |  15 | page usage ablation decides |
| showcase total                      |  40 |  24.5 | ~41 fps |

Main, built and measured the same afternoon with the same instruments,
presents the showcase scroll at 24.15 / 24.09 fps, period 40.5 / 40.4 ms,
against this branch's 24.2 and 40.0: parity to the tenth of a frame, with
30-33 passes and 21 MP of pass pixels against 5 passes, 17 copies and 12.4
MP here. Both renderers spend the frame in the same place, the material.
An independent audit of the shader and the device (sol, 2026-09-04,
`codex_review2.out` in the session scratchpad) puts the pixel-exact
ceiling at 29-32 ms: 19 bilinear taps per shaded pixel (three rays, five
reflection, nine frost, one resting) over 2.8 MP is 53 M samples a frame
before any ALU, and the exact steps (a guarded interior path that drops
the rays and reflection where their weights are provably zero, shadow
support cut from 3r to the kernel's r, blur variants per tile mode,
one immutable substrate for cards over a fixed background) earn 8-11 ms
together. Under that ceiling 60 fps needs what changes pixels: the blur
result kept at scratch scale, the frost neighbourhood from a
downsampled source, `f16` on the colour side, dispersion and reflection
confined to the edge band, the card face shaded below full resolution,
and a procedural substrate for an affine background. Each is listed with
its visual cost in the audit; none is assumed here.

Cranorbit reached the vsync on 2026-09-04 with the brush-class cut and
the variants (61.0 fps presented, period 16.8 ms, present 1.4 ms); its
CPU cycle is ~11 ms on the present thread. The showcase does not reach it with pixel-exact steps: 24.5 ms
is ~41 fps, and the 8 ms to the vsync are the page usage ablation's
unknown, the four strata (~4 ms this GPU charges per full-page pass, not
reducible without a render-area wgpu does not expose), and the material
decisions (the frost neighbourhood at a quarter resolution, the tone
curves as a lookup, the lens field's interior at half resolution), each
a bounded pixel change with a reference model and each the user's call.
This plan does not assume any of them; it states the number they would
have to earn.

## The showcase on the watch (Pixel Watch 3, 2026-09-04, late)

Showcase scroll, main against this branch after the instanced records,
alternated (`measure_watch.sh`): main 27-28 fps, branch 16-17. The
frame telemetry says where: the branch draws 14 passes over 2.69 MP of
pass pixels with 5 copies where main draws 8 over 0.73 MP; present p50
35 vs 18 ms, CPU p50 57 vs 42 ms. That is the pass structure of
resolve-then-compose on a 408 x 408 GPU, not the record path; the Mate
runs the same scroll at 23 (branch) against 24.2 (main), and the branch
as it stood before the instanced records ran it at 20.8 there.

## The watch is CPU-bound and this branch loses it (Pixel Watch 3, 2026-09-04)

A/B/A/B of cranorbit MEGA BOSS on the Pixel Watch 3 (armeabi-v7a, one
Cortex-A53 class core doing the frame, 408x408), main and this branch
built the same hour with the same cranorbit revision, each run 48 s,
SurfaceFlinger presented frames over a 40 s window:

| build  | presented fps | period p50 | render (encode) | update |
| ------ | ------------: | ---------: | --------------: | -----: |
| main   |    44.2, 42.9 |    21.8 ms |          7-9 ms |  13-15 |
| branch |    12.2, 12.2 |    83.3 ms |           57 ms |  23-25 |

The GPU is idle on both (present 0.5-0.8 ms, pass 0.17 MP). The arena is
one draw command of 17,500 primitives re-recorded every frame, and this
branch re-derives every one of them each frame. Its CPU frame, from the
stage telemetry and a temporary probe inside the pass:

| stage                                   |    ms |
| --------------------------------------- | ----: |
| scene rebuild (shell render phase)      |  18.7 |
| collect graph -> flat scene             | 13-16 |
| pass: merge items                       |   4.9 |
| pass: shape run loop (class test, batch)| ~15   |
| pass: convert records to `ShapeData`    | ~10.5 |
| pass: band mesh                         |   8.6 |
| pass: upload (4.7 MB)                   |   2.5 |

Disabling the band mesh moves nothing (pass 42.6 ms either way): the mesh
is a symptom of the same per-primitive walk. Main uploads 0.24 MB and
draws 21 calls on the same scene because its scene builder diffed each
re-recording against the previous one and kept the unchanged spans in
retained GPU slots; only the changed suffix was resolved and uploaded.
On the phone the GPU hid this (both at the vsync); on the watch the CPU
is the frame, and the branch costs 4x. This measurement put the record
architecture first; the reference point is main's 44 fps on the same
watch and the bar is the vsync there.

The build itself: the devices carry the store build (version 109,
release key), so a benchmark APK collides on install. `-PbenchArena=true`
now gives the release build a `.bench` application id in cranorbit, the
same way its debug build already had one, and a benchmark never evicts
the real app. `simpleperf` cannot record on the watch kernel (no perf
events); the stage properties (`debug.cranpose.render_stage_ms`,
`update_stage_ms`, `frame_stage_ms`) are the profiler there.

## Reimagined for both scenes (2026-09-05)

Measured on the Pixel Watch 3 with the showcase scrolling, main against
this branch after the instanced records: 27-28 fps against 16-17. The
frame telemetry puts the difference on the GPU: present p50 35 ms
against 18. Per frame the branch blurs four to five card-sized regions
of 0.24 MP each (one horizontal and one vertical pass per stage, every
region inside them, at 13-25 taps: some 30 M texture taps a frame on a
GPU that has ~1 G a second), re-renders one isolated card of 0.24 MP,
copies five regions and draws 14 passes over 2.6 MP for a 0.17 MP
screen; main runs two to three blur passes. The CPU is not where main
wins: the branch encodes in 12.3 ms (pass records 4.2, the plan the
rest) against main's 15.2, and updates in 6.6 against 4.2 (reconcile
3.2 against 0.8). The record path is at parity with main on the watch
(cranorbit 49 fps both). A first draft of this section (one page
pyramid per stage read by every glass, viewport-cropped surfaces keyed
by their window, the backdrop cache removed, records split into
templates and instances) went to review (sol, 2026-09-05,
`codex_review6.out`) and lost four of its five parts; what stands is
below, with why each draft part fell.

**1. Crop the transient backdrop-reading surface.** The showcase card
on the watch is 618 x 423 device pixels on a 408 x 408 screen. It reads
its backdrop, so no cache holds it, and it renders whole every frame,
and every glass button inside it blurs a 600 x 396 region of a page that
is 60% off screen. A child that reads its backdrop, is composited by a
translation, and carries no unbatched runtime shader renders the part of
its surface the viewport shows, grown by its effects' padding; its
captures follow. Nothing visible changes. Done 2026-09-05
(`viewport_crop.rs`, red-proven on the surface budget, the pixels
byte-identical to the card rendered whole): on the watch the card's
surface went from 618 x 423 to 431 x 226 device pixels, and the
presented frame did not move on either device (watch 15.7-16.2 against
15.0-16.7, Mate 23.2 against 23.3), so that surface was not the cost
and the frame's 14-15 passes over 2.3 MP on the frames that carry the
card are. The watch has GPU timestamps, so the frame was then read per
pass (`passes_watch.sh`, `debug.cranpose.pass_timing`): 49-61 ms of GPU
a frame, of which the layer passes (5-6 a frame) 25-38 ms, the vertical
blur passes (3.1 a frame) 18-20, the horizontal 1.7, everything else
under 3. A constant liquid-glass fragment takes the layer passes to
10.7-11.0 and the frame to 31-35 ms, so the glass material is 15-27 ms
of the watch's frame over 0.34 MP; shadows off takes the vertical blur
to 11.6-11.9 and the blur passes from 3.1 to 2.8 a frame, so the one
shadow that misses its cache every frame (`shadow_cache: shape_miss=1`)
re-blurs ~0.24 MP at full size for ~7 ms a frame. The shadow cache
diagnostic (`debug.cranpose.shadow_cache_diag`, wired to the property
table for this) names it: a card-sized shape shadow whose pixel radius
runs 44.0 to 45.3 with its size in step, a 3% uniform scale, the press
animation of the card a swipe touches, which re-renders the card, its
shadow and its glass at each scale exactly, as this renderer's grid
placement promises; main resampled a scaled surface instead. The
steady scroll frame without a press is 4-6 passes over 0.8-1.2 MP and
still ~35-45 ms of GPU: the glass ~20, the stage blurs ~12, the page
~11. Read for an exact cut, `liquid_glass.wgsl` offers little: its transmitted path is 25 taps
(81 in loupe mode) whenever the optical blur is on, each dispersion
channel 25 more, the frost 9, the reflection 5; only the reflection's
weights (`meniscus_reflection`, `bevel_reflection`) are exactly zero
past the rim bands, so a guarded interior skips 5 taps of 40 to 90 bit
for bit, and the rest is the material as specified. Everything larger
(the frost from a quarter-resolution source, the interior below full
resolution, one glass surface per card) changes pixels and is a
material decision, not a renderer one. Not cropped: a cached child
(its full surface is what makes a partly visible scrolling card a hit,
so the window stays out of its key), and a child under an unbatched
`RuntimeShader`, whose contract hands the shader the whole texture and
its uv (`render_effect.rs`). This is the measured offender and the
first step, on both devices, by present p50 and the pass inventory.

**2. A frost substrate for liquid glass, rays on the page.** The glass
shader's frost neighbourhood is nine taps of a blurred page; its
transmitted, dispersion and reflection paths reconstruct magnified
detail from level 0 (`liquid_glass.wgsl`: a refractive lookup is not a
blur) and stay there. The frost becomes one low-frequency texture per
stage, the page region under the stage's glasses downsampled and
blurred once at a quarter of its size, read by every liquid glass of the
stage; the exact per-region captures stay for what needs them. A
region's blur keeps its own capture, its clamp-to-texel-centre edges and
its tile mode (`effect_renderer.rs`); no page-wide pyramid replaces
them, because a stage's glasses sit at different z with ordinary draws
between them (`capture_culling.rs` pins the pass-through), and because a
page-wide texture has one boundary where each effect has its own. What
this buys is the frost's share of the glass's 14 ms on the Mate and the
per-glass frost taps on the watch, not the blur pairs; the blur pairs
are already one pair per stage.

**3. The plan's cost, measured before it is cut.** The backdrop result
cache hit 0 of 5 per frame in the showcase scroll on both devices, but
the suite pins what it is for: a still glass scene runs no blur and a
rigid scroll over a flat substrate reuses its rows (`glass_layer_cache.rs`),
and a watch face that does not move must not rebuild anything. It
stays. What goes is its per-frame price where it cannot hit: a glass
whose key has missed for a few frames in a row skips the hash until
its node rests, and the hash itself stops walking every op under every
glass (`capture_hash.rs`, linear per glass) and reads command and
subtree fingerprints instead. The plan's 8 ms on the watch is
attributed by ablation first (hash off, stages off, copies off), since
0 of 5 hits says nothing about what the hash costs.

**4. The record, by probe.** The arc's trig, padded sweep and band
class all follow from start and sweep, so they cannot be template
fields while start and sweep move; a template/instance split is not a
split of this record. What the watch measured is the front end's floor
per record and `write_buffer` at ~400 MB/s, so the questions are bytes
per frame and bytes per vertex, answered by probes before a layout is
chosen: a hot/cold split of the 112-byte record (the fields the vertex
stage reads for position first, the rest after) against the present
layout, on the upload and on the pass separately; band class decided
at the placement's effective scale, which the scope does not know
(`DrawScope` has its logical size only) and the run store does, so the
class is stamped where the scale is.

**5. Order.** 1, then 2, then 3, each with its device numbers and its
pass-budget test moved to the new budget, then 4's probes. No number
is forecast here; the draft's forecasts rested on a pass model that
counted blur pairs the renderer already shares.

**Candidates from a second reviewer (astra, 2026-09-05,
`codex_review7.out`), ranked by its impact per week; its numbers are
acceptance targets, not measurements, and they overlap.**

- *Compile the blur's resampling kernel, resize phase included* (~1
  week): the vertical pass reads the reduced scratch and writes full
  size (`effect_renderer.rs`, `blur_fs.wgsl` spaces its paired taps in
  destination pixels), so at 4x many fetches interpolate the same
  scratch rows; expanding the bilinear fetches into source-texel
  weights, merging repeats and re-pairing takes a 13-25 fetch interior
  to ~4-8 and drops the fragment's weight arithmetic. Contract: the
  same footprint, capture z, tile modes and decal normalisation, at
  most one 8-bit level from the reference, the scroll-stability
  contract intact. Target: 4-8 ms of the watch showcase's GPU, 2-4 on
  the Mate; measure on frozen captures first, then the scroll.
- *Native instance attributes as a record delivery path* (2-3 days): an
  instance-step vertex layout over the record buffer instead of
  storage pulls in the vertex stage, same bits, same arithmetic, all
  goldens byte-identical; a probe of the tiler's attribute path. Keep
  only if it takes 2 ms off the watch arena pass; a nil result closes
  the layout question rather than opening more variants.
- *Optical groups* (~2 weeks): one glass surface per card with its
  buttons as lens parameters inside it, declared in `render_effect.rs`
  and lowered through `collect.rs` and `frame.rs`, so a button no
  longer captures and blurs the already-composited card. A material
  and API decision that changes pixels by design (reference images to
  approve); worth it only if captures, blur texels and glass fragments
  disappear in the count. Target 8-15 ms watch, 5-8 Mate.
- *Retain the execution plan across scrolling frames* (2-3 weeks):
  keep the layer event order, dependency edges, batches and atlas
  decisions in `frame.rs`, with topology, footprint, placement and
  content on separate revisions, and patch placements per frame;
  target planning of 2-4 ms on the watch (4-6 ms off), latency and
  thermal headroom before fps.
- *Ordered tile compute for dense commands* (4-6 weeks, cranorbit
  only): conservative arc-to-tile lists in recording order, each tile
  evaluating coverage and blending locally, the raster path kept for
  WebGL and sparse scenes; hurdle 12-14 ms total GPU for the watch
  arena including list building, and the attachment's per-draw
  quantisation and colour conversion reproduced, not accumulated in
  float and rounded once.

The first two are probes that fit before step 3 above and do not
disturb it; the optical group is the one that reaches the showcase's
watch number and is the user's material call; the plan retention is
the CPU side's proper shape once the GPU side is known.

## The blur was wrong twice over and full-size (2026-09-05)

Reading the branch's blur beside main's for the watch's 22 ms of
vertical blur found three things, two of them pixel errors that no test
covered:

1. **The vertical pass wrote every region at full size.** `blur_regions`
   packed a full-size result slot per region and `encode_blur_atlas_passes`
   ran the vertical kernel over it, so at a downscale of 4 the pass shaded
   16 times the pixels main's does: main's direct shader tail
   (`direct_tail_intermediate_size`) keeps the chain's result at the
   scratch size and tells the glass its logical size through slot 252.
2. **The vertical pass reached a fraction of its radius.** `blur_fs.wgsl`
   stepped its taps by one destination pixel with the radius given in
   scratch texels, so writing at full size it blurred `radius / scale`
   pixels: at radius 20 the result lay 95 levels from the kernel. The
   glass on the watch (radius 12 px, scale 2) and the Mate (18 px, scale 4)
   was frosted half or a quarter as far vertically as horizontally.
3. **The horizontal pass skipped texels.** At scratch pitch it fetched one
   bilinear pair every `scale` texels, so at 4 half the source never
   reached the kernel and a 3 px bar could vanish or double: 36 levels
   from the kernel. Main's horizontal pass does the same; its pairing of
   taps `i` and `i + 1` into one fetch also assumes adjacent texels.

What stands now. A wide blur first averages each block of `scale x
scale` texels into a scratch-size downsample (`blur_downsample_fs`, two
bilinear fetches per axis at a block of four, one at two, a pipeline per
block), and both kernel passes step by one texel of what they read: the
horizontal over the downsample, the vertical over the scratch, so the
kernel keeps its paired taps and no source texel is skipped. The first
draft folded the block average into the kernel instead, four fetches per
tap at a block of four, and on the watch that cost 8 ms: the fetch count
per scratch pixel went from 9 to 68, and this GPU pays ~5 ns a fetch.
The result texture holds each region at the scratch size in the same
slot, and doubles as the downsample's target before the vertical pass
overwrites it; a blit composite reads it bilinearly, held to the
region's texel centers (`blit_fs_main`), a shader reads it with
`source_logical_size` in slot 252, and the mask of a batched shader is
measured in those logical pixels (`composite_coverage` in the glass and
gradient blur). Both blur paths pass the sampled texture's size and
explicit source and destination regions, so the shadow blur's horizontal
pass gets the same downsample (its golden moved by at most 14 levels on
0.6% of the image and was re-recorded). The unreferenced fused
blur-mask shader is gone.

Tests, each proven red first: `blur_reference` holds a radius-20 blur
within 8 levels of the CPU kernel (a CPU model of the block average,
quarter-size passes and bilinear upsample lands 5.3, the GPU 5.9; the
old passes 95 and 36) and requires the wide page to spend more than
half a wide capture less on pass pixels than the narrow one;
`backdrop_atlas_parity` has a probe shader painting slots 252 and 236 to
prove a shader after a blur is handed the capture size and the
downscaled region, keeps a blurred glass's page visible outside its
rounded corners, and the packed-against-alone parities cover the
scratch-size slots. The glass-against-its-chain comparison was dropped:
a scratch-size read and a full-size vertical pass are two
approximations of one Gaussian, and the glass amplifies their gap into
tens of levels along thin bands, so no tolerance says anything.

The watch after this, showcase scroll, per-pass GPU spans of the four
60-frame windows (list top first, glass rows last), main beside:

| build                              | window spans, ms          | Layer | Blur H | Blur V | Downsample |
|------------------------------------|---------------------------|-------|--------|--------|------------|
| branch before (full-size vertical) | 48 / 52 / 66 / 58         | 36-40 | 1.7    | 19-22  | -          |
| block average inside the kernel    | 43 / 44 / 60 / 64         | 33-36 | 9-10   | 15-16  | -          |
| downsample pass (this)             | 36 / 34 / 43 / 42 (34/31/44/40) | 29-31 | 1.1 | 6.6  | 0.3        |
| main                               | 36 / 37                   | 28 (fused 14.5 + shader 13.5) | 0.6 | 2.1 | - |

Presented fps in the warm window: 25.4-25.6 against main's 27.5-28.0
(the branch's `period p50` 31-38 ms against main's 31); the earlier
branch stood at 16-18. Two ablation builds attributed the rest. With the
atlas vertical pass removed outright the "Blur Vertical" line still
read 15.7 ms at 0.4 passes a frame: that is the non-atlas vertical pass
of one card shadow re-blurred at full size every frame (0.3 MP by 9
fetches, 3 M fetches, ~15 ms at this GPU's ~5 ns a fetch), astra's third
point and the next cut (the shadow result at the scratch size, the
cutout as an inverse mask in the blit). With the block average
disabled the horizontal returned to 2.4 ms, which put the in-kernel
block at 8 ms and led to the downsample pass. Every remaining
`Backdrop Result Pass` (9-10 ms at 2.2 a frame in the list-top windows,
in every build including main's ancestor) is `resolve_whole`: a glass
read by captures above it shaded whole into a retained texture, its
off-screen part included; cropping it to the visible reach as the child
surfaces are is the cut after the shadow.

### The shadow at the scratch size (2026-09-05, later)

The 15 ms "Blur Vertical" left after the atlas fix was one card shadow
re-blurred at full size on every press frame: the shadow's surface is the
caster with a margin of three radii, 0.3 MP on the watch, and its vertical
pass read the scratch back up to that whole surface. The shadow's result
now stays at the scratch size and its composite reads it bilinearly; a
post-blur cutout needs the surface's full size, so the scratch result is
interpolated back into it by one blit and the cutout drawn there exactly
as before. Reading the shadow beside its CPU kernel also found that the
paired-tap kernel dropped decal taps past the region and renormalised to
what was left, so a shadow near its surface's edge came out too strong
(16 levels at radius 20); the dropped tap keeps its weight now, as the
transparent texel it reads would, which is what main's unpaired kernel
does. `shadow_blur` holds a radius-20 drop shadow within 12 levels of the
kernel with and without a cutout (9.8 measured; the kernel truncates at
whole scratch texels, a fraction over four pixels each), and proves the
scratch-size blur by pass pixels: the cutout page's two extra full-size
passes size the surface, and the plain page spends under one and a half
surfaces beyond the empty page (a full-size vertical pass spends four).

Watch, showcase scroll: GPU spans 32-35 ms (Layer 28, every blur pass
line at or under 1 ms, main 36-37); fps by alternation 27.8-29.1 against
main's 27.5-28.1, the branch's `period p50` 28.7 ms against main's 31-37.
The frame is no longer GPU-bound by the blur: the branch encodes in 16 ms
(`render p50`) against main's 14 with a p99 of 48 against 20, and both
present at 14-17. The glass material's Layer Pass, 28 ms on both, is the
GPU floor for both renderers now. One 46 fps run (`period p50` 21) was the
scroll swipe landing on the nav bar and switching to the empty Saved
tab; a run whose `update p50` is 16 ms and `render` 3 ms measured that
tab, not the list.

### The pipeline cache stopped writing at 28 s (2026-09-05, later)

Every watch run of the showcase compiled four to seven liquid-glass
pipelines cold, 650-990 ms each, inside the measured window, whichever
build ran: the disk cache (`pipeline_disk_cache`) loaded 700-800 KB at
launch and wrote once at 8 s and once at 28 s, so a material variant
first reached later than that, which a scroll reaches every time, was
compiled again on every launch. The persist thread now ticks every two
seconds and writes when the pipeline count has grown since the last
write and held still for a tick: a burst of compiles is written once,
after its last one, and a variant reached at any point in a session is
on disk for the next. `PersistWatch` is unit-tested for both. The
measured fps means carried those stalls (a 1.4 s `render` max in a
120-frame window is a third of the window); the medians did not.

Glass on the watch, by ablation on this build: a constant glass fragment
takes the frame from 36 ms to 16.7 and the Layer Pass from 31 to 12, so
the material is 19 ms of the frame over `shader_px` 0.28-0.31 MP, 63 ns
a pixel; main's Shader Effect Pass is 13.5 ms. The visible glass is
about 0.24 MP, so a fifth of the branch's glass pixels are shaded
outside the screen or twice; the rest of the gap is per-pixel. At the
material's current definition neither renderer reaches 60 fps on the
watch: 16.7 ms a frame needs the glass at 6-8 ms.

Cranorbit, the MEGA BOSS arena, alternated with rests at 38-40 C on the
watch: branch 47.3 and 41.7 against main 50.8 and 44.3 presented fps,
7% behind in both pairs; on the Mate both sit at the 60 Hz cap (60.0
against 62.2 over a 40 s SurfaceFlinger window).

## The watch charges per tap (2026-09-05, evening)

Read for 60 fps, the watch showcase frame was attributed by shader
ablation on the device, one build per cut, the per-pass GPU spans of the
same four 60-frame windows compared (`passes_watch.sh`; the watch at
37-41 C, so within ±2 ms):

| build                                            | Layer Pass, ms | span, ms |
| ------------------------------------------------ | -------------- | -------- |
| baseline (4d80a9d0)                              | 27.5-30.9      | 32-36    |
| glass: adaptive frost off (9 taps and their tone)| 18.2-20.5      | 22-27    |
| glass: dispersion off (2 taps)                   | 29.2-31.0      | 34-36    |
| glass: 19 plain taps, no arithmetic              | 30.6-33.4      | 36-39    |
| glass: 19 plain taps within two pixels           | 28.6-31.7      | 32-38    |
| gradient blur: one tap instead of 37             | 22.8-25.5      | 27-30    |

Nineteen plain taps cost what the whole material costs, taps two pixels
apart cost what taps thirty apart do, and each tap removed from the
program takes about a millisecond off the frame over the 0.3 MP the
glass shades: this GPU charges the texture unit per fetch, ~3.5 ns a
fetch-pixel, its cache does not enter, and the arithmetic hides under
the fetches. The frame is therefore a tap count: the liquid glass at 19
taps a pixel (three rays, five reflection, nine adaptive frost, one
resting, one plain), the header's gradient blur at 37 over a quarter of
the screen, and the page under them at ~7 ms. An exact interior guard
(`GLASS_INTERIOR_GUARD`, an override every material raises: deeper
inside the shape than the widest rim band every rim term carries a zero
weight, so the fragment skips the two extra SDF evaluations and the
reflection's five taps and lands on the same bits, which
`glass_specialization_parity` holds byte for byte) moved the device
number by nothing: the branch is flattened and its fetches issued
anyway. What this GPU rewards is taps removed from the program.

**Substrates.** A batched shader now declares up to three low-frequency
copies of its capture (`RuntimeShader::set_substrates`,
`SubstrateSpec::Average { block }` or `Blur { radius_px }`), and the
stage packs them beside the capture: a slot per substrate in the result
texture, rendered by the stage's existing passes (an average by the
downsample pass, a blur as one more region of the pass pair, at its
scratch size), and for a shader that reads the atlas copied back into a
slot of the atlas, so every shader keeps one texture binding and reads
each substrate through a reserved region slot (224..236), held to its
texel centres like the source. A shader after a blur reads the result
texture and finds its substrates there without a copy. The adaptive
frost declares one blur at its neighbourhood radius (16 dp at the
effect's density) and reads it once where it walked nine points; the
gradient blur declares three blurs, the wide radius halving twice, and
realises each fragment's radius as the blend of the two levels around it
(the sharp source below the quarter level), two taps where it walked a
37-tap disc. The frost fold: the resting tap was the plain tap at the
same coordinate and is now one fetch.

Tests, each proven red first by leaving the substrates out of the atlas:
`backdrop_atlas_parity` probes an averaged substrate against the CPU
block mean (within 2 levels); `substrate_reference` probes a blurred
substrate against the CPU kernel at the substrate's own scratch scale
(11 measured inside, 32 at the edge whose blocks hold to the region;
budgets 14 and 36; 140 and more without the copy), holds the gradient
blur's wide row to the wide kernel (3.3, budget 7), its bottom row to
the page exactly, and the rows midway between levels to the kernel at
their radius within 14 on a page of four-pixel stripes (4.4, 9.6 and
4.4 measured; two levels four times apart in radius, the first draft,
landed 44 away); `glass_specialization_parity` counts the card's
substrate and holds the guard and the split byte-exact against the
single general draw. The robot examples for the adaptive frost and the
gradient blurs pass unchanged.

The watch after the substrates, showcase scroll: Layer Pass 16.5-18.2 ms
(from 27.5-30.9), the blur pass pair 3.1 + 3.1 ms at 3 a frame (from
1 + 1: the header's three levels and the frost's neighbourhood ride it),
frame 25.4-27.7 ms (from 32-36), presented 36-38.5 fps (from 28-29),
the picture unchanged. Cranorbit does not touch any of this.

**The interior guard, then the split.** The reflection's five taps are
rim-only; an exact interior guard (`GLASS_INTERIOR_GUARD`: the rim's
reach from the meniscus, border, rim and fold bands plus a pixel, the
five taps inside `if (in_rim)`) moved nothing on the watch: the driver
flattens the branch and issues the fetches either way. Only taps absent
from the compiled program pay, so the material declares a draw split
(`RuntimeShader::set_draw_split`, the `GLASS_RIM_DRAW` override) and the
stage draws it twice from two pipelines of the same specialization,
interior (1) and rim (2), each discarding the other's fragments before
its first fetch (`ShaderDrawVariant` in the pipeline key; the parity
test holds the pair byte-identical to the single draw). Layer Pass
15.0-15.9 ms, frame 24-25.

**The substrate's scratch scale.** A substrate blur ran at the source's
scale, so the frost's 16 dp neighbourhood cost the full-size pass pair
twice a frame. `substrate_scratch_size` blurs at the source's scale below a radius of
3 px, at half below 8 and at a quarter beyond, the kernel scaled with it
(the reference test measures at that scale). Blur pass pair 2.37 + 2.37 ms
(from 3.1 + 3.1), Layer Pass 14.8-15.6, frame 21.9-23.1 ms, presented
39.5-43.8 fps; the top-of-list screenshot differs from the baseline by
a mean of 0.3 levels.

**Where the frame stands.** The GPU frame of ~22 ms is the glass at ~4
taps a pixel inside and ~9 on the rim (~6-7 ms over 0.3 MP), the page
(~7), the blur pairs (~5.5), copies and captures (~2). The CPU chain, on
one thread on the watch (`update` 3.9 ms then `render` 16.7, of which
the encode 6-13, `finish` 5, `submit` 3.5), is ~20.5 ms and is the floor
once the GPU drops below it. Profiled (`simpleperf`, the scratch
showcase declared profileable): 41% in cranpose, 32% in libc (jemalloc,
memcpy, memset), 23% in the Adreno driver; the allocator's callers are
the composer's group slots and frames, the frame executor, `ShadowDraw`
drops, offscreen creation on the frames whose sizes change, and the
driver's own framebuffers and command buffers, one per pass, 14-22
passes a frame with 23-45 `write_buffer` calls.

**Next, in order.** The blur pass count: the block-2 downsample folds
into the horizontal pass (each tap a fetch at a texel corner, the block
average exactly), one pass fewer per stage (~-1.5 ms CPU, -0.5 GPU). The
uploads: one staging write per allocator per frame instead of one per
upload (23-45 to ~8). Transient textures whose size moves with the press
animation pooled by rounded size. Then the CPU chain is measured again
and the plan retained across frames where the profile says the plan is.

## The frame's other half: passes, uploads and the swipe that pressed the bar (2026-09-05, night)

**Where the watch's CPU goes.** The scroll's main thread (`simpleperf`,
DWARF unwinding, the profiling library beside the APK): 40% of the samples
sit in the Adreno Vulkan driver, and a third of the allocator's 42% is
the driver's own `calloc`/`realloc`, so the driver is the frame's largest
CPU consumer. Every render pass, staging buffer, barrier and descriptor
set is a driver call; a frame carried 14-22 passes and 22-45
`queue.write_buffer` calls, each of the latter a staging buffer created,
mapped, copied, barriered and destroyed. The renderer's own code, the
composer and layout are 7%, 7% and 6%. Per-frame `getenv` walks behind
`debug_toggle` were 2%: every toggle a hot path reads is now a
`DebugToggle` static that caches the value under a generation the test
override bumps.

**Where the watch's GPU goes, measured right.** The measurement swipe
started at (204,340), on the tab bar's pill, so every swipe pressed the
bar: its lift spring scaled it by 3%, which isolated it into a child layer
(three layer passes of its own and a blit capture instead of a copy),
re-blurred its 603x397 shadow at 44 px in half the frames (six passes),
and at every swipe's end admitted seven backdrops into the cache in one
frame at ~1.6 ms each. A swipe beside the pill, (36,300) to (60,80), is
the list scroll: 22-23 ms a frame with five layer passes and three blur
stages, the tab bar drawn direct. The stage diagnostic
(`CRANPOSE_GPU_STAGE_DIAG`, `debug.cranpose.gpu_stage_diag`) prints every
stage's members: the search field's blur (stage 0, read by), the selected
chip's blur whose 36 px padding reaches 20 px into the field (stage 1),
and the tab bar's blur whose capture holds both (stage 2) are real
dependencies, so the chain and its nine tiny blur passes stay; the
header's three substrates ride stage 1 because its capture padding is the
wide radius everywhere while its taper reads a pixel at the bottom edge,
which is a sharper reach for another day.

**What changed.** A shadow's surface, and so its blur, its cache entry and
its banded blits, ends where the kernel does: `blur_reach_px` is the
radius (capped by the pass's 32 taps at the scratch block) plus three
scratch blocks and the caster's own pixel, in place of three times the
radius. A radius-44 shadow's surface shrinks from 585x389 to 424x227; the
watch blitted 0.31 MP of shadow bands a frame. Backdrop admission is
budgeted by pixels per frame (`MAX_BACKDROP_ADMISSION_PIXELS`): the frame a
scroll stops in resolves a couple of glasses, not all seven at once, and
the rest follow over the next frames. The frame's uploads are arenas:
every effect uniform block lands in one uniform buffer at a dynamic
offset (the layouts are dynamic now, one bind group per block kind per
buffer), the image and glyph quads in one vertex and one index buffer,
and the shape arena's four tables in one buffer each with the chunks at
dynamic offsets; each buffer is written once before the submit. Three
blurred glasses over a page went from 21 buffer writes to 6 (a still
frame: 4); the watch's scroll frame from 22-45 writes to 8-9.

**Measured, interleaved.** The previous build and this one ran A B A B on
the watch under the list swipe, the temperature logged around every leg,
because the watch throttles above ~40 C (the untouched blur passes read
2.2 ms at 37 C and 3.1 at 41) and a build measured after another is the
hotter one. At 37.4 C both legs: GPU frame 22.3-23.3 ms before, 21.4-22.3
after (Layer Pass 15.2-16.0 -> 14.9-15.9, Backdrop Result 1.15-1.55 ->
0.75-0.95, shadow band pixels 0.38 -> 0.25 MP), CPU p50 22.05 -> 21.58 ms,
presented 37.8-39.1 -> 37.4-41.6 fps: unchanged, the frame is the GPU's.
The second pair, at 40-41 C, was throttled on both builds (29-30 fps).
The picture is the previous build's within the drifting stars.

**Where 60 fps still is.** The GPU frame of ~22 ms is five 408x408 layer
passes and the three-stage blur chain's nine tiny passes (~5 ms of pass
overhead for 5 thousand pixels each), then ~7 ms of page, ~6 of glass,
~1 of admissions. A render-pass blur cannot go below three passes a
stage; one compute dispatch a stage (the tile and its apron in workgroup
memory, both axes in one pass) would take the chain from nine passes to
three, ~-4.5 ms, and is the largest cut left, at the price of a second
blur implementation for the WebGL fallback the web build keeps. The
header's capture reads the wide radius everywhere while its taper reads
a pixel at the bottom edge; an exact reach would free stage 1 of its
three substrates. Under all of it the driver's 40% of the CPU frame is
proportional to passes and descriptor churn: fewer passes cut both sides.

## The transient pool that never evicted (2026-09-05, afternoon)

**Instrument.** The showcase list, composed by the robot driver at the
watch's 408 px over a visible window on the Mac, with wgpu-core's trace log
counting every API call per submit (the runner and its awk are in the
session scratchpad, `robot_api_count.rs` and `api_count.awk`). A scroll
frame issued ~8 render passes, ~5 texture copies, ~60 bind-group sets, ~25
pipeline sets and ~41 draws, and created 5 textures, 5 views and 5 bind
groups while dropping 5 of each: 19 resource destroys a frame. The still
phase was the same, at the same sizes every frame: 3 capture atlases, 3
blur scratches, 3 blur results and the list card's child capture.

**Cause.** `TransientTexturePool` held 16 entries, matched exact size, and
never evicted: once the first frames' shadow scratches and results and a
few odd sizes had filled it, every later release was rejected and every
acquire missed. A probe counted 10 "pool-full rejected" per still frame
and no `Rc` unwrap failures. The stats line was blind to it: `acq=0 new=0`
counted only the effect renderer's retained-surface pool. On the watch this
is the per-frame `vkCreateImage`, `vkCreateImageView`, bind and destroy
traffic the DWARF profile charged to the driver and its allocations, and
on every backend it is a first-use texture the driver must initialise.

**Fix.** The pool keeps its entries in release order, evicts the
longest-unreleased past 64 entries or 32 MiB, and acquires without
disturbing that order (`remove`, not `swap_remove`), so a size a frame
asks for again survives any number of stale ones. Its acquisitions and
creations now feed `acq=`/`new=`. The contract is
`tests/transient_pool.rs`: a page of nine distinctly sized shadows, then a
page of three blurred glasses drawn twice; the second glass frame creates
nothing. It went red at three creations before the eviction landed. The
correctness half is beside it: the same glasses drawn over a band right
after the same page without it, through the atlases, scratches and results
the first frame filled, match a renderer that never pooled byte for byte;
a pool that reuses by format alone turns it red with validation errors and
a mismatch, while a pool blind to height alone stays green here because no
two pooled sizes of this scene share a width.

**Measured.** Desktop trace, texture creations per counted frame: still
5.0 -> 1.0, scroll 4.8 -> 0.7, back 5.2 -> 0.2; resource destroys 19 -> 5,
the rest being the per-write staging buffers. Watch, A B A B, both builds
armeabi-v7a like every watch build before (the first attempt paired a
32-bit a4013903 with a 64-bit pool build and was thrown away), list swipe,
38.5 -> 42.0 C over the four legs: the pool build's stats line reads
`acq=10-15 new=0` outside admissions where a4013903's could not count
transients at all; GPU span 21.90 vs 22.38 ms and 31.37 vs 31.27 (the
second pair throttled), fps 37.4-38.3 vs 37.8-38.9 and 28.7-31.0 vs
29.3-30.4: unchanged, the frame is the GPU's; CPU p50 21.4-21.7 vs
21.2-21.6 ms, and p10 14.2-15.9 vs 12.4-14.5: the frames that do not wait
for the GPU got ~1.5 ms cheaper. The ring reclamation from the same
commit: an upload ring is discarded after a frame that staged under a
quarter of its capacity, never below the 64 KiB floor, an empty frame not
counting (`ring_outlives_frame`). What remains is the backdrop
result cache's retained surfaces: the showcase's stars drift under every
glass, so the cache misses every frame and each admission copies a result
it never reads back (`layer_cache: hit=0` in every watch log), a copy pass
and a surface a frame for nothing. The doorkeeper already admitted only
on the second consecutive frame of a key; the stars change the key every
second frame, so every admission was the last frame that key was seen.

**The gate (2026-09-05, evening).** Each glass, by node, keeps a
`BackdropGate`: the key its capture last hashed to, how many frames
running it has held, its patience, and whether the current key's retained
result was ever read back. A key is admitted once it has held for more
than the patience; a hit resets the patience to one; a key that changes
while its admitted result was never hit doubles the patience, to sixteen
frames at most. A still scene admits on its second frame as before, a
rigid scroll keeps its stable keys and their hits, and a glass over
something that changes every second frame is admitted once and then
waits longer than the change ever holds. The contract is
`a_glass_whose_backdrop_changes_every_other_frame_stops_being_admitted`
in `tests/glass_layer_cache.rs`: forty frames of an overlay drifting a
step every second frame, twenty admissions before the gate, at most two
with it. The five existing cache tests pin the still, scroll, change and
stop behaviour unchanged.

Measured on the watch, A B A B against f0008069, both armeabi-v7a, list
swipe, 36.7 -> 41.3 C: admissions 4-5 a frame -> 0, the retained cache
256 entries at 40 MB -> 8-48 at 1.3-5.9 MB, Backdrop Result Pass
0.71-1.15 ms -> 0.09-0.16 in the steady windows. The GPU span did not
move (21.7 ms both): an admitting frame had shaded its glass in the whole
resolve and blitted the result in the layer pass, so the layer bucket takes
back what the result pass gave up (15.0-15.4 -> 15.4-16.2 ms). CPU p50
21.4-21.7 -> 20.9-21.6 ms, p10 13.1-13.8 -> 12.1-12.6; fps 37.6-38.9 ->
38.2-38.9 in the cool pair, the gate's second leg throttled at 41 C. The
gate buys memory and CPU, not GPU; the frame is the five layer passes.

## The layer bucket split, and the blur's kernel computed once per draw (2026-09-05, late)

**The strata by label.** Every flush of a layer's page was one "Layer Pass"
to the pass timer, so a frame's five strata read as one 15 ms bucket. Each
flush now carries its index in its layer ("Layer Pass 0".."Layer Pass 4",
"Layer Pass 5+" past that). On the watch, showcase list swipe, 21.7 ms GPU
span: 3.3 / 3.1 / 3.5 / 3.2 / 2.4 ms, one pass each, no child page passes;
blur V/H/D 2.2 / 2.2 / 0.72 (three passes each); the rest under 0.3.

**What the strata hold** (desktop trace of the same frame, draws and scissor
pixels by pipeline per label): stratum 0 is the star background, a
full-screen radial gradient rect and 118 star instances, plus the shadow
bands; strata 1 to 3 are the search bar's glass (interior and rim draws
over 43k px), the filter chips' glass, the text and images, the header's
gradient blur; stratum 4 is the tab bar, two glass materials (the pill and
the lens) over one 283 x 214 px rect, the lens rasterized over the bar's
whole node because its node carries deformation headroom. The watch's
swipe never brings the planet cards into view, so every watch number in
this section is the header scene alone; the Mate 20 X swipe reaches the
cards, their glass, star buttons and shadows, and its numbers cover more.

**One-APK ablation** (`debug.cranpose.ablate`, GPU span ms/frame, cool):
none 21.6-22.1; an extra empty Load pass after every stratum 22.1 with
"Empty Pass 0.00 ms x5", and with the blur shaders discarding at entry the
three blur labels fall to 0.13 each, so a pass costs ~0.04 ms here and the
frame is pixels x shader, not pass boundaries; no glass draws 12.2 (glass
9.5-11 ms); no composite blits 20.0 (shadow bands 1.9); no page ops
17.5-18.2 (ops 4, the star gradient 2.4).

**The glass by pipeline** (`GLASS_ABLATE` override): the rim pipeline
discarding at entry 16.7-17.4 (rim shading 5.3 ms: at this rim reach the
band is a third of a 132 px card and pays five reflection taps and two
SDFs on top of the interior work), the interior pipeline discarding at
entry 18.8-19.0 (interior 3.0), both at entry 13.5-13.8 (the raster of
0.55 MP of glass quads ~1.5), discard after the uniform loads 14.0-14.2 and
after the SDF 13.5-13.9 (the prologue of a discarded fragment ~0.8 ns/px,
so a geometric interior/rim split would save at most ~1 ms), interior as
prologue plus one tap 18.0-18.2 (interior shading beyond the prologue 3.4),
interior with its ALU but the four extra taps dropped 19.6-20.4 (taps 1.5,
displacement and optics ALU 1.9). The rim's reflection taps and the
interior's dispersion taps are the material's definition; an exact
renderer cannot remove them.

**The blur's kernel, once per draw.** The kernel loop recomputed, per pixel
and per tap pair, two Gaussian exponentials and the division that places
the pair's one bilinear fetch, all functions of the draw's radius alone.
`BlurKernel::of_radius` (render-common) computes the pairs on the CPU, the
draw's uniform carries them (16 vec4: inner and outer weight, fetch offset,
fetch weight; then pair count and total weight), and the fragment reads
them. The decal mode still weighs each tap per pixel by whether it lands
inside the region, from the pair's two weights, folded by the `BLUR_DECAL`
pipeline constant into its own pipeline. A first version handed the table
down from the vertex stage as ten flat vec4 varyings, byte-identical to the
per-pixel loop on Metal, and was slower on the watch (Blur H/V 3.2x while
heat grew the rest 1.4x): forty varying floats per fragment cost more on
this GPU than the exponentials they replaced. Rounding: the CPU exponential
is not the GPU's, so the table's blur is within one level of the per-pixel
shader's (Codex's direct shader probe: 28 of 256 cases on Intel UHD 730 and
21 on lavapipe differ, worst one level); the kernel's contract is the CPU
reference within one step (`blur_reference.rs`, unchanged), which a
swapped pair offset misses by 28 and 14 levels. Watch, A B A B against
2c9fe018, both armeabi-v7a: fps 37.9 / 37.9 / 37.1 to 41.1 / 43.8 / 43.0
in the cool pair; hot pass timing puts Blur H/V at ~1.5 / 1.5 ms per frame
cool-equivalent against 2.2 / 2.2, ~1.4 ms of the frame.

**What is left for 60 fps on the watch, exact:** the tab bar lens's node
support (~1 ms of prologue and raster outside its shape), a cache for
static expensive fills such as the star gradient (~1.8 ms, the shape
pipeline at 14 ns/px against a 1:1 blit at ~4), the interior/rim raster
split (~1 ms). Beyond those the frame is the glass material's own taps
and the star field's per-pixel gradient, a picture decision.

## The plan to 60 fps on both devices, ranked by what was measured (2026-09-05, evening)

Where each scene stands, hot, on the list gesture, SurfaceFlinger frames:

| scene                        | fps        | period | what bounds it, by the instrument that showed it |
| ---------------------------- | ---------: | -----: | ------------------------------------------------ |
| Mate 20 X cranorbit MEGA BOSS| 60-62      |   16.6 | the vsync; done                                   |
| Mate 20 X showcase, cards    | 23.5-24.2  |   40.0 | the GPU: `present` 32 ms of the 40, whole-frame fence 49.7 ms, 33.9 without glass, 39.9 without page ops (Codex, 2026-09-05) |
| Pixel Watch 3 showcase, header only | 31.6 (main 22.8) | 31.6 | the GPU: a 5 ms sleep in the present thread's cycle moved fps not at all (41.4 / 44.7 with it, 38.6 / 42.8 without, warm legs at 37.6-39.6 C); the pass sum equals the period |
| Pixel Watch 3 cranorbit MEGA BOSS | 52 cool, 31 hot | 19-32 | the update: p50 16.9 ms cool, 30.1 hot; render 4.0-6.8 |
| Pixel Watch 3 showcase, cards| 27-38 hot (main 18-27) | 24.5 cool GPU | the GPU: the cards' glass is 61% of the span by ablation, its rim band and interior a quarter each (below) |

The watch's showcase row is the header scene alone, and the list never
scrolled under it: the bench gesture started its swipes at x = 36, which
on the watch's native 408 x 408 at density 320 lies inside the list's
40 px margin and outside its hit region (Codex, 2026-09-05, screenshots
in the session mailbox). Same-direction drags inside the list, from
(100, 236) to (100, 76) over 500 ms, bring the Sun card into view by the
third and Mercury by the fourteenth. Every watch number before this
finding is the header at rest under touch, not a scroll; every
full-scroll acceptance from here uses that drag, several forward and the
same number back, on both devices.

**The watch's cards, measured with the drag inside the list (2026-09-05,
evening).** Main 0d195313 against the merged head d61ac06a, same ABI,
eight forward and eight back drags per cycle, 48 s legs, hot and never
cooled, A B A B then B A B A: main 24.9 / 26.9 / 24.6 (38.4 C), merged
38.5 / 34.6 / 33.7 (38.5 to 40.3), main 19.2 / 17.9 / 16.8 (40.3 to
41.0), merged 28.6 / 26.3 / 27.3 (41.0); then merged 28.6 / 27.4 / 26.4
(41.0 to 41.7), main 18.1 / 20.5 / 17.6 (41.7 to 42.2), merged 18.5 /
20.9 / 18.3 (42.2), main 13.0 / 12.1 / 11.7 (42.2 to 42.4). The merged
head leads in all eight legs by 1.4-1.55x and heats the watch faster,
which is the extra frames; both throttle past 42 C. Screenshots at rest,
after three drags, after eight and after the way back match main in
layout, glass, text and effects; mid-scroll frames differ by a few
pixels of fling settle and by the animated stars, mean 1.5 levels at
rest and 2.5-5 mid-scroll, no artifacts. The GPU span on the cards is
24-25 ms cool. A wgpu-core trace of the same drags on the desktop puts
13 runtime-shader draws over 1.07 MP of scissor on the 0.17 MP screen
in the cards' stratum (`Layer Pass 1`), with 12 glyph draws and 11
blits beside them, and three blur triples a frame at ~0.1 MP. Ablation
on the same scene, throttled to a 55 ms span so the fractions are the
reading: no glass 39% of the span remains, no blur 84%, no page ops
88%, no blits 96%; inside the glass, discarding the rim draw removes
27% of the span and discarding the interior draw 28%, both together
54%, so the rim band costs as much as the whole interior and the cards'
stratum is those two draws (20.6 of its 22.6 ms). The star field's
stratum is untouched by any glass toggle.

**The frame on each device, by cost.** Mate 20 X, showcase cards, 40 ms
of GPU: the liquid shader ~14 ms over 2.8 MP; the composites replayed into
captures ~7.5 (each card's own drop shadow, the previous card's, and the
card's glass shaded again under each icon); the blur pass pairs ~8 (three
vertical passes at 13-25 taps written at full size, before the kernel
table); the page's expensive fills ~10 by the ops ablation; copies ~2;
shadow bands 5.7 MP. Pixel Watch 3, header, GPU 19.5 ms: glass 9.5-11
(rim pipeline 5.3, interior 3.0, raster of 0.55 MP of glass quads 1.5),
blur ~3 after the kernel table, page ops 4 of which the star radial
gradient 2.4, shadow bands 1.9. The same watch frame's encode, 10-18.7 ms
p50 on the present thread for ~110 draws (41% cranpose, 32% libc, 23% the
Adreno driver over 14-22 passes and 23-45 `write_buffer` calls), runs
beside the GPU with slack to spare: the encode delay probe above added
5 ms to it and the period did not move.
Pixel Watch 3 MEGA BOSS, update 17-30 ms: the recorder writes 15,161
112-byte arc records a frame (369 ns each on this core, 5.6 ms), the
store uploads 1.8 MB of them because every angle changes, and the rest is
compose, layout and the scene patch.

**Why the earlier levers stalled.** Every gain so far cut one term of a
sum whose other terms are the same size: on the watch the blur table took
2 ms of GPU and the period moved 2 ms, one for one, because the GPU is
the whole period there; on the Mate the strata, the pool
and the gate moved the launch scene from 23.6 to 23.5 fps because the
material and the fills they left untouched are 30 of the 40 ms. Sixty
frames a second is 16.7 ms for everything, so each device needs its whole
sum cut by more than half, and no single term is that big. The plan below
is therefore several changes that must all land, ordered by the size of
the term they cut over the confidence of the measurement behind it.

**The levers, with the conditions Codex's review put on them (2026-09-05
evening; the joint plan is `docs/mobile_60fps_architecture.md`, this
section is the measurement record behind it).**

1. *Stages are already the dependency.* A glass joins the stage after
   every earlier composite whose visible pixels lie under its capture
   rect (`ResolveStages::push`), and the capture's blur margin does read
   the neighbour, so nothing looser is exact: the header's strata are
   real dependencies, not a pessimistic grouping. Withdrawn as a lever.
2. *The material at substrate resolution, as a feasibility experiment.*
   The liquid shader's interior reads the blurred backdrop, which already
   lives at the scratch size; shading the interior there under a
   full-resolution rim band would cut the interior's pixels four-fold.
   But refraction, coverage and dispersion are nonlinear in position, so
   shading fewer pixels and interpolating changes the picture unless
   full-motion, edge and adversarial parity says otherwise. It ships only
   with every exact test unchanged and green, and its gain counts when
   measured, not before. The same holds for baking the rim's bevel and
   normal terms per shape.
3. *Incremental backdrop under rigid motion.* When the page under a fixed
   glass scrolls, the blurred backdrop of frame t+1 is frame t's shifted
   by the scroll, except in the strip the scroll exposed plus one kernel
   radius. Exact only when the shift is a whole number of downsample
   blocks and every sampled source under the capture is unchanged apart
   from the translation, which needs a per-source fingerprint, not the
   region's. The showcase animates its stars and planets under both
   header glasses, so it will not hit there; it is a lever for scenes
   whose content under a glass is static, and it is not in the
   showcase's budget. Gate if built: the one-pixel scroll stability
   capture robot, which already exists, plus a unit test for the
   block-phase rule.
4. *A raster cache for static expensive fills.* A radial or sweep
   gradient, a star field or any brush the profile shows above ~1 ms is
   drawn once into an atlas at its size and blitted while its brush, size
   and density are unchanged. The star radial is 2.4 ms a frame on the
   watch; the page ops are ~10 ms on the Mate. Exact for a 1:1 blit. Gate:
   byte identity of the blit against the direct draw, red-proven by
   breaking the key.
5. *Retained frame structures on the present thread.* Off the critical
   path on the header scene by the delay probe; it returns only for a
   scene whose encode outgrows its GPU.
6. *The orbit's recording.* The GPU columns are already a 64-byte body
   and a 32-byte curve per record, the arena writes once per non-empty
   stream and retained updates go through pooled staging
   (`run_store.rs`); the CPU `ShapeRecord` is 112 bytes and 15,161 of
   them are written a frame. The lever is the whole recording-to-
   submission interval on the device, Codex's track 2 in the joint plan,
   not a record size quoted from an older tree.

**No budget from addition.** The numbers above come from different
revisions, temperatures and gestures, and adding their savings into a
16.7 ms result would be fiction; the joint plan's first track is one
timeline for the integrated build on the full-scroll route. What the
numbers do say under exact pictures: the Mate's glass at ~14 ms over
2.8 MP is most of a 16.7 ms frame by itself, and the exact reductions
are the shaded support (the lens's output domain proven, ~1 ms on the
header) and the per-draw constants hoisted as the blur kernel was. The
picture is not a lever: the user has ruled out any reduction of it, so
there is no fallback to a cheaper material and no renewed question; if
the exact candidates miss the deadline, the remaining cost is reported
as measured and the search for an exact architecture continues.

**Order and ownership.** The watch encode delay probe
(`CRANPOSE_ENCODE_DELAY_MS`) was run first and answered: no response, so
the GPU levers lead on both devices. The remaining deciding measurement
is the full-scroll gesture on both devices with the pass inventory on the
card scene. Then item 4 and the lens's output domain, item 2 as an
experiment (the GPU and the passes; renderer
side), items 5 and 6 (recording, upload and the present thread; Codex's
side), each shipping only with its gate red-proven, the capture robots
and goldens green, and both scenes measured hot A B A B then B A B A on
the full-scroll gesture with screenshots at three scroll positions
against main. Nothing counts as done on a header-only number.

**Measured dead ends, not to repeat.** Parallel recording (padding, a
scalar queue, two workers, no metadata, tables borrowed per batch: none
beat the watch baseline); per-fragment blur kernels in varyings (3.2x
slower on the watch); a page flush before every glass (44 passes, no
gain); the page usage ablation (no gain); a compute blur (no evidence it
beats the pass pair on either GPU); a whole-result backdrop cache without
the rigid-motion rule (the stale-image scroll regression of 2026-08-29).

## The split draws cannot rasterize their bands exactly; the shader keeps a frozen reference (2026-09-05, late)

The candidate after the support unit was the split glass draws' raster
support: the interior pipeline rasterizing one quad (the shape rect
deflated by the rim reach less a margin) and the rim pipeline a ten-vertex
ring (outer edge the viewport, inner edge the rect deflated by the reach,
the margin and the corner sagitta), the fragment discards left in place as
the exact gate, the reach computed by one WGSL function both stages call.
A showcase card's rim draw discards ~74% of its fragments after the
uniform preamble and the scene field, so the prize was those fragments'
ALU.

**Measured** on the Mate 20 X, full 14-body scroll against 939c5ddd, A B
A B then B A B A without cooling: 39.40 / 39.61 (band) / 39.52 / 40.15
(band) then 39.50 (band) / 39.19 / 39.64 (band) / 38.85, 35 to 42 C: the
band ahead in every pair by 0.21, 0.63, 0.31 and 0.79 fps, means 39.24
against 39.72, +1.2%, about 0.3 ms of a 25 ms frame. The watch was not
measured; the taps model predicts nothing there, since discarded fragments
fetch nothing.

**Not exact, twice.** `glass_reference_shader.rs` renders the same graphs
through the same renderer with the shipped shader and with
`tests/fixtures/liquid_glass_reference.wgsl` (the 939c5ddd source,
verbatim), byte identity required: cover-mode cards on the page path,
cards on the child path with content, cards at 1.5x and 0.75x, and lenses
with blur and substrates. The band with uv derived from
`@builtin(position)` and a reserved viewport slot flipped 788 to 1,407
pixels by one level per scenario; the band with the interpolated uv
varying flipped 18,510, because a ring and a strip cover a pixel with
different triangles and the interpolator's plane equations do not agree to
the bit. The preamble extraction alone (`glass_geometry()` and
`rim_reach_of()`, the same expressions in one function) was byte-identical,
so the refactor was exact and the geometry was the drift. The exact form
of the idea is scissors: the interior draw scissored to the deflated rect
and the rim draw issued four times, one scissor per side band, all on the
unchanged strip so the varyings are the same; that costs five draws per
glass instead of two and a Rust copy of the reach formula the material
would have to keep in step with the WGSL, for a Mali gain of ~0.3 ms and
a watch gain of nothing. Dropped: the band, the position-derived uv, the
viewport slot, the vertex-count per variant and the vertex-visible bind
groups are all reverted; the shipped shader is the 939c5ddd source.

**What stays.** `glass_reference_shader.rs` and its fixture, the exactness
guard for every later shader edit: a refactor must render byte-for-byte
against the fixture, and a deliberate picture change re-freezes the file
in the same commit. `Glass::backdrop_effect` is public so a graph test can
build the real card and lens materials without composing a page. The
lens's blur is the next exact lever only with a proven sample domain for
the material's refraction; the cards' remaining cost is the material's
taps over the band and the interior, which no raster trick removes.

## A material declares where it writes and where it reads; the renderer shades and blurs only that (2026-09-05, night)

The tab bar's lens carries deformation headroom in its node: the node is
the bar's width and height, the pill it shades is a fraction of that, and
until now every pass over that node, the capture, the substrate, the blur
pair and the composite, ran over the whole node plus its padding. The
material knows its pill: `GlassMorph` gives the primary shape, its glued
neighbours, the wobble and bulge amplitudes, the glue and the deformation.
Two declarations on `RuntimeShader` now carry that knowledge to the
renderer, each with its own contract and its own default of "everything":

- `set_output_support`: the rect outside which the shader writes nothing.
  It bounds the composite's scissor, on the page path, the child path and
  the content-shader tail a glass draws in the final pass. It says nothing
  about sampling: a shader that shades a small region may still read the
  far corner of its rect, and the input padding contract only bounds reads
  outside the effect rect, not around an output pixel. The first cut of
  this unit pruned the blur to the support widened by the input padding;
  Codex's review caught it, and `effect_sample_domain.rs` now holds a
  shader that does exactly that (a small support, a far-corner fetch,
  padding zero, under a blur) on both paths, red against that pruning
  (4,928 and 5,408 pixels) and green now.
- `set_sample_domain`: the rect outside which the shader never samples its
  input. Only a declared domain lets the passes that feed the shader leave
  the rest unwritten: a blur before it writes the domain, widened per pass
  by the reach of the pass that reads it (the vertical pass writes what is
  read, the horizontal that widened by the vertical radius, the downsample
  by both; an averaged substrate one texel more), all in the capture's own
  texel space, so every texel a later pass reads was written and every
  texel read is the one the whole pass would have written. A blit that
  reads its scissor one to one prunes its blur to that scissor without any
  declaration (nearest sampling; linear widens by a texel). A chain hands
  the domain of its final shader to the stage feeding that shader and
  nothing to the stages before. Under `TileMode::Repeated` a tap past a
  slot edge wraps to the opposite edge, so a dilated span that touches
  either end of an axis writes that whole axis; Mirror reflects back into
  the same edge and Clamp and Decal read nothing beyond it, so those stay
  local. Both declarations enter `hash_runtime_shader`, so a draw whose
  declaration changes is a changed draw to the record cache. `blur_pixels`
  counts what the blur passes write; `CRANPOSE_NO_EFFECT_DOMAINS` ignores
  both declarations.

**What the renderer must not do.** The capture rect is exactly what it
was. Restricting it to the support rendered 99 pixels inside the pill one
level off: a capture that starts elsewhere moves the shader's texel
coordinates by float ULPs and moves the frost substrate's block phase.
Exact reductions keep every texture's origin and size and cut work with
scissors.

**The material's support.** `cranpose-liquid` declares the primary shape
widened by every reach the shader subtracts from its field (wobble twice,
bulge, the glued shapes' reach, the glue, the shadow's spread and blur, and
four pixels of ramp and slack), divided by the smaller strain because the
shader measures its field in the unstrained shape and scales the distance
by that strain, then stretched by the strain's absolute affine rows times
the half extents; glued shapes sit in display space and take the same
reach; the shadow's offset widens it vertically. The earlier scalar bound
under-declared an off-axis stretch: a 200 x 200 square strained 2 : 0.5
along 22.5 degrees reaches 231 px in x where the scalar gave 204, a
27 px clip (Codex's example, now a unit test that was red against the
scalar). The deformation was also missing from the output padding, masked
by the node's headroom; the padding is now the larger of the old formula
and the support's overhang past the node, so no capture shrinks and the
under-captured ones grow. Cover-mode glass, the cards, declares nothing
and is unchanged. The liquid material declares no sample domain yet: its
refraction, dispersion, zoom and loupe have no proven sampling bound, so
its blur stays whole until that bound is derived.

**Gates.** `glass_output_support.rs`: a lens with node 280 x 160 over a
120 x 40 pill, blur 12, wobble, bulge, ellipse blend and strain on,
rendered with the declarations and with the toggle: zero differing
pixels, the composite's shaded pixels smaller, the blur's written pixels
equal because no domain is declared. Red-proven: a support 6 px too small
makes 9 pixels differ at the pill's edge. `effect_sample_domain.rs`: the
far-corner shader on both paths; a shader reading 4 texels past each pixel
that declares a 4-texel domain lands on the same pixels with the blur
writing less; the same shader reading 12 past a 4-texel declaration
renders differently, so the pruning is live; the same far-corner shader
under a repeated-tile blur with a domain declared at the far corner
differs by 480 pixels on each path when the wrapped taps are not written
and by none with the whole-axis rule. The planner keeps its capture
and records the support; a chain's support and domain are its writer's.

**Measured.** The pre-review build (which still pruned the lens's blur)
against d61ac06a on the watch, hot, both orders: header at rest 40-46 fps
against 40-45 cool and 30-32 against 30-32 hot, the card traversal 27-29
against 27-29: no difference beyond the thermal steps. The final tree
against d61ac06a on the Mate 20 X, the full 14-body scroll (Codex's
verified route, 40 swipes per 60 s leg), A B A B then B A B A without
cooling: 34.50 / 35.25 / 37.50 / 38.21 then 38.70 (final) / 38.68 /
38.95 (final) / 38.97, battery 34 to 40 C across the run; the device
climbs 4 fps over the eight legs whichever build runs, the paired gaps
are +0.75 and +0.71 while it warms and +0.02 and -0.02 once warm, so the
unit changes nothing on the Huawei either. Preflight screenshots between
builds differ by the same spread as between legs of one build (the
settling scroll and the rotating planets). The lens's blur is a small
share of the header frame and the cards declare nothing; the unit is
exact infrastructure and a padding fix, and its gain arrives with a
declared sample domain for the material.

## The page's opaque prefix is drawn once and copied back (2026-09-05, night)

The showcase's starfield records a full-screen three-stop radial rect
and then ~160 moving stars in one draw callback, so the whole run changes
every frame and no layer-level cache can hold it, while the rect itself
never changes. Measured by drawing that rect as a flat fill instead,
full 14-body scroll, A B A B then B A B A without cooling: Mate 20 X
39.19 / 44.53 (flat) / 39.83 / 43.75 (flat) / 44.04 (flat) / 39.04 /
43.93 (flat) / 39.36, +5.3, +3.9, +5.0, +4.6 fps, means 39.36 against
44.06, 2.7 ms of a 25.4 ms frame; Pixel Watch 3 hot at 41.5 C, base
29.60 / flat 31.94 / flat 32.10 / base 29.58 (WATCH PAIRS), about +2.4
fps, 8%. One full-screen gradient rect is that share of the frame on both
tilers even with its stops already inline in the vertex stage.

**What the renderer does.** `opaque_prefix.rs` looks at the first op of a
page pass's first flush, when the pass still carries its clear colour:
it must be a run whose first segment in any lane is the shapes segment,
whose first record is a plain rect (no radii, no stroke, SrcOver in the
record and the segment) under a placement with alpha 1 and no colour
filter, with a solid brush of alpha 1 or a Clamp gradient whose every
stop has alpha 1, whose device edges, computed exactly as the vertex
stage computes them (rect plus placement offset and snap delta, times
the scale, canonicalized to 1/16 px under a snap anchor), are whole
pixels, and whose clip, if any, contains it; no pending composite may
sit at or below its z. The key is `LayerRasterCacheKey::prefix_snapshot`
over a hash of the record bytes, the brush and stop bytes and explicit
positions, the placement offset, snap, clip, the exact scale bits, the
clear colour bits, the composition format and the device rect and page
origin, so any change to what the bytes depend on is a different key.

A gate per draw command (the same patience gate the backdrop cache uses,
now `AdmissionGate`) admits a key on its second consecutive frame. On the
admitting frame the page's first pass is split: "Layer Pass Prefix" draws
the record alone over the clear, `copy_texture_to_texture` takes the
device rect into a retained texture of the page's format, and the main
pass loads and draws the rest of the run from its second record. On
every later frame with the same key the main pass keeps its clear and
draws a nearest-sampled composite of the retained bytes at the same
integer rect ahead of the run's remaining records. The run's record
window (`PassSegment::first_run_window`) reaches both the stored path,
where each segment's draw call range is clamped, and the arena path,
which appends from the window's start and stops at its end.

**Why it is exact.** The cached bytes are the bytes the draw produced in
place over the same clear colour, taken by a same-format copy. On a hit
the composite samples them nearest at texel centres, so the source is
the stored value, and it is opaque in the page's own format, so src-over
is src + dst * 0 and the blender's conversion of the source is the
identity whatever precision it works at. Nothing is re-rendered and
nothing is resampled. A retained *result* of a translucent or masked
composite does not have that property, which is why the backdrop result
cache's admissions are a separate question (below).

**Gates.** `opaque_prefix_cache.rs` renders every frame twice, through a
caching renderer and a second renderer that never caches
(`CRANPOSE_NO_FILL_CACHE`), and requires byte identity: a three-stop
radial ahead of forty moving stars on a layer at (8, 6), at scales 1.0,
1.5 (the rect then runs past the page edge and only the intersection is
retained) and 3.0, a five-stop gradient (the uniform stop walk) and a
solid fill; the stats prove the path runs (no admission on the first
frame, one on the second, one prefix hit on every later frame, and the
warm frame's shape fill smaller than the reference's by the rect); a
changed stop colour misses, is watched for a frame, re-admitted on the
next and served with the new bytes; and a translucent stop, a Repeated
tile, a stroke, corner radii, a fractional edge, a record that is not
first, a translucent layer and a run with an effect-range sibling
composited beneath it are never admitted. Each requirement was broken on
purpose and the suite went red: the stop bytes left out of the key (the
changed colour served the old bytes), the opacity requirement dropped
(the translucent stop admitted), the cached record redrawn on a hit (the
warm fill equal to the reference's), and the composites-beneath guard
removed (the split admission frame drew the effect over the prefix,
4,878 bytes off). The toggle is `debug.cranpose.no_fill_cache` on
Android, so one APK measures both ways.

**Measured (2026-09-06, early).** One APK per device with the property
flipped per leg, A = `debug.cranpose.no_fill_cache=1`, B = default,
full showcase scroll, 40 swipes in 60 s legs, A B A B then B A B A
without cooling. Mate 20 X: nocache 39.61 / prefix 41.90 / nocache 39.15
/ prefix 42.23 / prefix 41.76 / nocache 39.01 / prefix 41.63 / nocache
37.22 at 39-44 C, pairs +2.29, +3.08, +2.75, +4.41, means 38.75 against
41.88 (+8%, ~1.9 ms of a 25 ms frame). Pixel Watch 3: nocache 40.83 (34
C) / prefix 42.82 (37) / nocache 39.99 (39) / prefix 31.69 (41) / prefix
31.87 / nocache 29.78 / prefix 31.66 / nocache 29.46 (all 41-42 C): the
watch crossed its thermal step between legs 3 and 4, the cool pair is
+2.0 fps and the hot pairs +2.1 and +2.2 on a ~30 fps hot frame (+7%).
The flat-fill bound was +4.6 and +2.5; the rest is the nearest blit of
the full-screen rect and the split pass on admission frames.

## An admitted backdrop keeps its source, not a copy of its result (2026-09-05, night)

Codex's Linux run of `a_cached_glass_result_follows_a_change_beneath_it`
was red by one channel level, and removing one line made it exact: the
`outputs[index] = backdrop_blit(...)` substitution in `admit_backdrops`.
On an admission frame the glass was shaded a second time, whole and
unmasked, into a retained surface (`resolve_whole`, the "Backdrop Result
Pass"), and the frame drew a nearest blit of that surface through the
mask instead of the stage's own composite; every later hit blitted the
same surface. A stored *result* is not the bytes the composite produces:
the composite multiplies its shading by the mask and the alpha and lets
the blender convert the source at a precision the API leaves to the
implementation, so a value rounded once into the retained surface and
rounded again through the blit lands a level away from the value the
composite writes in one step. The prefix cache is exact for the opposite
reason: its bytes are opaque, same-format and unmasked, so src-over is the
identity on them. A backdrop's result never has that property.

**What the renderer keeps now.** The cache holds sources. `LayerCache`
maps a key to `Retained { texture, content }`, where content is
`Surface` for a texture drawn whole for its entry (retained child layers,
opaque prefixes) or `Composite(kind)` for an admitted backdrop. Admission
pins the stage texture the composite reads, the capture atlas or the
blur result the shader reads its substrates from, and stores the
composite's resolved kind: its atlas region, substrate regions, logical
size, shader and sample mode. Nothing is re-rendered, nothing is copied,
and the frame that admits draws the composite it would have drawn anyway;
its content is marked retained so a capture above it can hash it from
that frame on. A hit rebuilds the composite from the retained kind with
the backdrop's current dest, scissor, rounded mask and layer rect
(`replayed_kind`): the same texels sampled the same way by the same
shader with the same uniforms, so a warm frame produces the bytes the
admitting frame produced, which are the bytes a renderer that never
cached produces. `resolve_whole`, `unmasked_composite`, `clear_and_draw`
and `backdrop_blit` are gone.

**Accounting.** A stage texture is shared by every backdrop of its stage,
so the cache charges bytes per texture, not per entry: an
`AllocationLedger` keyed by the texture's pointer counts holders and
charges the texture when its first holder arrives. Eviction is
least-recently-used by entry under the same 96 MB budget; a texture
retires once, when its last holder leaves, into a pending list with the
descriptor it was acquired under, and returns to the transient pool under
that descriptor (a surface returns to the offscreen pool) only once no
frame's composite still holds it. The frame's `release_transients` skips a
pinned texture because the cache's clone keeps `Rc::try_unwrap` from
succeeding, which is the hand-over. The per-frame admission pixel budget
and the patience gate are unchanged; admission now costs a pin, not a
pass.

**Gates.** `layer_cache.rs` unit tests pin the ledger (a shared texture
charged once, retired by its last holder, a re-charge after retirement
starting fresh) and the cache with real textures (two entries replaying
one stage texture pay for it once and retire it once, waiting while
something else holds it; the budget evicts the least recently used entry
and hands its surface back). `CRANPOSE_NO_BACKDROP_CACHE` gives the tests
a renderer that never keys backdrops. `a_cached_glass_result_follows_a_change_beneath_it`
compares the cached cool frame, the frame after the change, the settled
(admitting) frame and two warm follow-ups whole-frame to that reference at
exact zero, and `independent_glasses_are_admitted_over_several_frames_without_changing_pixels`
compares the settled frame and two warm follow-ups at exact zero; both
were `<= 1` before and are red against the copied result. The
still-scene, rigid-scroll, change and stop tests are unchanged.

## A stage flushed past the backdrops still waiting behind it (2026-09-06, early)

The exact-zero test above went red for a reason that was not the cache:
the cold first frame of a fresh renderer, and every frame with backdrop
keys off, drew the last glass row of `glass_layer_cache.rs` over its own
text, blurred. A stage flushes the page up to the highest z among its
items, and stage 3 there holds row 3 (z 10) and the overlay (z 23), so
the flush drew every op below z 23, rows 4 and 5 included. The blockers
that make `release` defer ops covered only that stage's capture rects:
row 4 escaped because its rect overlaps row 3's capture by four pixels,
row 5 did not, so stage 5 captured a page already holding its tint and
text. The cache masked it: a hit's composite is pending before the flush,
so a warm frame was right and only the fresh frame wrong, which is why
the old test at tolerance 1 on the overlay's interior never saw it.

`run_stages` now sets the blockers to every backdrop still waiting to
capture, this stage's and every later one's, before each stage runs, and
drops a stage's blockers once it has captured, so an op above a waiting
backdrop inside its rect is deferred until that backdrop has read the
page. `a_cold_frame_draws_every_row_text_over_its_glass` counts the
bright glyph pixels of "Gla" in every row's text on the cold frame
against row 0's (row 5 had none, row 0 sixty-two) and was red before the
change.

## A backdrop's key names its place in the stage (2026-09-06, early)

Codex's Linux run of the nine-glass admission test was one level off
between the settled frame and a never-caching frame, while the Mac was
exact. The region map a member reads its capture through is
`region.xy / dims` and `region.zw / dims`, so a member's bytes depend on
its slot and on the atlas and side texture sizes; the alone-versus-packed
parity tests have always allowed one level of that drift. Retention
replayed bytes computed under the layout of the admitting frame, and a
hit removes its member from the stage before it is packed, so the other
members' slots moved between the admitting frame and the frame that
served the hit, and their fresh bytes were no longer the retained ones.

**What the renderer does now.** `run_stages` plans every stage over all
its items before any is served from the cache (`plan_stage`): the atlas
placements, the substrate slots and, per atlas, the side packing of blur
scratch slots and substrate slots. The plan is a pure function of the
stage's items, the layout a renderer that never caches computes for the
same frame; misses take their slots from it instead of packing again
(`StageLayout::restrict`), and each member's key hashes its own part of
it (`StageLayout::signature`): its placement, the padded atlas and side
sizes, its substrate slots and its blur slot. A neighbour joining,
leaving or moving in z changes the layout and so the keys, which miss
and are re-admitted by the gate two frames later. Two packing policies
keep hits under a co-member's animation without leaving that purity:
members pack in stage (z) order rather than by descending height, so a
later member's size change moves no earlier slot, and atlas and side
sizes pad in steps of an eighth of the next power of two above them
(never past the packer's limit), so a small size change rarely moves the
dimensions while the padding adds less than a quarter (2072 pads to
2560, +23.6%; a 16-texel floor applies to the smallest).

**Gates.** `backdrop_atlas_parity.rs` adds a stage of nine identified
glasses admitted and settled, then one removed, one added back and the
order rotated, every frame whole-frame at exact zero against a
never-caching renderer (`CRANPOSE_NO_BACKDROP_CACHE`) of the same graph,
each settling back into the cache within a few frames; the nine-glass
test and the animated-overlay test (still rows cached while the overlay's
blur radius pulses) are unchanged and green. Those byte proofs depend on
a sampler that drifts, which Apple's does not and, once the packing
policies above were in, Linux no longer did either: Codex ran the tree
with `layout.hash(&mut hasher)` deleted from `backdrop_cache_key` and
all 24 tests still passed there. The proof that holds on every platform
is `a_shader_reading_its_place_in_the_atlas_is_re_rendered_when_a_neighbour_moves_it`:
a shader is entitled to read its source region (`u[59]`) and
`textureDimensions`, and the probe paints both; six such glasses settle
at an atlas of 2048, a seventh widens it to 4096 and the first then
leaves, every frame exact against the never-caching renderer. Without
the layout in the key the first frame after the seventh replays six
stale results, red on the Mac as on Linux. Padded sizes are bounded by
the packer's limit (`padded_dimensions_step_by_an_eighth...` pins the
steps and a limit that is not a power of two), and a stage allocates an
atlas only when one of its misses lands in it:
`a_stage_spanning_two_atlases_allocates_only_the_atlas_its_misses_land_in`
puts two 2040 x 2030 glasses side by side on a 4200-wide page, so
neither two captures on one shelf nor two shelves fit the 4096 limit and
each capture takes an atlas of its own, repaints a rect under one of
them, and checks the partial-hit frame acquires one atlas and one side
pair fewer than the cold frame and is exact; acquiring the atlas before
the membership check turns it red.

## Order

Every step ships with its contract proven red first, the robot suite
green, and both scenes measured on the device with the delay probe at
zero, alternating rounds, temperature logged; the watch with the
`.bench` build, 60 s, p50 and p90.

1. Pinned revisions and baselines: cranpose-showcase 4ad4080 (v0.1.12),
   cranorbit 0334e16, the scripts in the session scratchpad
   (`measure.sh`, `orbit_measure.sh`, `orbit_measure_dev.sh`). Done.
2. Page usage ablation on the showcase: done, no gain (23.9-24.0 against
   24.0-24.5 fps); the toggle is gone, the number is in `TIME_WASTERS.md`.
3. Shape pipelines per (blend mode, stage, variant), batches cut on brush
   class: done, cranorbit at the vsync on the Mate 20 X (61.0 / 61.0 fps
   against main's 58), pixel-exact.
4. The record: `ShapeRecord`, lanes, per-command stop table, coalesced
   segments, bounds and summary produced while recording, the
   fingerprint on first use, the materialising iterator. Gates: every
   `DrawScope` call materialises to today's `DrawPrimitive` byte for
   byte (`record.rs` tests, red-proven); the summary and coverage rects
   equal a scan of the primitives; the layer hash reads the fingerprint.
   Done 2026-09-04. Measured on the watch core with a 17,600-call
   microbenchmark (`recbench` in the session scratchpad): the scope's
   record path went from 708 ns per call to 313 (arcs 372, rects 252)
   after the tight arc bounds moved to derivation on demand (191 ns),
   the trig row to the vertex stage (67), the fingerprint to first use
   (73) and the segment key to one integer compare (70); the arena's
   scene stage went from 19.1 to 11.4 ms. Collect rose from 16 to 22.5
   ms because materialisation now derives the arc bounds; step 6 removes
   the materialisation with it.
5. The compact GPU path: placement uniform ring, vertex-stage
   canonicalisation, colour matrix and paint order in the shader.
   Gates: the seven `record_path_goldens.rs` scenes captured from the
   CPU conversion before the landing (arena, scaled arena, clipped
   primitives, translated thin shapes, painted layers, blend modes,
   shadows), the snapping and translated-gradient byte-identity tests
   (`effect_semantics.rs`), the variant parity. Done 2026-09-04 with 6.
   The goldens caught two real defects on the way: the paint quantized
   every colour where the CPU path quantized only painted ones (one
   level in 3% of the arena's pixels), and a loose run closed during a
   later push took that push's snap anchor (a one-pixel shift of the
   background rect).
6. The run store with its arena tier, run references in `collect` and
   the pass, the vertex-stage bands with their coverage proof
   (`run_geometry.rs`), and the removal of `merge_items`, `shape_run`,
   the CPU conversion, `ShapeData` and `band_mesh.rs`, in one landing.
   Gates: the cancellation test (`cancellation_contract.rs`, red-proven),
   `band_fill.rs` budget and pixels, the goldens, the wgpu suite. Done
   2026-09-04. The first watch run showed the fill estimate walking every
   band vertex with trig on the present thread (render stage 128 ms);
   the strip area is analytic now. The first device runs then presented
   4 fps on the watch and 1 on the Mate 20 X, whose Mali lost the
   device on the first frame: bands bucketed by radius alone gave the
   arena's short bricks 2.53 M vertices a frame (the `shape_verts` stat
   was added to see it). Bands now take the segments their padded
   sweep needs and `band_pays` charges the vertices: 137 k vertices,
   Mate 62 fps. On the watch the remaining gap to main is the recorder
   itself (simpleperf through `--app`: `push_arc_band`, `band_pays`,
   `push_shape`, the 112-byte record copies), trimmed to one
   `Arc::make_mut` per record, tables re-allocated at their old
   capacities when a scene still holds them, the fingerprint and the
   fill estimate computed only when asked: scene 15.4 -> 11.6 ms. A
   stored run's tables are then re-written only in the 4 KB chunks that
   differ (`run_store_upload.rs`, red-proven), which the orbiting boss
   rings do not exercise: every arc's angle changes each frame and the
   store uploads 1.8 MB whichever way it compares. The recorder path,
   profiled per line on the watch itself (`simpleperf record -- recbench`
   with line tables, `annotate.py`), lost its by-value struct copies
   through the scope layers, the division and loop in the band decision,
   the NaN-ordering `f32::min` in the bounds (a libm call per edge on
   armv7) and a second bounds union: 588 -> 369 ns per arc, 346 -> 289
   per rect on the watch core; scene 11.6 -> 8.6 ms, update 16.8 ->
   13.6.
7. A/B/A/B against main, 30 s windows, SurfaceFlinger presented frames,
   2026-09-04. Mate 20 X: main 60.6 / 56.1 fps, branch 62.2 / 62.0 (the
   vsync; period p50 16.6 ms both runs). Pixel Watch 3, run back to
   back while it heated: main 51.3, branch 25.8, main 31.5, branch 16.4;
   the branch presents half of main's frames whatever the temperature.
   With the recorder trimmed its period stayed 28.8 ms while the update
   stage fell from 16.2 to 13.6 ms: the frame is a chain, the update
   thread waits on the scene, the scene waits on the render, and the
   render's pass spends 6.8 ms p50 (`[wgpu-render-stage:run-upload]`)
   comparing and staging the 1.9 MB the orbiting rings change every
   frame, before the present wait of 10 ms. The wait is the GPU:
   `debug.cranpose.pass_timing` puts the watch's Layer Pass at 29 ms a
   frame against main's 14-18, and skipping the band draws leaves 15.8
   ms, skipping the quad draws 21.4, with every band a single quad
   (radii under 96 px take one segment), so the cost is fill, 8-9 ms per
   megapixel of this fragment program, and main wins on fill. The
   strip's angular pad was the waste: a constant 0.05 rad plus asin at
   the mid radius, 65% more length on a 0.2 rad brick edge; it is now
   the angle the padded half-width subtends at the padded inner radius
   plus float slack, in the shader, the CPU mirror and the estimate, with
   the coverage proof widened to fat rings, thick caps and small radii:
   arc fill 1.64 -> 0.78 MP, the pass 29 -> 24.3 ms, 41 fps presented.
   Two probes then said what the rest is not: with no draws the pass is
   0.28 ms, and `fs_solid`, a brushless 8-vector varying set for solid
   batches (built, kept, byte-identical by the variant parity test),
   leaves the pass at 25 ms, so neither a fixed cost nor varying traffic
   carries it; a fragment stage returning a constant leaves 14 ms, so the
   fragment program is 11 ms and the rest is vertex work, raster and tile
   traffic. Reading the draw structure for that found a correctness hole
   with it: a segment drew its quads first and its bands after, so a rect
   recorded over a band-drawn ring ended up under it
   (`a_rect_recorded_after_a_banded_ring_covers_it`, red-proven). Now a
   record carries its band class in its flags and each segment is one
   draw at its largest class's vertex budget per record, in record order:
   the band lists, their table binding and the collapsed quad draws of
   banded records are gone. Cutting a segment at every class change gave
   MEGA BOSS 91 draws and a slower pass (28.4 ms against 24.4: on this
   GPU a draw call costs about what a few thousand collapsed vertices
   do), so a segment takes a record of another class while the quads its
   budget leaves collapsed stay under `SEGMENT_WASTE_QUADS` and is cut
   past that (unit test on the cut point): 10 draws, 105 k vertices, and
   still 27.2 ms against fix8's 25.0 in the same alternation. Turning
   the clip and rect rejections into a zero fragment under blends where
   one is a no-op, on the theory that the `discard` cost the tiler its
   early fragment paths, measured 28.5 / 25.0 / 28.2 against fix8 and
   moved five pixels of the clipped-halves parity by 1/255: the
   alpha-cutoff `discard` is in every program on main as well, so no
   pipeline was discard-free to begin with, and a zero fragment still
   blends, through the sRGB encode of the target. Rejection stays a
   `discard`. Ablation on the watch, alternated against fix8's 25.0:
   fix12 (unified draw, budget cut) 27.1; without the strip's rect test
   in every pipeline 26.2; without the segment cuts (three draws, every
   record at the largest class) 83 ms. The last one prices a vertex
   invocation: 6.6 M collapsed vertices, each a flags load and an
   early return, cost 56 ms, about 8.5 ns each, so this GPU shades some
   120 M vertices a second and the 104 k real ones, each fetching a
   record and a placement and deriving its varyings, are the pass's
   second half. Main's vertex stage is a copy of precomputed data. The
   lever is invocations: shared vertices through a per-class index
   buffer (four a quad, 2(n + 1) a strip) and a quad-only class-0
   pipeline. The first indexed build repeated the pattern per record
   (50 MB of indices for the widest class at the store's capacity) and
   collapsed surplus vertices to the screen centre; both went with the
   instanced draw and the pinned collapse above. The record carries the
   trig row and the padded sweep again: 67 ns per arc on the watch core
   (1.2 ms of the scene stage) against eight transcendentals per vertex
   invocation, and the GPU pass is the binding stage. Alternated on the
   watch: fix8 25.0, instanced 20.4 / 20.2 / 20.1, fix8 25.0 ms, ten
   draws, the same fill; main's pass was 14 to 18. A constant fragment
   on top of it, alternated: 20.2 / 18.6 / 29.1 (throttling) / 27.3 ms,
   so the whole fragment stage, distance fields and blending over 1.7 MP,
   is 1.6 ms of the pass, and the 18.6 ms floor is the vertex stage and
   what the tiler does around it: ~104 k invocations each loading a
   112-byte record and storing eight vectors of varyings, run again per
   bin (that early return also lets the linker drop every varying the
   fragment no longer reads, so the 18.6 covers neither varying traffic
   nor per-pixel work; the earlier 14 ms "constant fragment" kept the
   coverage and its discard). Folding the canonicalisation and the
   paint out of the vertex stage on top of it moves nothing (18.3 /
   17.9, 18.7 / 18.5), so the floor is not that arithmetic either; what
   is left is the record loads, the primitive processing and the
   binning. A flat indexed draw (the pattern repeated per record,
   `base_vertex` at the segment start, no instancing) is slower again:
   20.2 / 24.8 / 28.6 / 34.5 ms, the same vertices and shader, so this
   front end is bound by what it does per vertex and per index, and
   instancing, which fetches six indices once, is the cheapest draw
   form it has. One triangle over the rect instead of the quad's four
   vertices (a quarter fewer invocations, twice the quad's fill, the
   rect test back in the class-0 pipeline) is slower too: 20.2 / 22.7,
   28.8 / 32.0. So neither pixels, nor vertex arithmetic, nor vertex
   count, nor varyings move the 18 ms floor; what the four probes leave
   is the per-record work the tiler does for 17,600 instances and the
   record loads behind them, and the frame is CPU-bound at 28.8 ms
   anyway (update 13.6, upload 6.8 cool), so the CPU side is the next
   cut whatever the GPU floor turns out to be. Of the upload, the 4 KB
   chunk compare is free: uploading the whole table without comparing
   measures the same or worse (run-upload 4.2 / 5.1 / 5.2 / 8.2 ms as
   the watch heated), so the cost is `write_buffer` moving 1.85 MB into
   staging at some 400 MB/s, and only fewer bytes cut it: a smaller
   record, then per-frame instances beside immutable templates. The
   review that
   found the collapse (2026-09-04) also holds the rest of the list:
   the fingerprint hashed a segment's lane and range only (fixed: whole
   segments); the vertex stat charged each record its own class where
   the draw charges the segment's (fixed); `band_bucket_for` decides
   segments and the one-pixel margin in the command's units, before the
   root scale is known, so a scaled-up arc gets fewer segments and a
   wider overshoot than the shader's device-space strip wants, and a
   small arc that scales to forty pixels stays a quad; the store writes
   1.97 MB of records a frame for 17,600 arcs whose useful change is a
   few floats each, so the next record format keeps immutable
   templates apart from compact per-frame instances; `scene_budgets.rs`
   gates cached layers and never the 17,600-changing-record arena, which
   needs a gate of its own on indices, invocations, upload bytes and
   pass time. After that the levers are the fragment program's cost
   per pixel (the interior of a band shaded without the distance field,
   the analytic coverage only on its fringes), the stored run's records
   written once into GPU-visible memory instead of recorded, compared
   and staged, and the update of frame n+1 overlapping the render of
   frame n instead of waiting on it.
   Then the showcase's exact GPU steps (interior split, shadow support
   at r, blur variants, substrate) and the material decisions, each with
   its number.

8. The plan of 2026-09-05 (evening), in its order: the watch encode
   delay probe (done: no response, the GPU is the bound), the full-scroll
   gesture on both devices with the card-scene pass inventory, then the
   static fill raster cache and the lens's proven output domain, the
   material at substrate resolution as a feasibility experiment under
   unchanged exact tests, and, on the recording side, Codex's prepared
   dynamic runs; the joint plan is `docs/mobile_60fps_architecture.md`. Each with its gate red first, the robots
   green, both scenes hot A B A B and B A B A on the full-scroll gesture.

## A glass tap gate loses on Adreno (2026-09-06)

The showcase material is `Glass::regular()`: rim style 0, activity 1,
dispersion 1, adaptive frost 0.42, no optical blur. Per pixel it fetches
the plain backdrop, three transmitted paths, the frost substrate, and in
the rim draw five reflection taps. Two of those fetches are dead on most
pixels: at activity 1 the plain backdrop only reaches the output through
the outer coverage, zero wherever coverage is exactly 1, and with rim
style 0 the reflection only reaches it through the bevel term under that
same outer coverage. Gating both fetches behind their weights was
byte-exact against the frozen reference on Metal, Adreno and Linux, and
lost every stable pair on the Pixel Watch 3 (-2.21, -1.75, -2.02 fps on
a 31 fps hot frame, same route, eight legs). The skipped taps read the
neighbourhood the transmitted taps already brought into the cache, so a
divergent branch around a fetch costs more than the fetch and keeps the
compiler from issuing every fetch at the top of the shader. The gates
are not shipped. What stays is the reference scene they were proven
against: a lens-variant card (rim style 1, every reflection tap live) and
a resting card (activity 0.5, the plain path live), so any later edit to
either path is judged on both branches.

