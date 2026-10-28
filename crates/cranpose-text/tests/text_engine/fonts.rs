// The repository's fonts, read for tests.

use std::ops::Range;

use cranpose_text::{FontFamily, FontRequest, FontStyle, FontWeight, ShapingSpan, Typeface};

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
    }
}
