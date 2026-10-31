use cranpose_render_common::{
    glyph_mask::GlyphMaskCache,
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_fonts::TextFonts,
    text_raster::TextRasterRequest,
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, SpanStyle, TextLayoutOptions, TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, Rect};

fn fonts() -> TextFonts {
    TextFonts::from_fonts_or_default(&[
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

// The atlas glyphs `text` draws in the app font and its fallbacks.
fn drawn(text: &str) -> Vec<AtlasGlyph> {
    let fonts = fonts();
    let style = style();
    let text = AnnotatedString::from(text);
    let prepared = cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts.resolver());
        cranpose_ui::text::prepare_text_layout(&text, &style, TextLayoutOptions::default(), None)
    });
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(128);
    let mut output = Vec::new();
    collect_solid_text_atlas_run(
        TextRasterRequest::new(
            &prepared.layout,
            &text,
            Rect {
                x: 0.0,
                y: 0.0,
                width: prepared.metrics().width,
                height: prepared.metrics().height,
            },
            &style,
            Color::WHITE,
            1.0,
        ),
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
            output.iter().all(|glyph| glyph.key.glyph_id != 0),
            "missing glyph in {text}"
        );
    }
}

#[test]
fn shaped_text_that_ends_or_pauses_with_a_line_break_draws_every_line() {
    for text in ["Укупно 1.352,96\n", "Укупно\n\nИзнос 400,00", "مرحبا\n"] {
        let glyphs = drawn(text);
        let letters = text.chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            glyphs.len() >= letters / 2,
            "{text:?} drew {} glyph(s) for {letters} letter(s)",
            glyphs.len()
        );
    }
}
