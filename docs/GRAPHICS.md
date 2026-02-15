# Cranpose Graphics API — Jetpack Compose Parity Reference

This document tracks Cranpose's graphics subsystem against the Jetpack Compose
(JC) reference implementation. It covers every public API surface in
`cranpose-ui-graphics` and the corresponding `Modifier` entry points in
`cranpose-ui`.

---

## GraphicsLayer

Property-by-property comparison with JC `GraphicsLayer.kt`.

| JC Property | Cranpose Field | Status | Notes |
|---|---|---|---|
| `alpha` | `alpha` | ✅ | |
| `scaleX` | `scale_x` | ✅ | |
| `scaleY` | `scale_y` | ✅ | |
| `rotationX` | `rotation_x` | ✅ | |
| `rotationY` | `rotation_y` | ✅ | |
| `rotationZ` | `rotation_z` | ✅ | |
| `cameraDistance` | `camera_distance` | ✅ | Default `8.0` matches JC `DefaultCameraDistance` |
| `translationX` | `translation_x` | ✅ | |
| `translationY` | `translation_y` | ✅ | |
| `transformOrigin` | `transform_origin` | ✅ | `TransformOrigin::CENTER` default |
| `shadowElevation` | `shadow_elevation` | ✅ | |
| `ambientShadowColor` | `ambient_shadow_color` | ✅ | |
| `spotShadowColor` | `spot_shadow_color` | ✅ | |
| `clip` | `clip` | ✅ | |
| `shape` | `shape` | ✅ | `LayerShape::Rectangle` / `Rounded(RoundedCornerShape)` |
| `compositingStrategy` | `compositing_strategy` | ✅ | `Auto` / `Offscreen` / `ModulateAlpha` |
| `blendMode` | `blend_mode` | ✅ | `SrcOver`, `DstOut`, + 10 more variants |
| `colorFilter` | `color_filter` | ✅ | Full support (see [ColorFilter](#colorfilter)) |
| `renderEffect` | `render_effect` | ✅ | Full support (see [RenderEffect](#rendereffect)) |
| — | `backdrop_effect` | 🆕 | Cranpose extension; not in JC |
| — | `scale` | 🆕 | Uniform scale shortcut (multiplied with `scale_x`/`scale_y`) |

**Modifier entry points:** `graphics_layer`, `graphics_layer_value`,
`graphics_layer_params`, `graphics_layer_block`, `shadow`, `shadow_with`.

### API notes

- `graphics_layer_block(|layer| { layer.alpha = 0.5; })` mirrors JC's
  `Modifier.graphicsLayer { alpha = 0.5f }` block syntax.
- `graphics_layer_params(...)` mirrors JC's parameter-style overload.
- `LazyGraphicsLayerElement` defers evaluation to scene-build time, matching
  JC's recomposition-deferred semantics.

---

## Shadow

Comparison with JC `Shadow.kt`.

| JC Property | Cranpose Field | Status | Notes |
|---|---|---|---|
| `radius` | `radius: Dp` | ✅ | |
| `spread` | `spread: Dp` | ✅ | |
| `offset` | `offset: DpOffset` | ✅ | `DpOffset { x: Dp, y: Dp }` |
| `color` | `color: Color` | ✅ | |
| `brush` | `brush: Option<Brush>` | ✅ | |
| `alpha` | `alpha: f32` | ✅ | |
| `blendMode` | `blend_mode: BlendMode` | ✅ | |
| `Shadow.lerp(a, b, t)` | — | ❌ | Shadow interpolation not yet implemented |

`Shadow::to_scope(density)` converts Dp units to pixel-space `ShadowScope` at
draw time — matching JC's density resolution pipeline.

**Rendering:** Both drop and inner shadows are supported via `ShadowPrimitive::Drop`
and `ShadowPrimitive::Inner`. The wgpu renderer uses a two-pass Gaussian blur
pipeline; the pixels renderer uses a CPU box-blur approximation.

---

## RenderEffect

Comparison with JC `RenderEffect.kt`.

| JC Type | Cranpose Variant | Status | Notes |
|---|---|---|---|
| `BlurEffect` | `RenderEffect::Blur { radius_x, radius_y, edge_treatment }` | ✅ | `BlurredEdgeTreatment` with bounded/unbounded modes |
| `OffsetEffect` | `RenderEffect::Offset { x, y }` | ✅ | |
| `ChainEffect` → `.then()` | `RenderEffect::Chain { first, second }` | ✅ | `.then(other)` method supported |
| — | `RenderEffect::Shader { shader: RuntimeShader }` | 🆕 | Custom WGSL shaders (see [RuntimeShader](#runtimeshader)) |

### RuntimeShader

Cranpose's `RuntimeShader` is analogous to Android's `RuntimeShader` but uses
**WGSL** instead of AGSL. Key features:

- Up to 248 user-addressable float uniforms (slots 248–255 reserved for
  renderer-injected metadata like layer rect).
- `set_float(index, value)`, `set_float2(index, x, y)`,
  `set_float4(index, x, y, z, w)` for uniform setting.
- Pipeline caching via `source_hash()` — avoids redundant shader compilation.
- Full vertex+fragment WGSL modules with standard bindings:
  - `@group(0) @binding(0)` — `input_texture`
  - `@group(0) @binding(1)` — `input_sampler`
  - `@group(1) @binding(0)` — uniform buffer

### BlurredEdgeTreatment

| JC API | Cranpose API | Status |
|---|---|---|
| `BlurredEdgeTreatment(Shape?)` | `BlurredEdgeTreatment::with_shape(LayerShape)` | ✅ |
| `.shape` | `.shape()` | ✅ |
| `.clip` | `.clip()` | ✅ |
| `RECTANGLE` | `BlurredEdgeTreatment::RECTANGLE` | ✅ |
| `Unbounded` | `BlurredEdgeTreatment::UNBOUNDED` | ✅ |
| `TileMode` | `TileMode::Clamp` / `TileMode::Decal` / `TileMode::Repeat` / `TileMode::Mirror` | ✅ |

---

## Brush & Gradients

Comparison with JC `Brush.kt` / `Shader.kt`.

| JC Type | Cranpose Variant | Status | Notes |
|---|---|---|---|
| `SolidColor` | `Brush::Solid(Color)` | ✅ | |
| `linearGradient(...)` | `Brush::LinearGradient { ... }` | ✅ | `start`, `end`, `tile_mode`, optional `stops` |
| `radialGradient(...)` | `Brush::RadialGradient { ... }` | ✅ | `center`, `radius`, `tile_mode`, optional `stops` |
| `sweepGradient(...)` | `Brush::SweepGradient { ... }` | ✅ | `center`, optional `stops` |
| `ShaderBrush` | `RuntimeShader` (via `RenderEffect::Shader`) | ✅ | WGSL instead of AGSL |
| `ImageShader` | — | ❌ | Not yet implemented |
| `CompositeShader (Porter-Duff)` | — | ❌ | Not yet implemented |

**Convenience constructors:** `vertical_gradient`, `horizontal_gradient`,
`linear_gradient_range`, `*_stops`, `*_tiled` variants are all present.

---

## ColorFilter

Comparison with JC `ColorFilter.kt`.

| JC Factory | Cranpose API | Status |
|---|---|---|
| `ColorFilter.tint(color, blendMode)` | `ColorFilter::Tint { color, blend_mode }` | ✅ |
| `ColorFilter.colorMatrix(matrix)` | `ColorFilter::ColorMatrix(...)` | ✅ |
| `ColorFilter.lighting(multiply, add)` | `ColorFilter::Lighting { multiply, add }` | ✅ |

**Modifier entry points:** `Modifier::color_filter(filter)`, `Modifier::tint(color)`.

---

## DrawScope & DrawPrimitives

Comparison with JC `DrawScope.kt`.

| JC Method | Cranpose Method | Status | Notes |
|---|---|---|---|
| `drawRect(brush)` | `draw_rect(brush)` | ✅ | |
| `drawRect(brush, blendMode)` | `draw_rect_blend(brush, blend_mode)` | ✅ | |
| `drawRoundRect(brush, cornerRadius)` | `draw_round_rect(brush, radii)` | ✅ | |
| `drawImage(image)` | `draw_image(image)` | ✅ | |
| `drawImage(image, src, dst, alpha, colorFilter, blendMode)` | `draw_image_rect(image, src, dst, alpha, color_filter, blend_mode)` | ✅ | Full parameter set |
| `drawContent()` | `draw_content()` | ✅ | Marker for modifier pipeline |
| `drawCircle(...)` | — | ❌ | Not yet implemented |
| `drawLine(...)` | — | ❌ | Not yet implemented |
| `drawPath(...)` | — | ❌ | Not yet implemented |
| `drawArc(...)` | — | ❌ | Not yet implemented |
| `drawOval(...)` | — | ❌ | Not yet implemented |
| `drawPoints(...)` | — | ❌ | Not yet implemented |

---

## Cranpose Extensions (not in JC)

### AlphaMask Effects

WGSL-based masking effects with no JC equivalent. Three shader variants:

| Effect | Modifier | Description |
|---|---|---|
| `GradientCutMask` | `Modifier::gradient_cut_mask(w, h, spec)` | Directional reveal with rounded corners and feathered edge |
| `RoundedAlphaMask` | `Modifier::rounded_alpha_mask(w, h, radius, feather)` | Rounded-rect mask with soft edges |
| `GradientFadeDstOut` | `Modifier::gradient_fade_dst_out(w, h, spec)` | DstOut-style fade to transparent along one axis |

Directions: `LeftToRight`, `RightToLeft`, `TopToBottom`, `BottomToTop`.

### LiquidGlass Effect

Port of Android's AGSL LiquidGlass shader to WGSL. Applies refractive glass
material with SDF-based rounded rectangles, height profiles, and specular rim
highlights.

- `LiquidGlassSpec` — configuration (bezel, ri, highlight, profile, tilt, tint)
- `LiquidGlassRect` — rectangular glass region
- `liquid_glass_effect(rect, spec, w, h)` — single rect
- `liquid_glass_effect_multi(rects, spec, w, h)` — chained multi-rect

### BackdropEffect

`Modifier::backdrop_effect(effect)` and `Modifier::shader_background(shader)` allow
applying render effects to content **behind** the composable's bounds. This is
used by the demo's backdrop blur and glass overlays.

---

## Backend Support Matrix

| Feature | wgpu | pixels |
|---|---|---|
| Solid/Gradient fills | ✅ | ✅ |
| Image rendering | ✅ | ✅ |
| Image crop / ColorFilter / alpha | ✅ | ✅ |
| Shadow (drop/inner) | ✅ GPU blur | ✅ CPU box blur |
| RenderEffect::Blur | ✅ two-pass Gaussian | ✅ CPU approximation |
| RenderEffect::Offset | ✅ | ✅ |
| RenderEffect::Shader (WGSL) | ✅ | ❌ |
| RenderEffect::Chain | ✅ | Partial (blur+offset) |
| BackdropEffect | ✅ | ❌ |
| AlphaMask shaders | ✅ | ❌ |
| LiquidGlass | ✅ | ❌ |
| BlendMode (full set) | ✅ | Partial (`SrcOver`, `DstOut`) |
| CompositingStrategy | ✅ | ✅ |

---

## Missing Features (follow-up items)

| Feature | JC Reference | Priority |
|---|---|---|
| `Shadow.lerp(a, b, t)` | `Shadow.kt` | Low — needed for shadow animations |
| `ImageShader` | `Shader.kt` | Medium — needed for texture-based brushes |
| `CompositeShader` | `Shader.kt` | Low — Porter-Duff shader composition |
| `drawCircle` / `drawLine` / `drawPath` | `DrawScope.kt` | Medium — vector drawing primitives |
| `drawArc` / `drawOval` / `drawPoints` | `DrawScope.kt` | Low — less common drawing ops |
