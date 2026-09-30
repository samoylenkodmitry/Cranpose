#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::{
    software_text_raster::{
        SoftwareGlyphAtlasGlyph, SoftwareGlyphRasterCache, SoftwareTextMeasurer,
        collect_solid_text_atlas_glyphs, rasterize_annotated_text_to_image_with_glyph_cache,
        software_text_font_set_from_fonts_or_default,
    },
    text_measure::{CachedFontTextMeasurer, SoftwareTextResources},
};
use cranpose_ui::text::{
    AnnotatedString, LineHeightAlignment, LineHeightStyle, LineHeightTrim, RangeStyle, SpanStyle,
    TextLayoutOptions, TextMeasurer, TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect};

fn style(font_size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(font_size),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn cropped_shadowed_strokes_keep_the_full_paragraphs_visible_pixels() {
    let fonts = software_text_font_set_from_fonts_or_default(&[]);
    let mut text = AnnotatedString::from("H\nH\nH\nH\nH\nH\nH\nH\nH\nH");
    text.span_styles.push(RangeStyle {
        range: 0..1,
        item: SpanStyle {
            font_size: TextUnit::Sp(40.0),
            ..Default::default()
        },
    });
    let mut base = style(10.0);
    base.span_style.brush = Some(cranpose_ui_graphics::Brush::linear_gradient(vec![
        Color::WHITE,
        Color::WHITE,
    ]));
    base.span_style.draw_style = Some(cranpose_ui::text::TextDrawStyle::Stroke { width: 2.0 });
    base.span_style.shadow = Some(cranpose_ui::text::Shadow {
        color: Color::WHITE,
        offset: cranpose_ui_graphics::Point::new(-3.25, 5.75),
        blur_radius: 4.0,
    });
    base.span_style.font_weight = Some(cranpose_ui::text::FontWeight::BLACK);
    base.span_style.font_style = Some(cranpose_ui::text::FontStyle::Italic);
    let raster = |offset: f32, height: f32| {
        cranpose_render_common::software_text_raster::rasterize_annotated_text_region(
            &text,
            Rect {
                x: 4.0,
                y: 0.0,
                width: 64.0,
                height,
            },
            cranpose_ui_graphics::Point::new(4.0, -offset),
            &base,
            Color::WHITE,
            10.0,
            1.0,
            &fonts,
            None,
        )
        .expect("paragraph raster")
    };
    let whole = raster(0.0, 360.0);
    let cropped = raster(61.0, 30.0);
    assert_eq!(cropped.pixels(), &whole.pixels()[61 * 64 * 4..91 * 64 * 4]);
}

struct LineHeightOnlyMeasurer(SoftwareTextMeasurer);

impl TextMeasurer for LineHeightOnlyMeasurer {
    fn measure(&self, text: &AnnotatedString, style: &TextStyle) -> cranpose_ui::text::TextMetrics {
        self.0.measure(text, style)
    }

    fn line_height(&self, text: &AnnotatedString, style: &TextStyle) -> f32 {
        self.0.line_height(text, style)
    }

    fn line_box(&self, style: &TextStyle) -> Option<cranpose_ui::text::LineBox> {
        self.0.line_box(style)
    }

    fn get_offset_for_position(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        self.0.get_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        self.0.get_cursor_x_for_offset(text, style, offset)
    }

    fn layout(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
    ) -> cranpose_ui::text::TextLayoutResult {
        self.0.layout(text, style)
    }
}

#[test]
fn a_custom_measurer_keeps_its_span_aware_line_height_without_resolved_line_boxes() {
    let (software, _) = measurers();
    let custom = LineHeightOnlyMeasurer(software);
    let text = mixed_text("HH\nH");
    let base = style(10.0);
    let expected_height = 2.0 * custom.line_height(&text, &base);
    let prepared = custom.prepare_with_options(&text, &base, TextLayoutOptions::default(), None);
    assert_close(prepared.metrics.height, expected_height);
}

#[test]
fn multiline_height_baselines_and_cursors_follow_each_lines_styled_fonts() {
    let (software, cached) = measurers();
    let base = style(10.0);
    let text = mixed_text("HH\nH\nH");
    let small = software.measure(&"H".into(), &base);
    let large = software.measure(&"H".into(), &style(20.0));
    let small_baseline = software
        .line_box(&base)
        .expect("font metrics")
        .first_baseline();
    let large_baseline = software
        .line_box(&style(20.0))
        .expect("font metrics")
        .first_baseline();
    let expected_height = large.height + 2.0 * small.height;
    let expected_last = large.height + small.height + small_baseline;
    let (bitmap, glyphs) = render(&text, &base);
    let last_ink = bitmap_ink_bottoms(&bitmap, 2);
    assert_eq!(last_ink, atlas_ink_bottoms(&glyphs, 2));
    assert_eq!(last_ink.len(), 2);
    assert_close((last_ink[1] - last_ink[0]) as f32, small.height);
    for measurer in [&software as &dyn TextMeasurer, &cached] {
        let metrics = measurer.measure(&text, &base);
        let prepared =
            measurer.prepare_with_options(&text, &base, TextLayoutOptions::default(), None);
        assert_close(metrics.height, expected_height);
        assert_close(prepared.metrics.height, expected_height);
        assert_close(
            prepared
                .alignment_lines
                .first_baseline()
                .expect("first baseline"),
            large_baseline,
        );
        assert_close(
            prepared
                .alignment_lines
                .last_baseline()
                .expect("last baseline"),
            expected_last,
        );
        assert_close(
            measurer.get_cursor_x_for_offset(&text, &base, 2),
            small.width + large.width,
        );
        assert_eq!(
            measurer.get_offset_for_position(&text, &base, 0.0, large.height + 1.0),
            3
        );
        let layout = measurer.layout(&text, &base);
        assert_close(layout.lines[1].y, large.height);
        assert_close(layout.lines[2].y, large.height + small.height);
        assert_close(layout.get_cursor_x(2), small.width + large.width);
        let minimum = measurer.prepare_with_options(
            &text,
            &base,
            TextLayoutOptions {
                min_lines: 8,
                ..Default::default()
            },
            None,
        );
        assert_close(minimum.metrics.height, 8.0 * small.height);
        assert_eq!(minimum.alignment_lines, prepared.alignment_lines);
    }
}

#[test]
fn explicit_line_height_and_edge_trimming_match_the_painted_shared_baselines() {
    let (measurer, _) = measurers();
    for (height, trim) in [
        (32.0, LineHeightTrim::None),
        (32.0, LineHeightTrim::Both),
        (14.0, LineHeightTrim::None),
    ] {
        let mut base = style(10.0);
        base.paragraph_style.line_height = TextUnit::Sp(height);
        base.paragraph_style.line_height_style = Some(LineHeightStyle {
            alignment: LineHeightAlignment::Center,
            trim,
            ..Default::default()
        });
        let text = mixed_text("HH\nH");
        let mut reference_style = base.clone();
        reference_style.span_style.font_size = TextUnit::Sp(20.0);
        let reference = measurer.prepare_with_options(
            &"H\nH".into(),
            &reference_style,
            TextLayoutOptions::default(),
            None,
        );
        let prepared =
            measurer.prepare_with_options(&text, &base, TextLayoutOptions::default(), None);
        assert_close(prepared.metrics.height, reference.metrics.height);
        assert_eq!(prepared.alignment_lines, reference.alignment_lines);
        let (bitmap, glyphs) = render(&text, &base);
        let first = bitmap_ink_bottoms(&bitmap, 1)[0];
        let last = bitmap_ink_bottoms(&bitmap, 2)[0];
        assert_eq!(last - first, height as i32);
        assert_eq!(
            atlas_ink_bottoms(&glyphs, 2)[0] - atlas_ink_bottoms(&glyphs, 1)[0],
            height as i32
        );
    }
}

#[test]
fn wrapped_lines_and_retained_span_updates_report_the_drawn_height() {
    let (software, cached) = measurers();
    let base = style(10.0);
    let mut text = mixed_text("HH HH");
    let width = software.measure(&mixed_text("HH"), &base).width + 0.1;
    let options = TextLayoutOptions::default();
    let first = cached.prepare_with_options(&text, &base, options, Some(width));
    assert_eq!(first.text.text.split('\n').count(), 2);
    let (bitmap, glyphs) = render(&first.text, &first.visual_style);
    assert_eq!(
        bitmap_ink_bottoms(&bitmap, 2),
        atlas_ink_bottoms(&glyphs, 2)
    );
    let small = software.measure(&"H".into(), &base);
    let large = software.measure(&"H".into(), &style(20.0));
    assert_close(first.metrics.height, large.height + small.height);
    text.span_styles[1].item.font_size = TextUnit::Sp(30.0);
    let updated = cached.prepare_with_options(&text, &base, options, None);
    let updated_metrics = cached.measure(&text, &base);
    assert_close(
        updated.metrics.height,
        software.measure(&"H".into(), &style(30.0)).height,
    );
    assert_close(updated.metrics.height, updated_metrics.height);
    assert!(updated_metrics.width > software.measure(&mixed_text("HH HH"), &base).width);
    let (bitmap, _) = render(&updated.text, &updated.visual_style);
    assert!((bitmap_ink_bottoms(&bitmap, 0)[0] - bitmap_ink_bottoms(&bitmap, 1)[0]).abs() <= 1);
}
fn mixed_text(value: &str) -> AnnotatedString {
    let mut text = AnnotatedString::from(value);
    for (range, font_size, color) in [
        (0..1, 10.0, Color(1.0, 0.0, 0.0, 1.0)),
        (1..2, 20.0, Color(0.0, 1.0, 0.0, 1.0)),
        (2..value.len(), 10.0, Color(0.0, 0.0, 1.0, 1.0)),
    ] {
        if !range.is_empty() {
            text.span_styles.push(RangeStyle {
                range,
                item: SpanStyle {
                    font_size: TextUnit::Sp(font_size),
                    color: Some(color),
                    ..Default::default()
                },
            });
        }
    }
    text
}

fn render(
    text: &AnnotatedString,
    style: &TextStyle,
) -> (ImageBitmap, Vec<SoftwareGlyphAtlasGlyph>) {
    let fonts = software_text_font_set_from_fonts_or_default(&[]);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 128.0,
        height: 256.0,
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(32);
    let bitmap = rasterize_annotated_text_to_image_with_glyph_cache(
        text,
        rect,
        style,
        Color::WHITE,
        style.resolve_font_size(10.0),
        1.0,
        &fonts,
        &mut cache,
    )
    .expect("the styled text renders to a bitmap");
    let mut glyphs = Vec::new();
    collect_solid_text_atlas_glyphs(
        text,
        rect,
        style,
        Color::WHITE,
        style.resolve_font_size(10.0),
        1.0,
        &fonts,
        &mut cache,
        &mut glyphs,
    )
    .expect("the styled text renders through the atlas");
    (bitmap, glyphs)
}

fn bitmap_ink_bottoms(bitmap: &ImageBitmap, channel: usize) -> Vec<i32> {
    let mut bottoms = Vec::new();
    let mut previous = None;
    for (y, row) in bitmap
        .pixels()
        .chunks_exact(bitmap.width() as usize * 4)
        .enumerate()
    {
        let painted = row.as_chunks::<4>().0.iter().any(|pixel| {
            pixel[3] > 32 && (0..3).all(|other| other == channel || pixel[channel] > pixel[other])
        });
        if painted {
            if previous == Some(y.saturating_sub(1)) && !bottoms.is_empty() {
                if let Some(last) = bottoms.last_mut() {
                    *last = y as i32;
                }
            } else {
                bottoms.push(y as i32);
            }
            previous = Some(y);
        }
    }
    bottoms
}

fn atlas_ink_bottoms(glyphs: &[SoftwareGlyphAtlasGlyph], channel: usize) -> Vec<i32> {
    let mut bottoms = Vec::new();
    for glyph in glyphs {
        let color = [glyph.color.r(), glyph.color.g(), glyph.color.b()];
        if (0..3).any(|other| other != channel && color[other] >= color[channel]) {
            continue;
        }
        if let Some(bottom) = glyph
            .mask
            .alpha
            .iter()
            .enumerate()
            .filter(|(_, alpha)| **alpha > 32.0 / 255.0)
            .map(|(index, _)| glyph.y + (index / glyph.mask.width) as i32)
            .max()
        {
            bottoms.push(bottom);
        }
    }
    bottoms.sort_unstable();
    bottoms.dedup();
    bottoms
}

fn measurers() -> (SoftwareTextMeasurer, CachedFontTextMeasurer) {
    (
        SoftwareTextMeasurer::from_font_set(software_text_font_set_from_fonts_or_default(&[]), 256),
        CachedFontTextMeasurer::with_text_resources(SoftwareTextResources::default(), 256),
    )
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn differently_sized_spans_share_their_visible_baseline_in_bitmap_and_atlas_output() {
    let (bitmap, glyphs) = render(&mixed_text("HH"), &style(10.0));
    let bitmap_bottoms = [
        bitmap_ink_bottoms(&bitmap, 0),
        bitmap_ink_bottoms(&bitmap, 1),
    ];
    let atlas_bottoms = [atlas_ink_bottoms(&glyphs, 0), atlas_ink_bottoms(&glyphs, 1)];
    for (renderer, bottoms) in [("bitmap", &bitmap_bottoms), ("atlas", &atlas_bottoms)] {
        let small = *bottoms[0].first().expect("the small red H is visible");
        let large = *bottoms[1].first().expect("the large green H is visible");
        assert!(
            (small - large).abs() <= 1,
            "{renderer} places the small H at y={small} and the large H at y={large}; both must share a baseline (bitmap={bitmap_bottoms:?}, atlas={atlas_bottoms:?})"
        );
    }
}
