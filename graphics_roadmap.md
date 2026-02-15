# Graphics Roadmap (branch `shader` vs `main`)

Updated: 2026-02-13

## 1) Branch audit snapshot

- Branch is `10` commits ahead of `main` (`main...HEAD = 0 behind / 10 ahead`).
- Graphics-focused delta is large (`42` changed files, `~8.3k` insertions).
- Added major capabilities:
  - `RenderEffect` pipeline (blur/offset/runtime shader/chain) in WGPU.
  - Backdrop-effect layers.
  - `draw_with_content` + `draw_content()` splitting.
  - Primitive-level `BlendMode` plumbed through UI -> scene -> renderer.
  - New shader demos and robot coverage.

## 2) Compose parity check (against `/media/huge/composerepo/compose`)

### Already implemented

- `CompositingStrategy` enum (`Auto`, `Offscreen`, `ModulateAlpha`) exists.
- `CompositingStrategy.Auto` and `CompositingStrategy.ModulateAlpha` now render through distinct paths.
- `drawWithContent`/`drawContent()` ordering is implemented in renderer/style split.
- `Brush` gradients now support:
  - `TileMode` for linear/radial/horizontal/vertical gradients.
  - Color-stop APIs.
  - Compose-style default `+inf` end coordinates for linear/horizontal/vertical defaults.
- `Modifier` now has parameter-style and block-style graphics-layer entry points in addition to the struct form.
- `GraphicsLayer` now models Compose parity fields:
  - rotation (`rotationX`, `rotationY`, `rotationZ`)
  - `cameraDistance`
  - `transformOrigin`
  - `shape` + `clip`
  - shadow fields (`shadowElevation`, `ambientShadowColor`, `spotShadowColor`)
- `BlendMode` vocabulary is exposed in API.

### Not implemented / mismatched

- `Modifier.graphicsLayer(...)` API shape is still not fully Compose-equivalent:
  - We now expose a parameter/block parity layer, but not all Compose overloads/fields.
- Renderer semantics for newer `GraphicsLayer` parity fields are still incomplete:
  - WGPU now applies rotation (`X/Y/Z`), `cameraDistance`, `transformOrigin`, and shadow fields in rasterization.
  - Pixels backend still does not provide full parity for rotated quads/perspective (falls back to basic 2D raster behavior).
  - Rounded `LayerShape` clip semantics are still incomplete for arbitrary subtree clipping.
- RenderEffect API parity gaps:
  - Compose supports input-effect forms (e.g. blur over input effect) and capability checks; current model is still custom and incomplete.
- Blend mode runtime support mismatch:
  - Renderers only implement `SrcOver` + `DstOut`; other modes fall back.

## 3) What was “messing” (confirmed issues)

- **Stacked render effects on one modifier chain were lossy.**
  - In `crates/cranpose-ui/src/modifier/slices.rs`, `render_effect` merge used `overlay.or(base)`, dropping one effect when both existed.
- **Unsupported blend modes degraded silently.**
  - WGPU and Pixels both coerced unknown modes to `SrcOver` with no explicit diagnostic.
- **Robot suite instability due too-short timeout on shader/backdrop drag test.**
  - `robot_shader_backdrop_drag` intermittently hit timeout (`exit=124`) in full suite.
- **WGPU validation crashes from uniform layout mismatches.**
  - `GradientStop` and `BlitUniforms` had CPU/WGSL size drift, causing fatal runtime errors in robot runs.

## 4) Work completed in this pass

- [x] Fixed render-effect merge semantics in `ModifierNodeSlices`:
  - Compose-style nested ordering now composes as `inner.then(outer)`.
  - File: `crates/cranpose-ui/src/modifier/slices.rs`
- [x] Added integration coverage for stacked effect behavior:
  - `stacked_render_effects_chain_inner_then_outer`
  - `stacked_render_effects_keep_existing_when_inner_unset`
  - File: `crates/cranpose-ui/tests/graphics_layer_backdrop_integration.rs`
- [x] Made blend fallback explicit (one-time warning) in both renderers:
  - `crates/cranpose-render/wgpu/src/render.rs`
  - `crates/cranpose-render/pixels/src/draw.rs`
- [x] Fixed robot-suite timeout for shader drag test:
  - `robot_shader_backdrop_drag` timeout increased to 120s.
  - File: `run_robot_test.sh`
- [x] Expanded gradient API/model parity:
  - Added `TileMode` support (`Clamp`, `Repeated`, `Mirror`, `Decal`) and color-stop constructors.
  - Added default `+inf` endpoint behavior for linear/horizontal/vertical defaults.
  - Files: `crates/cranpose-ui-graphics/src/brush.rs`, `crates/cranpose-ui-graphics/src/render_effect.rs`
- [x] Added renderer support for new gradient features:
  - WGPU and Pixels now consume tile mode + stop positions.
  - Files: `crates/cranpose-render/wgpu/src/render.rs`, `crates/cranpose-render/wgpu/src/shaders.rs`, `crates/cranpose-render/pixels/src/draw.rs`
- [x] Extended graphics-layer API surface:
  - Added `scale_x`/`scale_y`.
  - Added `graphics_layer_params(...)` and `graphics_layer_block(...)`.
  - Added tests for new modifier entry points.
  - Files: `crates/cranpose-ui-graphics/src/geometry.rs`, `crates/cranpose-ui/src/modifier/graphics_layer.rs`, `crates/cranpose-ui/src/modifier/tests/modifier_tests.rs`
