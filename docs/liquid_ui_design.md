# Liquid UI — cranpose's component library

Status: design approved for implementation (2026-07-10). Owner: cranpose core.

## 1. What this is

Cranpose today is a *foundation* framework (layout, text, modifiers, animation,
render pipeline) with zero styled components. Every app (cranscan,
desktop-demo/hacker_news) has independently reinvented buttons, cards, chips
and menus. Liquid UI is cranpose's first-party component library: an
iOS-26-style "Liquid Glass" design system — translucent glass materials with
real refraction, background blur, saturation boost, chromatic aberration at
the bezel, specular rims, and spring-physics motion — implemented once, on
every platform cranpose runs on (desktop, android, ios, wasm).

Reference behavior: iOS 26 / SwiftUI `glassEffect`, the WWDC app tab bar,
morphing popup menus, Control Center tiles.

## 2. Crate placement and layering

New crate: `crates/cranpose-liquid` (`cranpose-liquid`), mirroring the
Compose Foundation → Material split:

```
cranpose-ui-graphics   (shader sources, RenderEffect, LiquidGlassSpec)
        ↑
cranpose-ui            (Modifier, widgets primitives, Popup, interaction)
        ↑
cranpose-animation     (springs — extended with value-space velocity)
        ↑
cranpose-liquid        (theme, materials, components, icons)   ← NEW
        ↑
cranpose (facade)      re-exports as `cranpose::liquid`
```

- The facade re-export is namespaced (`pub use cranpose_liquid as liquid;`),
  not flattened — apps write `use cranpose::liquid::*;`.
- `cranpose-liquid` contains **no** platform code and **no** `cfg(target_*)`.
- The glass *lens shader* lives with the other framework shaders in
  `cranpose-ui-graphics` (extending the existing `liquid_glass.wgsl`); the
  liquid crate only builds `RenderEffect` chains out of it.

## 3. The material: `Glass`

`Modifier` extension (via `LiquidModifierExt` trait, exported in the liquid
prelude):

```rust
Box(Modifier::empty()
    .glass_effect(Glass::regular().tint(colors.accent).interactive())
    .padding(12.0), spec, || { ... });
```

`Glass` (builder, plain data):

| field        | default            | meaning |
|--------------|--------------------|---------|
| `variant`    | `Regular`          | `Regular` (frosted, saturated) or `Clear` (transparent, minimal blur) |
| `shape`      | `LiquidShape::Capsule` | `Capsule`, `RoundedRect(Dp)`, `Circle` — SDF shape of the lens + clip |
| `tint`       | scheme-derived     | optional `Color` mixed over the refracted backdrop |
| `interactive`| `false`            | press → spring scale + brighter specular + haptic |
| `lensing`    | preset             | bezel width / displacement / refractive index (`LensSpec`) |

What `glass_effect` builds (all existing plumbing):

```
backdrop_effect( RenderEffect::blur(sigma).then(liquid_lens_shader) )
+ clip to shape + drop shadow + (content drawn on top)
```

The drop shadow uses `ShadowScope::cutout` — the element's own shape is
knocked out of the silhouette before the blur, so the backdrop sample never
refracts the glass's own shadow (the standard technique for translucent
surfaces; without it the material reads ~40 gray levels darker).

The **lens shader** is the existing `liquid_glass.wgsl` extended with:

1. **saturation** (uniform 18) — vibrancy boost of the refracted sample
   (Regular ≈ 1.8, Clear ≈ 1.25). Applied in-shader because backdrop
   effects cannot use `ColorFilter` (BackdropLayer carries only a
   RenderEffect).
2. **chromatic aberration** (uniform 19) — R/G/B sampled at
   `disp * (1 − ca)`, `disp`, `disp * (1 + ca)`, spread ∝ the edge-band
   slope; zero cost in the interior. Matches iOS edge dispersion.
3. **lift** (uniform 20) — scheme adaptation as a SCREEN blend toward white
   (multiply toward black when negative). Unlike an alpha tint it keeps the
   backdrop's colored ghosts alive while reading bright (Regular light ≈
   +0.48, dark ≈ −0.38).
4. **dither** (uniform 21) — hash-based ±1/255 noise to kill banding in
   blurred gradients.
5. **explicit light direction** (uniforms 22,23) — decoupled from tilt so the
   specular rim defaults to top-light; `tilt` remains the motion input.
