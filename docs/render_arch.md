# Cranpose Rendering Architecture

This is the authoritative rendering document for Cranpose.

It replaces:

- `docs/GRAPHICS.md`
- `docs/GPU_RENDERER_USAGE.md`

This document is intentionally architecture-first. It describes:

- the current renderer shape of the repo
- the structural cause of translation-dependent rendering bugs
- the production-grade target architecture
- the exact rewrite seams in the current file layout
- the validation strategy required to land the rewrite without relying on manual visual inspection

## Scope

The public graphics API lives primarily in `cranpose-ui-graphics`, and the current execution paths live in:

- `crates/cranpose-render/common`
- `crates/cranpose-render/wgpu`
- `crates/cranpose-render/pixels`
- `crates/cranpose-ui`
- `apps/desktop-demo`

The desktop demo currently defaults to the WGPU backend via `apps/desktop-demo/Cargo.toml`.
The pixels backend remains a CPU renderer and a useful reference path, but it must not define the long-term architecture.

Status note:

- the next two sections preserve the starting-point diagnosis this rewrite was written against
- they are not a status snapshot of the current tree
- current implementation status is tracked in the rewrite-status section below

## Starting-Point File Map

| Area | Files | Current Role | Architectural Problem |
|---|---|---|---|
| Reproducer surface | `apps/desktop-demo/src/app/lazy_list.rs` | Lazy list demo used to expose scroll-phase instability | Shows the bug; it is not the cause |
| Scroll layout | `crates/cranpose-ui/src/scroll.rs` | Applies scroll as layout `placement_offset` via `LayoutModifierMeasureResult` | Motion becomes child placement before rendering |
| Lazy list placement | `crates/cranpose-ui/src/widgets/lazy_list.rs` | Emits float `Placement::new(...)` values for visible item roots | Item subtree motion is already flattened into child positions |
| Shared transform helpers | `crates/cranpose-render/common/src/style_shared.rs` | Combines `GraphicsLayer` and applies transform/color/brush changes | `apply_layer_to_rect` and `apply_layer_to_quad` collapse subtree transform into primitive geometry too early |
| Renderer contract | `crates/cranpose-render/common/src/lib.rs` | Shared renderer traits (`Renderer`, `RenderScene`) | No shared hierarchical scene contract yet |
| WGPU scene build | `crates/cranpose-render/wgpu/src/pipeline.rs` | Traverses applier state and emits flat scene draw records | Builds a flat scene. Contains a dead-code `render_layout_tree` path (marked `#[allow(dead_code)]`); only the applier path (`render_from_applier`) is active |
| WGPU scene storage | `crates/cranpose-render/wgpu/src/scene.rs` | Stores `shapes`, `images`, `texts`, `shadow_draws`, `effect_layers`, `backdrop_layers` | Layering is encoded as z-ranges over flat arrays rather than explicit parent/child structure |
| WGPU execution | `crates/cranpose-render/wgpu/src/render.rs` | Renders flat draw lists, effects, and backdrops | Effect isolation uses full-frame offscreens and z-range replay, not stable local layers |
| Effect infrastructure | `crates/cranpose-render/wgpu/src/effect_renderer.rs` | Blur, offset, runtime shader, blit, offscreen pool | Useful building blocks, but still fed by the wrong scene model |
| WGPU shaders | `crates/cranpose-render/wgpu/src/shaders.rs` | Shape/image WGSL shaders | Coverage and clipping happen in device space; plain rects already use hard-coverage branches to fight seams |
| Pixels backend | `crates/cranpose-render/pixels/src/draw.rs` | CPU raster path | Shares the same flatten-first mental model, so it cannot be the architecture answer either |

## Starting-Point API Reality

The exposed graphics API is broader than the current renderer semantics.

The repo already exposes:

- `GraphicsLayer`
- `RenderEffect`
- `backdrop_effect`
- `Brush` gradients
- `ColorFilter`
- draw commands for shapes, images, and text

What matters is not API presence. What matters is whether the renderer preserves the correct semantics when those APIs are moved, clipped, blended, blurred, or cached.

The current reality is:

| Capability | Current API Surface | Current Execution Reality |
|---|---|---|
| Translation / scale / rotation / perspective | Exposed | Transform is flattened into child geometry during scene build via `combine_layers` + `apply_layer_affine_to_rect` |
| Subtree alpha / blend | Exposed | Isolation exists only as a flat z-range replay mechanism; not a real layer model |
| Blur / offset / runtime shader | Exposed | Implemented through offscreen passes, but fed by flat scene ranges and full-frame targets (all offscreens are viewport-sized) |
| Backdrop effect | Exposed | Implemented as a special event in a flat z-ordered scene |
| Text / shapes / images | Exposed | Rasterized directly in device space after transform flattening |
| Blend modes | Exposed | Current WGPU execution is only production-credible for `SrcOver` and `DstOut`; unsupported modes fall back |

