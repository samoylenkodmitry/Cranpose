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
   each region downscaled into a scratch texture, the vertical pass writes
   it at full size into a result texture, and both textures are packed to
   the blurred regions alone, so no pass loads or stores the atlas (on the
   Mate 20 X the atlas-sized pair cost 9 ms of bandwidth per frame);
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
captures follow. Nothing visible changes. Not cropped: a cached child
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
