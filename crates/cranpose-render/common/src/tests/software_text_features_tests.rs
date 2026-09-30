use cranpose_ui::text::{RangeStyle, SpanStyle};

use super::*;

const TABULAR_ZERO: u16 = 19;
const SLASHED_ZERO: u16 = 2251;
const SMALL_CAP_A: u16 = 1868;

fn font() -> SoftwareTextFont {
    default_software_text_font().expect("bundled default font")
}

fn featured(settings: Option<&str>) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        font_feature_settings: settings.map(str::to_owned),
        ..Default::default()
    })
}

fn shaped_glyph_ids(font: &SoftwareTextFont, style: &TextStyle, text: &str) -> Vec<u16> {
    let shaped = font.shaped_for(style);
    text.chars().map(|ch| shaped.font.glyph_id(ch).0).collect()
}

fn atlas_glyph_ids(
    fonts: &SoftwareTextFontSet,
    text: &AnnotatedString,
    style: &TextStyle,
) -> Vec<u32> {
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(64);
    let mut run = Vec::new();
    collect_solid_text_atlas_run(
        text,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 40.0,
        },
        style,
        Color::WHITE,
        18.0,
        1.0,
        fonts,
        &mut cache,
        &mut run,
    )
    .expect("atlas run");
    run.iter()
        .map(|glyph| glyph.placement().key.glyph_id)
        .collect()
}

fn layout_widths(font: &SoftwareTextFont, text: &str, style: &TextStyle) -> Vec<f32> {
    layout_text_with_font(text, style, font)
        .glyph_layouts()
        .iter()
        .map(|glyph| glyph.width)
        .collect()
}

#[test]
fn pnum_makes_digits_proportional_and_narrows_the_measured_width() {
    let font = font();
    let plain = featured(None);
    let pnum = featured(Some("\"pnum\""));
    let size = resolve_font_size(&plain);
    let tabular = measure_text_with_font("1111", &plain, size, &font).width;
    let proportional = measure_text_with_font("1111", &pnum, size, &font).width;
    assert!(
        proportional < tabular * 0.85,
        "proportional ones {proportional} should be narrower than tabular {tabular}"
    );
    let advances: Vec<f32> = layout_widths(&font, "1", &plain)
        .into_iter()
        .chain(layout_widths(&font, "1", &pnum))
        .collect();
    assert!(
        (advances[1] / advances[0] - 441.0 / 572.0).abs() < 1e-3,
        "{advances:?}"
    );

    let measurer = SoftwareTextMeasurer::new(font, 64);
    let text = AnnotatedString::from("1111");
    for (style, width) in [(&plain, tabular), (&pnum, proportional), (&plain, tabular)] {
        assert_eq!(measurer.measure(&text, style).width, width);
        let line = measurer
            .measure_line_width(&text, 0..text.text.len(), style)
            .expect("line width");
        assert!((line - width).abs() < 0.01, "{line} vs {width}");
    }
}

#[test]
fn zero_substitutes_the_glyph_of_zero_where_it_is_measured_and_drawn() {
    let font = font();
    let fonts = SoftwareTextFontSet::from_font(font.clone());
    let plain = featured(None);
    let zero = featured(Some("\"zero\" on"));
    assert_eq!(shaped_glyph_ids(&font, &plain, "10"), [20, TABULAR_ZERO]);
    assert_eq!(shaped_glyph_ids(&font, &zero, "10"), [20, SLASHED_ZERO]);
    let text = AnnotatedString::from("10");
    assert_eq!(
        atlas_glyph_ids(&fonts, &text, &plain),
        [20, u32::from(TABULAR_ZERO)]
    );
    assert_eq!(
        atlas_glyph_ids(&fonts, &text, &zero),
        [20, u32::from(SLASHED_ZERO)]
    );
    assert_eq!(
        measure_text_with_font("0", &zero, 18.0, &font).width,
        measure_text_with_font("0", &plain, 18.0, &font).width
    );
}

#[test]
fn zero_draws_different_pixels_from_the_plain_zero() {
    let font = font();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 48.0,
        height: 32.0,
    };
    let draw = |style: &TextStyle| {
        rasterize_text_to_image("0", rect, style, Color::WHITE, 24.0, 1.0, &font).expect("image")
    };
    assert_ne!(
        draw(&featured(Some("zero"))).pixels(),
        draw(&featured(None)).pixels()
    );
    assert_eq!(
        draw(&featured(Some("zero 0"))).pixels(),
        draw(&featured(None)).pixels()
    );
}

