use std::rc::Rc;

use cranpose_render_common::{
    brush_sampling::sample_brush_rgba,
    font_source::FontRegistry,
    glyph_mask::{GlyphMaskCache, GlyphMaskKey},
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_canvas::TextPixels,
    text_fonts::TextFonts,
    text_raster::{TextRasterRequest, rasterize_text},
};
use cranpose_text::{CaretDirection, Typeface};
use cranpose_ui::{
    PreparedTextLayout,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontWeight, RangeStyle, SpanStyle,
        TextLayoutOptions, TextStyle, TextUnit,
    },
};
use cranpose_ui_graphics::{Brush, BrushSampler, Color, Point, Rect};

const REGULAR: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");
const BOLD: &[u8] = include_bytes!("../assets/NotoSansBold.ttf");

fn font_set() -> (TextFonts, FontFamily, FontFamily) {
    let family = FontFamily::named("Glyph fallback");
    let mut registry = FontRegistry::new();
    registry
        .register_face_bytes(&family, FontWeight::NORMAL, FontStyle::Normal, REGULAR)
        .expect("regular test font");
    registry
        .register_face_bytes(&family, FontWeight::BOLD, FontStyle::Normal, BOLD)
        .expect("bold test font");
    let regular_only = FontFamily::named("Glyph fallback reference");
    registry
        .register_face_bytes(
            &regular_only,
            FontWeight::NORMAL,
            FontStyle::Normal,
            REGULAR,
        )
        .expect("reference regular test font");
    (
        TextFonts::new(registry.into_typefaces(&[])),
        family,
        regular_only,
    )
}

fn style(family: &FontFamily, weight: FontWeight) -> SpanStyle {
    SpanStyle {
        font_family: Some(family.clone()),
        font_weight: Some(weight),
        font_size: TextUnit::Sp(28.0),
        ..Default::default()
    }
}

// `text` in `style` laid out with `fonts`, as wide as it is.
fn prepare(text: &AnnotatedString, style: &TextStyle, fonts: &TextFonts) -> Rc<PreparedTextLayout> {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts.resolver());
        cranpose_ui::text::prepare_text_layout(text, style, TextLayoutOptions::default(), None)
    })
}

fn render_at_scale(
    text: &AnnotatedString,
    style: &TextStyle,
    fonts: &TextFonts,
    scale: f32,
) -> Vec<u8> {
    let prepared = prepare(text, style, fonts);
    rasterize_text(
        TextRasterRequest::new(
            &prepared.layout,
            text,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 180.0 * scale,
                height: 48.0 * scale,
            },
            style,
            Color::BLACK,
            scale,
        ),
        Some(&mut GlyphMaskCache::with_capacity_at_least_one(64)),
    )
    .and_then(TextPixels::into_image)
    .expect("test text produces a raster")
    .pixels()
    .to_vec()
}

fn render(text: &AnnotatedString, style: &TextStyle, fonts: &TextFonts) -> Vec<u8> {
    render_at_scale(text, style, fonts, 1.0)
}

fn assert_same_pixels(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len());
    let mut differing_channels = 0;
    let mut max_delta = 0;
    let mut first_difference = None;
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        let delta = actual.abs_diff(*expected);
        if delta > 0 {
            differing_channels += 1;
            max_delta = max_delta.max(delta);
            first_difference.get_or_insert((index / 4 % 180, index / 4 / 180));
        }
    }
    assert_eq!(
        differing_channels, 0,
        "{differing_channels} channels differ, max delta {max_delta}, first pixel {first_difference:?}"
    );
}

fn atlas_placements(
    text: &AnnotatedString,
    style: &TextStyle,
    fonts: &TextFonts,
) -> Vec<AtlasGlyph> {
    let prepared = prepare(text, style, fonts);
    let mut glyphs = Vec::new();
    collect_solid_text_atlas_run(
        TextRasterRequest::new(
            &prepared.layout,
            text,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 48.0,
            },
            style,
            Color::BLACK,
            1.0,
        ),
        &mut GlyphMaskCache::with_capacity_at_least_one(64),
        &mut glyphs,
    )
    .expect("test text produces atlas glyphs");
    glyphs
}

fn explicit_reference(bold: &SpanStyle, regular: &SpanStyle) -> AnnotatedString {
    let mut text = AnnotatedString::from("A→B");
    text.span_styles.extend([
        RangeStyle {
            range: 0..1,
            item: bold.clone(),
        },
        RangeStyle {
            range: 1..4,
            item: regular.clone(),
        },
        RangeStyle {
            range: 4..5,
            item: bold.clone(),
        },
    ]);
    text
}

