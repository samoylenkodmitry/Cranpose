//! Pair kerning, through the production measurer rather than the reader.
//!
//! The reader has its own unit tests; what these pin is that the value reaches
//! a measured width. It used not to: `ab_glyph::Font::kern_unscaled` reads the
//! TrueType `kern` table, the embedded fallback carries none, and neither does
//! the Roboto a Wear device ships — so every string measured wide.

#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::{
    gpos_kerning::GposKerning,
    software_text_raster::{
        default_software_text_font, SoftwareTextFontSet, SoftwareTextMeasurer,
        DEFAULT_SOFTWARE_TEXT_FONT_BYTES,
    },
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, SpanStyle, TextMeasurer, TextStyle, TextUnit,
};

const FONT_SIZE: f32 = 32.0;

fn measurer() -> SoftwareTextMeasurer {
    SoftwareTextMeasurer::from_font_set(
        SoftwareTextFontSet::from_font(default_software_text_font().expect("embedded fallback")),
        256,
    )
}

fn style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            font_family: Some(FontFamily::SansSerif),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn width(measurer: &SoftwareTextMeasurer, text: &str) -> f32 {
    measurer
        .measure(&AnnotatedString::from(text), &style())
        .width
}

/// The kerning the font declares for a pair, in pixels at `FONT_SIZE`.
fn expected_kern_px(pair: [char; 2]) -> f32 {
    let face = ttf_parser::Face::parse(DEFAULT_SOFTWARE_TEXT_FONT_BYTES, 0).expect("face");
    let kerning = GposKerning::parse(&face).expect("the fallback font has GPOS kerning");
    let first = face.glyph_index(pair[0]).expect("glyph").0;
    let second = face.glyph_index(pair[1]).expect("glyph").0;
    kerning.kern_unscaled(first, second) * FONT_SIZE / f32::from(face.units_per_em())
}

#[test]
fn a_kerned_pair_is_narrower_than_its_two_glyphs_by_exactly_the_kern() {
    let measurer = measurer();
    let kern = expected_kern_px(['A', 'V']);
    assert!(kern < 0.0, "AV must kern for this test to mean anything");

    let separate = width(&measurer, "A") + width(&measurer, "V");
    let together = width(&measurer, "AV");

    assert!(
        (together - (separate + kern)).abs() < 0.01,
        "AV measured {together}, expected {} = {separate} + {kern}",
        separate + kern
    );
}

#[test]
fn an_unkerned_pair_is_exactly_its_two_glyphs() {
    let measurer = measurer();
    let separate = width(&measurer, "n") + width(&measurer, "n");
    let together = width(&measurer, "nn");
    assert!(
        (together - separate).abs() < 0.01,
        "nn measured {together}, expected {separate}"
    );
}

/// The regression this whole rule exists for: a string of kerning pairs used to
/// measure as if the font declared none.
#[test]
fn kerning_accumulates_across_a_string() {
    let measurer = measurer();
    let kerned = width(&measurer, "AVAVAV");
    let unkerned: f32 = 3.0 * (width(&measurer, "A") + width(&measurer, "V"));
    let kern = expected_kern_px(['A', 'V']);
    // Five gaps in "AVAVAV": AV, VA, AV, VA, AV.
    assert!(
        kerned < unkerned + 4.0 * kern,
        "a six-glyph run should carry at least five kerns: {kerned} vs {unkerned}"
    );
}
