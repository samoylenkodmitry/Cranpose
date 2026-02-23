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
| `FontFamily` generic values (`Default`, `SansSerif`, `Serif`, `Monospace`, `Cursive`) | PARTIAL | Supported, plus `Fantasy`, `Named(String)`, `FileBacked(Vec<FontFile>)`, and `LoadedTypeface(path)`. Compose resolver/list internals are implemented in `wgpu` as a runtime resolver but API surface is not yet 1:1. |
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
| Font fallback/resolver behavior | PARTIAL | `wgpu` now uses a shared resolver for measurement + render (`TypefaceRequest` cache, named-family fallback to canonical loaded families, file-backed and loaded-typeface path resolution, wasm-safe path handling, generic fallback seeding, and embedded fallback font bootstrap). Full Compose fallback-chain parity remains ongoing. |
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
| `brush = ShaderBrush` | Shader sampled during glyph paint; not collapsed to a single fallback color. | `pixels` samples brush per glyph in text draw space; `wgpu` uses runtime shader masking/material evaluation for plain-text and span-batched fill/stroke with no software text fallback route. | Harden continuity on mixed-bidi and wrapped-line stress cases. |
| `drawStyle = Fill` | Fill glyph interior. | Explicit fill path used in both backends. In `wgpu`, non-solid fill uses GPU runtime shader masking over a glyphon alpha mask for plain text and span material batches. | Preserve semantics and harden stress-case continuity. |
| `drawStyle = Stroke(...)` | Outline glyph using stroke parameters (width/cap/join/miter/path). | `wgpu` plain-text stroke is now GPU-rendered through the same glyph-mask + runtime-shader material path as fill; `pixels` keeps software glyph-path stroking. | Add remaining stroke-parameter surface parity beyond width-only API. |
| Multi-paragraph brush continuity | Single brush shader continuity across paragraph segments. | `pixels` keeps brush continuity across wrapped text in one draw; `wgpu` keeps continuity through same-material batching and glyph-mask effect passes. | Match Compose continuity details across all paragraph/shaping paths, especially mixed bidi wraps. |
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
- Stroke effect layers now expand bounds from shader-packed stroke padding, preventing thick/high-scale stroke clipping while preserving brush-space coordinates.
- `FontFamily` model now includes Compose-like file-backed and loaded-typeface path variants in `cranpose-ui`.
- `wgpu` now runs a shared font resolver across measurement + render paths with request caching, canonical family-name resolution, and generic fallback seeding.
- Missing named families now resolve to GPU fallback families instead of failing family lookup.
- Renderer startup now guarantees at least one loaded face by injecting an embedded Roboto fallback when app settings provide no fonts (critical for wasm/web stability).
- Resolver now also enforces a runtime non-empty font-db guard before family resolution/shaping, injecting embedded fallback if the db is unexpectedly empty.
- Attr resolution now downgrades requested style/weight to an available face when exact style/weight is absent (critical for wasm where no system italic/bold fallback exists).
- Embedded Unicode fallback font (`DejaVu Sans`) is now force-loaded in `wgpu` bootstrap to provide cross-script coverage (Hebrew/arrows and similar glyph sets) when app fonts are script-limited.

### Current `wgpu` routing contract

| Case | Route | Current quality/perf implications |
|---|---|---|
| Solid fill (`color`, no stroke) | Glyphon text draw | Fast and stable; no software fallback. |
| Non-solid fill (`brush`, `drawStyle = Fill`, plain text with no inline spans) | Glyphon text mask + `RenderEffect::runtime_shader` text material pass | GPU path with gradient evaluation in shader. |
| Stroke (`drawStyle = Stroke { width > 0 }`), solid or gradient, plain text | Glyphon text mask + `RenderEffect::runtime_shader` text material pass | GPU path with expanded effect bounds to protect outline edges; no software image fallback. |
| Styled runs/inline span text | GPU material batching for paint overrides + glyph draw for same-material regions | No software text fallback route; decoration geometry now follows measured glyph-run visual boxes. Remaining quality work is stroke hardening and gradient-stop constraints. |

### Key code anchors (current branch)

