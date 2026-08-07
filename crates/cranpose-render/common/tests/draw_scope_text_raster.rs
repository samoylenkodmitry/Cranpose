//! `DrawScope` text against real fonts.
//!
//! The draw scope hands the renderer a block box it measured itself, so the
//! contract that matters is that the glyphs the rasterizer actually lays out
//! fit that box — and that redrawing an unchanged string does no work twice.
//! These run the production measurer and the production atlas collector over
//! the embedded fallback font, which is what the renderer uses when an app
//! registers none.

#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::software_text_raster::{
    collect_solid_text_atlas_run, software_text_font_set_from_fonts_or_default,
    SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFontSet,
    SoftwareTextMeasurer,
};
use cranpose_ui::text::{
    text_style_for_draw_style, AnnotatedString, TextMeasurer, TextStyle as UiTextStyle,
};
use cranpose_ui_graphics::{Color, Rect, TextStyle};

const GLYPH_CACHE_CAPACITY: usize = 512;

fn fonts() -> SoftwareTextFontSet {
    software_text_font_set_from_fonts_or_default(&[])
}

fn measurer() -> SoftwareTextMeasurer {
    SoftwareTextMeasurer::from_font_set(fonts(), 256)
}

/// The block box a draw scope would emit for `text`: what the production
/// measurer reports, laid out at `origin`.
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

/// Runs the atlas collector exactly as the renderer does for a text draw.
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

    // Glyph placements are relative to the block's own origin, so the box the
    // caller was told about is 0..width by 0..height here. A one-pixel margin
    // absorbs anti-aliased overhang and the rasterizer's integer flooring.
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
    // The case a game hits every frame: same digits, one of them ticking.
    let style = TextStyle::new(18.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    for score in 0..10 {
        let text = format!("SCORE {score}");
        let (rect, ui_style) = measured_block((0.0, 0.0), &text, &style);
        collect(rect, &text, &ui_style, &style, &mut cache);
    }
    // 'S', 'C', 'O', 'R', 'E', ' ' and ten digits — the shared prefix is
    // rasterized once, not once per frame.
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
    // Private-use codepoints have no outline in any shipped face.
    let text = "A\u{E000}\u{10FFFD}B";
    let style = TextStyle::new(20.0);
    let (rect, ui_style) = measured_block((0.0, 0.0), text, &style);
    assert!(rect.width > 0.0 && rect.height > 0.0);

    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(rect, text, &ui_style, &style, &mut cache);
    // The two drawable letters still make it through; the missing glyphs are
    // simply skipped rather than taking the run down with them.
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
    // 'H' sits on the baseline with no descender, so its ink bottom is the
    // baseline row (within the rasterizer's integer flooring).
    let ink_bottom = (placement.y + placement.height as i32) as f32;
    assert!(
        (ink_bottom - baseline).abs() <= 1.5,
        "cap-height ink ends at {ink_bottom}, reported baseline is {baseline}"
    );
}

#[test]
fn letter_spacing_moves_the_glyphs_it_widened_the_block_for() {
    // A style that measures wide and draws narrow leaves centred text visibly
    // off-centre, because the caller centres the box measurement reported.
    let plain = TextStyle::new(20.0);
    let tracked = TextStyle::new(20.0).with_letter_spacing(5.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut spans = |style: &TextStyle| {
        let (rect, ui_style) = measured_block((0.0, 0.0), "ORBIT", style);
        let run = collect(rect, "ORBIT", &ui_style, style, &mut cache);
        let ink = run
            .iter()
            .map(|glyph| {
                let placement = glyph.placement();
                placement.x as f32 + placement.width as f32
            })
            .fold(0.0_f32, f32::max);
        (rect.width, ink)
    };

    let (plain_width, plain_ink) = spans(&plain);
    let (tracked_width, tracked_ink) = spans(&tracked);

    let gaps = "ORBIT".chars().count() as f32 - 1.0;
    assert!((tracked_width - plain_width - gaps * 5.0).abs() < 0.5);
    assert!(
        (tracked_ink - plain_ink - gaps * 5.0).abs() < 2.0,
        "ink grew by {} where the block grew by {}",
        tracked_ink - plain_ink,
        tracked_width - plain_width
    );
}