6. **contrast** (uniform 24) — gentle pivot around mid-gray before the lift.
7. **edge band** (uniform 25) — fraction of the bezel occupied by a steep
   squircle lens carrying the strongest warp and the chromatic aberration;
   a broad gentle dome fills the rest of the bezel. The rim itself draws a
   crisp ~1.6px specular line with a counter-light arc and a faint inner
   contour so the edge stays legible on bright backdrops.

Geometry modes (container-size uniform): explicit-rect (dp), cover
(container == 0; `glass_effect`, node-sized), and local-rect
(container.x < 0; a px sub-rect of the node, animatable for shape morphs —
`ResolvedGlass::morph_rect`).

New samples use `textureSampleLevel(..., 0.0)` (no implicit derivatives —
safe under naga's GLSL/WebGL2 translation in divergent control flow).

Blur radii: Regular ≈ 18dp, Clear ≈ 3dp (density-scaled at slice
resolution time like `backdrop_blur` does today).

### Motion physics inputs

- `tilt` is fed per-frame through the lazy `graphics_layer` resolver — no
  recomposition. Sources: pointer position relative to the element (desktop /
  web), drag velocity (all platforms). The material subtly "looks at" the
  interaction — this is the iOS movement-physics feel.
- Press: scale 1.0 → 1.04 with `Spring(0.55, 380)`, specular `highlight`
  0.7 → 1.0, both velocity-preserving on release.

## 4. Theme

```rust
LiquidTheme(LiquidThemeSpec::default(), || { App() });          // auto light/dark
```

- `LiquidThemeSpec { scheme: SchemeMode::Auto | Light | Dark, accent: Color, typography, shapes }`.
  `Auto` follows `cranpose_services::isSystemInDarkTheme()`.
- Composition locals + accessors: `liquid_colors() -> LiquidColors`,
  `liquid_typography() -> LiquidTypography`, `liquid_glass_defaults()`.
- `LiquidColors` mirrors the iOS semantic palette: `label`,
  `secondary_label`, `tertiary_label`, `separator`, `fill`,
  `secondary_fill`, `background`, `secondary_background`,
  `grouped_background`, `secondary_grouped_background`, `accent`,
  `on_accent`, `destructive`, `success`, plus glass tints
  (`glass_tint`, `glass_stroke`). Light and dark palettes provided.
- `LiquidTypography`: iOS text-style ramp — `large_title` 34/700,
  `title1` 28/700, `title2` 22/700, `title3` 20/600, `headline` 17/600,
  `body` 17/400, `callout` 16/400, `subheadline` 15/400, `footnote` 13/400,
  `caption1` 12/400, `caption2` 11/400 — as `TextStyle` values (Sp units,
  font scale respected).

## 5. Components (v1)

All CamelCase composables, `(modifier, spec, callbacks…, content)` argument
order like the existing widgets. Every visual constant comes from the theme.

| Component | Behavior |
|---|---|
| `GlassSurface(modifier, Glass, content)` | the primitive container |
| `GlassButton(modifier, GlassButtonSpec, on_click, content)` | `.glass` / `.prominent` (accent-filled glass) / `.plain`; press spring + haptic |
| `GlassIconButton(modifier, spec, on_click, icon)` | circular glass, 44dp target |
| `GlassIconButtonGroup(modifier, spec, content)` | a row of circular actions whose pressed glass necks into its neighbor; declared with `scope.action(icon, description, on_click)` |
| `LiquidToggle(modifier, checked, on_change)` | iOS 26 switch: 63×28 capsule track, 37×26 capsule thumb; press lifts the thumb into a 58×39 magnifying lens (thumb dissolves, track color refracts through with a chromatic rim, track lightens); drag interpolates the track color with the finger; slow lens settle on release; haptic on commit |
| `LiquidSlider(modifier, spec, value, on_change)` | capsule track, glass thumb, optional haptic detents |
| `LiquidSegmentedControl(modifier, selected, on_select, content)` | sliding glass pill indicator with liquid stretch; `scope.segment(label)` or `scope.segment_content(description, content)` |
| `LiquidChip(modifier, spec, selected, on_click, label)` | filter pill (cranscan Library "All / Receipts") |
| `LiquidCard(modifier, content)` / `LiquidListSection(header, content)` / `LiquidListRow(spec, on_click, content)` | grouped-inset list look; rows get press wash + separators |
| `LiquidTabBar(modifier, spec, selected, on_select, content)` / `LiquidTabBarWithAccessory(…, content, accessory)` | floating glass pill; destinations declared with `scope.tab(icon, label)`; **liquid selection blob** (leading/trailing edges on separate springs → droplet stretch); optional detached circular accessory (search) |
| `LiquidNavBar(modifier, spec, scroll_offset)` | large-title → inline collapse; glass + progressive blur appears as content scrolls under (blur chained with a vertical fade mask) |
| `LiquidMenu(expanded, anchor, spec, absorbed, gesture, on_dismiss, content)` / `LiquidDropdownMenu(modifier, expanded, spec, on_dismiss, anchor_content, content)` | popup menu that **morphs out of its anchor**: rows declared with `scope.item(item, on_click)` / `scope.header(label)` / `scope.separator()`, each carrying its own action; glass bubble scales from the anchor corner (transform-origin anchored springs), items fade in staggered; checkmark/icon/destructive/section rows |
| `LiquidSearchField(modifier, state, spec)` | pill search field with icon + clear button |
| `liquid::icons` | built-in vector icon set (SF-Symbols-flavored, stroke SVGs rasterized via the existing SVG painter, tinted by the label color): chevrons, search, gear, ellipsis, plus, share, trash, doc, camera, folder, check, xmark, star, bookmark, eye, arrow up/down |

The liquid selection blob and toggle thumb use the **dual-spring stretch**
technique: the geometric leading edge runs a stiffer spring than the trailing
edge, so the shape elongates in motion and settles like a droplet. This
requires velocity-preserving springs (see §6).

## 6. Animation crate fix (prerequisite)

`Animatable<T>` today integrates the spring in *normalized progress space*
(0→1 per animation), so a mid-flight retarget rescales the physical velocity
by the new span length (visible hitch), and gesture handoff cannot inject
release velocity. Fix (in `cranpose-animation`, public API preserved):

- springs integrate in **value space**; velocity is stored in value-units/sec
  per dimension;
- `SpringScalar` generalizes to fixed-dimension vectors (f32=1, Color=4) with
  per-dimension velocity;
- new: `Animatable::animate_to_with_velocity(target, velocity)`,
  `Animatable::velocity()`; `animateTo` keeps the current velocity (true
  Compose interruption semantics);
- tween/decay paths unchanged.

## 7. Rendering cost model

Per glass element per frame: backdrop capture (region = bounds + input
padding) → 2 blur passes (ping-pong, scissored) → 1 lens pass → composite.
Static backdrops hit the existing layer raster cache; pooled offscreen
targets (`MAX_POOLED_TARGETS`) bound allocation. A screen with a tab bar +
nav bar + 2 buttons ≈ 4 captures of small regions — measured on the demo
with `CRANPOSE_GPU_STATS` before/after; robot perf harness gets a liquid
scene to lock the budget.

## 8. Demo + verification

- New `DemoTab::Liquid` page in apps/desktop-demo: component gallery over a
  scrollable colorful background (image + gradients), light/dark toggle,
  interactive tab bar / menu / toggle / slider / segmented / cards, a
  "materials" section with parameter sliders (variant, tint, lensing, CA).
- Robot: screenshot-driven headless run (`run_robot_test.sh`) exercising the
  gallery; unit tests for uniform packing, theme resolution, spring
  velocity-preservation, blob geometry.
- WASM: page included in `build-web.sh` build; shader must pass naga → WebGL2.

## 9. cranscan adoption map (follow-up phase)

| cranscan today | becomes |
|---|---|
| `widgets.rs` PrimaryButton/SoftButton/TextButton | `GlassButton` (.prominent/.glass/.plain) |
| `Chip` / segmented rows | `LiquidChip` / `LiquidSegmentedControl` |
| `card_modifier(_tinted)` + 5 hand-rolled row types | `LiquidCard` / `LiquidListRow` |
| hand-rolled header menus (library.rs, document.rs) | `LiquidMenu` |
| Chip("On"/"Off") pseudo-toggles | `LiquidToggle` |
| SearchRow / inline search | `LiquidSearchField` |
| FAB | prominent `GlassButton` capsule (floating) |
| headers + "‹ Back" | `LiquidNavBar` |
| theme constants (`ui/mod.rs::theme`) | `LiquidTheme` (accent #007AFF), dark mode enabled |

App-specific widgets stay app-side: ShutterButton, QuadOverlay, RowGlyph,
capture strip.
