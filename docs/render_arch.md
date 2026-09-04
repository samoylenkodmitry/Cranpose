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

## Why the frame is not 60 fps (read 2026-09-04, second pass)

Mate 20 X (Mali-G76 MP10, 1080x2143, Vulkan, 8-bit page drawn straight
into the swapchain image), `[android-frame]` p50 in ms. `update` is
compose, layout and the scene graph patch; `render` is the encode (plan,
record, finish, submit); `present` is the `frame.present()` call; the
collect sits inside `acquire` and is under 1 ms on both scenes.

| scene                       | fps  | period | update | render | present | uploads | GPU stats |
| --------------------------- | ---: | -----: | -----: | -----: | ------: | ------: | --------- |
| showcase scroll             | 24.2 |   40.0 |    4.5 |    7.7 |    32.0 |  0.1 MB | 5 passes, 17 copies, shader 2.8 MP, shadow bands 5.7 MP, 110 draws |
| cranorbit MEGA BOSS         | 52.4 |   18.7 |    6.9 |    9.9 |     8.1 |  4.5 MB | 1 pass, 1 draw, 11 MP shape fill |
| cranorbit MEGA BOSS on main | 58.0 |   17.2 |    6.1 |    6.4 |     1.7 |  0.3 MB | 1 pass, 29-40 draws |

The present call returns only when the GPU has drained the frame: under
`Mailbox` the showcase presents in 31.6 and cranorbit in 7.4, the same as
`Fifo`, so the mode is not the cause, and the producer's credit of two
changes nothing because the thread that encodes is the thread that waits.
On one thread the frame is therefore the sum `update + collect + encode +
gpu`, and the vsync paces it only once that sum is under 16.7 ms. That is
the requirement, for both scenes, on this device and on the watch.

Main's 58 fps on cranorbit came from doing less work per frame on the same
thread structure, not from more threads. Its recorder kept a typed tape per
draw command (`record_replay.rs`, `normalized_scene.rs::try_command_feed`),
compared each frame's records against it, derived a similarity transform
per segment from an anchor pair, and had the GPU replay retained shape
slots through that transform with color patches, so a spinning ring cost
one transform and no upload. It was a diff with tolerances, acknowledged
across the present thread, and it was removed with the rewrite. What the
rewrite left is the opposite: every primitive exists three times per frame
(`DrawPrimitive` from the recorder, `DrawShape` copied by `collect`,
`ShapeData` converted by the encoder), the arc bands are tessellated on the
CPU with polygon clipping (`band_mesh.rs`) and their vertices uploaded, and
4.5 MB reach the GPU every frame through `queue.write_buffer`. On the
showcase the encode is the same overhead at a smaller scale: 105 to 156
buffer writes and 25 to 33 pass records for 110 draws, finish 2.2 and
submit 3.3 ms of the 7.7. The showcase's GPU is the glass (14 ms of
position-only arithmetic over 2.8 MP), the blur pair (8 ms, the vertical
pass at full size), the cached shadow bands (5.7 MP composited, mostly a
Gaussian's transparent tail) and the copies (2 ms), and its CPU is the
4.5 ms `update` of a scroll frame plus the encode.

One more measured fact the design must explain before it budgets: under
the GL backend the showcase scroll presents at 33.5 fps (period 30, the
GPU's wait moved into the submit so `render` reads 27.7 and `present`
1.4), while cranorbit gets slower (`render` 27.8 against Vulkan's 9.9 +
8.1). GL is not a lever. What the showcase number says is that the same
passes cost ten milliseconds less through GLES on this driver, and the one
configuration difference is the page: on Vulkan it is the swapchain image
with `COPY_SRC | TEXTURE_BINDING` usage, which on Mali can cost the image
its framebuffer compression, so every stratum, copy and glass read of it
would pay uncompressed bandwidth. That is an ablation (an offscreen page
and one blit against the direct root), not a plan item, and it runs first
because its answer can move the showcase budget by up to a third.

The showcase's `update` on a scroll frame splits (`frame_stage_ms`, p50 of
the frames that did work) into layout 0.7 and the scene phase 3.2 ms: the
scene graph patch of the scrolled layer and what follows it, not the
lazy list's measure. That is where the update target is earned.

## Architecture: one record, retained by identity, drawn through its placement

Single thread. Every step removes work; nothing is hidden on another core.

