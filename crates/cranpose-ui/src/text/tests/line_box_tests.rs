use super::*;
use crate::text::{
    TextUnit,
    style::{ParagraphStyle, PlatformParagraphStyle},
};

fn roboto_16sp() -> FontExtent {
    FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0)
}

fn styled(line_height_px: f32, line_height_style: Option<LineHeightStyle>) -> TextStyle {
    TextStyle {
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Sp(line_height_px),
            line_height_style,
            ..ParagraphStyle::default()
        },
        ..TextStyle::default()
    }
}

fn wear() -> LineHeightStyle {
    LineHeightStyle {
        alignment: LineHeightAlignment::Center,
        trim: LineHeightTrim::None,
        mode: LineHeightMode::Minimum,
    }
}

#[test]
fn a_style_that_names_no_line_height_style_gets_composes_default() {
    let extent = roboto_16sp();
    let plain = line_box(&styled(36.0, None), extent, 36.0, 1.0);
    let default = line_box(
        &styled(36.0, Some(LineHeightStyle::default())),
        extent,
        36.0,
        1.0,
    );
    assert_eq!(plain, default);
    assert_eq!(plain.height, 36.0, "every line keeps the asked advance");
    assert_eq!(
        plain.first_baseline(),
        extent.ascent.round(),
        "the first line gives its leading back, so its glyphs start at the paragraph's top"
    );
    assert_eq!(
        plain.block_height(1),
        (extent.ascent.round() + extent.descent.round()),
        "a single line of it is exactly as tall as its font"
    );
}

#[test]
fn a_style_that_asks_for_no_line_height_gets_the_fonts_own_extent() {
    let extent = roboto_16sp();
    let unspecified = line_box(&TextStyle::default(), extent, 51.2, 1.0);
    assert_eq!(
        unspecified.height,
        extent.ascent.round() + extent.descent.round(),
        "whatever height the caller falls back to, the font's own extent wins"
    );
    assert_eq!(unspecified.trim_top, 0.0);
    assert_eq!(unspecified.trim_bottom, 0.0);
}

#[test]
fn font_padding_trims_nothing() {
    let extent = FontExtent::new(20.0, 10.0, 4.0);
    let padded = TextStyle {
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Sp(40.0),
            platform_style: Some(PlatformParagraphStyle {
                include_font_padding: Some(true),
                shaping: None,
            }),
            ..ParagraphStyle::default()
        },
        ..TextStyle::default()
    };
    let resolved = line_box(&padded, extent, 40.0, 1.0);
    assert_eq!(resolved.trim_top, 0.0);
    assert_eq!(resolved.trim_bottom, 0.0);
    assert_eq!(resolved.block_height(2), 80.0);
}

#[test]
fn an_unstyled_box_is_the_same_box_measured_in_points_or_in_pixels() {
    let density = 2.0;
    for glyph_px in [24.0f32, 26.0, 28.125, 30.0, 32.0, 37.2, 38.72] {
        let ascent_px = glyph_px * 1900.0 / 2048.0;
        let descent_px = glyph_px * 500.0 / 2048.0;
        let asked_px = (glyph_px * 4.0 / 3.0).round();

        let in_pixels = line_box(
            &styled(asked_px, None),
            FontExtent::new(ascent_px, descent_px, 0.0),
            asked_px,
            1.0,
        );
        let in_points = line_box(
            &styled(asked_px / density, None),
            FontExtent::new(ascent_px / density, descent_px / density, 0.0),
            asked_px / density,
            density,
        );

        assert!(
            (in_points.height * density - in_pixels.height).abs() < 1e-4,
            "{glyph_px}px: height {} in points against {} in pixels",
            in_points.height * density,
            in_pixels.height
        );
        assert!(
            (in_points.baseline * density - in_pixels.baseline).abs() < 1e-4,
            "{glyph_px}px: baseline {} in points against {} in pixels",
            in_points.baseline * density,
            in_pixels.baseline
        );
    }
}

