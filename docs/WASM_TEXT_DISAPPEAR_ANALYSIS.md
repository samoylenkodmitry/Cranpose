# WASM Text Disappear on Shaders Tab — Root Cause Analysis & Fix Plan

## Problem Statement

On WebGL2, opening the "shaders tab" causes text labels to disappear or be
truncated.  Desktop rendering is unaffected.

Additional symptom: scrolling the tab bar row horizontally moves the right-edge
clipping border of texts in the content area below.  This confirms the root
cause — the tab bar's scroll content inflates `scene_bounds`, changing the
offscreen→screen mapping, which shifts effective clip boundaries on screen.

## Architecture Context

Two rendering paths exist for the root layer:

1. **`render_root_direct`** — renders directly to the screen surface.  Used when
   the root layer has no effects, backdrops, or descendant backdrops.
2. **`render_layer_surface`** (offscreen path) — renders to an offscreen texture,
   then composites to screen.  Used when effects/backdrops exist anywhere in the
   tree.

The shaders tab contains descendant backdrops (LiquidGlass), forcing the root
through the offscreen path.

## Root Cause

`scene_bounds()` (render.rs:707) computes the bounding rect of the offscreen
surface by unioning **all draw command rects** in the flattened scene.  It does
NOT consider clip rects.

`collect_layer_contents_into()` (render.rs:1262) flattens child layers
(including scroll container content) into the parent scene.  Scroll containers
position their content at full scroll offsets — items far below the viewport have
large `y` coordinates, items far to the right have large `x` coordinates.  Each
item also carries a clip rect from `clip_to_bounds`, but `scene_bounds()` ignores
clips entirely.

### The cascade

1. Scroll content inflates `scene_bounds` far beyond the viewport.
   - Example: viewport = 800×600dp, scroll content height = 4000dp
   - `surface_rect` ≈ {0, 0, 800, 4000}

2. `render_layer_surface_uncached` (render.rs:2848) reduces `target_scale`:
   ```
   target_scale = root_scale
       .min(max_dim / surface_rect.width)
       .min(max_dim / surface_rect.height)
   ```
   On WebGL2: `max_texture_dimension_2d = 2048`.
   `target_scale = min(1.07, 2048/4000) = 0.512`

3. Font sizes scale with `target_scale` (render.rs:4840):
   `font_size_px = text_draw.font_size * text_draw.scale * target_scale`
   A 14sp font becomes `14 × 0.512 = 7.2px`.  With deeper scroll, can drop to
   2–3px.

4. The 2048×2048 offscreen is composited to a `root_dest_quad` that spans
   `surface_rect × root_scale` = 856×4280 pixels.  But only the viewport
   (856×642 pixels) is visible.  The offscreen's limited resolution is spread
   across the oversized quad — the visible portion gets a fraction of the
   available pixels.

5. Combined effect: text is rendered at tiny font sizes AND then stretched over
   a large area.  At extreme reductions, glyphon rasterizes glyphs with zero
   visible pixels → text vanishes.

### Why desktop is unaffected

Desktop GPU: `max_texture_dimension_2d = 16384+`.  Even with inflated
`scene_bounds`, `target_scale` stays at `root_scale`.  No quality loss.

## Chosen Fix: clip-aware `scene_bounds`

The fundamental flaw: `scene_bounds()` unions raw rects without considering
clips.  Every draw command already carries a `clip: Option<Rect>` from
`clip_to_bounds` on scroll containers.  Content outside the clip is invisible
and should NOT inflate the surface rect.

### Fix: intersect each rect with its clip before including in bounds

```rust
fn visible_draw_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}
```

Apply this in `scene_bounds()` for every draw type (shapes, images, texts).

### Fix part 2: clip-aware child layer bounds

`ChildLayerComposite` dest_quads (for child layers that need offscreen — blur,
LiquidGlass, etc.) were also unioned into bounds without clip consideration.
Added `visual_clip: Option<Rect>` to `ChildLayerComposite`, propagated from
`collect_layer_contents_into`, and applied `visible_draw_rect` in both
`render_layer_surface_uncached` and `estimate_layer_surface_rect_cached`.

