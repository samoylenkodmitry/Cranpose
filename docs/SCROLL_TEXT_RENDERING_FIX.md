# Scroll Text Rendering Investigation

## Problem

On the `renderer` branch, text and underline decorations render differently
depending on scroll position. The underline appears thin at one scroll
offset and thick at another. Text glyphs shift relative to decorations.
This does NOT happen on `main` branch and does NOT happen on WASM.

## Architecture Trace

### How text position reaches the GPU

1. `push_text_style_draws` (pipeline.rs) creates `TextDraw` with rect in logical pixels
2. `push_text_decorations` creates decoration `DrawShape` from the same text rect
3. `collect_layer_contents_into` (render.rs) assigns `snap_anchor` to both
4. For `translated_content_context` (scroll): `content_snap_anchor = None`
5. `prepare_text_for_render`: `left_px = rect.x * root_scale`, `top_px = rect.y * root_scale`
6. cosmic-text `physical()`: `Y = truncf(glyph.y + top_px)` (integer device pixel)
7. X sub-pixel quantized into `SubpixelBin` (4 bins per device pixel)

### How decorations reach the GPU

1. Same text rect origin as text
2. Decoration shape has `snap_anchor = None` for TCC
3. `prepare_shapes_batch`: `position = quad * root_scale` (device pixels, NOT rounded)
4. Fragment shader SDF: `select(0.0, 1.0, dist <= 0.0)` (hard binary edge)

### What `main` does differently

`main` has `snap_text_rect_for_motion()` which rounds text rect position to
**integer logical pixels** for `TextMotion::Static`. This was removed on `renderer`.
Result: on `main`, `left_px = round(x) * root_scale` is always at integer device pixel
boundaries (at integer scale factors). Glyph placement via `truncf()` is deterministic.

`main` has NO snap anchor system. Shapes render at exact sub-pixel positions.
With hard-edge SDF, a 2-device-pixel underline always covers exactly 2 rows
regardless of sub-pixel Y. Thickness is constant.

## 5 Suspects

### 1. Text and decorations on different sub-pixel grids (HIGH)

Text position: `truncf()` in cosmic-text snaps glyphs to integer device pixels.
Decoration position: at exact sub-pixel device coordinates (no rounding).
As scroll changes, text shifts by 1 device pixel at irregular intervals,
while decorations move continuously. The gap between text baseline and
underline varies by 0-1 device pixel, making underline appear thicker/thinner
relative to text.

### 2. Missing `snap_text_rect_for_motion` on renderer (HIGH)

On `main`, text rect is rounded to integer logical pixels. Decorations share
the same rounded origin. Both are at consistent positions. On `renderer`,
this function was removed. Text rect is at sub-pixel position. The
`truncf()` in cosmic-text creates a different device-pixel alignment than
the unrounded decoration position.

### 3. `content_snap_anchor = None` for TCC (MEDIUM)

Disabling snap for scrollable content means NOTHING is pixel-aligned.
Text glyphs have their own internal rounding (truncf + SubpixelBin),
creating an inconsistency with shape positions that have no rounding.
Both text and shapes need to be on the SAME device-pixel grid.

### 4. SDF smoothstep for non-rounded rects (CONFIRMED - was my mistake)

Changing from `select` (hard edge) to `smoothstep` for plain rects
created variable alpha at rect edges. A 2-device-pixel underline at
sub-pixel Y gets partial coverage on top/bottom rows, making it appear
to change thickness. REVERTED.

### 5. Different root_scale between desktop/WASM (MEDIUM)

Desktop typically 2.0, WASM depends on device. At 1x scale, a 1-logical-pixel
underline is 1 device pixel (always 1 row, always consistent). At 2x, it's
2 device pixels, and the relative shift between truncf'd text and unrounded
decoration is more visible.

## 3 Proposals

### A. Re-enable snap anchor for TCC content with device_pixel_step=1.0 (RECOMMENDED)

