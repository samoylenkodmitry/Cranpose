use cranpose_ui::text::{ParagraphStyle, TextAlign};

use super::*;

fn ink_columns(image: &ImageBitmap, rows: std::ops::Range<u32>) -> Option<(u32, u32)> {
    let width = image.width();
    let pixels = image.pixels();
    let mut min = u32::MAX;
    let mut max = 0u32;
    for y in rows {
        for x in 0..width {
            let index = ((y * width + x) * 4 + 3) as usize;
            if pixels.get(index).copied().unwrap_or(0) > 0 {
                min = min.min(x);
                max = max.max(x);
            }
        }
    }
    (min != u32::MAX).then_some((min, max))
}

fn centred_style(align: TextAlign) -> TextStyle {
    TextStyle {
        paragraph_style: ParagraphStyle {
            text_align: align,
            ..ParagraphStyle::default()
        },
        ..TextStyle::default()
    }
}

#[test]
fn a_wrapped_list_header_centres_both_its_lines() {
    let font = default_software_text_font().expect("bundled default font");
    let style = cranpose_ui::widgets::wear::list_header::ListHeaderSpec::default()
        .text_style
        .resolve(Color(1.0, 1.0, 1.0, 1.0));
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 80.0,
    };
    let image = rasterize_text_to_image(
        "wwwwwwwwwwww\nww",
        rect,
        &style,
        Color(1.0, 1.0, 1.0, 1.0),
        20.0,
        1.0,
        &font,
    )
    .expect("header image");
    let long = ink_columns(&image, 0..(image.height() / 2)).expect("first line ink");
    let short = ink_columns(&image, (image.height() / 2)..image.height()).expect("second line ink");
    let long_centre = (long.0 + long.1) as f32 * 0.5;
    let short_centre = (short.0 + short.1) as f32 * 0.5;
    assert!(
        (long_centre - short_centre).abs() <= 2.0,
        "a wrapped header's lines must share a centre: {long:?} vs {short:?}"
    );
}

#[test]
fn a_wrapped_line_is_centred_under_the_one_above_it_not_left_under_it() {
    let font = default_software_text_font().expect("bundled default font");
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 80.0,
    };
    let text = "wwwwwwwwwwww\nww";
    let image = rasterize_text_to_image(
        text,
        rect,
        &centred_style(TextAlign::Center),
        Color(1.0, 1.0, 1.0, 1.0),
        20.0,
        1.0,
        &font,
    )
    .expect("centred image");
    let long = ink_columns(&image, 0..(image.height() / 2)).expect("first line ink");
    let short = ink_columns(&image, (image.height() / 2)..image.height()).expect("second line ink");
    let long_centre = (long.0 + long.1) as f32 * 0.5;
    let short_centre = (short.0 + short.1) as f32 * 0.5;
    assert!(
        (long_centre - short_centre).abs() <= 2.0,
        "the two lines should share a centre: {long:?} vs {short:?}"
    );
    assert!(
        short.0 > long.0 + 4,
        "the short line must not start where the long one does: {long:?} vs {short:?}"
    );
}

#[test]
fn the_atlas_run_centres_each_line_of_a_wrapped_block() {
    let font = default_software_text_font().expect("bundled default font");
    let fonts = SoftwareTextFontSet::from_font(font);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(256);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 80.0,
    };
    let text = AnnotatedString::from("wwwwwwwwwwww\nww".to_string());

    let mut centred = Vec::new();
    collect_solid_text_atlas_run(
        &text,
        rect,
        &centred_style(TextAlign::Center),
        Color(1.0, 1.0, 1.0, 1.0),
        20.0,
        1.0,
        &fonts,
        &mut cache,
        &mut centred,
    )
    .expect("centred run");
    let mut flush = Vec::new();
    collect_solid_text_atlas_run(
        &text,
        rect,
        &centred_style(TextAlign::Start),
        Color(1.0, 1.0, 1.0, 1.0),
        20.0,
        1.0,
        &fonts,
        &mut cache,
        &mut flush,
    )
    .expect("start aligned run");

    let second_line_start = |glyphs: &[SoftwareGlyphAtlasRunGlyph]| {
        let placements: Vec<_> = glyphs
            .iter()
            .map(super::SoftwareGlyphAtlasRunGlyph::placement)
            .collect();
        let baseline = placements.iter().map(|p| p.y).max().expect("glyphs");
        placements
            .iter()
            .filter(|p| p.y == baseline)
            .map(|p| p.x)
            .min()
            .expect("second line")
    };
    assert_eq!(
        second_line_start(&flush),
        0,
        "a start-aligned second line begins at the block's left edge"
    );
    assert!(
        second_line_start(&centred) > 40,
        "a centred second line is indented by half the slack, was {}",
        second_line_start(&centred)
    );
}

#[test]
fn a_start_aligned_paragraph_still_stacks_its_lines_flush_left() {
    let font = default_software_text_font().expect("bundled default font");
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 80.0,
    };
    let image = rasterize_text_to_image(
        "wwwwwwwwwwww\nww",
        rect,
        &centred_style(TextAlign::Start),
        Color(1.0, 1.0, 1.0, 1.0),
        20.0,
        1.0,
        &font,
    )
    .expect("start aligned image");
    let long = ink_columns(&image, 0..(image.height() / 2)).expect("first line ink");
    let short = ink_columns(&image, (image.height() / 2)..image.height()).expect("second line ink");
    assert!(
        short.0.abs_diff(long.0) <= 1,
        "start-aligned lines share a left edge: {long:?} vs {short:?}"
    );
}
