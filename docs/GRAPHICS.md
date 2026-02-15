# Graphics Branch Code Review

**Branch:** `shader` vs `main`
**Scope:** ~14,800 lines added/changed across 52 files
**Date:** 2026-02-15

## Summary

The `shader` branch adds comprehensive graphics capabilities: shadows (drop + inner), render effects (blur, offset, runtime shaders), backdrop effects, alpha masks, liquid glass, expanded brush/gradient API, and image crop/clipping improvements. The architecture is solid and the feature set substantially closes the gap with Jetpack Compose's graphics API.

---

## DRY Violations

### D1: `intersect_rect` duplicated 5 times [HIGH]

Identical function defined in 5 separate locations:

| File | Line | Name |
|------|------|------|
| `crates/cranpose-render/wgpu/src/render.rs` | 2555 | `intersect_rect` |
| `crates/cranpose-render/wgpu/src/pipeline.rs` | 438 | `intersect_rect` |
| `crates/cranpose-render/pixels/src/pipeline.rs` | 413 | `intersect_rect` |
| `crates/cranpose-ui/src/widgets/image.rs` | 230 | `intersect_rect` |
| `crates/cranpose-render/common/src/style_shared.rs` | 227 | `intersect_rects` |

**Fix:** Move to `cranpose-ui-graphics::Rect::intersect(&self, other: Rect) -> Option<Rect>` as a method on `Rect`. All 5 call sites can then use `rect_a.intersect(rect_b)`.

### D2: WGSL shader boilerplate duplicated across effect shaders [HIGH]

The following WGSL functions are copy-pasted verbatim across multiple shader strings:

| Function | Occurrences | Files |
|----------|------------|-------|
| `fullscreen_vs` | 7+ | `alpha_mask.rs` (x3), `liquid_glass.rs` (x1), `shaders.rs` (x3+) |
| `get_float` | 5+ | `alpha_mask.rs` (x3), `liquid_glass.rs` (x1), `shaders.rs` |
| `sd_round_rect` | 4 | `alpha_mask.rs` (x2), `liquid_glass.rs` (x1), `shaders.rs` |
| `VertexOutput` struct | 7+ | same files |

WGSL doesn't support `#include`, but the Rust code can concatenate shader snippets at compile time. `shaders.rs` already defines `FULLSCREEN_QUAD_VS` as a const - the same pattern should be used to compose shader source strings for alpha_mask and liquid_glass rather than inlining duplicated WGSL.

**Fix:** Define shared WGSL snippets as `const &str` in a common location (e.g. `shaders.rs`) and use `format!()` or `concat!()` to build composite shader strings.

### D3: Robot test helper functions duplicated across test runners [MEDIUM]

These functions are copy-pasted identically between `robot_shader_backdrop_drag.rs` and `robot_shadow_fields.rs`:

- `changed_pixel_count_in_region`
- `changed_pixel_count`
- `scroll_down` / `scroll_up`
- `parse_slider_value`
- `scroll_prefix_into_view`
- `find_text_by_prefix_in_semantics`
- `root_bounds`
- `y_is_visible`

**Fix:** Extract to a shared `robot_helpers` module under `apps/desktop-demo/robot-runners/`.

---

## Architecture Issues

### A1: `scissor_rect_for_image` duplicates `scissor_rect_for_layer` logic [MEDIUM]

In `render.rs`, `scissor_rect_for_image` (line 2619) and `scissor_rect_for_layer` (line 2542) perform nearly identical clipping + scaling calculations. They should share a common helper.

### A2: Pixels backend does not support new features [LOW]

The pixels backend (`crates/cranpose-render/pixels/`) has no support for:
- Shadow rendering (`ShadowDraw`)
- Backdrop layers (`BackdropLayer`)
- Effect layers (blur, runtime shaders)
- Runtime shader cache

The wgpu scene (`crates/cranpose-render/wgpu/src/scene.rs`) has `shadow_draws`, `effect_layers`, `backdrop_layers` fields. The pixels scene does not. This is acceptable if pixels is only used for headless testing, but should be documented.

### A3: `style_shared.rs` partially unifies pipeline code [LOW]

`crates/cranpose-render/common/src/style_shared.rs` shares `apply_draw_commands`, `apply_layer_to_*`, etc. between wgpu and pixels pipelines. Good pattern, but `intersect_rects` is defined there yet both pipelines also have their own `intersect_rect` - see D1.

---

## JC API Conformance

### Shadow API

| JC (Shadow.kt) | Cranpose (shadow.rs) | Match |
|----------------|---------------------|-------|
| `Shadow(radius: Dp, color: Color, spread: Dp, offset: DpOffset, alpha: Float, blendMode: BlendMode)` | `Shadow { radius, color, spread, offset, alpha, blend_mode }` + `ShadowScope` | yes |
| `Shadow(radius: Dp, brush: Brush, ...)` | `ShadowScope.brush` field | yes |
| `Modifier.dropShadow(shape, shadow)` | `Modifier.drop_shadow(shape, shadow_scope)` | yes (snake_case is idiomatic Rust) |
| `Modifier.innerShadow(shape, shadow)` | `Modifier.inner_shadow(shape, shadow_scope)` | yes |
| `Modifier.shadow(elevation, shape, clip, ambientColor, spotColor)` | `GraphicsLayer.shadow_elevation` + modifier in `graphics_layer.rs` | yes |
| `Shadow.lerp()` interpolation | not implemented | missing |

