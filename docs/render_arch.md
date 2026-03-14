# Renderer Architecture

This is the authoritative renderer architecture/status document for the current
`renderer` branch.

## Architecture

The renderer is now a hierarchical graph plus a recursive compositor.

```text
layout/applier state
  -> RenderGraph { root: LayerNode }
  -> graph-based hit collection
  -> WGPU recursive compositor
     -> direct root path for pure translation/no-surface-reason scenes
     -> bounded local surfaces only when layer semantics require them
```

The current branch does not use the old global flat-scene model.

## Core Invariants

The branch now enforces these invariants:

1. Subtree motion is represented by `LayerNode::transform_to_parent`.
2. Active scroll/drag/fling ancestry is represented by `LayerNode::motion_context_animated`.
3. Direct-safe translation-only subtrees collapse into their parent target.
4. Plain text is not isolated just because it is text.
5. Decoration-only, gradient, stroke, and other styled text stay on the direct path.
6. Direct child collapse must rebase text primitives into parent space before
   text/decorations are emitted.
7. Unspecified text under active motion ancestry resolves to `TextMotion::Animated`.
8. Scene build does not round static text rects in logical space.
9. Scrolling images use linear sampling; static images stay nearest.
10. Mixed-content isolation is attributed by real local-surface reasons, not by
   tab-wide wrappers.
11. Hit testing uses exact transformed geometry, not axis-aligned approximations.
12. Raster-cache hashes are computed only when they are actually needed.
13. Robot screenshot capture must match `main` raw dimensions for the same
    logical surface.

## Current Execution Model

### Graph build

- `build_graph_from_applier(...)` is one-pass on the hot path
- it no longer allocates a full snapshot tree before building the graph
- child `content_offset` is composed into child transforms
- child `content_offset` does not by itself propagate `motion_context_animated`
- scroll and lazy-scroll modifiers report real motion activity into
  `ModifierNodeSlices::motion_context_animated()`
- scroll and lazy-scroll modifiers also mark translated-content ancestry only
  while that motion is active

Files:

- `crates/cranpose-render/common/src/scene_builder.rs`

### Hit testing

- `collect_hits_from_graph(...)` skips subtrees with `has_hit_targets == false`
- interactive regions keep exact transformed quads and inverse transforms

Files:

- `crates/cranpose-render/common/src/hit_graph.rs`
- `crates/cranpose-render/common/src/graph_scene.rs`

### WGPU compositor

- root direct path when `root_can_render_directly_cached(...)` succeeds
- per-frame layer-surface requirements cache
- one-pass direct-child collection into the parent `CompositorScene`
- bounded offscreens only for real surface reasons

Files:

- `crates/cranpose-render/wgpu/src/render.rs`
- `crates/cranpose-render/wgpu/src/gpu_stats.rs`

## Closed Work Items

These refactorings are complete in the checked-in branch state:

1. Exact transformed hit testing.
2. Root direct rendering for translation-only/no-surface-reason scenes.
3. Plain translation-only text on the direct path.
4. Decoration-only text on the direct path with parent-space text rebasing.
5. Removal of renderer-forced text local-surface classification.
6. Scroll and lazy-scroll motion-context propagation into the render graph.
7. Active translated-content propagation for scroll subtrees.
8. Motion-aware image sampling (`nearest` static, `linear` scrolling).
9. Removal of giant Shaders-tab mixed-content wrapper isolation.
10. Physical-size-aware presented-window screenshot capture.
11. Axis-aligned rect-to-quad fast path in `ProjectiveTransform::from_rect_to_quad(...)`.
12. Per-frame cache for `LayerSurfaceRequirements`.
13. One-pass direct-child content collection instead of build-then-translate merge.
14. One-pass hot applier graph build.
15. Lazy raster-cache hash computation through `cache_hashes_valid`.
16. Logical-to-physical mapping in robot screenshot helpers.
17. Deterministic micro-surface screenshot contract via `robot_renderer_micro_contract`.
18. Raw robot screenshot size parity with `main` for the same logical surface.
19. Structural translation contracts for direct text/list captures using box-downsampled normalized regions.
20. Pixel-identical micro screenshot parity with the committed `main` reference.

## Acceptance Status

Current checked-in status:

- translation-only effect semantics are covered by focused WGPU tests
- bounded blur/backdrop semantics are covered by focused WGPU tests
- motion-vs-translated-content text defaults are covered by `scene_builder` unit tests
- motion-aware image sampling policy is covered by WGPU unit tests
- oversized mixed-content isolate regressions on the Shaders tab are guarded by a robot runner
- screenshot-based robot checks use logical regions against captured images correctly
- attempted isolated-child-surface device-grid snapping was rejected because it
  violated the shared rigid-translation render contract
- raw robot screenshots now match `main` dimensions for the micro contract
- the current micro contract screenshot is pixel-identical to the committed
  `main` reference
- translation robots compare direct text/list captures structurally after
  downsampling, because glyphon grayscale mask AA is phase-sensitive under
  rigid translation even when geometry is stable

Latest sequential perf checks on this machine:

- `renderer` `opaque_scene`: `350.5 fps`
- temp `main` `opaque_scene`: `315.2 fps`
- `renderer` `backdrop_blur`: `207.1 fps`
- temp `main` `backdrop_blur`: `209.5 fps`

Interpretation:

- the previous `opaque_scene` regression is closed
- `backdrop_blur` is effectively at parity

## Validation Bar

The renderer branch is not considered done unless this bar is green:

- `cargo fmt`
- `cargo test > 1.tmp 2>&1`
- `cargo clippy > 2.tmp 2>&1`
- `cargo tree --duplicates`
- `apps/desktop-demo/build-web.sh`
- `apps/android-demo/android/./gradlew :app:assembleRelease`
- `./run_robot_test.sh --sequential`

When a renderer change touches crispness, placement, or screenshot correctness,
the review loop also includes:

1. run `robot_renderer_micro_contract`
2. inspect `/tmp/cranpose_renderer_micro_contract.png`
3. use `robot_measure_shaders` visual-compare mode when a full demo surface is
   needed
4. compare logical output against `docs/render-reference/main_renderer_micro_contract.png`, not raw PNG size alone

## Current Plan

The current checked-in state closes the screenshot-scale mismatch, returns idle
scroll text to the static crisp path, removes renderer-forced styled-text
isolation, and removes logical-space static text rounding. The remaining review
loop is:

- keep the current invariants green
- inspect saved demo screenshots when a new visual bug is reported
- add a new render contract only when a concrete remaining defect is reproduced

The next loop is:

1. Keep the current invariants green.
2. Start every new bug with a failing automated test.
3. Reject shortcuts that reintroduce global flattening, forced text isolation,
   or independent child snapping.
4. Keep screenshot-based acceptance tests aligned with the actual capture mode.
5. Inspect an actual saved screenshot when a bug report is visual.
6. Add a new contract only when a concrete remaining defect is reproduced.
