#![cfg(feature = "embedded-default-font")]

use std::rc::Rc;

use cranpose_render_common::{
    glyph_mask::{GlyphMask, GlyphMaskCache},
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_canvas::TextPixels,
    text_fonts::TextFonts,
    text_raster::{TextRasterRequest, rasterize_text},
};
use cranpose_text::CaretDirection;
use cranpose_ui::{
    PreparedTextLayout,
    text::{
        AnnotatedString, LineHeightAlignment, LineHeightStyle, LineHeightTrim, RangeStyle,
        SpanStyle, TextLayoutOptions, TextStyle, TextUnit,
    },
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

fn fonts() -> TextFonts {
    TextFonts::from_fonts_or_default(&[])
}

// `text` in `style` laid out with the default fonts up to `max_width`.
fn prepare(
    text: &AnnotatedString,
    style: &TextStyle,
    max_width: Option<f32>,
) -> Rc<PreparedTextLayout> {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts().resolver());
        cranpose_ui::text::prepare_text_layout(text, style, TextLayoutOptions::default(), max_width)
    })
}

// The width of the one line of `prepared`.
fn line_width(prepared: &PreparedTextLayout) -> f32 {
    prepared.layout.line_right(0) - prepared.layout.line_left(0)
}

#[test]
fn cropped_shadowed_strokes_keep_the_full_paragraphs_visible_pixels() {
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
    let prepared = prepare(&text, &base, None);
    let raster = |offset: f32, height: f32| {
        rasterize_text(
            TextRasterRequest::new(
                &prepared.layout,
                &text,
                Rect {
                    x: 4.0,
                    y: 0.0,
                    width: 64.0,
                    height,
                },
                &base,
                Color::WHITE,
                1.0,
            )
            .with_origin(cranpose_ui_graphics::Point::new(4.0, -offset)),
            None,
        )
        .and_then(TextPixels::into_image)
        .expect("paragraph raster")
    };
    let whole = raster(0.0, 360.0);
    let cropped = raster(61.0, 30.0);
    assert_eq!(cropped.pixels(), &whole.pixels()[61 * 64 * 4..91 * 64 * 4]);
}

#[test]
fn multiline_height_baselines_and_cursors_follow_each_lines_styled_fonts() {
    let base = style(10.0);
    let text = mixed_text("HH\nH\nH");
    let small = prepare(&"H".into(), &base, None);
    let large = prepare(&"H".into(), &style(20.0), None);
    let small_height = small.metrics.height;
    let large_height = large.metrics.height;
    let small_baseline = small.layout.line_baseline(0);
    let large_baseline = large.layout.line_baseline(0);
    let expected_height = large_height + 2.0 * small_height;
    let expected_last = large_height + small_height + small_baseline;
    let prepared = prepare(&text, &base, None);
    let (bitmap, glyphs) = render(&prepared);
    let last_ink = bitmap_ink_bottoms(&bitmap, 2);
    assert_eq!(last_ink, atlas_ink_bottoms(&glyphs, 2));
    assert_eq!(last_ink.len(), 2);
    assert_close((last_ink[1] - last_ink[0]) as f32, small_height);

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
    let layout = &prepared.layout;
    assert_close(
        layout.horizontal_position(2, CaretDirection::Primary),
        line_width(&small) + line_width(&large),
    );
    assert_eq!(layout.offset_for_position(0.0, large_height + 1.0), 3);
    assert_close(layout.line_top(1), large_height);
    assert_close(layout.line_top(2), large_height + small_height);
}

#[test]
fn explicit_line_heights_draw_each_line_on_its_layout_baseline() {
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
        let prepared = prepare(&mixed_text("HH\nH"), &base, None);
        let layout = &prepared.layout;
        if trim == LineHeightTrim::None {
            assert_close(layout.line_top(1) - layout.line_top(0), height);
            assert_close(prepared.metrics.height, 2.0 * height);
        }
        let (bitmap, glyphs) = render(&prepared);
        // The large green H sits on the first line, the blue H on the second.
        for (channel, line) in [(1, 0), (2, 1)] {
            let drawn = bitmap_ink_bottoms(&bitmap, channel)[0];
            let baseline = layout.line_baseline(line);
            assert!(
                (drawn as f32 + 1.0 - baseline).abs() <= 1.0,
                "{height} {trim:?}: line {line} ink ends at {drawn}, its baseline is {baseline}"
            );
            assert_eq!(atlas_ink_bottoms(&glyphs, channel)[0], drawn);
        }
    }
}

#[test]
fn wrapped_lines_and_span_updates_report_the_drawn_height() {
    let base = style(10.0);
    let mut text = mixed_text("HH HH");
    let width = line_width(&prepare(&mixed_text("HH"), &base, None)) + 0.1;
    let first = prepare(&text, &base, Some(width));
    assert_eq!(first.layout.line_count(), 2);
    let (bitmap, glyphs) = render(&first);
    assert_eq!(
        bitmap_ink_bottoms(&bitmap, 2),
        atlas_ink_bottoms(&glyphs, 2)
    );
    let small = prepare(&"H".into(), &base, None);
    let large = prepare(&"H".into(), &style(20.0), None);
    assert_close(
        first.metrics.height,
        large.metrics.height + small.metrics.height,
    );
    text.span_styles[1].item.font_size = TextUnit::Sp(30.0);
    let updated = prepare(&text, &base, None);
    assert_close(
        updated.metrics.height,
        prepare(&"H".into(), &style(30.0), None).metrics.height,
    );
    assert!(line_width(&updated) > line_width(&prepare(&mixed_text("HH HH"), &base, None)));
    let (bitmap, _) = render(&updated);
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

// `prepared` drawn as a bitmap and as atlas glyphs with their masks.
fn render(prepared: &PreparedTextLayout) -> (ImageBitmap, Vec<(AtlasGlyph, GlyphMask)>) {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 128.0,
        height: 256.0,
    };
    let request = TextRasterRequest::new(
        &prepared.layout,
        &*prepared.text,
        rect,
        &prepared.visual_style,
        Color::WHITE,
        1.0,
    );
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(32);
    let bitmap = rasterize_text(request, Some(&mut cache))
        .and_then(TextPixels::into_image)
        .expect("the styled text renders to a bitmap");
    let mut glyphs = Vec::new();
    collect_solid_text_atlas_run(request, &mut cache, &mut glyphs)
        .expect("the styled text renders through the atlas");
    let glyphs = glyphs
        .into_iter()
        .map(|glyph| {
            let mask = cache.cached(&glyph.key).expect("a collected mask").clone();
            (glyph, mask)
        })
        .collect();
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

fn atlas_ink_bottoms(glyphs: &[(AtlasGlyph, GlyphMask)], channel: usize) -> Vec<i32> {
    let mut bottoms = Vec::new();
    for (glyph, mask) in glyphs {
        let color = [glyph.color.r(), glyph.color.g(), glyph.color.b()];
        if (0..3).any(|other| other != channel && color[other] >= color[channel]) {
            continue;
        }
        if let Some(bottom) = mask
            .alpha
            .iter()
            .enumerate()
            .filter(|(_, alpha)| **alpha > 32)
            .map(|(index, _)| glyph.y + (index / mask.width) as i32)
            .max()
        {
            bottoms.push(bottom);
        }
    }
    bottoms.sort_unstable();
    bottoms.dedup();
    bottoms
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn differently_sized_spans_share_their_visible_baseline_in_bitmap_and_atlas_output() {
    let (bitmap, glyphs) = render(&prepare(&mixed_text("HH"), &style(10.0), None));
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
