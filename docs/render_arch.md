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

## Why the frame is not 60 fps (read 2026-09-04)

Two scenes on the Mate 20 X (Mali-G76 MP10, 1080x2143, Vulkan, FIFO, 8-bit
page drawn straight into the swapchain image), present-thread telemetry
p50 in ms. `update` is compose, layout and the scene graph patch on the
producer thread; `collect` (the packet build) is inside `acquire`, which is
otherwise the wait between the producer publishing and the present thread
taking the packet; `render` is the encode (plan, record, finish, submit) on
the present thread; `present` is the `frame.present()` call.

| scene                | period | update | acquire | render | present | GPU stats |
| -------------------- | -----: | -----: | ------: | -----: | ------: | --------- |
| showcase scroll      |   40.0 |    4.5 |    29.4 |    7.7 |    32.0 | 5 passes, 17 copies, shader 2.8 MP, shadow bands 5.7 MP, 110 draws, 0.1 MB uploads |
| cranorbit MEGA BOSS  |   18.7 |    6.9 |     6.4 |    9.9 |     8.1 | 1 pass, 1 draw of ~15k shapes, 11 MP shape fill, 4.5 MB uploads |

Both periods are the present thread's own cycle: `render + present`. On
cranorbit 9.9 + 8.1 = 18.0 of the 18.7 ms period; on the showcase 7.7 + 32
of 40. Four things follow from the code, and each is what stops 60 fps.

1. **The present thread serialises the CPU encode with the GPU.** The
   producer may run two packets ahead (`PresentHandle::has_credit` is
   `outstanding < 2`), so backpressure is not the limit. The present thread
   is: `PresentState::run` takes one packet, encodes and submits it, then
   calls `frame.present()`, which blocks in the driver until the GPU has
   drained the FIFO queue, and `acquire` waits on a fence for the image's
   previous use (`wgpu-hal` `swapchain/native.rs`). Only then does it look at
   the next packet. The GPU therefore idles for every encode, and the CPU
   idles for every GPU frame. Frame time is `encode + gpu`, never
   `max(encode, gpu)`. On cranorbit the GPU is 8 ms and the producer 7 ms:
   the scene is already a 60 fps scene on every stage but this sum.
2. **Encode time is wgpu submission, not scene work.** The showcase draws
   110 things and spends 7.7 ms encoding: `finish` 2.2 ms and `submit` 3.3
   ms (`[wgpu-render-stage:submit]`), with 105-156 `queue.write_buffer`
   calls per frame, each a staging allocation that the submit copies. Main
   had the same shape (finish 2.4-3.9, submit 3.1-4.3, 101-111 writes), so
   this is not the rewrite's regression, it is the cost the renderer has
   always paid. Cranorbit's 9.9 ms is that plus real work that runs on the
   encode's critical path though nothing in it needs the GPU: 15k
   `DrawShape` records converted to `ShapeData`, the band meshes tessellated
   on the CPU, 4.5 MB written, every frame, while the producer thread sits
   idle for the difference.
3. **The glass shades 2.8 MP at ~5 ns per pixel: 14 ms.** The showcase
   cards use `blur_radius(0.0)`: the transmission is sharp (one tap per
   wavelength), so the cost is ALU and the ~20 taps of the reflection,
   plain, and adaptive-frost paths, not a blur. The shader evaluates the
   scene SDF three times for the normal, three lens displacements with
   `pow` and `sin`, two meniscus bands, the bevel axes, three tone curves,
   and reads ~60 uniforms, per pixel, all of it a function of the pixel's
   position in the card and none of it of the frame's content. Seven cards
   of identical geometry recompute the identical field every frame.
4. **The blur pair writes its vertical pass at full size**, 13-25 taps per
   pixel over 0.2-0.5 MP per region, ~8 ms for the header and the glass
   buttons; the cached drop shadows composite 5.7 MP of bands per frame,
   most of it the transparent tail of a Gaussian; the four strata load and
   store the whole page four times (~1 ms each on this tiler).

The architecture below removes 1 and 2 without touching a pixel, which is
cranorbit's whole gap and the showcase's CPU half; the GPU half of the
showcase is 3 and 4, planned as exact steps first and pixel-changing steps
last, each behind the contract that pins it.

