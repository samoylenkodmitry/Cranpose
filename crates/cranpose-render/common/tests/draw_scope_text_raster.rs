#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::software_text_raster::{
    SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFontSet,
    SoftwareTextMeasurer, collect_solid_text_atlas_run,
    software_text_font_set_from_fonts_or_default,
};
use cranpose_ui::text::{
    AnnotatedString, TextMeasurer, TextStyle as UiTextStyle, text_style_for_draw_style,
};
use cranpose_ui_graphics::{Color, Rect, TextStyle};

const GLYPH_CACHE_CAPACITY: usize = 512;

fn fonts() -> SoftwareTextFontSet {
    software_text_font_set_from_fonts_or_default(&[])
}

fn measurer() -> SoftwareTextMeasurer {
    SoftwareTextMeasurer::from_font_set(fonts(), 256)
}

fn measured_block(origin: (f32, f32), text: &str, style: &TextStyle) -> (Rect, UiTextStyle) {
    let ui_style = text_style_for_draw_style(style);
    let measurer = measurer();
    let metrics = measurer.measure(&AnnotatedString::from(text), &ui_style);
    let line_height = metrics.line_height;
    let line_count = metrics.line_count.max(1);
    (
        Rect {
            x: origin.0,
            y: origin.1,
            width: metrics.width,
            height: line_count as f32 * line_height,
        },
        ui_style,
    )
}

fn collect(
    rect: Rect,
    text: &str,
    ui_style: &UiTextStyle,
    style: &TextStyle,
    cache: &mut SoftwareGlyphRasterCache,
) -> Vec<SoftwareGlyphAtlasRunGlyph> {
    let mut run = Vec::new();
    collect_solid_text_atlas_run(
        &AnnotatedString::from(text),
        rect,
        ui_style,
        Color::WHITE,
        style.resolved_font_size(),
        1.0,
        &fonts(),
        cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run
}

#[test]
fn every_glyph_lands_inside_the_block_measure_text_reported() {
    let style = TextStyle::new(24.0);
    let (rect, ui_style) = measured_block((40.0, 60.0), "SCORE 1234", &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "SCORE 1234", &ui_style, &style, &mut cache);
    assert!(!run.is_empty(), "the run must produce glyphs");

    const OVERHANG: f32 = 1.0;
    for glyph in &run {
        let placement = glyph.placement();
        let left = placement.x as f32;
        let top = placement.y as f32;
        let right = left + placement.width as f32;
        let bottom = top + placement.height as f32;
        assert!(
            left >= -OVERHANG && right <= rect.width + OVERHANG,
            "glyph spans x {left}..{right}, block is 0..{}",
            rect.width
        );
        assert!(
            top >= -OVERHANG && bottom <= rect.height + OVERHANG,
            "glyph spans y {top}..{bottom}, block is 0..{}",
            rect.height
        );
    }
}

#[test]
fn a_wider_string_measures_wider_and_draws_wider() {
    let style = TextStyle::new(20.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut ink_right = |text: &str| {
        let (rect, ui_style) = measured_block((0.0, 0.0), text, &style);
        let run = collect(rect, text, &ui_style, &style, &mut cache);
        let ink = run
            .iter()
            .map(|glyph| {
                let placement = glyph.placement();
                placement.x as f32 + placement.width as f32
            })
            .fold(0.0_f32, f32::max);
        (rect.width, ink)
    };

    let (short_width, short_ink) = ink_right("AB");
    let (long_width, long_ink) = ink_right("ABCDEFGH");
    assert!(long_width > short_width);
    assert!(
        long_ink > short_ink,
        "measured growth must be matched by drawn growth"
    );
}

#[test]
fn a_stable_string_rasterizes_once_and_is_served_from_the_glyph_cache_after_that() {
    let style = TextStyle::new(18.0);
    let (rect, ui_style) = measured_block((10.0, 10.0), "FPS 60", &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    let first = collect(rect, "FPS 60", &ui_style, &style, &mut cache);
    assert!(
        first
            .iter()
            .any(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::New(_))),
        "the first frame has to rasterize"
    );
    let rasterized_after_first = cache.stats().entries;

    for _ in 0..60 {
        let frame = collect(rect, "FPS 60", &ui_style, &style, &mut cache);
        assert!(
            frame
                .iter()
                .all(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::Cached(_))),
            "an unchanged string must not re-rasterize a single glyph"
        );
    }
    assert_eq!(
        cache.stats().entries,
        rasterized_after_first,
        "60 more frames must not add a mask to the cache"
    );
}

#[test]
fn only_the_new_characters_of_a_changing_counter_rasterize() {
    let style = TextStyle::new(18.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    for score in 0..10 {
        let text = format!("SCORE {score}");
        let (rect, ui_style) = measured_block((0.0, 0.0), &text, &style);
        collect(rect, &text, &ui_style, &style, &mut cache);
    }
    assert!(
        cache.stats().entries <= 16,
        "expected one mask per distinct glyph, got {}",
        cache.stats().entries
    );
    assert!(cache.stats().hits > cache.stats().misses);
}

#[test]
fn an_unknown_font_family_falls_back_to_the_framework_font() {
    let style = TextStyle::new(20.0).with_font_family("No Such Family At All");
    let (rect, ui_style) = measured_block((0.0, 0.0), "AB", &style);
    assert!(
        rect.width > 0.0,
        "an unresolvable family must still measure against the fallback font"
    );
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "AB", &ui_style, &style, &mut cache);
    assert!(
        !run.is_empty(),
        "an unresolvable family must still draw with the fallback font"
    );
}

#[test]
fn characters_the_font_cannot_draw_measure_and_collect_without_panicking() {
    let text = "A\u{E000}\u{10FFFD}B";
    let style = TextStyle::new(20.0);
    let (rect, ui_style) = measured_block((0.0, 0.0), text, &style);
    assert!(rect.width > 0.0 && rect.height > 0.0);

    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, text, &ui_style, &style, &mut cache);
    assert!(!run.is_empty());
    for glyph in &run {
        let placement = glyph.placement();
        assert!(placement.width > 0 && placement.height > 0);
    }
}