fn assert_matches_reference(
    actual: &AnnotatedString,
    expected: &AnnotatedString,
    style: &TextStyle,
    fonts: &TextFonts,
) {
    assert_same_pixels(
        &render(actual, style, fonts),
        &render(expected, style, fonts),
    );
    assert_eq!(
        atlas_placements(actual, style, fonts),
        atlas_placements(expected, style, fonts),
        "the glyph-atlas path should use the same fallback face and advances"
    );

    let actual_layout = prepare(actual, style, fonts);
    let expected_layout = prepare(expected, style, fonts);
    assert_eq!(actual_layout.metrics, expected_layout.metrics);
    assert_eq!(actual_layout.layout.width(), expected_layout.layout.width());
    assert_eq!(
        actual_layout.layout.height(),
        expected_layout.layout.height()
    );
    for byte_offset in [0, 1, 4, 5] {
        assert_eq!(
            actual_layout
                .layout
                .horizontal_position(byte_offset, CaretDirection::Primary),
            expected_layout
                .layout
                .horizontal_position(byte_offset, CaretDirection::Primary),
            "cursor position at byte offset {byte_offset}"
        );
    }
}

fn gradient(family: &FontFamily, origin_x: f32, width: f32) -> SpanStyle {
    let mut span = style(family, FontWeight::BOLD);
    span.brush = Some(gradient_brush(origin_x, width));
    span
}

fn gradient_brush(origin_x: f32, width: f32) -> Brush {
    Brush::linear_gradient_range(
        vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)],
        Point::new(-origin_x, 0.0),
        Point::new(width - origin_x, 0.0),
    )
}

fn check_global_gradient(
    text: &AnnotatedString,
    style: &TextStyle,
    fonts: &TextFonts,
    scale: f32,
) -> Result<(), String> {
    let width = 180.0 * scale;
    let mut scaled_text = text.clone();
    let mut scaled_style = style.clone();
    if scaled_style.span_style.brush.is_some() {
        scaled_style.span_style.brush = Some(gradient_brush(0.0, width));
    }
    for range in &mut scaled_text.span_styles {
        if range.item.brush.is_some() {
            range.item.brush = Some(gradient_brush(0.0, width));
        }
    }
    let actual = render_at_scale(&scaled_text, &scaled_style, fonts, scale);
    let mut mask_text = scaled_text;
    let mut mask_style = scaled_style;
    mask_style.span_style.brush = Some(Brush::solid(Color::WHITE));
    for range in &mut mask_text.span_styles {
        range.item.brush = Some(Brush::solid(Color::WHITE));
    }
    let mask = render_at_scale(&mask_text, &mask_style, fonts, scale);
    let height = 48.0 * scale;
    let brush = Brush::linear_gradient_range(
        vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)],
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
    );
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let mut differing_channels = 0;
    let mut max_delta = 0;
    let mut first_difference = None;
    let mut checked_right_of_initial_run = false;
    for (index, alpha) in mask.iter().skip(3).step_by(4).enumerate() {
        if *alpha != 255 {
            continue;
        }
        let x = index % width.ceil() as usize;
        let y = index / width.ceil() as usize;
        if x as f32 > width * 0.2 {
            checked_right_of_initial_run = true;
        }
        let sample = sample_brush_rgba(
            &BrushSampler::new(&brush, rect),
            x as f32 + 0.5,
            y as f32 + 0.5,
            Point::default(),
        );
        for channel in 0..3 {
            let expected = (sample[channel].clamp(0.0, 1.0) * 255.0).round() as u8;
            let actual = actual[index * 4 + channel];
            let delta = actual.abs_diff(expected);
            if delta > 0 {
                differing_channels += 1;
                max_delta = max_delta.max(delta);
                first_difference.get_or_insert((x, y));
            }
        }
    }
    if !checked_right_of_initial_run {
        return Err(format!("no opaque fallback-run pixels at scale {scale}"));
    }
    if differing_channels != 0 {
        return Err(format!(
            "{differing_channels} channels differ, max delta {max_delta}, first pixel {first_difference:?} at scale {scale}"
        ));
    }
    Ok(())
}

