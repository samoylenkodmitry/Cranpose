use cranpose_render_common::software_text_raster::{
    SoftwareTextFont, SoftwareTextMeasurer, cursor_x_for_offset_with_font, measure_text_with_font,
    rasterize_text_to_image,
};
use cranpose_ui::text::{AnnotatedString, SpanStyle, TextMeasurer, TextStyle, TextUnit};
use cranpose_ui_graphics::{Color, Rect, Size};

const FONT: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");
const TRACKING: &[u8] = include_bytes!("fixtures/default_tracking.trak");

fn tracked_font() -> SoftwareTextFont {
    let tables = u16::from_be_bytes(FONT[4..6].try_into().unwrap());
    let directory_end = 12 + usize::from(tables) * 16;
    let mut bytes = FONT[..directory_end].to_vec();
    bytes[4..6].copy_from_slice(&(tables + 1).to_be_bytes());
    for record in bytes[12..].as_chunks_mut::<16>().0 {
        let offset = u32::from_be_bytes(record[8..12].try_into().unwrap());
        record[8..12].copy_from_slice(&(offset + 16).to_be_bytes());
    }
    bytes.extend_from_slice(b"trak");
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&((FONT.len() + 16) as u32).to_be_bytes());
    bytes.extend_from_slice(&(TRACKING.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&FONT[directory_end..]);
    bytes.extend_from_slice(TRACKING);
    SoftwareTextFont::from_bytes(bytes).unwrap()
}

fn style(spacing: TextUnit) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(10.0),
            letter_spacing: spacing,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn default_font_tracking_reaches_measurement_cache_prefixes_and_cursors() {
    let tracked = tracked_font();
    let plain = SoftwareTextFont::from_bytes(FONT).unwrap();
    let automatic = style(TextUnit::Unspecified);
    let expected = style(TextUnit::Sp(0.24));
    let text = "Account";
    let width = measure_text_with_font(text, &expected, 10.0, &plain).width;
    assert!(
        (measure_text_with_font(text, &automatic, 10.0, &tracked).width - width).abs() < 0.0001
    );
    assert!(
        (cursor_x_for_offset_with_font(text, &automatic, text.len(), &tracked) - width).abs()
            < 0.0001
    );
    let measurer = SoftwareTextMeasurer::new(tracked, 8);
    let text = AnnotatedString::from(text);
    for _ in 0..2 {
        assert!((measurer.measure(&text, &automatic).width - width).abs() < 0.0001);
        let prefixes = measurer
            .measure_line_prefix_widths(&text, 0..text.text.len(), &automatic)
            .unwrap();
        assert!((prefixes.width_for_char_range(0, 7).unwrap() - width).abs() < 0.0001);
    }
}

#[test]
fn automatic_tracking_raster_matches_explicit_spacing_and_can_be_overridden() {
    let tracked = tracked_font();
    let plain = SoftwareTextFont::from_bytes(FONT).unwrap();
    let raster = |font: &SoftwareTextFont, style: &TextStyle, scale| {
        rasterize_text_to_image(
            "Account",
            Rect::from_size(Size::new(100.0, 20.0)),
            style,
            Color::BLACK,
            10.0,
            scale,
            font,
        )
        .unwrap()
        .pixels()
        .to_vec()
    };
    for scale in [1.0, 3.0] {
        assert_eq!(
            raster(&tracked, &style(TextUnit::Unspecified), scale),
            raster(&plain, &style(TextUnit::Sp(0.24)), scale)
        );
        assert_ne!(
            raster(&tracked, &style(TextUnit::Unspecified), scale),
            raster(&plain, &style(TextUnit::Unspecified), scale)
        );
        for spacing in [TextUnit::Sp(0.0), TextUnit::Sp(-0.2), TextUnit::Em(0.1)] {
            assert_eq!(
                raster(&tracked, &style(spacing), scale),
                raster(&plain, &style(spacing), scale)
            );
        }
    }
}
