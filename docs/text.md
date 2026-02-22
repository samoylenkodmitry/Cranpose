# Text Parity Tracker

Last Updated: 2026-02-22

This document tracks API and behavior parity between Cranpose text rendering and Jetpack Compose text.

## Scope

Target parity baseline in Jetpack Compose:

- Local checkout: `/media/huge/composerepo/`

- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/TextStyle.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/SpanStyle.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/ParagraphStyle.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/font/FontFamily.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/font/FontWeight.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/style/*`

Cranpose implementation anchors:

- `crates/cranpose-ui/src/text/font.rs`
- `crates/cranpose-ui/src/text/paragraph.rs`
- `crates/cranpose-ui/src/text/style.rs`
- `crates/cranpose-ui/src/text/measure.rs`
- `crates/cranpose-ui/src/text_modifier_node.rs`
- `crates/cranpose-render/pixels/src/draw.rs`
- `crates/cranpose-render/pixels/src/pipeline.rs`
- `crates/cranpose-render/wgpu/src/lib.rs`
- `crates/cranpose-render/wgpu/src/pipeline.rs`
- `crates/cranpose-render/wgpu/src/render.rs`

## API Parity Status

| Feature | Status | Notes |
|---|---|---|
| `FontFamily` generic values (`Default`, `SansSerif`, `Serif`, `Monospace`, `Cursive`) | PARTIAL | Supported, plus `Fantasy` and `Named(String)`. Compose also supports resolver/file-backed/list/typeface families which are not modeled 1:1 yet. |
| `FontWeight` range and named constants | ALIGNED | Range validation and constants (`W100..W900`, aliases) implemented. |
| `TextAlign` values (`Left`, `Right`, `Center`, `Justify`, `Start`, `End`, `Unspecified`) | ALIGNED | Implemented in `paragraph.rs`. |
| `TextDirection` values (`Ltr`, `Rtl`, `Content`, `ContentOrLtr`, `ContentOrRtl`, `Unspecified`) | ALIGNED | Implemented with resolver helper and content heuristic. |
| `SpanStyle` structure | PARTIAL | Core and stable fields are modeled, including foreground variants (`color` / `brush` + `alpha`), platform style, and draw style. In `wgpu`, plain-text non-solid fill and stroke now share one GPU text-material path (glyph mask + runtime shader), with no style-driven software image fallback. `pixels` remains software-rendered. |
| `ParagraphStyle` structure | ALIGNED | Core and stable fields are modeled, including platform paragraph style. |
| `TextStyle` combining span + paragraph | PARTIAL | `TextStyle { span_style, paragraph_style }` plus merge/plus/from/to/platform-style helper APIs are implemented. Full Compose constructor/copy/saver overload surface is not fully mirrored. |
| `TextStyle` API shape parity (no flattened fields) | ALIGNED | Cranpose now only exposes `TextStyle { span_style, paragraph_style }` for Compose-like API structure. |

## Behavior Parity Status

| Behavior | Status | Notes |
|---|---|---|
| Direction-aware `Start`/`End` alignment in layout pipeline | ALIGNED | Implemented in both pixels and wgpu pipelines. |
| Style-aware text measurement caching | ALIGNED | Cache keys include style hash (not only text/font size). |
| Overflow handling (`Clip`, `Ellipsis`, `StartEllipsis`, `MiddleEllipsis`, `Visible`) | PARTIAL | Implemented in fallback measurer/path and now width-clamped to content bounds in render pipelines; parity with platform line-breaking engines is not exact. |
| Multiline cursor/offset mapping | PARTIAL | Fixed in pixels measurer for Y-based line selection; broader shaping-cluster parity still pending. |
| Font fallback/resolver behavior | GAP | Compose resolver and fallback chain behavior not fully replicated. |
| Glyph shaping and bidi parity | GAP | Current backend behavior is good but not yet a strict equivalent of Compose/Skia text shaping behavior in all scripts. |
| `lineHeightStyle` exact trim/alignment mode semantics | PARTIAL | Modeled and carried through style; full rendering semantics are not fully enforced yet. |
| `lineBreak`, `hyphens`, `textMotion` rendering impact | ALIGNED | `lineBreak` differentiates `Simple` (greedy) from `Heading`/`Paragraph` (word-balance-aware with different last-line penalties); `Hyphens::Auto` injects correct dictionary-based line splits without mutating source text content across embedded locales; and `textMotion` fractionally offsets sampling natively. Compose-exact platform shaping/hinting behavior remains approximate. |
| `baselineShift`, `textGeometricTransform`, `localeList`, `fontFeatureSettings` rendering impact | PARTIAL | `baselineShift` now affects rendered Y position in both pixels and wgpu pipelines. Other knobs remain partially applied/stored. |
| `TextDecoration` rendering (`Underline`, `LineThrough`) | PARTIAL | Decoration lines now render in both pipelines. Geometry is Compose-like but still approximate versus platform paragraph engines. |
| Non-solid brush foreground behavior | PARTIAL | `wgpu` renders plain-text gradient fill and stroke via one GPU material shader over a glyph mask, with no style-based software fallback path. `pixels` remains software-rendered. |

## Phase-1 Parity Contract Matrix

This matrix tracks the implementation contract and current status for the major text parity gaps.
Expected behavior is derived from Compose sources under `/media/huge/composerepo/`:

- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/SpanStyle.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/ParagraphStyle.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/style/Hyphens.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/style/LineBreak.kt`
- `compose/ui/ui-text/src/commonMain/kotlin/androidx/compose/ui/text/style/TextMotion.kt`
- `compose/ui/ui-text/src/androidMain/kotlin/androidx/compose/ui/text/style/LineBreak.android.kt`
- `compose/ui/ui-text/src/androidMain/kotlin/androidx/compose/ui/text/style/TextMotion.android.kt`
- `compose/ui/ui-text/src/androidMain/kotlin/androidx/compose/ui/text/platform/AndroidTextPaint.android.kt`
- `compose/ui/ui-text/src/androidMain/kotlin/androidx/compose/ui/text/platform/extensions/TextPaintExtensions.android.kt`
- `compose/ui/ui-text/src/androidDeviceTest/kotlin/androidx/compose/ui/text/style/LineBreakTest.kt`
- `compose/ui/ui-text/src/androidDeviceTest/kotlin/androidx/compose/ui/text/style/HyphensTest.kt`

### Brush + DrawStyle

| Case | Compose Contract | Cranpose Current | Target |
|---|---|---|---|
| `brush = SolidColor(c)` with `alpha` | Treated as color with alpha modulation. | Works through color resolution. | Keep behavior. |
| `brush = ShaderBrush` | Shader sampled during glyph paint; not collapsed to a single fallback color. | `pixels` samples brush per glyph in text draw space; `wgpu` uses runtime shader masking/material evaluation for plain-text fill and stroke with no software text fallback route. | Extend the same material model to span-rich text. |
| `drawStyle = Fill` | Fill glyph interior. | Explicit fill path used in both backends. In `wgpu`, plain-text non-solid fill uses GPU runtime shader masking over a glyphon alpha mask. | Preserve semantics and extend to span-rich text material routing. |
| `drawStyle = Stroke(...)` | Outline glyph using stroke parameters (width/cap/join/miter/path). | `wgpu` plain-text stroke is now GPU-rendered through the same glyph-mask + runtime-shader material path as fill; `pixels` keeps software glyph-path stroking. | Add remaining stroke-parameter surface parity beyond width-only API. |
| Multi-paragraph brush continuity | Single brush shader continuity across paragraph segments. | `pixels` keeps brush continuity across wrapped text in one draw; `wgpu` raster fallback preserves continuity within each emitted text draw. | Match Compose continuity details across all paragraph/shaping paths. |
| Bidi with brush/stroke | Brush/stroke applies consistently to visual glyph order. | Brush/stroke now applies in both backends for emitted glyph runs; full Compose bidi/shaping parity remains broader work. | Equivalent semantics in `pixels` and `wgpu` for mixed-direction text. |

### Shadow Blur (`shadow.blur_radius`)

| Case | Compose Contract | Cranpose Current | Target |
|---|---|---|---|
| `blur_radius = 0` | Visible hard shadow (Android maps to tiny non-zero radius for paint semantics). | Both backends render hard shadow. | Keep behavior. |
| `blur_radius > 0` | Soft shadow from blurred glyph alpha mask; larger radius increases softness/spread. | Shared raster blurs the effective glyph mask (fill vs stroke) using Skia-style radius→sigma mapping, and GPU `wgpu` path now injects local, high-speed bounded `ShadowDraw` items clipping blurs solely around active text bounds rather than heavy `EffectLayer` screen clears. | Expand local bounds optimizations to background images and shapes. |
| Ordering with fill/decorations | Shadow composited with text paint semantics, then visible text/decorations. | Shared raster composes shadow first from the same glyph mask, then paints text fill/stroke. Pipeline draw ordering remains shadow, then decorations, then main text draw. | Verify decoration ordering against Compose paragraph engine edge cases. |

### Paragraph: `lineBreak`, `hyphens`, `textMotion`

| Case | Compose Contract | Cranpose Current | Target |
|---|---|---|---|
| Unspecified defaults | Resolve to `LineBreak.Simple`, `Hyphens.None`, `TextMotion.Static`. | Defaults now resolved before layout/paint decisions. | Keep behavior. |
| `LineBreak.Simple` vs `Heading` vs `Paragraph` | Distinct break strategies; Compose tests show distinct wrap points for same text/width. | Fallback measurer now uses greedy wrapping for `Simple` and word-balance-aware wrapping for `Heading`/`Paragraph` with different cost weighting, producing Compose-inspired distinct breaks in regression tests. | Extend strategy fidelity for CJK strictness/word-style details. |
| `Hyphens.None` vs `Hyphens.Auto` | Different wrap opportunities for long words; Compose tests show distinct line splits. | `Hyphens::Auto` now dynamically matches standard dictionary mappings (`fr-FR`, `de-DE`, `en-US`, etc.) from embedded data arrays, deferring to trailing-balance heuristics only on unmatched languages. Source text content is not mutated. | Keep matching dictionary rules to specialized platform dictionary revisions. |
| `TextMotion.Static` vs `Animated` | Static favors readability/hinting; Animated enables linear/subpixel text behavior. | `Static` keeps pixel-snapped placement; `Animated` keeps fractional placement. Shared raster shadow sampling now also respects this distinction (fractional vs quantized). | Extend text-motion parity to full hinting/linearity controls in GPU text paths. |

### Compose Edge Cases To Lock In Tests

- `LineBreak` presets map to Android triples:
  - `Simple = Strategy.Simple + Strictness.Normal + WordBreak.Default`
  - `Heading = Strategy.Balanced + Strictness.Loose + WordBreak.Phrase`
  - `Paragraph = Strategy.HighQuality + Strictness.Strict + WordBreak.Default`
- `Hyphens.Auto` and `Hyphens.None` must diverge on long tokens in constrained width.
- `shadow.blur_radius = 0` must still render shadow (not equivalent to no shadow).
- Brush rendering must not regress to fallback-color output for non-solid brushes.
- Constrained paragraph layout with finite `max_lines` must preserve wrap points and must not expand measurement width to parent max width.

## 2026-02-22 Handoff Snapshot

This section is a context handoff for the next implementation chat.

### What changed in this handoff

- `wgpu` stroke and gradient+stroke no longer route through software-rasterized text images.
- Plain-text non-solid fill and plain-text stroke now share one GPU text-material path: glyph mask submission + runtime shader material evaluation.
- `push_text_style_draws(...)` no longer emits `scene.push_image(...)` for text style routing in `wgpu`.
- Runtime stroke rendering is packed through shader material uniforms and evaluated from the glyph alpha mask in GPU space.

### Current `wgpu` routing contract

| Case | Route | Current quality/perf implications |
|---|---|---|
| Solid fill (`color`, no stroke) | Glyphon text draw | Fast and stable; no software fallback. |
| Non-solid fill (`brush`, `drawStyle = Fill`, plain text with no inline spans) | Glyphon text mask + `RenderEffect::runtime_shader` text material pass | GPU path with gradient evaluation in shader. |
| Stroke (`drawStyle = Stroke { width > 0 }`), solid or gradient, plain text | Glyphon text mask + `RenderEffect::runtime_shader` text material pass | GPU path; no software image fallback. |
| Styled runs/inline span text | Glyphon text draw (span-aware attrs) | No software text fallback route; brush/drawStyle parity across spans remains open work. |

### Key code anchors (current branch)

- `crates/cranpose-render/wgpu/src/pipeline.rs`
- `gpu_text_material_for_style(...)`: builds one material contract (brush, alpha, draw mode, stroke width) for GPU text effects.
- `build_gpu_text_effect(...)`: packs brush + draw-mode uniforms for runtime shader execution.
- `GPU_TEXT_BRUSH_EFFECT_SHADER`: now evaluates fill and stroke from one shader contract.
- `push_text_style_draws(...)`: text style routing now chooses glyph draw or glyph-mask+effect, never software image fallback.
- `crates/cranpose-render/wgpu/src/lib.rs`: `text_raster` module is test-only; runtime renderer does not initialize raster fallback fonts.
- `crates/cranpose-render/pixels/src/draw.rs`: software raster blit sizing fix remains in place for pixels backend.

### Known open gaps

- Runtime shader path still caps gradient stops at `GPU_TEXT_BRUSH_EFFECT_MAX_STOPS` (`16`).
- Rich span runs are not yet unified under one span-material shader contract (global plain-text material path is complete).
- `TextDrawStyle` API still exposes width-only stroke controls (cap/join/miter/path parity is pending).
- `push_text_decorations(...)` in `wgpu` still uses single-line approximation instead of measured visual line boxes.

### Route invariants now locked by tests

- `push_text_style_draws_stroke_contract_uses_gpu_shader_mask`
- `push_text_style_draws_gradient_stroke_contract_uses_gpu_shader_mask`
- Stroke and gradient+stroke text do not emit `scene.push_image(...)`.

### Decoration parity contract (explicit remaining gap)

Current state:

- `push_text_decorations(...)` computes decoration geometry by linear chunk widths.
- The current implementation is marked in code as a simple/single-line approximation.

Required end-state:

- Decoration segments are generated from measured line boxes (`prepare_text_layout(...)` output), not from linearized offsets.
- Underline and line-through honor wrapped lines, alignment (`Start`/`End`), and span boundaries in the same visual order as text draws.
- Decoration brush resolution matches the span foreground contract (`color`/`brush`/`alpha`) with no style-dependent fallback route.

Tests to add/update:

- Wrapped multiline underline with mixed span styles validates one decoration segment per visual line.
- Wrapped multiline line-through with bidi text validates visual ordering consistency.
- Baseline shift + decoration test verifies decoration Y placement remains tied to shifted line metrics.

### Validation snapshot for this branch (latest run)

- `cargo fmt` passed.
- `cargo clippy -p cranpose-render-wgpu --tests -- -D warnings` passed.
- `cargo test -p cranpose-render-wgpu` passed (`70` tests).
- `apps/desktop-demo/build-web.sh` passed.
- `(cd apps/android-demo/android && ./gradlew :app:assembleRelease)` passed.
- `./run_robot_test.sh` passed (`77`/`77`).

### Branch working set at snapshot time

- `crates/cranpose-render/wgpu/src/pipeline.rs`: GPU text-material stroke support in runtime shader; software text image routing removed from live path.
- `crates/cranpose-render/wgpu/src/lib.rs`: software text raster module gated to tests only for `wgpu`.

## Demo Coverage

Desktop demo tab added:

- `apps/desktop-demo/src/app/text_showcase.rs`

It exercises:

- Span styling: color, size, weight, style, family, synthesis, spacing, decoration, shadow, baseline shift, geometric transform, locale list, feature settings.
- Span foreground/style extras: brush + alpha + draw_style + platform span style.
- Paragraph styling: align, direction, line height, indent, line-height style, line break, hyphenation, text motion.
- TextStyle composition helpers: `from_span_style`, `from_paragraph_style`, `merge`, `plus`, `with_platform_style`, `to_span_style`, `to_paragraph_style`.
- Overflow modes via `BasicText`.

## Remaining Work to Reach Strict 1:1

- Add Compose-like font resolver/fallback model (file-backed families and loaded typeface paths).
- Add rich text primitives equivalent to `AnnotatedString` span/paragraph runs and paint behavior.
- Tighten shaping/bidi parity across scripts and punctuation according to Unicode algorithm behavior.
- Tighten renderer-side behavior parity for `lineHeightStyle`, text motion raster quality, geometric transform, locale, and feature settings.
- Add targeted cross-backend visual regression tests for remaining parity gaps.

## Verification Commands

```bash
cargo fmt
cargo test > 1.tmp 2>&1
cargo clippy > 2.tmp 2>&1
cargo tree --duplicates

# platform checks
(cd apps/android-demo/android && ./gradlew :app:assembleRelease)
apps/desktop-demo/build-web.sh
./run_robot_test.sh
```