This is why API parity claims are not enough. A production UI renderer is judged by execution semantics under motion and composition.

## Root Cause

The current architecture is wrong for production-grade UI rendering.

The core failure is this:

1. Scroll applies `placement_offset` via `ScrollNode::measure()`, which becomes part of child node positions.
2. Scene building via `render_node_from_applier` reads each node's absolute position and combines it with the accumulated `GraphicsLayer` state.
3. `combine_layers` accumulates parent translation/scale/rotation into a single `GraphicsLayer` per node.
4. `apply_layer_affine_to_rect` / `apply_layer_to_quad` bake that accumulated transform directly into every child primitive's device-space coordinates.
5. The renderer rasterizes each primitive at those device-space coordinates.
6. Effects and backdrop are then layered back on top as flat z-range events over the flat arrays.

That guarantees phase-dependent output.

The same subtree can produce a different picture when the parent translation changes by a fraction, because:

- sibling primitives are no longer preserved as one local picture
- clip bounds are recomputed in device space
- effect bounds are recomputed in device space
- coverage is evaluated primitive-by-primitive against the pixel grid
- full-frame offscreen replays re-sample content at the wrong granularity

This is why the bug is not "about scroll". Scroll is only the easiest way to produce fractional parent translation continuously.

The architecture failure is renderer-side:

- subtree motion is not represented as subtree motion
- it is represented as rewritten child coordinates

That is the wrong model.

## Non-Negotiable Rendering Invariants

The rewrite must satisfy these invariants:

1. Rigid subtree motion preserves the subtree picture.
   After compensating for parent translation, the subtree image is identical.

2. Parent translation does not change child-relative spacing.
   A 4 px gap inside a moving subtree stays a 4 px gap.

3. Blur, shader effects, backdrop, and subtree alpha operate on layer results, not on ad hoc primitive groups.

4. Offscreen rendering is bounded by real layer bounds, not the full frame, unless the effect itself truly needs the full frame.

5. The same scene model is consumed by both renderer backends.

6. There is one scene-building implementation.
   The dead-code `render_layout_tree` path in `pipeline.rs` must be removed as part of cleanup.

7. No "fix" may rely on snapping scroll, snapping child positions, or papering over the bug in lazy list logic.

## Proper Architecture

The correct model is a hierarchical render graph with an explicit compositor.

The renderer must do two different jobs:

1. Paint content in stable local coordinates.
2. Composite that content with transforms, clips, alpha, blend, and effects.

### Target Scene Model

The current flat `Scene` in `crates/cranpose-render/wgpu/src/scene.rs` should be replaced by a hierarchical graph conceptually shaped like this:

```rust
enum RenderNode {
    Primitive(PrimitiveNode),
    Layer(LayerNode),
}

enum PrimitiveNode {
    Shape(ShapePrimitive),
    Text(TextPrimitive),
    Image(ImagePrimitive),
}

struct LayerNode {
    node_id: Option<NodeId>,
    local_bounds: Rect,
    transform_to_parent: Transform,
    clip: Option<ClipNode>,
    opacity: f32,
    blend_mode: BlendMode,
    effect: Option<RenderEffect>,
    backdrop: Option<RenderEffect>,
    isolation_reasons: IsolationReasons,
    cache_policy: CachePolicy,
    children: Vec<RenderNode>,
}
```

The exact Rust type names can differ. The architectural requirements cannot.

### What Must Move Out Of Primitive Records

These values must stop being baked into every primitive draw record:

- parent translation
- parent alpha
- parent clip result
- parent effect bounds
- parent backdrop order

Primitives should keep:

- stable local geometry
- stable local brush/image/text inputs
- primitive-local clip or mask references

Layers should own:

- transform
- opacity
- blend mode
- effect
- backdrop
- cache policy
- isolation decision

### Transform Model

The transform representation must become explicit instead of implicit.

Today `crates/cranpose-render/common/src/style_shared.rs` uses:

- `combine_layers(...)` — accumulates parent+child `GraphicsLayer` fields (translation, scale, rotation) into a single struct
- `apply_layer_affine_to_rect(...)` — bakes accumulated translation and scale into primitive coordinates
- `apply_layer_to_quad(...)` — bakes accumulated transform including rotation and perspective into quad vertices
- `apply_layer_to_rect(...)` — convenience wrapper: `quad_bounds(apply_layer_to_quad(...))`