#[test]
fn multiline_text_stacks_by_the_line_height_it_was_measured_with() {
    let style = TextStyle::new(16.0);
    let (rect, ui_style) = measured_block((0.0, 0.0), "AB\nCD", &style);
    let measured_line_height = rect.height / 2.0;

    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "AB\nCD", &ui_style, &style, &mut cache);
    let tops: Vec<i32> = run.iter().map(|glyph| glyph.placement().y).collect();
    let first_row = *tops.iter().min().expect("glyphs");
    let last_row = *tops.iter().max().expect("glyphs");
    let drawn_line_height = (last_row - first_row) as f32;
    assert!(
        (drawn_line_height - measured_line_height).abs() <= 1.0,
        "second line drawn {drawn_line_height} below the first, measured line height is \
         {measured_line_height}"
    );
}

#[test]
fn the_measured_baseline_is_the_row_glyphs_are_actually_placed_on() {
    let style = TextStyle::new(32.0);
    let ui_style = text_style_for_draw_style(&style);
    let baseline = measurer()
        .first_baseline(&ui_style)
        .expect("a font-backed measurer reports a baseline");
    let (rect, _) = measured_block((0.0, 0.0), "H", &style);

    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "H", &ui_style, &style, &mut cache);
    let placement = run.first().expect("one glyph").placement();
    let ink_bottom = (placement.y + placement.height as i32) as f32;
    assert!(
        (ink_bottom - baseline).abs() <= 1.5,
        "cap-height ink ends at {ink_bottom}, reported baseline is {baseline}"
    );
}

