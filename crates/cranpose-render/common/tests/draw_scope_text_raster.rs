#![cfg(feature = "embedded-default-font")]

use std::sync::Arc;

use cranpose_render_common::{
    glyph_mask::GlyphMaskCache,
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_fonts::TextFonts,
    text_raster::TextRasterRequest,
};
use cranpose_text::TextLayout;
use cranpose_ui::text::{
    AppContextTextMeasurer, LineBox, TextStyle, draw_style_line_box, text_style_for_draw_style,
};
use cranpose_ui_graphics::{Color, DrawTextMeasurer, DrawTextStyle, Rect, TextMeasurement};

const GLYPH_CACHE_CAPACITY: usize = 512;

// What a draw scope measures and lays out for `text` in `style`, with the
// fonts a renderer without app fonts hands its app.
struct Measured {
    measurement: TextMeasurement,
    layout: Arc<TextLayout>,
    line_box: Option<LineBox>,
}

fn measured(text: &str, style: &DrawTextStyle) -> Measured {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(TextFonts::from_fonts_or_default(&[]).resolver());
        let handle = AppContextTextMeasurer
            .layout_text(text, style)
            .expect("an app context lays text out");
        Measured {
            measurement: AppContextTextMeasurer.measure_text(text, style),
            layout: handle.downcast::<TextLayout>().expect("an engine layout"),
            line_box: draw_style_line_box(style),
        }
    })
}

fn measured_block(origin: (f32, f32), text: &str, style: &DrawTextStyle) -> (Rect, Measured) {
    let measured = measured(text, style);
    let size = measured.measurement.size;
    (
        Rect {
            x: origin.0,
            y: origin.1,
            width: size.width,
            height: size.height,
        },
        measured,
    )
}

fn collect(
    rect: Rect,
    text: &str,
    measured: &Measured,
    style: &DrawTextStyle,
    cache: &mut GlyphMaskCache,
) -> Vec<AtlasGlyph> {
    let ui_style: TextStyle = text_style_for_draw_style(style);
    let mut run = Vec::new();
    collect_solid_text_atlas_run(
        TextRasterRequest::new(&measured.layout, text, rect, &ui_style, Color::WHITE, 1.0),
        cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run
}

#[test]
fn every_glyph_lands_inside_the_block_measure_text_reported() {
    let style = DrawTextStyle::new(24.0);
    let (rect, measured) = measured_block((40.0, 60.0), "SCORE 1234", &style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "SCORE 1234", &measured, &style, &mut cache);
    assert!(!run.is_empty(), "the run must produce glyphs");

    const OVERHANG: f32 = 1.0;
    for glyph in &run {
        let left = glyph.x as f32;
        let top = glyph.y as f32;
        let right = left + glyph.width as f32;
        let bottom = top + glyph.height as f32;
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
    let style = DrawTextStyle::new(20.0);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut ink_right = |text: &str| {
        let (rect, measured) = measured_block((0.0, 0.0), text, &style);
        let run = collect(rect, text, &measured, &style, &mut cache);
        let ink = run
            .iter()
            .map(|glyph| glyph.x as f32 + glyph.width as f32)
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
    let style = DrawTextStyle::new(18.0);
    let (rect, measured) = measured_block((10.0, 10.0), "FPS 60", &style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    let first = collect(rect, "FPS 60", &measured, &style, &mut cache);
    assert!(!first.is_empty());
    let after_first = cache.stats();
    assert!(after_first.misses > 0, "the first frame has to rasterize");

    for _ in 0..60 {
        collect(rect, "FPS 60", &measured, &style, &mut cache);
    }
    assert_eq!(
        cache.stats().misses,
        after_first.misses,
        "an unchanged string must not re-rasterize a single glyph"
    );
    assert_eq!(
        cache.stats().entries,
        after_first.entries,
        "60 more frames must not add a mask to the cache"
    );
}

#[test]
fn only_the_new_characters_of_a_changing_counter_rasterize() {
    let style = DrawTextStyle::new(18.0);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    for score in 0..10 {
        let text = format!("SCORE {score}");
        let (rect, measured) = measured_block((0.0, 0.0), &text, &style);
        collect(rect, &text, &measured, &style, &mut cache);
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
    let style = DrawTextStyle::new(20.0).with_font_family("No Such Family At All");
    let (rect, measured) = measured_block((0.0, 0.0), "AB", &style);
    assert!(
        rect.width > 0.0,
        "an unresolvable family must still measure against the fallback font"
    );
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "AB", &measured, &style, &mut cache);
    assert!(
        !run.is_empty(),
        "an unresolvable family must still draw with the fallback font"
    );
}

#[test]
fn characters_the_font_cannot_draw_measure_and_collect_without_panicking() {
    let text = "A\u{E000}\u{10FFFD}B";
    let style = DrawTextStyle::new(20.0);
    let (rect, measured) = measured_block((0.0, 0.0), text, &style);
    assert!(rect.width > 0.0 && rect.height > 0.0);

    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, text, &measured, &style, &mut cache);
    assert!(!run.is_empty());
    for glyph in &run {
        assert!(glyph.width > 0 && glyph.height > 0);
    }
}

#[test]
fn multiline_text_stacks_by_the_line_height_it_was_measured_with() {
    let style = DrawTextStyle::new(16.0);
    let (rect, measured) = measured_block((0.0, 0.0), "AB\nCD", &style);
    let measured_line_height = rect.height / 2.0;

    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "AB\nCD", &measured, &style, &mut cache);
    let tops: Vec<i32> = run.iter().map(|glyph| glyph.y).collect();
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
    let style = DrawTextStyle::new(32.0);
    let (rect, measured) = measured_block((0.0, 0.0), "H", &style);
    let baseline = measured.measurement.first_baseline;

    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "H", &measured, &style, &mut cache);
    let glyph = run.first().expect("one glyph");
    let ink_bottom = (glyph.y + glyph.height as i32) as f32;
    assert!(
        (ink_bottom - baseline).abs() <= 1.5,
        "cap-height ink ends at {ink_bottom}, reported baseline is {baseline}"
    );
}

#[test]
fn a_drawn_run_that_names_a_line_height_policy_is_laid_out_by_it() {
    use cranpose_ui::text::{LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim};

    let asked = 30.0;
    let plain = DrawTextStyle::new(32.0).with_line_height(asked);
    let styled = plain.clone().with_line_height_style(LineHeightStyle {
        alignment: LineHeightAlignment::Center,
        trim: LineHeightTrim::None,
        mode: LineHeightMode::Minimum,
    });

    let plain_box = measured("H", &plain)
        .line_box
        .expect("an app context reports a line box");
    let styled_box = measured("H", &styled)
        .line_box
        .expect("an app context reports a line box");

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

    let (rect, measured) = measured_block((0.0, 0.0), "H", &styled);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, "H", &measured, &styled, &mut cache);
    let glyph = run.first().expect("one glyph");
    let ink_bottom = (glyph.y + glyph.height as i32) as f32;
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
    let plain = DrawTextStyle::new(20.0);
    let tracked = DrawTextStyle::new(20.0).with_letter_spacing(TRACKING);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut spans = |style: &DrawTextStyle| {
        let (rect, measured) = measured_block((0.0, 0.0), WORD, style);
        let run = collect(rect, WORD, &measured, style, &mut cache);
        let left = run
            .iter()
            .map(|glyph| glyph.x as f32)
            .fold(f32::INFINITY, f32::min);
        let right = run
            .iter()
            .map(|glyph| glyph.x as f32 + glyph.width as f32)
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