Those helpers are useful for transform math, but they are currently used to flatten hierarchy into final geometry.

The rewrite should preserve transform as data:

- compose transforms when traversing the graph
- compute device bounds for culling and surface allocation
- do not rewrite descendants into final device-space rectangles as the primary representation

The key mechanism: primitives must carry local-space vertex data. The parent transform must be applied as a GPU uniform matrix (or per-instance data) at draw time. This is what makes subtree pictures stable under parent translation — the GPU applies the transform after rasterization coverage is computed in local space.

Because `GraphicsLayer` already exposes scale, translation, rotation, and perspective, the target transform type should be a real transform object (3x2 affine or 4x4 matrix), not just offset plus scale.

## Composition Model

### Direct Path

Some layers can still draw without offscreen isolation:

- opaque content
- no effect
- no backdrop
- no subtree alpha isolation requirement
- no special blend mode
- no cache boundary

For the direct path, the compositor concatenates the layer's transform into the GPU uniform and draws primitives directly to the parent target. This path exists for performance.

### Isolated Path

A layer must isolate when semantics require it, including:

- subtree blur
- runtime shader effect
- backdrop sampling
- subtree alpha over overlapping children
- nontrivial blend mode
- explicit offscreen compositing strategy
- raster cache boundary for rigid motion

Isolation here means:

1. Render the subtree once in local coordinates into a bounded offscreen target.
2. Apply the effect in local layer space.
3. Composite the resulting texture into the parent using the layer transform.

### Backdrop Path

Backdrop is a compositor feature, not a primitive feature.

The proper backdrop sequence is:

1. Composite prior content behind the layer.
2. Snapshot only the backdrop region required by the layer bounds.
3. Filter that backdrop snapshot.
4. Composite the filtered backdrop result.
5. Composite the layer's own content above it.

The current `BackdropLayer` event approach in `crates/cranpose-render/wgpu/src/render.rs` proves the repo already needs compositor ordering. The graph model should encode it directly instead of reconstructing it from flat z-ranges.

## Why This Fixes The Scroll/Lazy Bug

Today a lazy list item is not preserved as a picture. Its children are redrawn at their final translated coordinates every frame.

The proper architecture changes that:

- the lazy list item subtree is painted in local coordinates
- the compositor moves the subtree as one unit
- the spacing inside the item is frozen in the local raster result

That is what prevents the pixel-wide gap from changing when the list moves by a fraction.

For production use, scroll and lazy content should normally move cached item layers or cached tiles, not force every descendant primitive to re-rasterize at a new device-space phase.

## Effects, Transparency, And Shader Semantics

Blur, backdrop, transparency, and runtime shaders are not special exceptions. They are the reason the layer/compositor architecture is required.

### Content Blur

Correct model:

- render subtree into a bounded local offscreen
- blur that surface
- composite the result with the layer transform

Wrong model:

- blur individual descendants independently
- or blur a flat z-range as if it were a real retained layer

### Backdrop Blur / Backdrop Shader

Correct model:

- sample already-composited background in the layer bounds
- filter that background snapshot
- composite the layer content on top

Wrong model:

- try to encode backdrop as a regular primitive draw

### Transparency

Correct model:

- use premultiplied alpha everywhere
- modulate directly only when semantics are provably equivalent
- isolate the subtree when overlap would otherwise change the result

Wrong model:

- multiply alpha into descendants and hope it matches group opacity in all cases

### Runtime Shader Effects

Correct model:

- shader input is a layer texture or a backdrop snapshot
- uniforms live in layer space
- bounds are explicit and bounded

Wrong model:

- tie shader meaning to flattened device-space child primitives

## Performance

The proper architecture can be faster than the current one, but only if isolation is selective and caching is real.

### Where The Win Comes From

- less CPU churn from rewriting descendant geometry every frame
- cleaner batching because transforms become layer or instance data
- bounded offscreen surfaces instead of full-frame effect surfaces
- real raster cache for scrollable and repeatedly transformed subtrees
- cleaner invalidation because content identity and transform identity are separate
- cheaper list scrolling because cached rows can be composited rather than repainted

### Where It Can Lose

- if every node becomes an offscreen layer
- if cache bounds are loose
- if large backdrop or blur surfaces are re-rendered unnecessarily
- if layer invalidation is too coarse

### Performance Rules For The Rewrite

1. Do not isolate every layer.
2. Do not allocate full-frame offscreens for ordinary subtree effects.
3. Cache stable subtrees.
4. Bound all cache entries tightly.
5. Track the total offscreen pixel budget per frame.
6. Keep the direct path for simple content.

### Expected High-Value Cache Boundaries