#[test]
fn title_medium_overflows_its_own_line_height_and_the_font_wins() {
    let box_ = line_box(&styled(36.0, Some(wear())), roboto_16sp(), 36.0, 1.0);
    assert_eq!(box_.height, 38.0);
}

#[test]
fn a_line_height_the_font_fits_inside_is_honoured_as_asked() {
    let extent = FontExtent::new(30.0 * 1900.0 / 2048.0, 30.0 * 500.0 / 2048.0, 0.0);
    let box_ = line_box(&styled(36.0, Some(wear())), extent, 36.0, 1.0);
    assert_eq!(box_.height, 36.0);
    assert_eq!(box_.baseline, 28.0);
}

#[test]
fn the_font_metrics_are_rounded_the_way_the_platform_rounds_them() {
    for (size_px, ascent_px, descent_px) in [
        (24.0_f32, 22.0_f32, 6.0_f32),
        (26.0, 24.0, 6.0),
        (30.0, 28.0, 7.0),
        (32.0, 30.0, 8.0),
        (37.2, 35.0, 9.0),
        (38.72, 36.0, 9.0),
        (43.76, 41.0, 11.0),
        (38.0, 35.0, 9.0),
    ] {
        let extent = FontExtent::new(size_px * 1900.0 / 2048.0, size_px * 500.0 / 2048.0, 0.0);
        let tight = line_box(
            &styled(
                0.0,
                Some(LineHeightStyle {
                    mode: LineHeightMode::Tight,
                    ..wear()
                }),
            ),
            extent,
            0.0,
            1.0,
        );
        assert_eq!(
            (tight.baseline, tight.height - tight.baseline),
            (ascent_px, descent_px),
            "{size_px}px",
        );
    }
}

#[test]
fn the_wear_type_scale_lays_out_in_the_boxes_the_platform_gives_it() {
    for (name, size_px, line_height_px, height, baseline) in [
        ("titleMedium 1.0", 32.0_f32, 36.0_f32, 38.0_f32, 30.0_f32),
        ("labelMedium 1.0", 30.0, 36.0, 36.0, 28.0),
        ("labelSmall 1.0", 26.0, 32.0, 32.0, 25.0),
        ("titleMedium 1.24", 38.72, 41.76, 45.0, 36.0),
        ("labelMedium 1.24", 37.2, 41.76, 44.0, 35.0),
        ("labelSmall 1.24", 32.72, 38.72, 39.0, 30.0),
    ] {
        let extent = FontExtent::new(size_px * 1900.0 / 2048.0, size_px * 500.0 / 2048.0, 0.0);
        let resolved = line_box(
            &styled(line_height_px, Some(wear())),
            extent,
            line_height_px,
            1.0,
        );
        assert_eq!(
            (resolved.height, resolved.baseline),
            (height, baseline),
            "{name}",
        );
    }
}

#[test]
fn the_odd_unit_of_leading_goes_below_the_baseline_not_above() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let box_ = line_box(&styled(33.0, Some(wear())), extent, 33.0, 1.0);
    assert_eq!(box_.height, 33.0);
    assert_eq!(box_.baseline, 21.0);
    assert_ne!(box_.baseline, 20.0 + 1.5);
}

#[test]
fn a_line_height_is_a_whole_number_of_pixels() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let box_ = line_box(&styled(33.4, Some(wear())), extent, 33.4, 1.0);
    assert_eq!(box_.height, 34.0);
}