**Missing:** Shadow animation interpolation (`lerp`). JC has `lerpNonNull` and `transparentCopy` for animating shadows in/out. Cranpose should add this when animation support matures.

### RenderEffect API

| JC (RenderEffect.kt) | Cranpose (render_effect.rs) | Match |
|----------------------|----------------------------|-------|
| `BlurEffect(radiusX, radiusY, edgeTreatment)` | `RenderEffect::blur(radius)` / `blur_xy(rx, ry)` | yes |
| `OffsetEffect(offsetX, offsetY)` | `RenderEffect::offset(x, y)` | yes |
| `RenderEffect.then(other)` chaining | `RenderEffect.then(other)` | yes |
| `BlurredEdgeTreatment` (Bounded/Unbounded) | `BlurredEdgeTreatment` enum | yes |
| `RuntimeShader` | `RuntimeShader` with WGSL source + uniforms | adapted (WGSL vs AGSL) |

**Note:** JC uses AGSL (Android Graphics Shading Language) for runtime shaders. Cranpose uses WGSL. This is the correct adaptation for wgpu.

### GraphicsLayerScope

| JC Property | Cranpose `GraphicsLayer` field | Match |
|-------------|-------------------------------|-------|
| `scaleX`, `scaleY` | `scale` (uniform only) | partial - JC has separate X/Y |
| `alpha` | `alpha` | yes |
| `translationX`, `translationY` | `translation_x`, `translation_y` | yes |
| `shadowElevation` | `shadow_elevation` | yes |
| `ambientShadowColor` | `ambient_shadow_color` | yes |
| `spotShadowColor` | `spot_shadow_color` | yes |
| `rotationX`, `rotationY`, `rotationZ` | `rotation_x`, `rotation_y`, `rotation_z` | yes |
| `shape` | `shape: LayerShape` | yes |
| `clip` | `clip` | yes |
| `renderEffect` | `render_effect` | yes |
| `blendMode` | `blend_mode` | yes |
| `compositingStrategy` | `compositing_strategy` | yes |
| `cameraDistance` | not implemented | missing |
| `transformOrigin` | `transform_origin` | yes |
| `colorFilter` on layer | not implemented | missing |

**Missing:** `scaleX`/`scaleY` separation (only uniform `scale`), `cameraDistance`, layer-level `colorFilter`.

### Brush/Gradient API

| JC | Cranpose | Match |
|----|---------|-------|
| `SolidColor(color)` | `Brush::Solid(color)` | yes |
| `linearGradient(colors, start, end, tileMode)` | `Brush::LinearGradient { colors, stops, start, end, tile_mode }` | yes |
| `radialGradient(colors, center, radius, tileMode)` | `Brush::RadialGradient { colors, stops, center, radius, tile_mode }` | yes |
| `sweepGradient(colors, center)` | `Brush::SweepGradient { colors, stops, center }` | yes |
| Color stops support | `stops: Option<Vec<f32>>` | yes |
| `horizontalGradient` / `verticalGradient` convenience | `horizontal_gradient` / `vertical_gradient` | yes |

Full parity on gradients.

### AlphaMask / LiquidGlass

These are **not present** in the JC reference repo at `/media/huge/composerepo/`. They appear to be cranpose-specific extensions or from a newer unreleased JC API. The implementations look well-structured with proper WGSL shaders, uniform management, and test coverage.

---

## Code Quality Findings

### Q1: No `unsafe` code [PASS]

Grep across all changed files confirms zero `unsafe` blocks. Good.

### Q2: Test coverage is solid

- `cranpose-ui-graphics`: 54 tests covering all new modules
- `cranpose-ui/modifier/tests/modifier_tests.rs`: 928+ lines of shadow/blur modifier tests
- `shadow_api_integration.rs`: 250 lines of integration tests
- `graphics_layer_backdrop_integration.rs`: 167 lines
- Two robot E2E test runners with visual verification

### Q3: Effect renderer two-pass blur [VERIFIED]

`effect_renderer.rs` correctly handles the two-pass separable blur with separate encoder+submit per pass, avoiding the wgpu staging bug. Each blur pass (horizontal then vertical) gets its own command encoder and queue submission.

### Q4: Image batching before render pass [VERIFIED]

`render.rs` batches image vertices into the buffer at different offsets BEFORE the render pass, using `base_vertex` in `draw_indexed`. This correctly avoids the `queue.write_buffer` staging issue.

### Q5: DrawCommand::WithContent splitting [REVIEW NEEDED]