- lazy list items
- generic scroll container tiles for very large content
- effect-heavy cards
- text-heavy composite widgets
- explicitly offscreen layers

## Rewrite Status

Status snapshot as of 2026-03-10:

| Phase | Status | Notes |
|---|---|---|
| Phase 1 | Partial | Shared normalized translation-invariance tests landed for WGPU and Pixels, real-app robot translation coverage landed, and dedicated WGPU translated-backdrop capture coverage landed. Current translation-diff budgets are still freeze-the-failure thresholds rather than tight final budgets. |
| Phase 2 | Done | Shared hierarchical graph types, shared scene builder, explicit `transform_to_parent`, graph hashes, and shared graph-scene hit contract are in production paths. |
| Phase 3 | Done | WGPU renders by graph traversal with local-coordinate primitives, bounded layer surfaces, and compositor-driven transforms instead of flat z-range replay. |
| Phase 4 | Done | Bounded local-surface effect and backdrop execution landed in WGPU, and WGPU capture coverage now locks subtree alpha, bounded blur, and bounded backdrop semantics to bounded local surfaces. |
| Phase 5 | Partial | The cache and perf instrumentation work is implemented: layer raster cache, stable subtree hashes, per-frame GPU stats, per-frame upload-bytes accounting, the scenario-driven perf harness, and perf-script counter summaries all landed. The non-headless Shaders comparison already exposed and fixed a root presentation-scale bug, but recorded `main` vs `renderer` acceptance data and the remaining Shaders-tab over-isolation/perf regression still need to be closed. |
| Phase 6 | Mostly done | Pixels and WGPU now share the graph scene contract, shared graph builder, shared transform semantics, and shared render-contract tests. Remaining work is more shared semantic coverage, not another scene-model rewrite. |

## Rewrite Plan

This rewrite is significant. That is correct. The current architecture is already the wrong foundation.

Each phase replaces the previous architecture entirely. No phase may leave the repo in a half-state where both old and new models coexist in production paths.

### Phase 1: Freeze The Failure With Tests [Partial]

Status:

- done: shared normalized translation-invariance cases for translated subtrees and translated text decorations run against both WGPU and Pixels
- done: robot translation regression covers decorated text and lazy-list subtree motion in the real desktop app
- done: a dedicated WGPU translated-backdrop capture case now covers rigid motion for a subtree that contains both backdrop source content and the backdrop layer itself
- not done: current normalized-diff budgets are still loose freeze-the-failure thresholds and must tighten after the remaining capture coverage lands

Add automated failures before touching the renderer:

- a WGPU capture test for two separated rounded blocks inside one translated subtree
- a WGPU capture test for text plus decoration plus shadow inside one translated subtree
- a lazy-list-specific test that verifies the gap between `Item #N` and `Hello #N` stays constant across fractional scroll offsets
- a backdrop plus translated-content capture test
- equivalent CPU-path expectations where the backend claims the same semantics

Validation target:

- failing tests reproduce the current instability without depending on a human looking at screenshots

### Phase 2: Hierarchical Scene Graph And Scene Builder [Done]

Status:

- done: shared graph types, shared builder, shared graph-scene hit model, and explicit graph-side transform propagation are in place
- done: the dead layout-tree scene-builder path is gone from production use

Replace the flat scene model and scene builder in one step. Introducing the graph types without simultaneously switching the builder to emit them would create a half-state.

Files:

- `crates/cranpose-render/common/src/lib.rs` — shared graph types
- new shared graph types in `crates/cranpose-render/common`
- `crates/cranpose-render/wgpu/src/scene.rs` — replace flat `Scene`
- `crates/cranpose-render/wgpu/src/pipeline.rs` — `render_from_applier` emits graph nodes instead of flat draw lists; delete dead-code `render_layout_tree` path

Work:

- define render-graph node types, transform types, clip types, effect descriptors, cache hints, and isolation reasons
- replace the flat `Scene` struct (flat arrays + z-indices) with the hierarchical `RenderNode` graph
- modify `render_node_from_applier` to emit `LayerNode` / `PrimitiveNode` instead of calling `scene.push_shape_with_geometry` etc.
- stop using `apply_layer_to_rect` / `apply_layer_to_quad` as the primary output representation — preserve local primitive coordinates and explicit layer transforms
- keep hit-testing identity attached to the graph
- delete the dead `render_layout_tree` / `render_layout_node` code path

Validation target:

- unit tests for graph construction, transform composition, layer bounds propagation, and isolation reason derivation
- new tests confirm parent translation changes only layer transform data, not child local geometry
- existing scene-build unit tests still pass after being ported

### Phase 3: Compositor And Local-Coordinate Rendering [Done]

Status:

