# Text support and validation

Source audit: 2026-10-02, starting from `59ac28c6b` (v0.1.177 consumer update).
This document describes the implemented Cranpose surface and named behavioral
evidence. It does not claim complete Compose/Skia parity.

## Implemented surface

| Area | Current implementation | Source and behavioral evidence |
| --- | --- | --- |
| Authored styles | `TextStyle` contains `SpanStyle` and `ParagraphStyle`; font, brush, decoration, direction, line-height and overflow options are represented. | [Style types](../crates/cranpose-ui/src/text/style.rs), [paragraph types](../crates/cranpose-ui/src/text/paragraph.rs). |
| Rich text | `AnnotatedString` has span and paragraph ranges, string annotations and link annotations. Its builder composes these ranges. | [AnnotatedString](../crates/cranpose-ui/src/text/annotated_string.rs), [builder and link tests](../crates/cranpose-ui/src/text/tests/annotated_string_tests.rs). |
| Links | `LinkedText` dispatches URLs and custom clickable annotations and publishes accessible link actions. | [Widget](../crates/cranpose-ui/src/widgets/linked_text.rs), [composition integration test](../crates/cranpose-ui/tests/widget_composition.rs). |
| Fonts and shaping | The shared text backend resolves fonts and offers basic/advanced shaping. Font coverage and platform behavior remain application validation concerns. | [Shared text backend](../crates/cranpose-render/common/src/software_text_raster.rs), [font types](../crates/cranpose-ui/src/text/font.rs). |
| Mixed styled lines | Line height, baselines, cursor positions, explicit line height and edge trimming share the resolved styled-line geometry. | [Annotated text integration tests](../crates/cranpose-render/common/tests/annotated_text_baselines.rs). |
| Input | `TextFieldState`, `BasicTextField` and its options form support editing through the platform input integration. | [Input architecture](TEXT_INPUT_ARCHITECTURE.md), [native validation](accessibility_screen_identity_validation.md). |
| Draw-only text | `DrawTextStyle` is the resolved graphics primitive, separate from authored `TextStyle`. | [Graphics typography](../crates/cranpose-ui-graphics/src/typography.rs). |

Rich text is not remaining implementation work. The previous May tracker said
to add an AnnotatedString equivalent even though the current implementation and
its consumers already provide it.

## Recent observable corrections

[PR #1045](https://github.com/samoylenkodmitry/Cranpose/pull/1045), present in the
audited tree as `f6c0cfdcf`, aligns annotated text and preserves viewport rendering.
The integration suite covers:

- Differently sized spans sharing the visible baseline in bitmap and atlas output.
- Multiline heights, baselines and cursors following each line's styled fonts.
- Explicit line height and edge trimming matching painted baselines.
- Wrapped lines and retained span updates reporting the drawn height.
- Cropped shadowed strokes preserving the full paragraph's visible pixels.
- Custom measurers retaining their span-aware line height.

These are named Cranpose behavior contracts. They do not establish identical
output for every script, font, line-breaking engine or platform.

The September physical-iPhone evidence includes activating a field through
VoiceOver, entering text, saving and reopening it, plus retaining an active
editor while a lazy list scrolls. See
[screen identity and text input validation](accessibility_screen_identity_validation.md).
That report names its source revisions, device and scope.

## Validation for 0.9 and 1.0

Before claiming a supported text behavior, retain a regression or device result
for it. The remaining acceptance matrix should include:

- Complex scripts, mixed-direction punctuation, emoji and combining sequences.
- Font fallback and missing-glyph behavior with the actual bundled/system fonts.
- Wrapped rich text, overflow, cursor mapping and selection at style boundaries.
- IME composition, clipboard, password protection and multiline editing.
- Font scale, display scale and resizing across supported targets.
- Reader/browser combinations from the [accessibility protocol](accessibility_validation.md).

An unverified cell means evidence is missing; it does not mean the feature is
absent. Old branch-specific test totals and commands are not current validation.

## Reproduce

Run the relevant public-behavior suites through the repository's pinned
toolchain and host policy:

```bash
cargo test --profile ci -p cranpose-render-common --test integration annotated_text_baselines
cargo test --profile ci -p cranpose-ui --test integration widget_composition
just test-reader-actions
```

The [Text demo](../apps/desktop-demo/src/app/text_showcase.rs) exercises the
style surface, and the Documentation and Markdown tabs exercise styled runs,
links and code blocks. Full release validation also needs the shipped web,
Android and iOS configurations and the render robot suites.
