# Renderer Rewrite Plan

## Status

Finished on 2026-03-19.

The renderer now follows the planned split:

```text
RenderGraph
  -> normalized_scene.rs
  -> surface_plan.rs
  -> surface_executor.rs
  -> render.rs backend primitives
```

`render.rs` no longer owns the recursive surface architecture. It provides GPU
backend primitives plus segment rasterization, while:

- `normalized_scene.rs` owns scene collection, rebasing, and windowing
- `surface_plan.rs` owns surface requirements and planning policy
- `surface_executor.rs` owns recursive isolated-surface execution

Validation at completion:

- `cargo test`
- `cargo clippy`
- `apps/desktop-demo/build-web.sh`
- Android `./gradlew :app:assembleRelease`
- `./run_robot_test.sh --sequential` -> `84/84`

## Problem

The current WGPU renderer mixes five responsibilities in one place:

1. scene normalization
2. motion semantics
3. surface planning
4. raster execution
5. effect compositing

That makes the system fragile. The same visual problem is currently expressed
through several overlapping mechanisms:

- `translated_content_context`
- `motion_context_animated`
- `LayerSurfaceReasons::text_local_surface`
- `EffectLayer`
- `EffectLayerSampleMode`
- `surface_scale_multiplier`
- `SnapAnchor`
- translated text raster-origin state

The result is not one renderer architecture. It is several partially-overlapping
ones sharing the same file.

## Goal

Replace the current ad hoc render flow with one planned render model:

```text
RenderGraph
  -> NormalizedRenderItems
  -> SurfacePlanTree
  -> RasterPasses
  -> CompositePasses
```

The renderer must have one way to answer each question:

- What is being drawn?
- In which logical space is it defined?
- Which subtree needs an isolated surface?
- At which scale is that surface rasterized?
- How is that surface composited back?

## Non-Negotiable Rules

1. No duplicated isolation concepts. A subtree either requires a surface or it does not.
2. Scroll-stable rendering is a surface requirement, not a text-only trick.
3. Text material effects, blur, backdrop, group opacity, blend mode, and stable translated capture all use the same surface planner.
4. A surface boundary is applied once. Descendants do not restart the same requirement inside it.
5. Raster execution does not decide architecture. It executes a precomputed plan.
6. Scene building does not encode compositor policy in scattered booleans.
7. There is no fast path vs slow path as separate architectures. There is one planner and one executor. Some planned surfaces are trivial and collapse away.

## What Stays

These parts are already useful and should be kept, but moved behind clearer
boundaries:

- `OffscreenPool`
- `EffectRenderer`
- glyph atlas / text preparation ownership
- GPU stats collection
- existing robot and render-contract coverage

## What Must Disappear

These concepts are currently symptoms of missing architecture and should be
removed or absorbed into stronger abstractions:

- translated local-picture emission as a special pipeline-side effect-layer trick
- separate layer-surface planning and effect-layer planning
- compositor policy hidden in `sample_mode` plus `surface_scale_multiplier`
- text-specific motion fixes living outside surface planning
- direct-child collapse logic that has to know too much about later raster behavior

## Target Abstractions

### 1. Normalized render items

Every draw emitted from the graph becomes one normalized item with:

- logical bounds
- transform to parent
- clip
- z order
- material payload
- motion policy
- surface requirements

Suggested types:

- `RenderItem`
- `RenderMaterial`
- `MotionPolicy`
- `SurfaceRequirementSet`

### 2. Surface requirements

All reasons for isolation become values in one enum:

```text
SurfaceRequirement
  GroupOpacity
  BlendMode
  RenderEffect
  Backdrop
  TextMaterialMask
  MotionStableCapture
```

The planner computes the minimal surface tree that satisfies the union of
requirements. No later stage invents new boundaries.

### 3. Surface plan tree

A surface plan node owns:

- logical rect
- raster scale policy
- composite sample policy
- blend/composite policy
- child surfaces
- direct items that raster into this surface

Suggested types:

- `SurfacePlan`
- `SurfaceScalePolicy`
- `CompositePolicy`

### 4. Single executor

`render.rs` should execute one structure:

1. allocate target for a `SurfacePlan`
2. raster direct items into it
3. execute child surfaces
4. apply declared effect/composite steps
5. return the target

No separate recursive architecture for child layers versus effect layers.

## Rewrite Order

### Phase 1. Freeze behavior with contracts

Completed.

1. keep `robot_render_translation_contract`
2. keep `robot_scroll_decoration_invariance`
3. keep `robot_tabs_scroll`
4. keep `robot_underline_screenshot`
5. keep the focused WGPU render tests that assert translated capture behavior

These are the acceptance locks for the rewrite.

### Phase 2. Extract planning data structures

Completed.

Create new modules:

- `normalized_scene.rs`
- `surface_plan.rs`
- `surface_executor.rs`

Types and logic both live in those modules now.

### Phase 3. Normalize scene emission

Completed.

Stop encoding translated local capture as `push_translated_local_picture(...)`.
Instead:

- pipeline emits normalized items
- items carry `MotionPolicy`
- planner decides whether that implies `MotionStableCapture`

### Phase 4. Unify surface planning

Completed.

Replace:

- `LayerSurfaceReasons`
- ad hoc `EffectLayer` subtree scheduling
- `EffectLayerSampleMode`
- `surface_scale_multiplier`

with one `SurfaceRequirementSet -> SurfacePlanTree` step.

### Phase 5. Move text under the same planner

Completed.

Text should not have a separate architectural escape hatch. The planner decides:

- direct glyph draw
- text material mask surface
- motion-stable surface

That means underline, glyph, shadow, gradient fill, and stroke all obey the
same surface tree.

### Phase 6. Collapse executor entry points

Completed.

Replace the current split between:

- root direct path
- layer surface path
- effect layer path

with one executor that can elide trivial surfaces after planning.

## Result

The rewrite removed the overlapping renderer concepts that caused the earlier
loop:

- translated local-picture special casing is gone
- `EffectLayerSampleMode` and ad hoc scale multipliers are gone as planner policy
- surface requirements are explicit in one shared type
- translated capture is represented as `SurfaceRequirement::MotionStableCapture`
- executor recursion is centralized in `surface_executor.rs`

## Review Standard

The rewrite is only acceptable if it leaves the renderer easier to explain than
it is today.

The correct explanation should fit in a few sentences:

1. graph build emits normalized items
2. planner builds the minimal surface tree
3. executor rasterizes that tree
4. text is one material inside that system, not its own architecture

If a new concept cannot be explained in those terms, it is probably the wrong
concept.
