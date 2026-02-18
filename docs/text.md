# Text Parity Tracker

Last Updated: 2026-02-17

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
| `SpanStyle` structure | PARTIAL | Core and stable fields are modeled, including foreground variants (`color` / `brush` + `alpha`), platform style, and draw style. Renderer behavior for non-solid brush/draw-style text is still limited. |
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
| `lineBreak`, `hyphens`, `textMotion` rendering impact | PARTIAL | Stored and hashed; backend behavior is not yet feature-complete with Compose. |
| `baselineShift`, `textGeometricTransform`, `localeList`, `fontFeatureSettings` rendering impact | PARTIAL | API available and hashed; backend application is incomplete/limited depending on renderer path. |

## Demo Coverage

Desktop demo tab added:

- `apps/desktop-demo/src/app/text_showcase.rs`

It exercises:

- Span styling: color, size, weight, style, family, synthesis, spacing, decoration, shadow, baseline shift, geometric transform, locale list, feature settings.
- Paragraph styling: align, direction, line height, indent, line-height style, line break, hyphenation, text motion.
- Overflow modes via `BasicText`.

## Remaining Work to Reach Strict 1:1

- Add Compose-like font resolver/fallback model (file-backed families and loaded typeface paths).
- Add rich text primitives equivalent to `AnnotatedString` span/paragraph runs and paint behavior.
- Implement full platform-style and draw-style text surface parity.
- Tighten shaping/bidi parity across scripts and punctuation according to Unicode algorithm behavior.
- Complete renderer-side effect application for all style knobs (`lineHeightStyle`, `lineBreak`, `hyphens`, `textMotion`, geometric transform, locale/feature settings).
- Add targeted robot and integration tests for each missing parity item.

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