## Architecture: a three-stage frame

```text
producer thread        encoder thread                 presenter thread
compose, layout,       plan strata and stages,        acquire the next image
scene graph patch,     record passes, one upload      ahead of need,
collect -> packet      ring write, submit             present, return timings
      |  credit 2            |  acquired image <-------------|
      +--------------------->+  submitted frame ------------->+
```

**Present runtime.** `PresentState` splits in two. The encoder thread owns
`GpuRenderer` and does exactly what `render_to_surface` does today minus the
swapchain calls: validate the packet, encode, submit, hand `RenderReturns`
back. The presenter thread owns the `wgpu::Surface`: it acquires one
`SurfaceTexture` ahead of the encoder's need and parks it in a one-slot
mailbox, and after every submit it takes the frame's texture and calls
`present`, then acquires the next. `SurfaceTexture`, `Surface` and `Queue`
are `Send + Sync` in wgpu 30, so nothing is unsafe about this. The encoder
never blocks on the swapchain unless no image is free, which is the
GPU-bound case where nothing can help. Surface control messages (replace,
reconfigure, drop) go to the presenter, which drains its mailbox and bumps
the epoch exactly as `handle_control` does now, so the cancellation
protocol and its tests keep their meaning: a packet built against an old
epoch is refused whole. The frame period becomes
`max(producer, encoder, gpu)`. On cranorbit that is `max(7 + collect, 9.9,
8.1)`, a vsync-paced 60 fps before any other step; on the showcase it is
the GPU alone, 32 ms today, and the encode is no longer added to it.
Telemetry reports the three stages by name (`producer`, `encoder`, `gpu`,
where `gpu` is submit-to-present-return) instead of the present thread's
`acquire`/`render`/`present`, which mean something else once the thread is
split. The contract test runs the runtime inline with an injected clock and
a presenter whose `present` sleeps: the encoder's second packet must finish
encoding before the first present returns.

**One upload ring per frame.** Every per-frame byte the GPU reads (viewport
uniforms, shape and gradient data, band meshes, image and glyph vertices,
blur, composite and shader uniforms) is sub-allocated from one persistently
sized buffer per usage class, written with one `write_buffer` at the end of
the encode, and bound with dynamic offsets. `ViewportUniformRing` is
already this shape; `UploadAllocator`'s per-slot buffers and cached bind
groups become ranges of the ring with one bind group per ring. The
`upload_writes` count on the submit line is the contract: a frame has at
most one write per ring, the test pins that on the atlas scene. Expected:
finish + submit from 5.5 ms to under 2 on the showcase.

**GPU-ready records leave the producer.** `collect` already knows the root
scale and every op's snap anchor, so `convert_shape_into_slots` and
`band_mesh::mesh_batch` run there, once, and the packet carries `ShapeData`
and mesh geometry per z-ordered run with the blend mode and brush table the
encoder batches by; the encoder copies bytes into the ring and records
draws. This is the same conversion in one place (`render_contract.rs`
keeps queue writes in `frame_graph.rs`; the conversion is not a write). On
cranorbit the encoder drops to the copy and the draw, the producer rises to
~10 ms, and both sit under the vsync. Retained GPU records for unchanged
draw runs are not planned: a `Canvas` re-records each frame in the
reference too, and with the stages overlapped the per-frame conversion is
paid on a thread that has the time.