**One representation.** The draw scope records shapes straight into the
GPU record (`ShapeRecord`: the layout `shape.wgsl` reads, 160 bytes, in the
layer's local space at scale one), appended to the buffer the draw command
retains between frames. `DrawPrimitive` stays for what is not a shape
(text, images, shadows, the content marker); a blend-mode wrapper becomes
a field of the record. Nothing converts a shape again: `collect` does not
copy it, the encoder does not translate it, and the record buffer is the
upload's source. A record holds no device coordinates. Where the frame
puts it is the **placement** of the draw: the layer-to-device affine (the
subtree's snapped translation, the uniform scale times the root scale, the
rotation), the device clip, the dither origin and the pixel scale for
anti-aliasing, one uniform per draw at a dynamic offset. The shaders
evaluate the SDFs in local units and scale the distance to device pixels
by the placement's scale, and the analytic box coverage for a plain rect
does the same. The pixel-stability contract holds by construction: a rigid
scroll changes the placement's whole-pixel translation and nothing else,
so the record's arithmetic is identical frame to frame; the canonicalised
device coordinates that `convert_shape_into_slots` computes per shape today
exist to make that true after a snap, and with the snap in the placement
they are not needed. A record loses what the placement now carries (the
device quad, the clip, the dither origin), 112 bytes instead of 160, and
every consumer of a run's primitives (the rounded-clip admission, the
coverage rect, the run summary) reads the record's rect. The tests are
the ones that exist: `effect_semantics.rs`, the robot fixtures at their
tolerances, and the liquid scroll contract exact; the arithmetic moves
from device to local space, so a fixture may move by an ulp's rounding
and no more, and the scroll contract may not move at all.

**Retained by identity.** The scene graph already re-records only a dirty
node's draw commands and keeps every other `DrawRunNode` with the same
`Rc<Vec<..>>` across frames; a run gains a generation that its re-recording
bumps. The renderer's `RunStore` maps `DrawCommandId` to a range of one
arena buffer, its generation and its local bounds; a run whose generation
matches uploads nothing, a run that changed uploads its own bytes, and the
frame's uploads are one staging write and a copy per changed range. Runs unreferenced for
a hundred frames free their range. `collect` emits a run as one op
(`DrawOpKind::Run { command, placement }`) at its z, so a scene of ten
thousand shapes is ten ops, and the capture hash of a run is its generation
and its placement relative to the capture, not a walk over its primitives.
Text, images and shadows keep their present path; they are tens per frame,
not thousands.

**Rotation and scale draw direct.** A child layer whose transform is a
similarity and whose content is runs and shapes draws through its
placement in the parent's pass, exactly as a Compose `RenderNode` replays
its display list under its matrix; only an effect, a group alpha, a blend
mode, an explicit offscreen, a rounded clip its content does not admit, or
pixel-sensitive content (text, images) still isolate into a surface. A
spinning ring is therefore vector-exact at every angle and costs one
placement, where today it is a surface render plus a projective composite
of a raster, or a full re-record.

**Bands in the vertex shader.** An arc band or a stroked circle draws as
an instanced ring strip: the vertex stage takes the segment count from
the record's outer radius with the same overshoot and margin rule
`emit_band` applies, and computes each vertex from the record; no CPU
tessellation, no polygon clipping, no vertex upload. The strip length of
a draw is the largest segment count any of its records needs, decided
when the record is written, so a ring of small bricks costs a few vertices
each and a full circle sixty-four; the varyings stay flat per record as
they are now, so the fragment stage fetches nothing. Quads stay quads. The
pixel-center walk in `band_mesh.rs`'s tests becomes a GPU parity test
against the quad path (`band_fill.rs` already compares the two), and the
fill statistic keeps its meaning.

**Shape pipelines specialised per run segment.** A draw command mixes
kinds (cranorbit's arena records arcs, dots and gradient falloffs in one
closure), so the recorder cuts a run into segments where the kind changes,
in order, and each segment's draw selects the pipeline whose fragment
stage carries only that kind: solid arc band, solid fill, solid stroke,
and the gradient variants. A ring of bricks is one segment. `fs_solid` is
the pattern and the Mali uber-shader finding is the reason;
`glass_specialization_parity.rs` is the contract's shape. On WebGL the
arena is a uniform buffer and a segment draws in windows of 93 records at
dynamic offsets, which is the batch limit it has today.

**One upload ring per frame.** Every remaining per-frame byte (viewport
and placement uniforms, effect uniforms, glyph and image vertices, the
changed run ranges) is sub-allocated from persistently sized buffers per
usage class and written once at the end of the encode; the
`upload_writes` on the submit line become at most one per class.