#[test]
fn smcp_substitutes_lowercase_letters_only() {
    let font = font();
    let plain = featured(None);
    let smcp = featured(Some("'smcp'"));
    let lower: String = ('a'..='z').collect();
    let upper: String = ('A'..='Z').collect();
    let small_caps = shaped_glyph_ids(&font, &smcp, &lower);
    for (ch, (small, plain)) in lower.chars().zip(
        small_caps
            .iter()
            .zip(shaped_glyph_ids(&font, &plain, &lower)),
    ) {
        assert_ne!(*small, plain, "{ch}");
    }
    assert_eq!(small_caps[0], SMALL_CAP_A);
    assert_eq!(
        shaped_glyph_ids(&font, &smcp, &upper),
        shaped_glyph_ids(&font, &plain, &upper)
    );
    let fonts = SoftwareTextFontSet::from_font(font);
    let small_caps_run = atlas_glyph_ids(&fonts, &AnnotatedString::from("abc"), &smcp);
    assert_eq!(small_caps_run[0], u32::from(SMALL_CAP_A));
}

#[test]
fn no_settings_or_disabled_features_give_todays_glyphs_and_advances() {
    let font = font();
    let fonts = SoftwareTextFontSet::from_font(font.clone());
    let plain = featured(None);
    let text = "Tabular 0123456789 abc AV To";
    let annotated = AnnotatedString::from(text);
    let plain_width = measure_text_with_font(text, &plain, 18.0, &font).width;
    let plain_glyphs = atlas_glyph_ids(&fonts, &annotated, &plain);
    let plain_layout = layout_widths(&font, text, &plain);
    for settings in [
        "\"pnum\" 0",
        "pnum off, zero 0",
        "liga",
        "frac",
        "",
        "bogus",
    ] {
        let style = featured(Some(settings));
        assert!(
            matches!(font.shaped_for(&style), Cow::Borrowed(_)),
            "{settings} should shape nothing"
        );
        assert_eq!(
            measure_text_with_font(text, &style, 18.0, &font).width,
            plain_width
        );
        assert_eq!(atlas_glyph_ids(&fonts, &annotated, &style), plain_glyphs);
        assert_eq!(layout_widths(&font, text, &style), plain_layout);
    }
    assert!(matches!(font.shaped_for(&plain), Cow::Borrowed(_)));
}

#[test]
fn a_shaped_face_shares_raster_caches_but_not_character_glyphs() {
    let font = font();
    let zero = font.shaped_for(&featured(Some("zero")));
    assert_eq!(zero.content_hash(), font.content_hash());
    assert_ne!(zero.glyph_map_hash, font.glyph_map_hash);
    assert!(matches!(
        zero.shaped_for(&featured(Some("\"zero\" 1"))),
        Cow::Borrowed(_)
    ));
    let unshaped = zero.shaped_for(&featured(None));
    assert_eq!(unshaped.glyph_map_hash, font.glyph_map_hash);
    assert_eq!(
        shaped_glyph_ids(&unshaped, &featured(None), "0"),
        [TABULAR_ZERO]
    );
}

#[test]
fn a_measurer_keeps_feature_and_plain_widths_apart_for_non_ascii_text() {
    let measurer = SoftwareTextMeasurer::new(font(), 64);
    let text = AnnotatedString::from("ééé");
    let plain = featured(None);
    let smcp = featured(Some("smcp"));
    let line = |style: &TextStyle| {
        measurer
            .measure_line_width(&text, 0..text.text.len(), style)
            .expect("line width")
    };
    let plain_width = line(&plain);
    let small_caps_width = line(&smcp);
    assert!(
        small_caps_width < plain_width * 0.9,
        "{small_caps_width} vs {plain_width}"
    );
    assert_eq!(line(&plain), plain_width);
    assert_eq!(line(&smcp), small_caps_width);
}

#[test]
fn a_span_shapes_only_its_own_range() {
    let fonts = SoftwareTextFontSet::from_font(font());
    let text = AnnotatedString {
        text: "0000".to_owned(),
        span_styles: vec![RangeStyle {
            item: SpanStyle {
                font_feature_settings: Some("zero".to_owned()),
                ..Default::default()
            },
            range: 0..2,
        }],
        ..Default::default()
    };
    let slashed = u32::from(SLASHED_ZERO);
    let plain = u32::from(TABULAR_ZERO);
    assert_eq!(
        atlas_glyph_ids(&fonts, &text, &featured(None)),
        [slashed, slashed, plain, plain]
    );
}