- [x] Expanded `GraphicsLayer` data model with Compose parity fields:
  - Added rotation/cameraDistance/transformOrigin/shape/clip/shadow properties.
  - Threaded merge + hashing through modifier/renderer style pipelines.
  - Added tests covering field propagation and merge behavior.
  - Files: `crates/cranpose-ui-graphics/src/geometry.rs`, `crates/cranpose-ui/src/modifier/slices.rs`, `crates/cranpose-ui/src/modifier_nodes.rs`, `crates/cranpose-render/pixels/src/style.rs`, `crates/cranpose-render/wgpu/src/pipeline/style.rs`
- [x] Reworked compositing/isolation architecture:
  - Replaced `Offscreen => offset(0,0)` sentinel with explicit layer-isolation metadata.
  - Added explicit composite alpha path for isolated layers.
  - Implemented distinct behavior for `Auto` vs `ModulateAlpha`.
  - Files: `crates/cranpose-render/wgpu/src/pipeline.rs`, `crates/cranpose-render/wgpu/src/scene.rs`, `crates/cranpose-render/wgpu/src/render.rs`, `crates/cranpose-render/wgpu/src/effect_renderer.rs`
- [x] Fixed WGPU uniform-layout crashes:
  - `GradientStop` switched to `vec4`/`[f32; 4]` aligned layout.
  - `BlitUniforms` switched to `vec4`/`[f32; 4]` aligned layout.
  - Files: `crates/cranpose-render/wgpu/src/render.rs`, `crates/cranpose-render/wgpu/src/effect_renderer.rs`, `crates/cranpose-render/wgpu/src/shaders.rs`
- [x] Robot suite stabilized after shader/layout fixes:
  - Full suite now passes (`74/74`).
- [x] Added explicit renderer capability helpers + tests:
  - Blend-mode support matrix tests for WGPU and Pixels.
  - Render-effect support matrix tests for WGPU and Pixels.
  - Files: `crates/cranpose-render/wgpu/src/render.rs`, `crates/cranpose-render/pixels/src/draw.rs`, `crates/cranpose-render/pixels/src/pipeline.rs`
- [x] Implemented WGPU graphics-layer transform semantics in scene/raster path:
  - Added quad-based geometry emission for transformed layers.
  - `rotationX/rotationY/rotationZ`, `cameraDistance`, and `transformOrigin` now affect rendered primitives.
  - Added regression tests for transform math in `crates/cranpose-render/wgpu/src/pipeline/style.rs`.
- [x] Fixed perspective blow-up for low `cameraDistance` values:
  - Scaled projection camera distance to match Compose-like effective units and prevent vertex explosion/ray artifacts.
  - Kept backend transform math aligned between WGPU and Pixels styles.
- [x] Implemented graphics-layer shadow field rendering path:
  - `shadowElevation` + ambient/spot colors now emit shadow primitives in scene build.
  - Wired in both layout-tree and applier render paths.
  - Files: `crates/cranpose-render/wgpu/src/pipeline.rs`, `crates/cranpose-render/pixels/src/pipeline.rs`
- [x] Fixed graphics-layer demo coverage issues:
  - Shape+clip overflow content moved inside the clipped layer scope.
  - Shadow demo now sets layer `shape` explicitly for visible shape-dependent shadow behavior.
  - Files: `apps/desktop-demo/src/app/shaders.rs`

## 5) Validation status (post-fix)

- [x] `cargo test > 1.tmp 2>&1`
- [x] `cargo clippy > 2.tmp 2>&1`
- [x] `cargo fmt`
- [x] `cargo tree --duplicates` inspected (duplicates remain; no dependency changes applied)
- [x] Android release build:
  - `apps/android-demo/android :app:assembleRelease` -> `BUILD SUCCESSFUL in 6m 41s`
- [x] WASM build:
  - `apps/desktop-demo/build-web.sh` -> complete, wasm generated (`6.6M`)
- [x] Robot suite:
  - `./run_robot_test.sh` -> `74/74` passed

## 6) Current capability matrix

| Capability | WGPU renderer | Pixels renderer |
| --- | --- | --- |
| Blend modes | `SrcOver`, `DstOut` (others fallback to `SrcOver` + warn) | `SrcOver`, `DstOut` (others fallback to `SrcOver` + warn) |
| Render effects | `Blur`, `Offset`, `Shader`, `Chain` | Unsupported (fallback to base-layer rendering + warn) |
| Offscreen compositing strategy | Supported (`Auto` / `Offscreen` / `ModulateAlpha`) | `Offscreen` unsupported (warn + fallback) |

## 7) Refactor roadmap (next hard steps)

### Priority A: correctness parity

- [x] Implement true `CompositingStrategy.Auto` alpha/effect isolation semantics (offscreen when required).
- [x] Implement `ModulateAlpha` as distinct path from `Auto`.
- [ ] Add targeted parity tests for overlap/clipping edge cases to close remaining semantic drift.

### Priority B: public API parity

- [x] Align `graphicsLayer` developer API with parameterized/block-based parity entry points.
- [x] Add gradient `TileMode` and color-stop APIs to `Brush`.
- [x] Expand `GraphicsLayer` model to missing transforms/shadow/clip-related properties.
- [x] Implement WGPU renderer semantics for 3D rotation/perspective and shadows.
- [ ] Bring Pixels backend to parity for rotated/perspective graphics-layer transforms.

### Priority C: architecture cleanup

- [x] Replace `Offscreen => offset(0,0)` sentinel approach with explicit isolation/effect-layer metadata.
- [ ] Break up `crates/cranpose-render/wgpu/src/render.rs` (currently monolithic) into effect scheduling, base passes, text/image/shape passes.
- [ ] De-duplicate near-identical style/pipeline logic between WGPU and Pixels (`pipeline/style.rs` copies).

### Priority D: explicit capability matrix

- [x] Keep unsupported behavior explicit (warn + deterministic fallback) until full support lands.
- [x] Document and test renderer support matrix by blend mode and effect type.