### Fix part 3: viewport rect override for root layer

Diagnostic logging revealed that some draw commands (text decorations — underline
shapes from demos deep in scroll content) have `clip=None` despite being inside
scroll containers with `clip_to_bounds`.  Root cause: a clip propagation gap in
the layer tree flattening (separate issue to investigate).

Belt-and-suspenders: pass the viewport rect as `logical_rect_override` when
calling `render_layer_surface` for the root layer in `render_graph`.  The root's
visible area is always the viewport — no draw command outside the screen is ever
visible.  This ensures `surface_rect == viewport` for the root, regardless of
any inflation from unclipped draw commands.

```rust
let viewport_rect = Rect {
    x: 0.0, y: 0.0,
    width: width as f32 / root_scale,
    height: height as f32 / root_scale,
};
let root_surface = self.render_layer_surface(
    text_state, &graph.root, root_scale, None, false, Some(viewport_rect),
)?;
```

### Why this is the most robust fix

1. **Fixes all layers**, not just root — any layer with scroll content benefits.
2. **No wasted GPU memory** — offscreen textures are sized to visible content.
3. **No scale reduction on WebGL2** — viewport-sized offscreen fits within 2048.
4. **Zero behavioral change for non-clipped content** — `visible_draw_rect` with
   `clip: None` returns the original rect.
5. **Consistent with how rendering works** — clipped content is never visible,
   so it should never influence surface sizing.

### What about `estimate_layer_surface_rect_cached`?

This function (render.rs:1457) also calls `scene_bounds()` through
`collect_layer_contents`.  The fix applies there too, ensuring child layer
offscreens are correctly sized.  Child effect layers (blur boxes, LiquidGlass)
typically don't contain scroll containers, so in practice they're unaffected.
But the fix makes them correct by construction.

## Implementation Plan

### Step 1: Add `visible_draw_rect` helper

Add to render.rs near `scene_bounds`:
```rust
fn visible_draw_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}
```

### Step 2: Update `scene_bounds` to use clip-aware rects

For shapes, images, texts: intersect rect with clip before union.
For effect_layers and backdrop_layers: these use `rect` and `clip` fields too —
intersect before union.

### Step 3: Update `shadow_draws_bounds` similarly

Shadow shapes and texts also have clips.  The shadow's own `clip` field should
intersect with the expanded shadow bounds.

### Step 4: Also fix `estimate_layer_surface_rect_cached`

No code change needed — it calls `scene_bounds()` internally, so the fix
propagates automatically.

### Step 5: Add tests

- Test that `scene_bounds` with clipped content returns the clipped area.
- Test that large scroll content with clip doesn't inflate bounds beyond clip.
- Test `visible_draw_rect` directly.

### Step 6: Verify

- `cargo test`
- `cargo clippy --workspace`
- `cargo fmt`
- `apps/desktop-demo/build-web.sh` (WASM build)
- Android build
- Robot tests

## Risks & Mitigations

**Risk**: Effect layers sample from the offscreen.  If the offscreen is smaller,
they might sample from edges incorrectly.
**Mitigation**: Effect layers (blur, LiquidGlass) have their own offscreens.
The root offscreen only serves as backdrop underlay.  Backdrop underlay creation
(`create_projected_child_underlay`) captures a region of the parent offscreen,
which now correctly covers the visible area.  Effects don't need content outside
the viewport.

**Risk**: Shadow draws extend beyond their source shape.  Clipping scene_bounds
to clip might undersize the surface.
**Mitigation**: `expand_blurred_rect` already expands shadow bounds by blur
radius, then intersects with clip.  This existing logic handles shadow extent.
We keep the existing `shadow_draws_bounds` logic and only apply
`visible_draw_rect` to the shadow's internal shapes/texts.

**Risk**: Overlapping clips from nested scroll containers.
**Mitigation**: `collect_layer_contents_into` already resolves clips via
`resolve_clip()` (intersection of inherited + local clips).  The clip on each
draw command is the fully resolved clip.  No further clip resolution needed.