#[test]
fn top_alignment_puts_the_glyphs_at_the_top_and_bottom_at_the_bottom() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let top = line_box(
        &styled(
            40.0,
            Some(LineHeightStyle {
                alignment: LineHeightAlignment::Top,
                ..wear()
            }),
        ),
        extent,
        40.0,
        1.0,
    );
    assert_eq!(top.baseline, 20.0);
    let bottom = line_box(
        &styled(
            40.0,
            Some(LineHeightStyle {
                alignment: LineHeightAlignment::Bottom,
                ..wear()
            }),
        ),
        extent,
        40.0,
        1.0,
    );
    assert_eq!(bottom.baseline, 30.0);
    assert_eq!(bottom.height - bottom.baseline, extent.descent);
}

#[test]
fn proportional_alignment_splits_the_leading_the_way_the_font_is_split() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let style = LineHeightStyle {
        alignment: LineHeightAlignment::Proportional,
        ..wear()
    };
    let box_ = line_box(&styled(60.0, Some(style)), extent, 60.0, 1.0);
    assert_eq!(box_.baseline, 40.0);
}

#[test]
fn a_fixed_line_height_lets_the_font_overflow_and_tight_ignores_the_ask() {
    let extent = roboto_16sp();
    let fixed = line_box(
        &styled(
            36.0,
            Some(LineHeightStyle {
                mode: LineHeightMode::Fixed,
                ..wear()
            }),
        ),
        extent,
        36.0,
        1.0,
    );
    assert_eq!(
        fixed.height, 36.0,
        "the ask wins even though the font needs 38"
    );
    let tight = line_box(
        &styled(
            80.0,
            Some(LineHeightStyle {
                mode: LineHeightMode::Tight,
                ..wear()
            }),
        ),
        extent,
        80.0,
        1.0,
    );
    assert_eq!(tight.height, 38.0);
    assert_eq!(tight.baseline, 30.0);
}

#[test]
fn trimming_removes_the_leading_on_the_edge_it_names() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let both = line_box(
        &styled(
            40.0,
            Some(LineHeightStyle {
                trim: LineHeightTrim::Both,
                ..wear()
            }),
        ),
        extent,
        40.0,
        1.0,
    );
    assert_eq!(both.height, 40.0, "every line keeps the asked advance");
    assert_eq!(both.baseline, 25.0);
    assert_eq!(both.first_baseline(), 20.0);
    assert_eq!(both.block_height(1), 30.0);
    assert_eq!(
        both.block_height(3),
        110.0,
        "only the first line's top and the last line's bottom are given back"
    );

    let top_only = line_box(
        &styled(
            40.0,
            Some(LineHeightStyle {
                trim: LineHeightTrim::FirstLineTop,
                ..wear()
            }),
        ),
        extent,
        40.0,
        1.0,
    );
    assert_eq!(top_only.block_height(1), 35.0);
    assert_eq!(top_only.first_baseline(), 20.0);
    assert_eq!(top_only.block_height(2), 75.0);
}

#[test]
fn font_padding_is_only_spent_when_a_style_asks_for_it() {
    let extent = FontExtent::new(20.0, 10.0, 4.0);
    let without = line_box(&styled(30.0, Some(wear())), extent, 30.0, 1.0);
    assert_eq!(without.height, 30.0);
    assert_eq!(without.baseline, 20.0);

    let padded = TextStyle {
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Sp(30.0),
            line_height_style: Some(wear()),
            platform_style: Some(PlatformParagraphStyle {
                include_font_padding: Some(true),
                shaping: None,
            }),
            ..ParagraphStyle::default()
        },
        ..TextStyle::default()
    };
    let with = line_box(&padded, extent, 30.0, 1.0);
    assert_eq!(with.height, 34.0, "the line gap widens the font's demand");
    assert_eq!(with.baseline, 22.0, "and half of it sits above the ascent");
}

#[test]
fn a_nonsense_line_height_falls_back_to_the_font_rather_than_producing_nan() {
    let extent = FontExtent::new(20.0, 10.0, 0.0);
    let box_ = line_box(&styled(30.0, Some(wear())), extent, f32::NAN, 1.0);
    assert_eq!(box_.height, 30.0);
    assert!(box_.baseline.is_finite());
}