fn assert_global_gradient_at_scales(text: &AnnotatedString, style: &TextStyle, fonts: &TextFonts) {
    let failures = [1.0, 2.75]
        .into_iter()
        .filter_map(|scale| check_global_gradient(text, style, fonts, scale).err())
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bold_text_uses_a_covering_face_for_missing_glyphs_across_layout_and_renderers() {
    let (fonts, family, regular_only) = font_set();
    let bold = style(&family, FontWeight::BOLD);
    let regular = style(&regular_only, FontWeight::BOLD);
    let mut actual = AnnotatedString::from("A→B");
    actual.span_styles.push(RangeStyle {
        range: 0..actual.text.len(),
        item: bold.clone(),
    });
    let expected = explicit_reference(&bold, &regular);
    let base_style = TextStyle::default();

    assert_matches_reference(&actual, &expected, &base_style, &fonts);
}

#[test]
fn plain_bold_text_uses_a_covering_face_for_missing_glyphs() {
    let (fonts, family, regular_only) = font_set();
    let bold = style(&family, FontWeight::BOLD);
    let regular = style(&regular_only, FontWeight::BOLD);
    let actual = AnnotatedString::from("A→B");
    let expected = explicit_reference(&bold, &regular);
    let base_style = TextStyle {
        span_style: bold,
        ..Default::default()
    };

    assert_matches_reference(&actual, &expected, &base_style, &fonts);
}

#[test]
fn bold_text_fallback_keeps_one_gradient_extent_across_font_runs() {
    let (fonts, family, _) = font_set();
    let bold = gradient(&family, 0.0, 180.0);
    let mut actual = AnnotatedString::from("A→B");
    actual.span_styles.push(RangeStyle {
        range: 0..actual.text.len(),
        item: bold,
    });
    let base_style = TextStyle::default();
    assert_global_gradient_at_scales(&actual, &base_style, &fonts);
}

#[test]
fn plain_bold_fallback_keeps_one_gradient_extent_across_font_runs() {
    let (fonts, family, _) = font_set();
    let actual = AnnotatedString::from("A→B");
    let base_style = TextStyle {
        span_style: gradient(&family, 0.0, 180.0),
        ..Default::default()
    };
    assert_global_gradient_at_scales(&actual, &base_style, &fonts);
}

#[test]
fn ascii_text_falls_back_for_letters_the_requested_face_lacks() {
    const EMOJI: &[u8] = include_bytes!("../assets/TwemojiMozilla.ttf");
    let emoji_face = Typeface::from_bytes(EMOJI).expect("emoji test font");
    assert!(emoji_face.glyph('H').is_none(), "the emoji face has no H");
    let fonts = latin_and_emoji(EMOJI);
    let text = AnnotatedString::from("Hi");
    let glyphs = |family: &FontFamily| {
        let style = TextStyle {
            span_style: style(family, FontWeight::NORMAL),
            ..Default::default()
        };
        atlas_placements(&text, &style, &fonts)
            .into_iter()
            .map(|glyph| glyph.key)
            .collect::<Vec<GlyphMaskKey>>()
    };

    assert_eq!(glyphs(&emoji()), glyphs(&latin()));
}

fn latin() -> FontFamily {
    FontFamily::named("Latin")
}

fn emoji() -> FontFamily {
    FontFamily::named("Emoji only")
}

// A Latin family and an emoji family that has no Latin letters.
fn latin_and_emoji(emoji_bytes: &'static [u8]) -> TextFonts {
    let mut registry = FontRegistry::new();
    registry
        .register_face_bytes(&latin(), FontWeight::NORMAL, FontStyle::Normal, REGULAR)
        .expect("Latin test font");
    registry
        .register_face_bytes(&emoji(), FontWeight::NORMAL, FontStyle::Normal, emoji_bytes)
        .expect("emoji test font");
    TextFonts::new(registry.into_typefaces(&[]))
}

#[test]
fn a_prepared_text_takes_the_line_height_of_the_faces_it_draws_in() {
    const EMOJI: &[u8] = include_bytes!("../assets/TwemojiMozilla.ttf");
    let fonts = latin_and_emoji(EMOJI);
    let text = AnnotatedString::from("Hi");
    let prepared = |family: &FontFamily| {
        let base_style = TextStyle {
            span_style: style(family, FontWeight::NORMAL),
            ..Default::default()
        };
        prepare(&text, &base_style, &fonts)
    };
    let in_latin = prepared(&latin());
    let falling_back = prepared(&emoji());

    assert_eq!(
        falling_back.metrics.line_height, in_latin.metrics.line_height,
        "letters the emoji face lacks draw in the Latin face, whose line they take"
    );
    assert_eq!(
        falling_back.alignment_lines.first_baseline(),
        in_latin.alignment_lines.first_baseline()
    );
}
