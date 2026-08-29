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
