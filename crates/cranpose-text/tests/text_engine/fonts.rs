// The repository's fonts, read for tests.

use std::ops::Range;

use cranpose_text::{
    FontFamily, FontFamilyResolver, FontRequest, FontStyle, FontWeight, LayoutInput,
    ParagraphLengths, ParagraphStyle, ShapingSpan, TextLayout, TextLayoutOptions, TextLayouter,
    Typeface,
};

pub(crate) const ROBOTO: &str = "benchmarks/compose-vs-cranpose/fonts/Roboto-Regular.ttf";
pub(crate) const ROBOTO_MEDIUM: &str = "benchmarks/compose-vs-cranpose/fonts/Roboto-Medium.ttf";
pub(crate) const ROBOTO_BOLD: &str = "benchmarks/compose-vs-cranpose/fonts/Roboto-Bold.ttf";
pub(crate) const DEJAVU: &str = "apps/desktop-demo/assets/leetcodedaily/fonts/DejaVuSans.ttf";
pub(crate) const DEJAVU_MONO: &str =
    "apps/desktop-demo/assets/leetcodedaily/fonts/DejaVuSansMono.ttf";
pub(crate) const TWEMOJI: &str = "apps/desktop-demo/assets/TwemojiMozilla.ttf";
pub(crate) const NOTO_SANS_LATIN: &str = "apps/esp32-demo/assets/NotoSans-Latin.ttf";
pub(crate) const NOTO_SANS_MERGED: &str = "apps/desktop-demo/assets/NotoSansMerged.ttf";
pub(crate) const ARABIC_VF: &str = "crates/cranpose-fonts/assets/NotoSansArabic-VF.ttf";
pub(crate) const DEVANAGARI_VF: &str = "crates/cranpose-fonts/assets/NotoSansDevanagari-VF.ttf";
pub(crate) const DEVANAGARI: &str =
    "crates/cranpose-render/common/tests/fixtures/localization/Devanagari.ttf";

// The bytes of the font at `path`, relative to the repository root.
pub(crate) fn bytes(path: &str) -> Vec<u8> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    std::fs::read(format!("{root}/{path}")).unwrap_or_else(|error| panic!("{path}: {error}"))
}

// The font at `path` read with no declared family.
pub(crate) fn typeface(path: &str) -> Typeface {
    Typeface::from_bytes(bytes(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

// A span of `range` at 16 pixels in `family` at `weight`, with no features or
// language.
pub(crate) fn span(range: Range<usize>, family: &FontFamily, weight: u16) -> ShapingSpan<'_> {
    ShapingSpan {
        range,
        font: FontRequest {
            family,
            weight: FontWeight::new(weight),
            style: FontStyle::Normal,
        },
        size: 16.0,
        features: &[],
        language: None,
        letter_spacing: None,
        baseline_shift: 0.0,
    }
}

// Lays `text` out in one span of the default family at 400, with
// `paragraph`, `width` and `options`.
pub(crate) fn lay_out(
    resolver: &FontFamilyResolver,
    text: &str,
    paragraph: &ParagraphStyle,
    width: f32,
    options: TextLayoutOptions,
) -> TextLayout {
    lay_out_with(
        resolver,
        text,
        (paragraph, ParagraphLengths::default()),
        width,
        options,
    )
}

// Lays `text` out in one span of the default family at 400, with
// `paragraph`, `lengths` and `options`, exactly `width` wide when it is
// finite, as text filling a fixed width is.
pub(crate) fn lay_out_with(
    resolver: &FontFamilyResolver,
    text: &str,
    (paragraph, lengths): (&ParagraphStyle, ParagraphLengths),
    width: f32,
    options: TextLayoutOptions,
) -> TextLayout {
    let family = FontFamily::Default;
    let spans = [span(0..text.len(), &family, 400)];
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph,
            lengths,
            min_width: if width.is_finite() { width } else { 0.0 },
            max_width: width,
            options,
        },
        resolver,
    )
}