- done: WGPU traverses the graph directly, renders primitives in local coordinates, and composites bounded layer surfaces
- done: direct-path vs isolated-path selection is driven by layer semantics rather than flat z-range replay

Replace the z-range event replay renderer with graph traversal, and simultaneously port primitive paint to use local coordinates with GPU transform uniforms. These are one unit of work: the compositor decides how to draw each layer, and each layer's primitives must be in local coordinates for the compositor's transform to be meaningful.

Files:

- `crates/cranpose-render/wgpu/src/render.rs`
- `crates/cranpose-render/wgpu/src/shaders.rs`
- `crates/cranpose-render/wgpu/src/effect_renderer.rs`

Work:

- replace z-range event replay with graph traversal
- compute per-layer device bounds for culling and bounded offscreen allocation
- choose direct draw vs isolated offscreen per layer based on isolation reasons
- render shapes, images, and text using local geometry with a compositor-provided transform uniform (3x2 affine or 4x4 matrix) — this is the mechanism that makes subtree pictures stable
- make offscreen allocation bounded by computed layer bounds instead of full-frame
- preserve backdrop ordering in the traversal itself

Validation target:

- translation-invariance capture tests from Phase 1 stop failing
- ordinary static rendering remains pixel-correct
- tests confirm bounded offscreen allocation for small isolated layers
- tests confirm nested isolated layers and nested backdrop layers produce correct ordering

### Phase 4: Effect Correctness At Layer Boundaries [Done]

Status:

- done: blur, runtime shader effects, and backdrop execution operate on bounded local layer surfaces in WGPU
- done: explicit WGPU capture assertions cover subtree alpha correctness, bounded blur correctness, and bounded backdrop correctness

Make effects operate on bounded local layer surfaces instead of full-frame z-range replays.

Files:

- `crates/cranpose-render/wgpu/src/effect_renderer.rs`
- `crates/cranpose-render/wgpu/src/render.rs`

Work:

- make blur operate on bounded local layer surfaces
- make runtime shader effects consume local layer inputs
- make backdrop consume bounded backdrop snapshots
- enforce premultiplied alpha semantics through the effect pipeline

Validation target:

- capture tests for blur, alpha, and backdrop match layer semantics
- no effect path allocates a full-frame target when layer bounds are small

### Phase 5: Raster Cache And Motion-Aware Reuse [Partial]

Status:

- done: isolated-layer raster caching, stable subtree cache hashes, and frame-level cache hit/miss/eviction stats landed in WGPU
- done: rigid-scroll cache reuse is covered by automated tests
- done: the perf harness now exposes explicit `lazy_list_scroll`, `text_heavy_scroll`, `backdrop_blur`, and `opaque_scene` scenarios
- done: the perf harness now emits machine-readable `PERF_MEMORY_SUMMARY`, `PERF_RENDER_SUMMARY`, `PERF_FPS_SUMMARY`, and `PERF_SCENARIO_COMPLETE` lines
- done: perf scripts now collect and summarize renderer counters for the perf scenarios
- done: the planned per-frame upload-bytes counter now exists in renderer stats
- not done: the acceptance numbers from those scenarios are not yet recorded in the document
- not done: there is not yet a written pass/fail evaluation for each scenario against the acceptance criteria
- not done: the benchmark harness still has to be applied identically to `main` and `renderer` before any branch-to-branch judgment is valid
- not done: there is not yet a checked-in comparison table for median `main` vs `renderer` scenario results
- done: the non-headless Shaders comparison exposed a root presentation-scale bug, and the compositor now scales the root surface to the physical swapchain correctly
- not done: the same Shaders comparison still shows `renderer` slower than `main` during deep Shaders-tab scroll, and the remaining cost cannot be fixed by collapsing moving text subtrees into parent-space rasterization without breaking translation invariance

Files:

- `crates/cranpose-render/wgpu/src/render.rs`
- `crates/cranpose-render/wgpu/src/lib.rs`
- cache support types in `crates/cranpose-render/common`
- `apps/desktop-demo/robot-runners/robot_perf_harness.rs`
- `perf_robot_cpu.sh`
- `perf_robot_heap.sh`

Work:

- define cache keys from stable subtree identity, content hash, effect parameters, local bounds, and scale bucket
- cache isolated layer results
- reuse cached textures during scroll and rigid translation
- instrument hit rate, evictions, and offscreen pixel cost

Validation target:

- lazy list scroll uses cache reuse instead of repainting every descendant
- performance counters show reduced uploads and reduced isolated repaints in scroll-heavy scenes
- acceptance data for all four scenarios is recorded in this document so the repo has a concrete baseline
- the branch-to-branch verdict is based on identical benchmark code running on both branches, not on branch-specific tooling differences

