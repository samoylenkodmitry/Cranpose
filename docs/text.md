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
| `SpanStyle` structure | PARTIAL | Core and stable fields are modeled, including foreground variants (`color` / `brush` + `alpha`), platform style, and draw style. `wgpu` now uses a hybrid route: GPU mask + runtime shader for plain-text gradient fill, software glyph raster for stroke and gradient+stroke; `pixels` still uses software glyph raster for styled text. |
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
| Non-solid brush foreground behavior | PARTIAL | `wgpu` renders plain-text gradient fill via glyph mask + runtime shader, and routes stroke (with or without gradient) through software glyph rasterization. `pixels` remains software-rendered. `wgpu` no longer collapses fill gradients to first-stop/single-color. |

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
| `brush = ShaderBrush` | Shader sampled during glyph paint; not collapsed to a single fallback color. | `pixels` samples brush per glyph in text draw space; `wgpu` uses runtime shader masking for plain-text non-solid fill and software glyph raster for stroke or unsupported style combinations. | Move to a single GPU text-material path for both fill and stroke. |
| `drawStyle = Fill` | Fill glyph interior. | Explicit fill path used in both backends. In `wgpu`, non-solid plain-text fill now uses GPU runtime shader masking over a glyphon alpha mask. | Preserve semantics and extend to span-rich text without software fallback. |
| `drawStyle = Stroke(...)` | Outline glyph using stroke parameters (width/cap/join/miter/path). | Shared software raster stroke now uses glyph-path stroking with Compose-like defaults (`Butt` cap, `Miter` join, miter limit `4`) instead of pure mask dilation; both `pixels` and `wgpu` consume this path. | Add remaining stroke-parameter surface parity beyond width-only API. |
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

### Why text quality still feels inconsistent

- `wgpu` text is currently a hybrid pipeline with two execution paths.
- Fill text can go through glyphon/GPU masking.
- Stroke text still routes through software glyph rasterization.
- Different AA/hinting/rasterization stacks produce visibly different edges and perceived sharpness.

### Current `wgpu` routing contract

| Case | Route | Current quality/perf implications |
|---|---|---|
| Solid fill (`color`, no stroke) | Glyphon text draw | Fast and stable. |
| Non-solid fill (`brush`, `drawStyle = Fill`, plain text with no inline spans) | Glyphon text mask + `RenderEffect::runtime_shader` gradient pass | GPU path, avoids previous first-stop color collapse bug. |
| Stroke (`drawStyle = Stroke { width > 0 }`), solid or gradient | Software rasterized text image | Functional, but stroke edge quality differs from fill path. |
| Styled runs/inline span text with non-solid brush | Still falls back to software raster path in current architecture | Behavior works but does not share GPU fill quality path yet. |

### Key code anchors (current branch)

- `crates/cranpose-render/wgpu/src/pipeline.rs`
- `gpu_text_effect_for_style(...)`: decides when gradient text can use GPU mask + shader.
- `build_gpu_text_effect(...)`: packs gradient uniforms and creates runtime shader effect.
- `resolve_gradient_component(...)`: resolves infinite gradient endpoints (`+inf/-inf`) to local text bounds.
- `push_text_style_draws(...)`: route split between glyphon text, effect layer, and software text image.
- `crates/cranpose-render/wgpu/src/text_raster.rs`
- `requires_rasterized_glyph_path(...)`: currently true for non-solid brush and stroke.
- `crates/cranpose-render/common/src/software_text_raster.rs`
- `align_glyph_for_text_motion(...)`: static text motion snaps glyph placement for software raster path.
- `crates/cranpose-render/pixels/src/draw.rs`
- Raster blit uses image dimensions from raster output rect to avoid clipping.

### Fixes already landed on this branch

- Fill gradient in `wgpu` no longer collapses to first color when using default linear gradient endpoints.
- Gradient+stroke no longer incorrectly routes into GPU fill-effect path that ignores stroke width.
- Static text-motion snapping and raster blit rect sizing were tightened to reduce clipping artifacts.
- Demo text showcase now highlights fill gradient path instead of stroke example.

### Known open gaps

- The hybrid path remains the core architecture problem: same API, different renderer internals.
- Stroke quality depends on software raster rules and does not match GPU fill path crispness.
- GPU gradient path is currently scoped to plain text draws; rich span runs still need unified handling.
- Runtime shader path currently supports up to 16 gradient stops (`GPU_TEXT_BRUSH_EFFECT_MAX_STOPS`).
- `TextDrawStyle` API currently exposes only stroke width; cap/join/miter/path effect parity is incomplete.

### Direction to make this production-grade

- Remove text software fallback from `wgpu` runtime rendering path.
- Move to one GPU text material pipeline where fill, stroke, gradient, and alpha are handled in one shader contract.
- Keep one shaping source (`cosmic-text`) and one glyph cache/atlas strategy for all text draws.
- Keep behavior parity tests as the contract gate during migration.

### Suggested migration plan (no half-migrated state)

1. Define a single `GpuTextMaterial` contract for all text draws:
   - Encodes fill/stroke/brush/alpha, text motion, and shadow inputs.
   - Replace route checks in `push_text_style_draws(...)` with one material builder.
2. Replace `requires_rasterized_glyph_path(...)` routing with GPU path support for stroke:
   - Implement stroke in shader using distance-to-edge data (SDF/MSDF or equivalent).
   - Keep gradient evaluation in the same shader path.
3. Extend from plain text to full span-run text:
   - Preserve brush continuity across runs and bidi ordering.
   - Keep one draw model and one AA strategy.
4. Delete software text image fallback path for `wgpu`:
   - Remove text-driven `scene.push_image(...)` calls in `wgpu` pipeline.
   - Keep software raster only for explicit bitmap/image features, not text paint semantics.
5. Lock final acceptance with tests:
   - Unit tests for route invariants and uniform packing.
   - Robot visual tests for fill vs stroke sharpness, gradient continuity, baseline shift, clipping.

### Minimum acceptance criteria for completion

- `wgpu` text rendering has one execution pipeline for fill + stroke + gradient.
- No runtime routing from text style to software image fallback in `wgpu`.
- Fill/stroke visual quality is consistent under the same font size and scale.
- Existing text parity tests and robot tests pass with zero warnings in clippy/test commands.

### Validation snapshot for this branch (latest run)

- `cargo fmt` passed.
- `cargo clippy -p cranpose-render-wgpu --tests -- -D warnings` passed.
- `cargo test -p cranpose-render-wgpu` passed (`69` tests).

### Branch working set at snapshot time

- `apps/desktop-demo/src/app/text_showcase.rs`: showcase text switched to fill-gradient sample.
- `crates/cranpose-render/common/src/software_text_raster.rs`: static text-motion glyph snapping in software raster path.
- `crates/cranpose-render/pixels/src/draw.rs`: raster text blit rect uses actual image dimensions to avoid clipping.
- `crates/cranpose-render/wgpu/src/pipeline.rs`: GPU gradient mask shader path, route guards for stroke, and gradient endpoint fixes.

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
