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
5. Decoration-only text stays on the direct path.
6. Complex text uses bounded local surfaces through `LayerSurfaceReasons::text_local_surface`.
6. Direct child collapse must rebase text primitives into parent space before
   text/decorations are emitted.
7. Unspecified text under active motion ancestry resolves to `TextMotion::Animated`.
8. Scene build does not round static text rects in logical space.
9. Idle pure-text leaves participate in the same rigid snap-anchor path as
   mixed text+draw leaves.
10. Image primitives carry an explicit sampling policy: low-level draw calls
    default to nearest for atlases/skins, while the high-level `Image` widget
    opts into linear sampling for application imagery.
11. Mixed-content isolation is attributed by real local-surface reasons, not by
   tab-wide wrappers.
12. Hit testing uses exact transformed geometry, not axis-aligned approximations.
13. Raster-cache hashes are computed only when they are actually needed.
14. Root direct rendering only runs when the collected root scene has no local
    effect/backdrop events.
15. Robot screenshot capture must match `main` raw dimensions for the same
    logical surface.

## Current Execution Model

### Graph build

- `build_graph_from_applier(...)` is one-pass on the hot path
- it no longer allocates a full snapshot tree before building the graph
- child `content_offset` is composed into child transforms
- child `content_offset` does not by itself propagate `motion_context_animated`
- scroll and lazy-scroll modifiers report real motion activity into
  `ModifierNodeSlices::motion_context_animated()`
- scroll and lazy-scroll modifiers keep `translated_content_context` enabled for
  the whole translated subtree
- rested translated clip containers stay on the direct path; active translated
  clip containers use motion-stable capture only while motion is active

Files:

- `crates/cranpose-render/common/src/scene_builder.rs`

### Hit testing

- `collect_hits_from_graph(...)` skips subtrees with `has_hit_targets == false`
- interactive regions keep exact transformed quads and inverse transforms

Files:

- `crates/cranpose-render/common/src/hit_graph.rs`
- `crates/cranpose-render/common/src/graph_scene.rs`

### WGPU compositor

- root direct path only when `root_can_render_directly_cached(...)` succeeds
  and the collected root scene has no local `effect_layers` / `backdrop_layers`
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
7. Persistent translated-content propagation for scroll subtrees.
8. Explicit image sampling through `ImageSampling` with nearest as the
   low-level default and linear selected by the high-level `Image` widget.
9. Static snap-anchor coverage for pure-text leaves.
10. Complex-text local-surface classification (`text_local_surface`).
11. Root direct fallback to the root surface path when local effect/backdrop events exist.
12. Removal of giant Shaders-tab mixed-content wrapper isolation.
13. Physical-size-aware presented-window screenshot capture.
14. Axis-aligned rect-to-quad fast path in `ProjectiveTransform::from_rect_to_quad(...)`.
15. Per-frame cache for `LayerSurfaceRequirements`.
16. One-pass direct-child content collection instead of build-then-translate merge.
17. One-pass hot applier graph build.
18. Lazy raster-cache hash computation through `cache_hashes_valid`.
19. Logical-to-physical mapping in robot screenshot helpers.
20. Deterministic micro-surface screenshot contract via `robot_renderer_micro_contract`.
21. Raw robot screenshot size parity with `main` for the same logical surface.
22. Shared rigid-translation render contract without downsample fallback.

## Acceptance Status

Current checked-in status:

- translation-only effect semantics are covered by focused WGPU tests
- bounded blur/backdrop semantics are covered by focused WGPU tests
- motion-vs-translated-content text defaults are covered by `scene_builder` unit tests
- motion-aware image sampling policy is covered by WGPU unit tests
- atlas isolation and rested-scroll crispness are guarded by
  `robot_render_crispness_contract`
- oversized mixed-content isolate regressions on the Shaders tab are guarded by a robot runner
- screenshot-based robot checks use logical regions against captured images correctly
- attempted isolated-child-surface device-grid snapping was rejected because it
  violated the shared rigid-translation render contract
- raw robot screenshots now match `main` dimensions for the micro contract
- the current micro contract screenshot is pixel-identical to the committed
  `main` reference
- shared rigid-translation contracts now pass without the temporary render-contract downsample shortcut
- translated decorated/shadow text in the desktop demo is stable because it now
  uses bounded local surfaces instead of loose direct primitives

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

The current checked-in state closes the screenshot-scale mismatch, keeps scroll
text on one translated-content path across active and idle states, routes
complex text through bounded local surfaces, and removes logical-space static
text rounding. The remaining review loop is:

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