#[test]
fn a_drawn_run_that_names_a_line_height_policy_is_laid_out_by_it() {
    use cranpose_ui::text::{LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim};

    let asked = 30.0;
    let plain = TextStyle::new(32.0).with_line_height(asked);
    let styled = plain.clone().with_line_height_style(LineHeightStyle {
        alignment: LineHeightAlignment::Center,
        trim: LineHeightTrim::None,
        mode: LineHeightMode::Minimum,
    });

    let measurer = measurer();
    let plain_box = measurer
        .line_box(&text_style_for_draw_style(&plain))
        .expect("a font-backed measurer reports a line box");
    let styled_box = measurer
        .line_box(&text_style_for_draw_style(&styled))
        .expect("a font-backed measurer reports a line box");

    assert_eq!(
        plain_box.height, asked,
        "with no policy the request is the box, whatever the font needs"
    );
    assert!(
        styled_box.height > asked,
        "under the platform rule a line height shorter than the font does not \
         shrink the line: got {}",
        styled_box.height
    );
    assert_eq!(
        styled_box.height,
        styled_box.height.round(),
        "the platform's line advance is a whole pixel, not a float: got {}",
        styled_box.height
    );
    assert_ne!(plain_box.baseline, styled_box.baseline);

    let ui_style = text_style_for_draw_style(&styled);
    let (rect, _) = measured_block((0.0, 0.0), "H", &styled);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "H", &ui_style, &styled, &mut cache);
    let placement = run.first().expect("one glyph").placement();
    let ink_bottom = (placement.y + placement.height as i32) as f32;
    assert!(
        (ink_bottom - styled_box.baseline).abs() <= 1.5,
        "cap-height ink ends at {ink_bottom}, the styled line box puts the \
         baseline at {}",
        styled_box.baseline
    );
}

#[test]
fn letter_spacing_pads_a_run_with_half_a_space_at_each_edge() {
    const WORD: &str = "ORBIT";
    const TRACKING: f32 = 5.0;
    let plain = TextStyle::new(20.0);
    let tracked = TextStyle::new(20.0).with_letter_spacing(TRACKING);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut spans = |style: &TextStyle| {
        let (rect, ui_style) = measured_block((0.0, 0.0), WORD, style);
        let run = collect(rect, WORD, &ui_style, style, &mut cache);
        let left = run
            .iter()
            .map(|glyph| glyph.placement().x as f32)
            .fold(f32::INFINITY, f32::min);
        let right = run
            .iter()
            .map(|glyph| {
                let placement = glyph.placement();
                placement.x as f32 + placement.width as f32
            })
            .fold(0.0_f32, f32::max);
        (rect.width, left, right)
    };

    let (plain_width, plain_left, plain_right) = spans(&plain);
    let (tracked_width, tracked_left, tracked_right) = spans(&tracked);

    let chars = WORD.chars().count() as f32;
    assert!(
        (tracked_width - plain_width - chars * TRACKING).abs() < 0.5,
        "the block grew by {} where {chars} characters of {TRACKING}pt tracking \
         should have widened it by {}",
        tracked_width - plain_width,
        chars * TRACKING
    );
    assert!(
        (tracked_left - plain_left - TRACKING * 0.5).abs() < 2.0,
        "ink starts {} further in where the lead-in is half a letter space, {}",
        tracked_left - plain_left,
        TRACKING * 0.5
    );
    assert!(
        (tracked_right - plain_right - (chars * TRACKING - TRACKING * 0.5)).abs() < 2.0,
        "ink ends {} further out; the last glyph carries every gap but not the \
         trailing half space, so it should be {}",
        tracked_right - plain_right,
        chars * TRACKING - TRACKING * 0.5
    );

    let plain_slack = (plain_width - plain_right) - plain_left;
    let tracked_slack = (tracked_width - tracked_right) - tracked_left;
    assert!(
        (tracked_slack - plain_slack).abs() < 2.0,
        "tracked ink sits {tracked_slack} off-centre in its block where plain ink \
         sits {plain_slack}; the edge halves must balance"
    );
}
