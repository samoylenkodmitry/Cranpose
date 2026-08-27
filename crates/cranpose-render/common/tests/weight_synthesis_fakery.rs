//! When a requested weight is heavier than the face that answered, and the
//! framework may embolden it itself.
//!
//! Android decides this in Minikin's `computeFakery`
//! (`frameworks/minikin/libs/minikin/FontFamily.cpp`):
//!
//! ```text
//! bool isFakeBold = wanted.weight() >= 600 && (wanted.weight() - actual.weight()) >= 200;
//! ```
//!
//! Semibold or darker, *and* at least two grades above the resolved face.
//! Anything short of both draws the resolved face untouched.
//!
//! This matters twice over. Wear Material 3 asks `sans-serif` for body 450 and
//! title 550 against a family `/system/etc/fonts.xml` declares only in
//! hundreds; Android resolves those to 400 and 500 and emboldens neither. And
//! because only the measure paths scale advances by the synthesis, while the
//! glyph layout loops do not, a run that synthesises measures wider than it
//! draws — which walks centred text left by half the difference.

#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::software_text_raster::{
    SoftwareTextFontSet, SoftwareTextMeasurer, default_software_text_font,
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontWeight, SpanStyle, TextMeasurer, TextStyle, TextUnit,
};

const FONT_SIZE: f32 = 32.0;
const SAMPLE: &str = "Designed and built for Wear";

fn measurer() -> SoftwareTextMeasurer {
    SoftwareTextMeasurer::from_font_set(
        SoftwareTextFontSet::from_font(default_software_text_font().expect("embedded fallback")),
        256,
    )
}

fn width(measurer: &SoftwareTextMeasurer, weight: u16) -> f32 {
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            font_family: Some(FontFamily::SansSerif),
            font_weight: Some(FontWeight(weight)),
            ..Default::default()
        },
        ..Default::default()
    };
    measurer
        .measure(&AnnotatedString::from(SAMPLE), &style)
        .width
}

/// The face this measures against, so the test says what it is resting on.
fn resolved_weight() -> FontWeight {
    default_software_text_font()
        .expect("embedded fallback")
        .weight()
}

#[test]
fn a_weight_below_semibold_is_never_synthesized() {
    assert_eq!(
        resolved_weight(),
        FontWeight::NORMAL,
        "the fixture must resolve to a 400 face for these gaps to be the ones under test"
    );
    let measurer = measurer();
    let regular = width(&measurer, 400);

    // The two Wear Material 3 asks for, and the grade between them. None is
    // semibold, so Android draws the resolved face and so must this.
    for weight in [450u16, 500, 550, 599] {
        assert_eq!(
            width(&measurer, weight),
            regular,
            "{weight} against a 400 face must draw the 400 face untouched"
        );
    }
}

#[test]
fn semibold_within_two_grades_of_the_resolved_face_is_not_synthesized_either() {
    let measurer = measurer();
    let regular = width(&measurer, 400);

    // 600 is semibold, but only 200 above 400 at 600 exactly — the boundary is
    // inclusive, so 600 fires and 599 does not.
    assert_eq!(
        width(&measurer, 599),
        regular,
        "599 is below semibold, whatever the gap"
    );
    assert!(
        width(&measurer, 600) > regular,
        "600 is semibold and exactly two grades up, which is where fakery starts"
    );
}

#[test]
fn a_genuine_fake_bold_still_widens_the_run() {
    let measurer = measurer();
    let regular = width(&measurer, 400);
    let bold = width(&measurer, 700);

    assert!(
        bold > regular,
        "700 against a 400 face is Android's fake bold and must still be synthesized: \
         regular={regular} bold={bold}"
    );
}