Set `content_snap_anchor = Some(rigid_snap_anchor)` for TCC layers.
Both text AND shapes snap to the SAME 1-device-pixel grid.

- Text: snap_delta rounds position to device pixel → `left_px`, `top_px` at integers
  → SubpixelBin always Zero → stable glyph rendering
- Shapes: snap_delta rounds position to device pixel → integer pixel edges
  → hard-edge SDF always covers same rows → stable thickness
- Both on same grid → no relative shift between text and decorations

Scroll moves in 1 device pixel = 0.5 logical pixel steps at 2x.
This is standard for native apps (macOS, Windows).

### B. Round text AND shape positions to device pixels in GPU preparation

In `prepare_text_for_render`, round `left_px`/`top_px` to integer.
In `prepare_shapes_batch`, round shape positions to integer device pixels.
Both independently reach the same grid. But this duplicates snap logic
and bypasses the snap anchor system.

### C. Add offscreen surface for scrollable content

Render scroll container content to offscreen surface at stable local
coordinates. Composite surface at exact scroll position via texture
interpolation. This is what iOS/Android/Chrome do. Gives smooth scrolling
AND stable rendering. But significant performance cost (extra GPU memory,
extra render passes per scroll container).

## Chosen Solution: A

Re-enable `content_snap_anchor` for TCC content. The previous attempt
failed because `draw_snap_anchor` and `text_snap_anchor` were on
DIFFERENT grids (shapes snapped, text not). Now both are unified under
`content_snap_anchor`. With both on the same 1-device-pixel grid:

1. Text glyphs: stable (SubpixelBin always Zero at integer positions)
2. Decorations: stable (integer device pixel positions → consistent SDF)
3. Background shapes: stable (same grid as text and decorations)
4. No relative shift between any elements

The 0.5 logical pixel scroll steps at 2x scale are standard and acceptable.
At 3x (mobile), steps are 0.33 logical pixels. At 1x, 1.0 logical pixels.

## Implementation Plan

### Step 1: Change content_snap_anchor for TCC layers

In `collect_layer_contents_into`, change:
```rust
let content_snap_anchor = if layer.translated_content_context {
    None
} else {
    layer_snap_anchor
};
```
To use `layer_snap_anchor` for ALL layers (remove the TCC special case).
This means TCC content gets `translated_snap_anchor` (step=0.25).

Actually, for fully stable text rendering, we need step=1.0 (rigid).
But `translated_snap_anchor` has step=0.25. We need to either:
- Change `translated` step to 1.0
- Or use rigid snap for TCC content

Using `layer_snap_anchor` directly gives `translated_snap_anchor` for TCC
(step=0.25) which is smoother but still has SubpixelBin changes.

Decision: use `layer_snap_anchor` (step=0.25) first. If text still jitters,
increase to step=1.0.

### Step 2: Keep ChildLayerComposite without snap

The ChildLayerComposite removal is correct — offscreen compositing should
not snap the composite destination. The primitives inside are already
snapped individually.

### Step 3: Revert failed attempts

- Reverted smoothstep shader change (made underline worse with variable alpha)
- Reverted left_px/top_px rounding (only rounded text, not shapes → misalignment)
- Restored hard-edge SDF for non-rounded rects

### Step 4: Changed SnapAnchor::translated step from 0.25 to 1.0

At step=0.25, SubpixelBin boundaries are crossed at every snap step, causing
glyph rendering changes. At step=1.0, positions are at integer device pixels,
SubpixelBin is always Zero, glyph rendering is completely stable.

## Results

- All 152 unit tests pass
- Render contract: `translated_plain_text` now PASSES (was failing with 477 differing pixels)
- Render contract: `translated_text_decorations` still failing (269 differing pixels) —
  shadow text has separate rendering path that may need similar investigation
- Zero clippy warnings
- Scroll content moves in 1 device pixel = 0.5 logical pixel steps at 2x
- Text, decorations, and shapes all on same device pixel grid
