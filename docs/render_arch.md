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
2. Scroll/content-offset ancestry is represented by `LayerNode::motion_context_animated`.
3. Direct-safe translation-only subtrees collapse into their parent target.
4. Plain text is not isolated just because it is text.
5. Decoration-only text also stays on the direct path when no other
   text-local-surface reason exists.
6. Direct child collapse must rebase text primitives into parent space before
   text/decorations are emitted.
7. Unspecified text under scrolling/content-offset ancestry resolves to
   `TextMotion::Animated`.
8. Scrolling images use linear sampling; static images stay nearest.
9. Mixed-content isolation is attributed by real local-surface reasons, not by
   tab-wide wrappers.
10. Hit testing uses exact transformed geometry, not axis-aligned approximations.
11. Raster-cache hashes are computed only when they are actually needed.

## Current Execution Model

### Graph build

- `build_graph_from_applier(...)` is one-pass on the hot path
- it no longer allocates a full snapshot tree before building the graph
- child `content_offset` is composed into child transforms
- child `content_offset` also propagates `motion_context_animated`

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
5. Precise text local-surface attribution under `text_local_surface`.
6. Scroll/content-offset motion context propagation into the render graph.
7. Animated-by-default motion for unspecified text inside scrolling subtrees.
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

## Acceptance Status

Current checked-in status:

- no known P0/P1 correctness issue is open on the branch after the latest self-review loop
- translation-only effect semantics are covered by focused WGPU tests
- bounded blur/backdrop semantics are covered by focused WGPU tests
- scrolling text motion defaults are covered by `scene_builder` unit tests
- motion-aware image sampling policy is covered by WGPU unit tests
- oversized mixed-content isolate regressions on the Shaders tab are guarded by a robot runner
- screenshot-based robot checks use logical regions against physical captures correctly
- attempted isolated-child-surface device-grid snapping was rejected because it
  violated the shared rigid-translation render contract

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

There is no open architecture refactor queued from the current code review loop.

The only valid next renderer work is:

1. Keep the current invariants green.
2. Start every new bug with a failing automated test.
3. Reject shortcuts that reintroduce global flattening, forced text isolation,
   or independent child snapping.
4. Keep screenshot-based acceptance tests aligned with the real direct-path
   invariant: bounded fractional-phase drift is acceptable, large subtree
   distortion is not.
5. Inspect an actual saved screenshot when a bug report is visual.
