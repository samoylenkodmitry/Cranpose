# Out-of-Clip Rendering — Root Cause Analysis

## Symptoms

Content from clipped containers (scroll areas, TabContent boxes) renders
beyond the parent's clip_to_bounds boundary.  Observed on Shaders tab
(offscreen path) and HN tab (root_direct path).

## Architecture

Two rendering paths exist for the root layer:

1. **root_direct** — flattens all layers into root scene, renders directly.
   Used when root has no effects/backdrops and no descendant backdrops.
2. **offscreen** — renders root to offscreen texture, composites to screen.
   Used when effects, backdrops, or descendant backdrops exist in tree.

Layer flattening (`collect_layer_contents_into`) propagates `visual_clip`
from `clip_to_bounds` parents to child draw commands AND stores it on
`ChildLayerComposite.visual_clip` for layers that need their own surface.

## Root Cause

### Primary: resolve_clip semantic gap in collect_layer_contents_into

`resolve_clip(parent, child)` returns `None` when the two rects don't
intersect. Throughout the rendering system, `None` means "no clipping" —
shapes render without bounds. But when a layer's own `clip_rect()` (from
`graphics_layer.clip = true`, e.g. via `rounded_surface()`) doesn't
overlap the inherited clip (from a parent's `clip_to_bounds`), the correct
meaning is "fully clipped / invisible", not "no clipping".

In practice: LazyColumn pre-composes items beyond the visible area.
Items whose `graphics_layer.clip` rect is entirely below the parent's
`clip_to_bounds` boundary get `visual_clip = None` from `resolve_clip`.
Their `draw_behind` shapes (card backgrounds) then render with no clip at all.

**Fix**: Early return in `collect_layer_contents_into` when
`layer_clip.is_some() && inherited_clip.is_some() && visual_clip.is_none()`.
The entire subtree is invisible — skip it completely.

### Secondary: Child composite scissor bugs

Two additional bugs in the compositing path (relevant for isolated layers):

1. **Child composite scissor used layer's own clip, not inherited clip.**
   `render_layer_surface_uncached` and `render_root_direct` used
   `layer.clip_rect()` (the rendering layer's own clip) for child composite
   scissor instead of `child.visual_clip` (the inherited clip from parent's
   `clip_to_bounds`). Fixed by using `child.visual_clip`.

2. **Missing visual_clip shift in post-collection coordinate transform.**
   `dest_quad`, `backdrop_rect`, and `shadow_draws` were shifted by
   `-surface_rect.origin`, but `child.visual_clip` was not. Fixed by
   shifting `child.visual_clip` alongside other coordinates.

## Confirmed Non-Issues

- **Shape shader clip mechanism**: `world_pos` and `clip_rect` are both in
  physical pixel space; fragment discard works correctly.
- **Image/text clip mechanisms**: `scissor_rect_for_image` and
  `text_bounds_for_clip` correctly constrain rendering to clip bounds.
- **Flattened draw command clips**: 25/26 shapes and 14/15 texts have
  correct clips when the layer hierarchy is properly set up.

## Tests

- `clip_to_bounds_propagates_visual_clip_to_all_descendant_shapes`:
  Verifies all shapes inside a clip_to_bounds container get the correct clip.
- `clip_to_bounds_culls_child_layers_outside_boundary`:
  Verifies child layers with `graphics_layer.clip=true` positioned entirely
  outside the parent's clip_to_bounds boundary are culled (no shapes emitted).