- `crates/cranpose-render/wgpu/src/pipeline.rs`
- `gpu_text_material_for_style(...)`: builds one material contract (brush, alpha, draw mode, stroke width) for GPU text effects.
- `build_gpu_text_effect(...)`: packs brush + draw-mode uniforms for runtime shader execution.
- `GPU_TEXT_BRUSH_EFFECT_SHADER`: now evaluates fill and stroke from one shader contract.
- `push_text_style_draws(...)`: text style routing now chooses glyph draw or glyph-mask+effect, never software image fallback.
- `crates/cranpose-render/wgpu/src/lib.rs`: no `wgpu` software text raster module remains.
- `crates/cranpose-render/pixels/src/draw.rs`: software raster blit sizing fix remains in place for pixels backend.

### Known open gaps

- Runtime shader path still caps gradient stops at `GPU_TEXT_BRUSH_EFFECT_MAX_STOPS` (`16`).
- Rich span runs now use GPU material batching for paint overrides (`color` / `brush` / `alpha` / `draw_style`) via per-material glyph-mask effect passes. Remaining work is follow-up hardening for mixed-bidi and wrapped-line continuity stress cases.
- `TextDrawStyle` API still exposes width-only stroke controls (cap/join/miter/path parity is pending).
- Stroke quality hardening now includes shader-side stroke padding + expanded effect bounds and static-vs-animated stroke snapping regression coverage; additional manual visual tuning remains follow-up.
- Font resolver currently approximates Compose resolver behavior but is not yet a strict equivalent for every fallback-chain and synthesis edge case.

### Remaining GPU Work Plan

Target:

- Software text raster is runtime-only in `pixels` renderer.
- `wgpu` text uses one GPU text-material execution model for fill/stroke/gradient in runtime paths.

Current checkpoint:

- `wgpu` runtime has no `software_text_raster` references.
- `wgpu` text style routing does not use `scene.push_image(...)`.
- Plain text and span text without paint overrides already use GPU material masking.
- `push_text_decorations(...)` now consumes measured glyph-run visual boxes from layout data, with logical-line fallback only when glyph boxes are unavailable.

Non-negotiable invariants:

- `crates/cranpose-render/wgpu/src` must stay free of runtime software raster hooks.
- Style differences (fill/stroke/gradient/alpha) must be encoded as GPU material state, not renderer path switching.
- Any span case not yet materialized must still stay on GPU glyph draw path and never fallback to software image text.

Workstream 1: span paint material unification

1. DONE: per-span paint material batching for spans that override paint fields (`color`, `brush`, `alpha`, `draw_style`) is in place.
2. DONE: non-paint span attributes (weight/style/family/size/spacing) remain intact in the mask pass model.
3. Remaining: extend coverage for mixed-bidi and wrapped-line continuity edge cases as follow-up hardening.

Done gate:

- Mixed span tests (`solid`, `gradient fill`, `gradient stroke`, mixed bidi) run through GPU material or GPU glyph path only.
- No software image text draws in any `wgpu` text tests.

Workstream 2: decoration rewrite from real line layout

1. DONE: removed single-line-only decoration emission and split geometry per prepared visual line.
2. DONE: underline and line-through now use measured glyph-run visual boxes (not only logical/prepared-line segmentation), including complex bidi visual ordering.
3. DONE: decoration brush/alpha resolution now follows span foreground contract (`color`/`brush`/`alpha`) in the GPU path.
4. DONE: avoid shaping overhead for non-decorated text by short-circuiting decoration layout work when no visible decorations are present.

Done gate:

- New multiline decoration tests pass for mixed spans, bidi visual ordering, and baseline shift.
- Decoration ordering remains stable with shadows and text body draws.

Workstream 3: stroke quality hardening

1. Improve edge behavior for thick strokes and small glyphs under high scale factors.
2. Validate static vs animated text-motion quality remains consistent after stroke refinements.
3. Keep the material shader contract stable for wasm/android precision constraints.

Done gate:

- Regression tests cover stroke width scaling, shader stroke padding packing, expanded effect bounds, gradient+stroke uniform packing, and static-vs-animated stroke motion snapping.
- Automated renderer/platform gates pass after stroke hardening changes; keep manual showcase checks for halo tuning as follow-up.

Workstream 4: release-quality verification gates

1. Keep zero-warning quality gates for renderer crate.
2. Re-run platform viability checks after each major text pipeline change.
3. Keep route-invariant tests as hard blockers.

