# Text Parity Tracker

Last Updated: 2026-02-19

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
| `SpanStyle` structure | PARTIAL | Core and stable fields are modeled, including foreground variants (`color` / `brush` + `alpha`), platform style, and draw style. Both `pixels` and `wgpu` apply non-solid brush and stroke draw-style text rendering through a shared software glyph raster path (`wgpu` uses renderer-configured fonts and style-aware face selection). |
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
| `lineBreak`, `hyphens`, `textMotion` rendering impact | PARTIAL | `lineBreak`/`hyphens` are resolved and applied in fallback layout, and `textMotion` affects rendering/placement. Renderer width resolution now keeps constrained wrap behavior when `max_lines` is finite (instead of over-expanding to parent measurement width). Compose-exact mode behavior is still approximate. |
| `baselineShift`, `textGeometricTransform`, `localeList`, `fontFeatureSettings` rendering impact | PARTIAL | `baselineShift` now affects rendered Y position in both pixels and wgpu pipelines. Other knobs remain partially applied/stored. |
| `TextDecoration` rendering (`Underline`, `LineThrough`) | PARTIAL | Decoration lines now render in both pipelines. Geometry is Compose-like but still approximate versus platform paragraph engines. |
| Non-solid brush foreground behavior | PARTIAL | Both backends render non-solid brush text through shared software glyph rasterization with per-glyph gradient sampling. `wgpu` no longer falls back to first-stop/single-color for these cases. |

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
| `brush = ShaderBrush` | Shader sampled during glyph paint; not collapsed to a single fallback color. | `pixels` samples brush per glyph in text draw space; `wgpu` rasterizes glyphs with brush sampling and draws the result as an image, using renderer-configured runtime fonts (no hardcoded embedded runtime font). | Tighten quality/perf to match direct GPU glyph-shader behavior. |
| `drawStyle = Fill` | Fill glyph interior. | Explicit fill path used in both backends (`wgpu` uses glyphon for simple solid-fill and raster fallback for non-solid/stroke cases). | Preserve fill semantics. |
| `drawStyle = Stroke(...)` | Outline glyph using stroke parameters (width/cap/join/miter/path). | `pixels` supports mask-based stroke width; `wgpu` now supports mask-based stroke via rasterized glyph fallback. | Improve stroke geometry fidelity vs Compose/Skia. |
| Multi-paragraph brush continuity | Single brush shader continuity across paragraph segments. | `pixels` keeps brush continuity across wrapped text in one draw; `wgpu` raster fallback preserves continuity within each emitted text draw. | Match Compose continuity details across all paragraph/shaping paths. |
| Bidi with brush/stroke | Brush/stroke applies consistently to visual glyph order. | Brush/stroke now applies in both backends for emitted glyph runs; full Compose bidi/shaping parity remains broader work. | Equivalent semantics in `pixels` and `wgpu` for mixed-direction text. |

### Shadow Blur (`shadow.blur_radius`)

| Case | Compose Contract | Cranpose Current | Target |
|---|---|---|---|
| `blur_radius = 0` | Visible hard shadow (Android maps to tiny non-zero radius for paint semantics). | Both backends render hard shadow. | Keep behavior. |
| `blur_radius > 0` | Soft shadow from blurred glyph alpha mask; larger radius increases softness/spread. | `pixels` blurs glyph alpha masks; `wgpu` blurs isolated shadow text pass via effect layer. | Tune quality and spread to match Compose more closely. |
| Ordering with fill/decorations | Shadow composited with text paint semantics, then visible text/decorations. | Shadow is emitted as its own pass before decorations and main text in both pipelines; still approximate vs Compose paint order. | Compose-like order/opacity relative to fill/decorations. |

### Paragraph: `lineBreak`, `hyphens`, `textMotion`

| Case | Compose Contract | Cranpose Current | Target |
|---|---|---|---|
| Unspecified defaults | Resolve to `LineBreak.Simple`, `Hyphens.None`, `TextMotion.Static`. | Defaults now resolved before layout/paint decisions. | Keep behavior. |
| `LineBreak.Simple` vs `Heading` vs `Paragraph` | Distinct break strategies; Compose tests show distinct wrap points for same text/width. | Fallback measurer now applies mode-specific wrap strategy. | Tighten strategy details to Compose behavior. |
| `Hyphens.None` vs `Hyphens.Auto` | Different wrap opportunities for long words; Compose tests show distinct line splits. | `Hyphens::Auto` now inserts hyphenated breaks for long tokens; `None` keeps non-hyphenated splits. | Improve parity with platform hyphenation quality. |
| `TextMotion.Static` vs `Animated` | Static favors readability/hinting; Animated enables linear/subpixel text behavior. | `pixels` snaps glyph positions for `Static`; `wgpu` snaps text placement for `Static` and keeps fractional placement for `Animated`. | Align both backend implementations more closely with Compose raster behavior. |

### Compose Edge Cases To Lock In Tests

- `LineBreak` presets map to Android triples:
  - `Simple = Strategy.Simple + Strictness.Normal + WordBreak.Default`
  - `Heading = Strategy.Balanced + Strictness.Loose + WordBreak.Phrase`
  - `Paragraph = Strategy.HighQuality + Strictness.Strict + WordBreak.Default`
- `Hyphens.Auto` and `Hyphens.None` must diverge on long tokens in constrained width.
- `shadow.blur_radius = 0` must still render shadow (not equivalent to no shadow).
- Brush rendering must not regress to fallback-color output for non-solid brushes.
- Constrained paragraph layout with finite `max_lines` must preserve wrap points and must not expand measurement width to parent max width.

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
- Improve `wgpu` brush/stroke raster fallback quality/perf and align it with Compose/Skia glyph rendering behavior.
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