In `renderer.rs`, `split_with_content` uses `.position()` to find the first `Content` marker. The logic splits primitives before Content to behind-layer, and after Content to overlay-layer. This matches JC's `drawWithContent` semantics where content is drawn between before/after draw calls.

However, each `WithContent` command is processed independently per-command in the loop (line 170-174), so multiple `WithContent` commands work correctly - each command's primitives are split individually.

### Q6: LazyGraphicsLayerElement uses `always_update() -> true` [GOOD]

The `LazyGraphicsLayerElement` correctly sets `always_update() -> true` (line 832), which means the `update()` method is always called regardless of equality. This sidesteps the Rc pointer-equality issue because the node is always updated with the latest closure.

### Q7: Image Crop/Clip improvements [GOOD]

New `crop_source_rect`, `intersect_rect`, `map_destination_clip_to_source` functions in `image.rs` properly handle ContentScale::Crop with alignment-aware source rect computation. Well-tested with 5 unit tests covering centered/aligned crops and proportional scaling.

---

## Pixel-Level Findings

### P1: Shadow elevation constants are hardcoded approximations [LOW]

In `pipeline.rs` (lines 106-111):
```rust
let spread = (elevation * 0.24).max(0.8);
let spot_offset_x = elevation * 0.18;
let spot_offset_y = elevation * 0.62;
let ambient_blur_radius = (elevation * 0.95).max(0.5);
let spot_blur_radius = (elevation * 0.72).max(0.5);
```

These are approximations of Android's native shadow rendering. Acceptable for cross-platform, but should be documented as approximations.

### P2: ShaderPipelineCache keying on source hash [GOOD]

`shader_cache.rs` uses a `u64` hash of the WGSL source string as the cache key. This is correct - same source text produces same pipeline. The cache avoids recompiling identical shaders.

### P3: OffscreenPool reuse strategy [GOOD]

`offscreen.rs` implements a pool of offscreen render targets with size-matching reuse. Targets are recycled when dimensions match, avoiding unnecessary GPU allocations.

---

## Action Items

### Must Fix (before merge)

1. **D1:** Extract `intersect_rect` to a single location (e.g. `Rect::intersect` method on `cranpose-ui-graphics::Rect`)
2. **D2:** Factor out duplicated WGSL shader snippets into shared const strings
3. **D3:** Extract duplicated robot test helpers to shared module

### Should Fix (follow-up)

4. **A2:** Document that pixels backend doesn't support effects/shadows
5. Add `scaleX`/`scaleY` separation to `GraphicsLayer` (JC has independent X/Y scaling)
6. Add shadow `lerp` interpolation for animation support
7. Add `cameraDistance` to `GraphicsLayer`

### Nice to Have

8. Unify `scissor_rect_for_image` and `scissor_rect_for_layer`
9. Add `Debug` impl for `DrawCommand`
10. Document `HeadlessRenderer` limitations re: graphics layers

---

## Files Changed Summary

### New modules
- `cranpose-ui-graphics/src/alpha_mask.rs` - Alpha mask effects with WGSL shaders
- `cranpose-ui-graphics/src/liquid_glass.rs` - LiquidGlass effect (iOS-style)
- `cranpose-ui-graphics/src/render_effect.rs` - RenderEffect hierarchy (blur, offset, shader, chain)
- `cranpose-ui-graphics/src/shadow.rs` - Shadow data types and scope
- `cranpose-ui/src/modifier/shadow.rs` - Drop/inner shadow modifiers
- `cranpose-ui/src/modifier/blur.rs` - Blur modifier
- `cranpose-render/wgpu/src/effect_renderer.rs` - GPU effect rendering infrastructure
- `cranpose-render/wgpu/src/shader_cache.rs` - Runtime shader pipeline cache
- `cranpose-render/wgpu/src/offscreen.rs` - Offscreen render target pool
- `cranpose-render/common/src/style_shared.rs` - Shared style/pipeline code

### Major changes
- `cranpose-render/wgpu/src/render.rs` - Segment-based rendering with z-order, effect layers, backdrop layers, shadow draws
- `cranpose-render/wgpu/src/pipeline.rs` - Shadow elevation rendering, backdrop layer support
- `cranpose-render/wgpu/src/shaders.rs` - Blur, offset, blit, composite shaders
- `cranpose-ui-graphics/src/brush.rs` - Full gradient API with stops, tile modes, sweep gradients
- `cranpose-ui-graphics/src/geometry.rs` - GraphicsLayer extended with render_effect, backdrop_effect, shadows
- `cranpose-ui/src/modifier/graphics_layer.rs` - Lazy evaluation, shadow integration
- `cranpose-ui/src/modifier_nodes.rs` - DrawCommand::WithContent, lazy graphics layer element

### New tests
- 54 unit tests in `cranpose-ui-graphics`
- 928+ lines of modifier tests
- 250 lines shadow API integration tests
- 167 lines backdrop integration tests
- 2 robot E2E test runners (shadow fields, shader backdrop drag)

### Demo
- `apps/desktop-demo/src/app/shaders.rs` - 2439-line shader demo with interactive controls