**Update phase.** The scroll frame's 3.2 ms scene phase is attributed
with `update_stage_ms` and `scene_update_diag` before it is changed: the
translation patch of the scrolled layer is meant to be a walk that touches
one layer, and the hit graph, the raster hashes and the changed-node list
must not be rebuilt for a translation. The target for the whole update is
2 ms.

**The showcase's GPU** keeps the three exact steps: the glass's lens field
rendered once per geometry and material and read per frame (the content
pass is ~20 taps and tone), the cached shadow bands trimmed to their
visible alpha extent, and the blur pair at the scratch scale with the
reference model in `blur_reference.rs`. The page-usage ablation above
comes first because it may move every remaining item.

## Review of this design

What the first draft got wrong, and what reviewing this one changed:

- Threads were the first item; they hide work rather than remove it, the
  watch has no cores for them, and the present call blocks the encoding
  thread whatever the credit. Gone.
- "Retained records are not planned because Compose re-records a Canvas"
  was false: Compose retains the display list of every unchanged node and
  replays it under the node's matrix. Retention by identity and drawing
  through the placement is that model.
- Local-space records were checked against the pixel-stability contract
  and against `convert_shape_into_slots`: the per-shape canonicalisation
  exists to survive the snap; the placement carries the snap, so records
  are invariant and the canonicalisation is deleted, not moved.
- Per-run draws replace per-batch draws: a frame becomes tens to a few
  hundred draws with a dynamic offset each, which is where main sat
  (29-40) and within wgpu's per-draw cost; consecutive runs of one kind
  share their pipeline. The alternative, indexing a placement per record
  to keep one draw, costs every fragment a uniform fetch and was rejected.
- Cranorbit without any app change still re-records the whole arena each
  frame; the plan is measured against that case first (record straight
  into the buffer, one write, no conversion, no tessellation), and only
  then with the arena's spinning groups as `Canvas` children under
  `graphicsLayer { rotationZ }`, which is the Compose structure for a
  rigid group and makes a ring cost a placement. Main's tape diff found
  that structure by comparison; the scene graph states it.
- The GL measurement is not evidence for a backend change; it is evidence
  that the Vulkan page configuration costs something the design must
  measure before it budgets.

## Budget, single thread, p50 ms

The `after` columns are estimates from the code read and the ablations
above; each step replaces its estimate with the device's number or is
reverted.

Cranorbit MEGA BOSS, arena re-recorded every frame:

| stage   | now  | after | why |
| ------- | ---: | ----: | --- |
| update  |  6.9 |   3.0 | the closure writes 112-byte records once; the graph patch touches the dirty node only |
| collect |  1.0 |   0.2 | run ops, no copy |
| encode  |  9.9 |   2.0 | one write of the arena, no conversion, no tessellation, one draw per run |
| gpu     |  8.1 |   5.0 | same fill through a kind-specialised pipeline, band strips from the vertex stage |
| total   | 26   |  10   | vsync-paced 60 fps; main's 14.6 |

With the spinning groups as rotated children, update and encode fall to
about 1 ms each and the upload to the bricks that changed.

Showcase scroll:

| stage   | now  | exact steps | with material decisions |
| ------- | ---: | ----------: | ----------------------: |
| update  |  4.5 |         2.0 |                     2.0 |
| collect |  1.0 |         0.5 |                     0.5 |
| encode  |  7.7 |         2.5 |                     2.5 |
| gpu     | 32   |        14.5 |                    11.5 |
| total   | 45   |        19.5 |                    16.5 |

The exact steps land at ~50 fps. The last three milliseconds are the
adaptive-frost neighbourhood at a quarter resolution, the tone curves as
a lookup, or the lens field's interior at half resolution, each a bounded
pixel change with a reference model, and each the user's call; the page
usage ablation may change this table before any of them is needed.

## Order

Every step ships with its contract proven red first, the robot suite
green, and both scenes measured on the device against this table; a step
whose number does not move is reverted.

1. Page usage ablation on the showcase (offscreen page and blit against
   the direct root). A measurement, half a day, before the budget is
   trusted.
2. Records from the recorder, the run store, run ops, placements. The
   renderer's output must not change: every robot fixture, byte for byte.
   Cranorbit's encode and update move here.
3. Bands in the vertex shader; `band_mesh.rs`'s CPU path goes.
4. Kind-specialised shape pipelines per run segment.
5. Rotated and scaled children direct through the placement.
6. The upload ring.
7. Update-phase attribution and the scroll frame at 2 ms.
8. Lens field, shadow extents, blur at the scratch scale.
9. Measure; report what remains and what each material decision costs in
   pixels.