**The glass shades content, not geometry, per frame.** `liquid_glass.wgsl`
partitions by what each term depends on. Everything that depends only on
the fragment's position in the effect rect and the material's uniforms
(the scene SDF and its normal, coverage and the two coverage bands, the
three channel displacements, the reflection displacement and tangent, the
meniscus, bevel, specular and face-light weights) is the *lens field*, and
everything that depends on the captured pixels (the transmitted, plain,
reflection and adaptive taps, tone, tint, ink, dither) is the *content
pass*. The lens field is rendered once per distinct (effect rect size,
material uniforms, scale) into a retained texture keyed like the layer
cache (`Rg32Float` for the three displacements, `Rgba16Float` for the
weights, exact to the shader's own arithmetic), and the per-frame draw
reads it and does the content pass: ~20 taps and ~100 ops per pixel. The
seven showcase cards share one field. A morphing or wobbling glass whose
uniforms change every frame misses the cache and renders its field every
frame in the same two draws, so it costs what it costs today plus one
small write; the specialisation that folds inactive features stays for
that path. Contract: `backdrop_atlas_parity.rs` gains the field-cached
glass against the monolithic shader, byte for byte, with the field
rendered at the effect's own resolution. Expected on the showcase: 14 ms
to ~6 (field reads ~2-3 ms of bandwidth, taps and tone ~3).

**Blur at the scratch scale, once.** The vertical pass writes at the
scratch scale too, and the composite (the masked blit, or the glass's
region read) samples the small result bilinearly. This is the reference's
own algorithm (Skia downsamples a wide blur, blurs, and upsamples), and it
changes pixels: the upsampled result differs from the full-size vertical
pass by the bilinear reconstruction of a signal the kernel already
band-limited. `blur_reference.rs` gets the reference model of that
pipeline (downsample, separable Gaussian at the scaled radius, bilinear
upsample) and holds the GPU to it within one step; the robot fixtures
that show a blurred glass are re-baked against the reference model, not
loosened. Expected: ~8 ms to ~2.

**Shadow bands to their visible extent.** A cached blurred shadow is
composited as up to four bands of the full blur margin; the band outside
the radius where the cached alpha is zero contributes nothing. The cache
entry records the alpha extent it holds (measured from its own content
once, when it is rendered), and the bands shrink to it. Pixel-exact by
construction; the contract test draws a shadow with and without the trim
and compares byte for byte, and pins `shadow_cache` `hit_px` on the card
row to the visible extent. Expected: 5.7 MP to ~2.5 MP, ~1.5 ms.

**Strata stay.** Four full-page passes are ~3-4 ms of tile traffic here;
wgpu has no render-area, so a stratum cannot be confined to the tiles it
touches, and merging strata reintroduces the re-draws the copies removed.
They are the price of exact captures on a tiler and are not on the path
to 16 ms.

## Budget

Showcase scroll, GPU ms, present-thread p50:

| item                          | now | after | step |
| ----------------------------- | --: | ----: | ---- |
| liquid shader                 |  14 |     6 | lens field |
| blur pairs                    |   8 |     2 | scratch-scale vertical pass |
| shadow bands                  |   3 |   1.5 | visible extent |
| copies                        |   2 |     2 | |
| page fill, strata, composites |   5 |     5 | |
| total                         |  32 |  16.5 | |

CPU per frame, ms: producer 4.5 + collect ~1; encoder 7.7 to ~3 (ring);
neither on the GPU's path once the stages overlap. The showcase's 60 fps
is therefore the GPU budget above, and the table lands at the vsync with
no margin. The only remaining lever that does not change what the glass
transmits is the material's own arithmetic (`f16` behind the one-step
parity gate, an analytic normal for the plain rounded shape), worth an
estimated 1-2 ms of the field's render, which the cache already takes off
the per-frame path. Past that the levers change pixels (the tone curves,
the frost neighbourhood, the field at half resolution) and are the user's
call; the plan does not assume any of them.

Cranorbit MEGA BOSS: the three-stage frame alone makes the period
`max(7 + collect, 9.9, 8.1)` ms, under the 16.7 ms vsync; moving the
conversion to the producer balances it to ~10 / ~4 / 8. No pixel changes.

## Order

Each step ships with its contract test proven red first and the device
numbers for both scenes against the numbers above; a step whose number
does not move is reverted, not kept.

1. Three-stage frame (present runtime split, telemetry renamed). Both
   scenes; cranorbit's 60 fps lands here.
2. One upload ring per frame. Both scenes.
3. GPU-ready records from the producer. Cranorbit's encoder margin.
4. Lens field cache. Showcase's largest GPU item, pixel-exact.
5. Shadow bands to their visible extent. Pixel-exact.
6. Blur at the scratch scale. Pixel-changing within the reference model;
   fixtures re-baked against it.
7. Measure; if the showcase is not at the vsync, the material's arithmetic
   (`f16`, analytic normal) behind the parity gate, then stop and report
   what remains and what it would cost in pixels.
