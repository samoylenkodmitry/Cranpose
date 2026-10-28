use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, ParagraphLengths, ParagraphStyle, ShapingSpan,
    TextLayoutOptions, TextLayouter, Typeface,
};

use crate::fonts::{NOTO_SANS_MERGED, bytes, span};

// Noto Sans with a `trak` table added whose normal track spaces letters by
// 0.024 em at 10 points.
fn tracked() -> Typeface {
    let font = bytes(NOTO_SANS_MERGED);
    let tracking = bytes("crates/cranpose-text/tests/fixtures/default_tracking.trak");
    let tables = u16::from_be_bytes([font[4], font[5]]);
    let directory_end = 12 + usize::from(tables) * 16;
    let mut tracked = font[..directory_end].to_vec();
    tracked[4..6].copy_from_slice(&(tables + 1).to_be_bytes());
    // Every table moves down by the new record.
    for record in tracked[12..].chunks_exact_mut(16) {
        let offset = u32::from_be_bytes([record[8], record[9], record[10], record[11]]);
        record[8..12].copy_from_slice(&(offset + 16).to_be_bytes());
    }
    let length = |bytes: usize| u32::try_from(bytes).expect("the font is small");
    tracked.extend_from_slice(b"trak");
    tracked.extend_from_slice(&0u32.to_be_bytes());
    tracked.extend_from_slice(&length(font.len() + 16).to_be_bytes());
    tracked.extend_from_slice(&length(tracking.len()).to_be_bytes());
    tracked.extend_from_slice(&font[directory_end..]);
    tracked.extend_from_slice(&tracking);
    Typeface::from_bytes(tracked).expect("the tracked font reads")
}

// The width of "Account" at 10 pixels in `face` with `letter_spacing`.
fn width(face: &Typeface, letter_spacing: Option<f32>) -> f32 {
    const TEXT: &str = "Account";
    let resolver = FontFamilyResolver::new([face.clone()]);
    let family = FontFamily::Default;
    let spans = [ShapingSpan {
        size: 10.0,
        letter_spacing,
        ..span(0..TEXT.len(), &family, 400)
    }];
    let laid = TextLayouter::default().layout(
        &LayoutInput {
            text: TEXT,
            spans: &spans,
            paragraph: &ParagraphStyle::default(),
            lengths: ParagraphLengths::default(),
            min_width: 0.0,
            max_width: f32::INFINITY,
            options: TextLayoutOptions::default(),
        },
        &resolver,
    );
    laid.line_right(0) - laid.line_left(0)
}

#[test]
fn a_font_s_default_tracking_spaces_its_letters_unless_the_style_sets_a_spacing() {
    let plain = Typeface::from_bytes(bytes(NOTO_SANS_MERGED)).expect("the font reads");
    let tracked = tracked();

    assert!((width(&tracked, None) - width(&plain, Some(0.24))).abs() < 0.001);
    assert!(width(&tracked, None) > width(&plain, None));
    for spacing in [0.0, -0.2, 1.0] {
        assert!(
            (width(&tracked, Some(spacing)) - width(&plain, Some(spacing))).abs() < 0.001,
            "{spacing}"
        );
    }
}