Done gate:

- `cargo clippy -p cranpose-render-wgpu --tests -- -D warnings` passes.
- `cargo test -p cranpose-render-wgpu` passes.
- `apps/desktop-demo/build-web.sh` passes.
- `(cd apps/android-demo/android && ./gradlew :app:assembleRelease)` passes.
- `./run_robot_test.sh` passes.

Execution order:

1. Workstream 1
2. Workstream 2
3. Workstream 3
4. Workstream 4

### Recommended pre-alpha focus (next cycle)

1. `60%` effort: harden mixed-bidi and wrapped-line continuity edge cases in span material batching.
2. `20%` effort: tighten Compose resolver parity details on top of the implemented resolver architecture (fallback chain nuances, synthesis fidelity, wasm path provisioning strategy).
3. `20%` effort: keep Workstream 4 gates + regression coverage updates as strict blockers after each major change.
4. Defer API-surface expansion (`TextDrawStyle` cap/join/miter/path) until priorities `1` and `2` are stable.
5. Treat gradient-stop cap expansion as lower priority unless blocked by a concrete product/demo requirement.

### New chat bootstrap context (copy/paste)

```text
Continue work on cranpose text GPU unification.

Project: /home/s/develop/projects/compose-rs-proposal
Branch: text
Primary doc: docs/text.md

Current state:
- wgpu runtime has no software text raster module and no software text image fallback routing.
- Plain text + span paint overrides are on GPU material batching (glyph mask + runtime shader effects).
- Decorations are generated from measured glyph-run visual boxes (`layout_text(...)` glyph layouts), with logical-line fallback only if glyph boxes are unavailable.
- Decoration parity gap tracked in docs is resolved on this branch.
- Non-decorated text now short-circuits decoration layout work to avoid shaping overhead.
- Compose-like font resolver/fallback architecture is implemented in `wgpu` and wired into both measurement and render paths.
- Web/wasm startup now includes embedded fallback font bootstrap when no app fonts are provided, preventing missing-font crash paths.
- Resolver path now enforces a non-empty font db before shaping and injects embedded fallback at runtime if needed.
- Style/weight requests are now normalized to available faces before shaping to avoid `cosmic-text` default-font panics on wasm text showcase content.
- `wgpu` now also injects an embedded Unicode fallback family for script coverage so text tabs with Hebrew/symbol content do not degrade to tofu boxes when demo fonts are Roboto-only.

Latest validation snapshot:
- cargo fmt passed
- cargo clippy -p cranpose-render-wgpu --tests -- -D warnings passed
- cargo test -p cranpose-render-wgpu passed (87 tests)
- cargo test -p cranpose-ui text_layout_result -- --nocapture passed
- apps/desktop-demo/build-web.sh passed
- (cd apps/android-demo/android && ./gradlew :app:assembleRelease) passed
- ./run_robot_test.sh --sequential passed (77/77)
- rg -n "software_text_raster|rasterize_text_to_image|requires_rasterized_glyph_path" crates/cranpose-render/wgpu/src -g'*.rs' returned no matches

Hard invariants:
- crates/cranpose-render/wgpu/src must not reference software text raster hooks.
- Style differences (fill/stroke/gradient/alpha) must remain GPU material state, not route switching.
- No `scene.push_image(...)` text fallback in wgpu text style routing.

Next priority (pre-alpha):
1) `60%`: mixed-bidi and wrapped-line continuity hardening for span material batching.
2) `20%`: resolver parity refinements beyond the implemented architecture (Compose fallback-chain details and synthesis behavior).
3) `20%`: regression/gates discipline after each major change.
4) Keep stroke-edge manual QA as follow-up polish, not primary scope for this cycle.

Primary done gates for this cycle:
- Mixed-bidi/wrapped-line stress tests pass in `wgpu` material batching with stable visual ordering/continuity.
- Font resolver/fallback design is implemented (not stubbed), wired into measurement + render paths, and validated on desktop/web/android.
- Full mandatory verification commands pass after each major checkpoint.

Useful anchors:
- crates/cranpose-render/wgpu/src/pipeline.rs
- crates/cranpose-render/wgpu/src/lib.rs
- crates/cranpose-render/pixels/src/draw.rs
- crates/cranpose-ui/src/text_layout_result.rs

Mandatory verification commands:
- cargo fmt
- cargo clippy -p cranpose-render-wgpu --tests -- -D warnings
- cargo test -p cranpose-render-wgpu
- cargo test -p cranpose-ui text_layout_result -- --nocapture
- apps/desktop-demo/build-web.sh
- (cd apps/android-demo/android && ./gradlew :app:assembleRelease)
- ./run_robot_test.sh --sequential
- rg -n "software_text_raster|rasterize_text_to_image|requires_rasterized_glyph_path" crates/cranpose-render/wgpu/src -g'*.rs'
```