### Phase 6: Port The Pixels Backend And Clean Up [Mostly Done]

Status:

- done: Pixels consumes the shared hierarchical graph and the public scene contract is shared between both backends
- done: flat scene storage no longer defines the backend API surface
- done: shared render-contract tests run against both backends
- not done: more shared semantic coverage is still needed so backend-local pixel assertions keep collapsing into common tests

Port the pixels backend to consume the same hierarchical graph and remove all remnants of the flat scene architecture.

Files:

- `crates/cranpose-render/pixels/src/draw.rs`
- `crates/cranpose-render/common/src/style_shared.rs`
- related pixels scene code

Work:

- consume the same hierarchical graph in the pixels backend
- keep local/layer semantics aligned with WGPU
- accept feature gaps only where the backend truly cannot execute a feature, but do not accept a different scene model
- delete any remaining flat-scene-only structures
- delete transform-flattening helpers that no longer belong in scene output (`apply_layer_to_rect`, `apply_layer_to_quad`, `apply_layer_affine_to_rect` as scene-output functions — keep them if still needed for hit-testing or bounds computation)
- remove code paths that preserve the previous architecture

Validation target:

- shared semantic tests pass for both backends wherever the feature contract overlaps
- there is one renderer architecture left in the repo, not two competing ones

## Validation Strategy

The validation must not depend on human eyes.

### 1. Scene-Structure Unit Tests [Done]

Status:

- done: graph construction, transform propagation, layer bounds, hit semantics, and cache-hash behavior are covered close to the graph and scene-builder code

Add unit tests that assert:

- translated parent layers do not rewrite child local geometry
- layer bounds are propagated correctly
- isolation reasons are stable
- backdrop dependencies are explicit
- cache policy selection is deterministic

These belong close to the scene builder and graph types.

### 2. WGPU Capture Tests [Done]

Status:

- done: shared WGPU capture coverage exists for rigid translated subtrees, rounded-block spacing, translated text with shadow and decorations, and translated backdrop content
- done: explicit capture assertions cover subtree alpha correctness, bounded blur correctness, and bounded backdrop correctness

Use `capture_frame(...)` / `capture_frame_with_scale(...)` from `crates/cranpose-render/wgpu/src/lib.rs`.

Add tests for:

- rigid subtree translation invariance
- translated rounded blocks with fixed gap
- text plus shadow plus decoration under translation
- subtree alpha correctness
- bounded blur correctness
- bounded backdrop correctness

Important rule:

- compare normalized subtree output, not just whole-frame raw pixels at different translations

Whole-frame pixels at different absolute positions are not the right assertion. The correct assertion is that the subtree picture is preserved after compensating for the parent transform.

### 3. Robot Regression Tests [Done]

Status:

- done: lazy-list fractional scroll, translated text/shadow/decorations, backdrop drag, and scroll visual coverage all run in the desktop robot suite
- done: the suite now includes a normalized screenshot comparison for rigid subtree motion in the real app
- not done: the current robot screenshot path is still a logical-scene capture, not a physical presented-window capture, so it does not catch HiDPI / viewport presentation-scale bugs in the desktop window

Use the desktop robot harness and keep the checks measurable.

Required robot coverage:

- lazy list fractional scroll regression
- translated text/shadow/decorations regression
- backdrop drag regression
- scroll visual regression

Relevant existing paths:

- `apps/desktop-demo/robot-runners/robot_scroll_visual.rs`
- `apps/desktop-demo/robot-runners/robot_lazylist_redraw_bug.rs`
- `apps/desktop-demo/robot-runners/robot_shader_backdrop_drag.rs`
- `apps/desktop-demo/robot-runners/robot_lazy_list.rs`

The robot assertions should measure:

- stable pixel gap between blocks across fractional scroll deltas
- stable local bounds for translated cards
- no frame-to-frame picture drift inside a rigidly moving subtree

Important limitation that still needs to be fixed:

- `Robot::screenshot()` currently captures the renderer scene at layout-root logical size instead of the actual presented desktop window surface
- that means it can miss physical-window failures where the live WGPU output is scaled, cropped, or letterboxed incorrectly on screen
- the desktop validation plan therefore still needs a true presented-window capture path for non-headless comparisons on HiDPI displays

### 4. Performance Validation [Partial]

Status:

