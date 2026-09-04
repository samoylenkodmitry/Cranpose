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
its whole disc for a band a few pixels wide. `band_mesh.rs` turns every
plain solid, unclipped, axis-aligned stroked circle and arc band whose quad
exceeds `BAND_MESH_MIN_QUAD_PIXELS` into an annular triangle mesh with one
pixel of slack around the band; a batch holding one band draws every shape
as triangles through `vs_mesh`, the other shapes as their quads, with the
same fragment stage. The stat `shape_fill_pixels` counts what the shape
draws rasterize. `band_fill.rs` pins the budget (a ring costs its band,
not its disc) and the pixels (a meshed ring matches the ring drawn as two
clipped quads to interpolation rounding); the unit tests in `band_mesh.rs`
walk every pixel center the shader would shade and assert the mesh holds
it. On the Mate 20 X, cranorbit's MEGA BOSS arena went from 16 MP to 12 MP
of shape fill and from 15.9 ms to 10.6 ms present with this.

## Uploads and passes

- `ViewportUniformRing`: one uniform buffer with dynamic offsets. Every pass
  and every retained glyph run claims a slot; the whole ring is written once
  per frame.
- Shape and image batches write into per-frame slot pools
  (`shape_slots`, `image_slots`), trimmed after the frame to what it used.
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
effect applies and the pixels the runtime shaders shade (`shader_pixels`,
the number that decides a glass frame's cost on a tiling GPU).
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

## Architecture: less GPU work per frame, then one record retained by identity

The gate on the phone is the GPU on both scenes, so the GPU steps come
first and are measured first; the CPU steps that follow remove the work
the watch and the battery pay, and they are the same steps whether the
frame overlaps or not.

**Cranorbit: the shape fragment program pays for what it does not draw.**
`shape.wgsl` carries every kind (fill, rounded fill, stroke, arc band),
every brush (solid, three gradients, four tile modes, dither) and the
clip in one fragment program, and `fs_solid` already showed that leaving
the gradient path out of a solid batch pays on Mali. The arena is arcs
and solid discs. The fix is the one the glass got: a pipeline per (kind,
brush class, blend mode) selected per draw, with the flat varyings each
variant does not read removed from its vertex output, and the arc-band
variant's coverage computed from the record's radii without the branches
of the other kinds. Contract: byte parity against the general program
per variant (`glass_specialization_parity.rs` is the shape). Measured
per variant on the device with the delay probe held at zero, since the
gate is the GPU; the estimate from the glass precedent is a third of the
fill cost. What remains after that is pixels: the band mesh's one-texel
margin on a band a few pixels wide is 20-40% of its fill, and the quads
of the discs and radial falloffs are 27% more than their circles; the
mesh's margin is measured by the same walk that pins the mesh, and the
disc quad becomes an octagon in the vertex stage, both pixel-exact.

**The showcase GPU, in the order of the milliseconds.** The page usage
ablation first. Then the glass's lens field (the position-only terms
rendered once per geometry and material into a retained texture, read
per frame under a content pass of ~20 taps and tone; its own cost is a
texture read of ~2-3 ms of bandwidth per frame and one render per
distinct card geometry, counted in the budget), the blur pair at the
scratch scale with the reference model in `blur_reference.rs`, and the
shadow bands trimmed to their visible alpha extent. Each is a same-binary
ablation with cold and warm frames, added passes, retained bytes and
cache misses during the scroll reported beside the period.