### Route invariants now locked by tests

- `push_text_style_draws_stroke_contract_uses_gpu_shader_mask`
- `push_text_style_draws_gradient_stroke_contract_uses_gpu_shader_mask`
- `push_text_style_draws_span_gradient_without_paint_override_uses_gpu_shader_mask`
- `push_text_style_draws_span_gradient_with_paint_override_uses_gpu_shader_mask_batches`
- `push_text_style_draws_adjacent_span_paint_overrides_batch_same_material`
- Stroke and gradient+stroke text do not emit `scene.push_image(...)`.
- `decoration_segments_from_glyph_layouts_line_through_preserves_bidi_visual_order`
- `decoration_segments_from_glyph_layouts_multiline_line_through_keeps_visual_line_boxes`

### Decoration parity contract (resolved on this branch)

Current state:

- `push_text_decorations(...)` now emits decoration segments from measured glyph-run visual boxes (`layout_text(...)` glyph layout), preserving visual ordering and wrapped-line geometry.
- Decoration brush and alpha are resolved from span foreground style and modulated through layer color/alpha.

Required end-state:

- DONE on this branch for the tracked gap: decoration segments are generated from measured glyph-run visual boxes.
- DONE on this branch for the tracked gap: underline/line-through follow wrapped lines and bidi visual ordering.
- DONE on this branch for the tracked gap: brush resolution matches the span foreground contract (`color`/`brush`/`alpha`) with no style-dependent fallback route.

Tests to add/update:

- Wrapped multiline underline with mixed span styles validates one decoration segment per visual line. (DONE via unit tests)
- Wrapped multiline line-through with bidi text validates visual ordering consistency. (DONE via unit tests)
- Baseline shift + decoration test verifies decoration Y placement remains tied to shifted line metrics. (DONE via unit tests)

### Validation snapshot for this branch (latest run)

- `cargo fmt` passed.
- `cargo clippy -p cranpose-render-wgpu --tests -- -D warnings` passed.
- `cargo test -p cranpose-render-wgpu` passed (`87` tests).
- `cargo test -p cranpose-ui text_layout_result -- --nocapture` passed.
- `apps/desktop-demo/build-web.sh` passed.
- `(cd apps/android-demo/android && ./gradlew :app:assembleRelease)` passed.
- `./run_robot_test.sh --sequential` passed (`77`/`77`).
- `rg -n "software_text_raster|rasterize_text_to_image|requires_rasterized_glyph_path" crates/cranpose-render/wgpu/src -g'*.rs'` returned no matches.

### Branch working set at snapshot time

- `crates/cranpose-render/wgpu/src/lib.rs`: shared `WgpuFontFamilyResolver` added and wired through text measurement/render preparation (request cache, canonical family resolution, generic fallback seeding, file-backed and loaded-typeface path loading, embedded fallback bootstrap, runtime non-empty font-db enforcement before shaping, style/weight normalization to available faces, and embedded Unicode fallback-family loading for script coverage).
- `crates/cranpose-render/wgpu/src/render.rs`: `GpuRenderer` now owns resolver handle and uses it for all text buffer `ensure(...)` calls.
- `crates/cranpose-ui/src/text/font.rs`: `FontFile`, `FileBackedFontFamily`, and `LoadedTypefacePath` added; `FontFamily` now models file-backed/typeface-path variants.
- `crates/cranpose-ui/src/text/mod.rs`: new font model types re-exported for app usage.
- `crates/cranpose/src/web.rs`: web startup now logs configured font-blob count (or missing-font warning) for direct wasm diagnostics.

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

- Tighten resolver fallback-chain and synthesis semantics to align with Compose edge cases across all platforms.
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