- done: renderer-side counters exist for isolated layers, offscreen acquires, cache hit/miss/evictions, compositor submits, and per-frame upload bytes
- done: the robot perf harness now has explicit scenario selection and machine-readable summary output
- done: `perf_robot_cpu.sh` and `perf_robot_heap.sh` now capture and summarize those counters for the perf scenarios
- not done: acceptance data for lazy-list scroll, text-heavy scroll, backdrop blur, and simple opaque scenes is not recorded yet
- not done: the document still lacks explicit interpretation of those numbers against the acceptance criteria
- not done: the Shaders-tab branch comparison exposed a concrete WGPU regression that must be mitigated before final baseline recording
- not done: the current robot screenshot path is insufficient for this regression because it captures logical scene output rather than the physical desktop window presentation

The rewrite is only acceptable if correctness improves without turning the renderer into an offscreen-everything system.

The comparison method matters:

- do not compare `main` using one harness and `renderer` using a newer harness
- do not use renderer-only counters as the primary cross-branch regression verdict
- use the same benchmark harness code on both branches, then use renderer-only counters only to explain results on `renderer`
- keep build artifacts separate per branch so incremental build state does not contaminate the measurement

Recommended setup:

- create a `renderer` worktree
- create a `main` worktree
- apply the same benchmark-only harness changes to both worktrees
- use separate `CARGO_TARGET_DIR` values per worktree
- run each scenario multiple times and compare medians, not single runs

Collect before/after data for:

- lazy list scroll
- text-heavy list scroll
- backdrop blur panel
- simple opaque scene

Use:

- `./perf_robot_cpu.sh`
- `./perf_robot_heap.sh`
- `CRANPOSE_PERF_SCENARIO=<scenario> cargo run -p desktop-app --example robot_perf_harness --features robot-app`
- `git worktree` for side-by-side `main` and `renderer` runs
- renderer counters already present in `crates/cranpose-render/wgpu/src/render.rs`
- text telemetry in `crates/cranpose-render/wgpu/src/lib.rs`

Cross-branch verdict data:

- branch-neutral metrics on both branches:
  - FPS summary
  - memory summary
  - CPU profile / heap profile output
- renderer-only diagnostic metrics on `renderer`:
  - cache hit/miss
  - isolated-layer area
  - upload bytes
  - offscreen allocations

Scenarios:

- `lazy_list_scroll`
- `text_heavy_scroll`
- `backdrop_blur`
- `opaque_scene`

Collected counters now include:

- number of isolated layers
- isolated layer pixel area
- cache hit rate
- cache miss rate
- cache evictions
- per-frame offscreen allocations
- per-frame upload bytes
- compositor submits

Acceptance criteria:

- rigid-motion regressions are gone
- small isolated effects do not allocate full-frame surfaces
- simple opaque scenes do not regress materially
- scroll-heavy scenes show cache reuse and reduced repaint work

Recording template for the required branch comparison:

| Scenario | `main` median summary | `renderer` median summary | Verdict | Notes |
|---|---|---|---|---|
| `lazy_list_scroll` | pending | pending | pending | compare identical harness code in separate worktrees |
| `text_heavy_scroll` | pending | pending | pending | compare identical harness code in separate worktrees |
| `backdrop_blur` | pending | pending | pending | compare identical harness code in separate worktrees |
| `opaque_scene` | pending | pending | pending | compare identical harness code in separate worktrees |

Observed non-headless branch gap on 2026-03-10:

- same visual scroll choreography in separate `main` and `renderer` worktrees showed a real regression on the desktop demo `Shaders` tab
- first concrete cause already found and fixed: the root isolated surface was being composited into the swapchain in logical coordinates instead of physical coordinates, which presented the whole scene at the wrong on-screen scale on HiDPI displays
- second concrete cause already found: translation-only wrapper layers were still taking the isolated child-layer path, which forced ordinary card chrome and text through nested offscreens and made the whole scrolled surface visibly softer
- a broad direct-render collapse for translation-only non-isolating child layers materially reduced the compositor cost on this surface:
  - before that fix: `submits=724`, `offscreen_acquires=361`, `isolated_layer_renders=336`, `isolated_layer_pixels=34.95 MP`, `composite_passes=370`
  - during that experiment: `shaders_open=66.3 FPS`, `scroll_down_4=6.9 FPS`, `submits=209`, `offscreen_acquires=83`, `isolated_layer_renders=64`, `isolated_layer_pixels=13.23 MP`, `composite_passes=88`
- that experiment was rejected because it broke the shared translation-invariance contract for text-bearing subtrees and also broke the desktop robot lazy-list translation contract; moving text, shadow, and decoration draws by post-raster translation is not semantically correct in the current renderer
- the current checked-in `main` reference run for the same choreography is:
  - `shaders_open=30.6 FPS`
  - `scroll_down_4=10.5 FPS`
