# Liquid scroll pixel stability

## Contract

At a fractional display density, moving a rigid scroll subtree by exactly one
physical pixel must only translate its raster by one physical pixel. After
undoing that translation, adjacent presented frames may differ by at most 48
summed 8-bit channel levels across the full crop. This bounded allowance covers
sub-one-pixel linear-sampling quantization; moving even one antialiased edge
exceeds it by orders of magnitude.

`robot_liquid_scroll_exact_external_contract` exercises this contract at the
launched application's 1.3541667x density and 800x600 logical window size. It
first scans the fixed scroll-viewport boundary while both shader cards cover
it, then compares six independent scrolling regions: the optical shader body,
both shader-card edges, the bottom glass bar, the controls card, and the
segmented control. Before each region it crosses ten one-logical-pixel scroll
phases, then drives the `ScrollState` directly by `1 / density` logical units.
It checks semantic movement at every step, captures the real X11 window, and
compares every adjacent normalized frame. The phase sweep ensures that
boundaries which only fail at particular fractional offsets are part of the
contract.

## Reproduction evidence

- The production-density semantic target moves by approximately `-0.738462`
  logical units per exact step, so every requested move is one physical pixel.
- At 1.25x diagnostic density, all six regions failed before the shared
  coordinate fixes. The optical body reached a summed adjacent difference of
  361,377; the right edge reached 304,354; the bottom bar reached 7,108; and
  the ordinary controls and segmented control reached roughly 960 and 1,200
  respectively.
- The strongest discontinuity is not confined to the runtime shader. A static
  text run flips between two rasterizations at a half-device-pixel boundary.
  Ordinary shape and glass pixels show the same phase-dependent drift at lower
  amplitude.
- Focused renderer regressions reproduce the half-pixel text flip, backdrop
  shader-local drift, and projective-composite drift without the demo UI.
- After those faults were corrected, the production-density boundary probe
  still failed at scroll step 1: one entire sampled viewport row became
  `[214, 214, 219]`, with a dominant-color fraction of `1.0000`, while the
  adjacent shader-card rows retained their orange/purple split. Restoring the
  fixed ancestor clip makes every sampled row retain that split through all
  ten scroll phases.
- The remaining 3,785-3,797 score in the broad optical scene was isolated to a
  slider's cached drop shadow. Disabling only that shadow made the scene exact,
  proving a second shared renderer fault rather than a Liquid component fault.

## Ranked causes

1. **Draw raster snapping was incorrectly applied to ancestor clips.** Scene
   normalization resolves each clip from the layer that owns it. Applying a
   descendant draw's snap delta to that result moved a fixed scroll-viewport
   edge between adjacent device rows. Shapes, images, text, and backdrop
   effects all repeated this ownership violation in renderer-specific paths.
2. **Device snapping is round-tripped through logical `f32` coordinates.**
   `snap_delta_for_anchor` computes a device-grid decision, divides it by the
   root scale, adds it to every absolute logical coordinate, and later
   multiplies those coordinates by the root scale again. Repeated fractional
   scroll accumulation changes the low bits of otherwise rigid relative
   geometry. At half-pixel coverage and rounding boundaries those low bits
   select different raster results. This explains the cross-component failures
   and is reproduced without the Liquid shader.
3. **Cached shadow surfaces used unanchored logical bounds.** Shadow pixels
   used the draw's snap anchor, but the source surface origin and extent were
   independently floored from accumulated logical `f32` bounds. The allocation
   could therefore jump while its contents stayed anchored, changing the
   one-to-one composite sampling phase.
4. **Backdrop layers discarded the scroll snap anchor.** Shapes, images, text,
   and ordinary effect layers carried one rigid motion anchor through scene
   normalization; `BackdropLayer` did not. Its capture rect, shader rect,
   scissor, and destination therefore made independent rounding decisions.
5. **Projective child composites scaled absolute coordinates.** Rotated cards
   moved rigidly with their scroll parent, but their four destination vertices
   were reconstructed independently from large absolute `f32` values. Their
   linear sample phase changed even when the intended movement was one device
   pixel.

## Architecture decision

The renderer must canonicalize rigidly snapped draw geometry in device space
and preserve the anchor-relative subpixel phase. Direct shapes, images, text,
isolated surfaces, backdrop/effect composites, and projective child quads
derive their device coordinates from the same 1/16-device-pixel canonical
grid. A resolved clip remains in its owning layer's scene space; a descendant
draw may not borrow that clip and move it with its own raster snap. Backdrops
retain and translate the same snap anchor as their sibling primitives, and
projective vertices and cached-surface bounds are calculated relative to that
anchor instead of round-tripping an absolute logical delta. One-to-one cached
shadow composites use texel-exact sampling after the anchored device bounds
have been established.

Patching an individual Liquid component is incorrect: the failure affects
unrelated primitives and would leave the shared coordinate discontinuity
intact. The comparison allowance remains smaller than a single visible edge
change and is documented as a total channel budget rather than a per-pixel
tolerance.