**Then the CPU, as work removal.** The draw scope writes the GPU shape
record once (the layout `shape.wgsl` reads, 176 bytes today and ~112
without the device quad, clip and dither origin), in the layer's local
space, into the buffer the draw command retains; `collect` emits a run
as one op with its placement (the layer-to-device affine with the snapped
translation, scale times root scale and rotation, a local clip rect
evaluated through the inverse affine, the dither origin and the pixel
scale for anti-aliasing) instead of copying its shapes; the encoder draws
a run's segments, cut where kind or blend mode changes, from a retained
arena range and uploads only runs whose content changed. Identity is the
`DrawCommandId` plus a content fingerprint of the record bytes, so a
dirty command that re-records identical bytes keeps its range and its
backdrop-cache hits, and two runs at one placement never collide; the
capture hash of a run is that fingerprint and its placement relative to
the capture. Gradient stops live in a second arena addressed by the
record, with the same range lifetime. A child whose transform is a
similarity and whose content is shapes draws direct through its
placement, as a Compose `RenderNode` replays its display list under its
matrix; effects, group alpha, blend modes, explicit offscreens, rounded
clips the content does not admit and pixel-sensitive content still
isolate. Arc bands come from the vertex stage as ring strips whose
length is the largest segment count a draw's records need, so no CPU
tessellation and no vertex upload. The remaining per-frame bytes go
through one upload ring per usage class. The software renderer keeps
consuming `DrawPrimitive` for what it draws; the shape record is the
wgpu renderer's, and the contract between them is the primitive emitter
both already share. On the phone this moves nothing the vsync sees; on
the watch it is the frame, and on the phone it is the battery.

## Review of this design

- The second draft added stage p50s into a frame and read `present` as
  GPU time; the repository's own attribution guide forbids both, and the
  delay probe refuted the model: the CPU overlaps the GPU by a frame on
  this driver. Both scenes are GPU-bound, cranorbit at ~19 ms, not 8.
- The single-thread requirement stands as a design constraint (the
  watch), and it changes nothing on the phone: the CPU is already under
  the vsync there, and the GPU does not care how many threads fed it.
- Retention by generation was unsafe: a re-record can be byte-identical
  and two runs can share a generation. Identity is now command plus
  content fingerprint.
- A rotated direct child needs its rectangular clip through the inverse
  affine; the shader's device clip is axis-aligned. The placement carries
  the local clip.
- Segments cut on blend mode as well as kind; a pipeline's blend state is
  fixed at creation.
- The record is 176 bytes today, not 160; gradients need their own arena
  and lifetime; the pixels renderer is a second consumer of primitives.
- The GL measurement is not evidence for a backend change; it is evidence
  that the Vulkan page configuration costs something the design must
  measure before it budgets.
- Neither target workload is pinned: the showcase is an external
  repository and cranorbit is another; every number here names the day
  and the build, and the order below starts by pinning both revisions
  and their scripted inputs so a step's number is a comparison, not a
  recollection.

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

Cranorbit reaches the vsync at 11-13 ms of GPU with its CPU cycle at
~11 ms on the present thread, which is under the vsync and beats main's
period. The showcase does not reach it with pixel-exact steps: 24.5 ms
is ~41 fps, and the 8 ms to the vsync are the page usage ablation's
unknown, the four strata (~4 ms this GPU charges per full-page pass, not
reducible without a render-area wgpu does not expose), and the material
decisions (the frost neighbourhood at a quarter resolution, the tone
curves as a lookup, the lens field's interior at half resolution), each
a bounded pixel change with a reference model and each the user's call.
This plan does not assume any of them; it states the number they would
have to earn.

## Order

Every step ships with its contract proven red first, the robot suite
green, and both scenes measured on the device with the delay probe at
zero, alternating rounds, temperature logged.

1. Pin the showcase and cranorbit revisions and their scripted inputs;
   record the two baselines above with the probe method.
2. Page usage ablation on the showcase.
3. Shape pipelines per kind, brush class and blend mode; varyings
   trimmed per variant. Cranorbit's gate.
4. Band-mesh margin and disc octagons. Cranorbit's margin to the vsync.
5. Lens field, blur at the scratch scale, shadow extents. The showcase's
   exact steps.
6. Records from the recorder, the run store, placements, run segments,
   bands from the vertex stage, the upload ring. The watch's frame and
   the phone's battery.
7. Measure; report what remains on the showcase and what each material
   decision would earn and cost.