- current visual state:
  - the root-scale bug is gone
  - the checked-in renderer still shows visible softness on ordinary text and slider labels in the scrolled Shaders cards
  - the current checked-in non-headless renderer run is `shaders_open=13.5 FPS`, `scroll_down_4=4.4 FPS`, `submits=545`, `offscreen_acquires=250`, `isolated_layer_renders=225`, `isolated_layer_pixels=34.07 MP`, `composite_passes=263`
  - the remaining gap is still both visual softness and scroll-time cost, but the rejected direct-collapse experiment showed that the cost can drop sharply if the wrapper-layer isolation problem is solved without violating translation invariance
- the current checked-in robot screenshot path still does not expose physical-window presentation bugs because it captures logical scene output rather than the actual presented window; live-window validation still has to become part of the acceptance path for this class of bug

Mitigation plan for the Shaders-tab regression:

- instrument the remaining Shaders-tab isolated renders at subtree granularity so each one is attributed to a concrete card or preview and an explicit isolation reason
- identify wrapper layers that exist only to position or group descendants and collapse those wrappers without collapsing the descendant text/effect surfaces themselves into parent-space rasterization
- keep text-bearing, shadow-bearing, and effect-bearing moving subtrees on local surfaces; their rigid motion has to be expressed by compositing cached child surfaces, not by translating already-rasterized text draws inside the parent scene
- reduce repeated repaint work for those remaining cached child surfaces during rigid scroll so the deep-scroll path approaches the rejected experiment's cost model without sacrificing translation invariance
- keep auditing local-surface resolution and parent-to-child compositing on the scrolled Shaders cards so ordinary text and slider content stop looking softer than `main`
- replace the current logical-scene screenshot path with a true presented-window capture path, or add that as a second explicit robot capture mode, so future non-headless branch comparisons can lock down physical-window scale and viewport correctness
- add automated capture coverage for the Shaders tab surface itself so this regression is locked down as a render failure, not just a human visual complaint
- rerun the non-headless `main` vs `renderer` comparison after the fix, then record the formal median baseline table and pass/fail verdicts for the four Phase 5 scenarios

Still required to close this section:

- reduce the remaining Shaders-tab scroll-time isolation and repaint cost identified above until the deep-scroll path is no worse than `main`, without regressing the translation-invariance contract
- add a physical-window capture path for desktop robot validation so presentation-scale bugs are observable in automated artifacts
- create a comparison-safe benchmark baseline on `main` using the same harness code as `renderer`
- record medians from repeated runs, not single-run spot checks
- run the CPU perf script and record the summary block for each scenario
- run the heap perf script and record the summary block for each scenario
- write down whether each scenario meets the acceptance criteria above
- keep those recorded numbers up to date when the renderer changes materially

### 5. Full Repository Gates [Done]

Status:

- done: the landing workflow already uses the repository-wide fmt, test, clippy, wasm, Android, duplicate-tree, and robot gates listed below

Every landing step must pass:

```bash
cargo fmt
cargo test > 1.tmp 2>&1
cargo clippy > 2.tmp 2>&1
cargo tree --duplicates
apps/desktop-demo/build-web.sh
cd apps/android-demo/android && ./gradlew :app:assembleRelease
./run_robot_test.sh --sequential
```

`cargo tree --duplicates` is an inspection gate, not permission to rewrite dependencies without review.

## Shortcuts That Must Be Rejected

These are not acceptable fixes:

- snapping scroll offsets
- snapping translated children to integer pixels as a global policy
- changing lazy list placement math to hide renderer instability
- using multisampling as the primary fix
- isolating every node
- keeping both the flat scene and the hierarchical scene in parallel long-term

These can sometimes hide symptoms. They do not fix the architecture.

## Immediate Next Step

The correct next implementation steps are now the narrowed Shaders-tab perf gap and then the remaining validation gaps:

- attribute the remaining Shaders-tab isolated renders to concrete preview subtrees and remove only wrapper isolation that can be removed without collapsing moving text/effect subtrees into parent-space rasterization
- reduce deep-scroll repaint cost on the Shaders tab by reusing cached child surfaces during rigid motion, not by translating already-rasterized text draws in the parent scene
- add a presented-window capture mode for desktop robot validation; the current logical-scene screenshot path is insufficient for HiDPI scale bugs
- add automated capture coverage for that Shaders surface so the visual softness cannot regress silently
- apply the benchmark harness identically to `main` so branch-to-branch perf comparison is valid
- record acceptance data for lazy list scroll, text-heavy scroll, backdrop blur, and simple opaque scenes
- write explicit pass/fail evaluation for those recorded numbers against the acceptance criteria
- tighten the current translation-invariance diff budgets now that the capture suite is in place

Anything smaller is avoidance, not closure.
