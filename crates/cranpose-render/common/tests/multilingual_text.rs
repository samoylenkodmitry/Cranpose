#![cfg(feature = "text-shaping")]

use ab_glyph::{Font, FontRef};
use cranpose_render_common::software_text_raster::{
    SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFont, SoftwareTextFontSet,
    SoftwareTextMeasurer, collect_solid_text_atlas_run,
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, SpanStyle, TextMeasurer, TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, Rect};

fn fonts() -> SoftwareTextFontSet {
    SoftwareTextFontSet::from_fonts_or_default(&[
        include_bytes!("../assets/NotoSansMerged.ttf"),
        include_bytes!("fixtures/localization/Arabic.ttf"),
        include_bytes!("fixtures/localization/Devanagari.ttf"),
    ])
}

fn style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_family: Some(FontFamily::named("Noto Sans")),
            font_size: TextUnit::Sp(24.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn drawn(text: &str) -> Vec<SoftwareGlyphAtlasRunGlyph> {
    let fonts = fonts();
    let style = style();
    let text = AnnotatedString::from(text);
    let metrics = SoftwareTextMeasurer::from_font_set(fonts.clone(), 32).measure(&text, &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(128);
    let mut output = Vec::new();
    collect_solid_text_atlas_run(
        &text,
        Rect {
            x: 0.0,
            y: 0.0,
            width: metrics.width,
            height: metrics.height,
        },
        &style,
        Color::WHITE,
        24.0,
        1.0,
        &fonts,
        &mut cache,
        &mut output,
    )
    .expect("multilingual text uses the glyph atlas");
    output
}

#[test]
fn an_app_font_falls_back_for_arabic_and_devanagari() {
    for text in ["العربية", "हिंदी"] {
        let output = drawn(text);
        assert!(!output.is_empty());
        assert!(
            output
                .iter()
                .all(|glyph| glyph.placement().key.glyph_id != 0),
            "missing glyph in {text}"
        );
    }
}

#[test]
fn arabic_lam_alef_uses_contextual_forms() {
    let output = drawn("لا");
    let face = FontRef::try_from_slice(include_bytes!("fixtures/localization/Arabic.ttf"))
        .expect("Arabic face");
    let isolated = [face.glyph_id('ل').0 as u32, face.glyph_id('ا').0 as u32];
    assert!(
        output
            .iter()
            .all(|glyph| !isolated.contains(&glyph.placement().key.glyph_id)),
        "lam and alef must use contextual forms: {:?}",
        output
            .iter()
            .map(|glyph| glyph.placement().key.glyph_id)
            .collect::<Vec<_>>()
    );
    let arabic =
        SoftwareTextFont::from_bytes(include_bytes!("fixtures/localization/Arabic.ttf").as_slice())
            .expect("Arabic face");
    assert_eq!(output[0].placement().key.font_hash, arabic.content_hash());
}

#[test]
fn arabic_cursor_edges_follow_visual_order_and_logical_offsets() {
    let measurer = SoftwareTextMeasurer::from_font_set(fonts(), 32);
    let style = style();
    let text = AnnotatedString::from("العربية");
    let layout = measurer.layout(&text, &style);
    let start = measurer.get_cursor_x_for_offset(&text, &style, 0);
    let end = measurer.get_cursor_x_for_offset(&text, &style, text.text.len());
    assert!(
        start > end + 20.0,
        "Arabic starts at the right edge: {start} {end}"
    );
    assert_eq!(layout.get_cursor_x(0), start);
    assert_eq!(layout.get_cursor_x(text.text.len()), end);
    assert_eq!(layout.get_offset_for_x(start), 0);
    assert_eq!(layout.get_offset_for_x(end), text.text.len());
    assert_eq!(
        measurer.get_offset_for_position(&text, &style, start, 0.0),
        0
    );
    assert_eq!(
        measurer.get_offset_for_position(&text, &style, end, 0.0),
        text.text.len()
    );
    assert!((layout.width - measurer.measure(&text, &style).width).abs() < 0.01);
}

#[test]
fn mixed_text_keeps_latin_and_arabic_runs_in_their_own_direction() {
    let measurer = SoftwareTextMeasurer::from_font_set(fonts(), 32);
    let style = style();
    let text = AnnotatedString::from("English العربية 123");
    let layout = measurer.layout(&text, &style);
    assert!(layout.get_cursor_x(0) < layout.get_cursor_x(3));
    let arabic = text.text.find('ا').expect("Arabic start");
    assert!(layout.get_cursor_x(arabic) > layout.get_cursor_x(arabic + 2));
    let digits = text.text.find('1').expect("number start");
    assert!(layout.get_cursor_x(digits) < layout.get_cursor_x(digits + 1));
    assert!((layout.width - measurer.measure(&text, &style).width).abs() < 0.01);
}

#[test]
fn devanagari_conjunct_is_one_cursor_cluster() {
    let measurer = SoftwareTextMeasurer::from_font_set(fonts(), 32);
    let style = style();
    let text = AnnotatedString::from("क्षि");
    let layout = measurer.layout(&text, &style);
    assert_eq!(
        layout.glyph_layouts().len(),
        1,
        "conjunct must stay together"
    );
    assert_eq!(layout.get_cursor_x(0), layout.get_cursor_x(3));
    assert!(layout.get_cursor_x(text.text.len()) > layout.get_cursor_x(0));
}

#[test]
fn shaped_line_and_subsequence_measurements_preserve_span_styles_and_ranges() {
    use cranpose_ui::text::RangeStyle;
    let measurer = SoftwareTextMeasurer::from_font_set(fonts(), 32);
    let mut text = AnnotatedString::from("ééé العربية ééé\nक्षि");
    let second = text.text.rfind("ééé").expect("second Latin run");
    text.span_styles.push(RangeStyle {
        item: SpanStyle {
            font_size: TextUnit::Sp(36.0),
            font_feature_settings: Some("smcp".into()),
            ..Default::default()
        },
        range: second..second + "ééé".len(),
    });
    let arabic = text.text.find('ا').expect("Arabic start");
    let hindi = text.text.find('क').expect("Hindi start");
    let style = style();
    for range in [
        0..6,
        second..second + 6,
        arabic..arabic + "العربية".len(),
        hindi..text.text.len(),
        6..6,
        0..6,
    ] {
        let expected = measurer.measure(&text.subsequence(range.clone()), &style);
        let measured = measurer.measure_subsequence(&text, range.clone(), &style);
        let width = measurer
            .measure_line_width(&text, range, &style)
            .expect("single-line width");
        assert!((width - expected.width).abs() < 0.01);
        assert!((measured.width - expected.width).abs() < 0.01);
        assert!((measured.height - expected.height).abs() < 0.01);
    }
    assert!(
        measurer
            .measure_line_width(&text, 0..text.text.len(), &style)
            .is_none()
    );
}

#[test]
fn single_font_entry_points_share_shaped_measurement_and_caret_positions() {
    use cranpose_render_common::software_text_raster::{
        cursor_x_for_offset_with_font, layout_text_with_font, measure_text_with_font,
        text_offset_for_position_with_font,
    };
    let font =
        SoftwareTextFont::from_bytes(include_bytes!("fixtures/localization/Arabic.ttf").as_slice())
            .expect("font");
    let text = "العربية 123";
    let style = style();
    let layout = layout_text_with_font(text, &style, &font);
    let metrics = measure_text_with_font(text, &style, 24.0, &font);
    assert!((layout.width - metrics.width).abs() < 0.01);
    let start = cursor_x_for_offset_with_font(text, &style, 0, &font);
    assert!(start > 20.0, "the logical start is on the right");
    assert_eq!(layout.get_cursor_x(0), start);
    assert_eq!(
        text_offset_for_position_with_font(text, &style, start, 0.0, &font),
        0
    );
}

#[test]
fn synthetic_weight_and_tracking_preserve_cursive_connections() {
    let measurer = SoftwareTextMeasurer::from_font_set(fonts(), 32);
    let text = AnnotatedString::from("العربية");
    let mut style = style();
    let normal = measurer.layout(&text, &style);
    style.span_style.font_weight = Some(cranpose_ui::text::FontWeight::BOLD);
    style.span_style.letter_spacing = TextUnit::Sp(2.0);
    let bold = measurer.layout(&text, &style);
    assert_eq!(normal.glyph_layouts().len(), bold.glyph_layouts().len());
    for (normal, bold) in normal.glyph_layouts().iter().zip(bold.glyph_layouts()) {
        assert!(
            (normal.x - bold.x).abs() < 0.01,
            "weight must not separate connected outlines"
        );
    }
}
