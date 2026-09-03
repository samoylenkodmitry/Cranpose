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

A tiling GPU pays a fixed cost per render pass whatever the pass draws, so
the resolve step batches. The backdrop effects of one layer scene queue into
stages (`ResolveStages`): an effect joins the stage after every queued
effect below it whose composite lies under its capture, so every capture in
a stage reads only composites of earlier stages. A stage runs as:

1. one capture pass into an atlas that holds every capture of the stage,
   shelf-packed with a gap of the effect's blur radius around each region so
   no blur tap reads a neighbour;
2. one blur pass pair over every blurred region: the horizontal pass writes
   each region downscaled into a scratch texture, the vertical pass writes
   it at full size into a result texture, and both textures are packed to
   the blurred regions alone, so no pass loads or stores the atlas (on the
   Mate 20 X the atlas-sized pair cost 9 ms of bandwidth per frame);
3. composites reading their region: a blur blits its region of the result
   through the effect's rounded mask; a runtime shader that declares
   `batched_source` draws in the final pass reading its region, applying the
   mask and alpha through its reserved uniform slots;
4. one resolve pass for the stage's shader tails that a later capture reads
   (`CaptureReaders::reads`): a capture re-shades every tail under it, so a
   read tail is shaded once into a packed texture and becomes a blit for
   the captures and the final pass alike. `shader_pixels` in the stats
   counts what the shaders shade per frame; `backdrop_atlas_parity.rs`
   pins that a glass under two glass buttons shades exactly its own pixels.

Effects the renderer cannot batch (an app shader that reads the whole input
texture, an offset, a chain the shader does not end) resolve one at a time
inside their stage. A list of glass cards over a page therefore costs one
capture pass; the glass buttons inside the cards read the cards' glass and
form the next stage: one capture pass and one blur pair for all of them.

An isolated child that draws nothing itself and whose effect is a runtime
shader (a shader-drawn planet) is a shader composite in the final pass over
a shared transparent input of its surface size, and costs no pass. A shader
drawn in the final pass is re-run inside every capture above it, so the
renderer sums the capture area that reads such a child (`CaptureReaders`)
and resolves it into a texture once when more than half of it is read
again: a page-wide shader under a list of glass cards runs once, a corner
shader under one card stays a tail.

A shader that reads its region of the atlas maps region-local uv to the
texture once per fragment (`region_map` in `liquid_glass.wgsl` and
`gradient_blur.wgsl`) and threads that map through its sampling loops, so
no tap pays for a texture query or a uniform fetch.

`backdrop_atlas_parity.rs` pins the pixels: a glass renders the same alone
and packed beside others (a shader within one 8-bit step, the float region
mapping moving a tap by a few ulps; a blur exactly), the in-shader mask
matches the masked blit, the shader-only child matches its surface resolve,
and the heavily read shader child resolves exactly once.
`backdrop_pass_batching.rs` pins the budget: extra glasses in a stage add
no pass.

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

`RenderStatsSnapshot` reports passes and pass pixels, transient and retained
texture bytes, uploads, isolated layer renders and pixels, layer cache
traffic, blur passes, composite passes, effect applies and the pixels the
runtime shaders shade (`shader_pixels`, the number that decides a glass
frame's cost on a tiling GPU).
`backdrop_pass_batching.rs` pins the pass budget: a frame has one full-screen
pass, a stage adds one capture pass and one blur pair however many glasses it
holds, and a capture never splits the final pass.

## Validation bar

`just fmt`, `just clippy` (and the target-specific clippy recipes), `just
test`, `just robot`. A renderer change that touches placement, crispness or
pass counts ships with a test that fails without it.

## Where a glass frame's time goes

Measured on the Mate 20 X (Mali-G76) scrolling the showcase list, by
removing one thing at a time from a single build and reading the present
time: with no backdrop effect the page presents in 3 ms at 59 fps. Every
remaining millisecond is fill: the glass material's own shading at roughly
10 ms per megapixel of glass, the page re-drawn under each capture (its
gradient shapes and cached shadow bands), and the shadow bands in the final
pass. Pass count stopped mattering once the atlas stages landed; the atlas
blur bandwidth and the re-shading of read tails were the two renderer costs
left on top of the fill, and both are gone. The material's per-pixel
cost is the material's; the renderer shades each glass pixel once.
