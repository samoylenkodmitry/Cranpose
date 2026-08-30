#![cfg(feature = "embedded-default-font")]

use std::path::{Path, PathBuf};

use cranpose_render_common::{
    font_source::SoftwareTextFontRegistry,
    software_text_raster::{
        SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache, SoftwareTextFontSet,
        SoftwareTextMeasurer, collect_solid_text_atlas_run,
    },
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontFile, FontWeight, SpanStyle, TextMeasurer, TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, Rect};

const GLYPH_CACHE_CAPACITY: usize = 512;
const FONT_SIZE: f32 = 20.0;
const REGULAR: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");
const BOLD: &[u8] = include_bytes!("../assets/NotoSansBold.ttf");

fn font_dir(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/test-output/cranpose-app-fonts")
        .join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("font directory");
    path
}

fn write_font(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("font file");
    path.to_string_lossy().into_owned()
}

fn app_family(dir: &Path) -> FontFamily {
    FontFamily::file_backed(vec![
        FontFile::new(write_font(dir, "App-Regular.ttf", REGULAR)),
        FontFile::new(write_font(dir, "App-Bold.ttf", BOLD)).with_weight(FontWeight::BOLD),
    ])
    .expect("a family needs at least one file")
}

fn font_set(family: &FontFamily) -> SoftwareTextFontSet {
    let mut registry = SoftwareTextFontRegistry::new();
    let _ = registry.register_family(family);
    registry.into_font_set_or_default(&[])
}

fn style_for(family: &FontFamily, weight: FontWeight) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            font_family: Some(family.clone()),
            font_weight: Some(weight),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn measured_block(fonts: &SoftwareTextFontSet, text: &str, style: &TextStyle) -> Rect {
    let metrics =
        SoftwareTextMeasurer::from_font_set(fonts.clone(), 256).measure(&text.into(), style);
    Rect {
        x: 0.0,
        y: 0.0,
        width: metrics.width,
        height: metrics.line_count.max(1) as f32 * metrics.line_height,
    }
}

fn collect(
    fonts: &SoftwareTextFontSet,
    rect: Rect,
    text: &str,
    style: &TextStyle,
    cache: &mut SoftwareGlyphRasterCache,
) -> Vec<SoftwareGlyphAtlasRunGlyph> {
    let mut run = Vec::new();
    collect_solid_text_atlas_run(
        &AnnotatedString::from(text),
        rect,
        style,
        Color::WHITE,
        FONT_SIZE,
        1.0,
        fonts,
        cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run
}

fn ink_width(run: &[SoftwareGlyphAtlasRunGlyph]) -> f32 {
    run.iter()
        .map(|glyph| {
            let placement = glyph.placement();
            placement.x as f32 + placement.width as f32
        })
        .fold(0.0_f32, f32::max)
}

#[test]
fn an_app_supplied_family_draws_in_the_face_it_was_measured_with() {
    let dir = font_dir("measure-agrees");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);

    let rect = measured_block(&fonts, "SCORE 1234", &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(&fonts, rect, "SCORE 1234", &style, &mut cache);
    assert!(!run.is_empty(), "the app face must produce glyphs");

    const OVERHANG: f32 = 1.0;
    for glyph in &run {
        let placement = glyph.placement();
        let right = (placement.x + placement.width as i32) as f32;
        let bottom = (placement.y + placement.height as i32) as f32;
        assert!(
            placement.x as f32 >= -OVERHANG && right <= rect.width + OVERHANG,
            "glyph spans x {}..{right}, measured block is 0..{}",
            placement.x,
            rect.width
        );
        assert!(
            placement.y as f32 >= -OVERHANG && bottom <= rect.height + OVERHANG,
            "glyph spans y {}..{bottom}, measured block is 0..{}",
            placement.y,
            rect.height
        );
    }
}

#[test]
fn the_requested_weight_picks_its_own_face_for_both_measuring_and_drawing() {
    let dir = font_dir("weights");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let regular = style_for(&family, FontWeight::NORMAL);
    let bold = style_for(&family, FontWeight::BOLD);

    let regular_rect = measured_block(&fonts, "Weighted", &regular);
    let bold_rect = measured_block(&fonts, "Weighted", &bold);
    assert!(
        bold_rect.width > regular_rect.width,
        "the bold face must measure wider: regular={regular_rect:?} bold={bold_rect:?}"
    );

    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let regular_ink = ink_width(&collect(
        &fonts,
        regular_rect,
        "Weighted",
        &regular,
        &mut cache,
    ));
    let bold_ink = ink_width(&collect(&fonts, bold_rect, "Weighted", &bold, &mut cache));
    assert!(
        bold_ink > regular_ink,
        "the drawn run must follow the measured face: regular={regular_ink} bold={bold_ink}"
    );
}

#[test]
fn a_stable_string_in_an_app_supplied_font_rasterizes_once() {
    let dir = font_dir("caching");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);
    let rect = measured_block(&fonts, "FPS 60", &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    let first = collect(&fonts, rect, "FPS 60", &style, &mut cache);
    assert!(
        first
            .iter()
            .any(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::New(_))),
        "the first frame has to rasterize"
    );
    let entries_after_first = cache.stats().entries;

    for _ in 0..60 {
        let frame = collect(&fonts, rect, "FPS 60", &style, &mut cache);
        assert!(
            frame
                .iter()
                .all(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::Cached(_))),
            "an unchanged string must not re-rasterize a single glyph"
        );
    }
    assert_eq!(
        cache.stats().entries,
        entries_after_first,
        "60 more frames must not add a mask to the cache"
    );
}

#[test]
fn the_two_weights_of_one_family_keep_separate_glyph_cache_entries() {
    let dir = font_dir("cache-keys");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let regular = style_for(&family, FontWeight::NORMAL);
    let bold = style_for(&family, FontWeight::BOLD);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    collect(
        &fonts,
        measured_block(&fonts, "AB", &regular),
        "AB",
        &regular,
        &mut cache,
    );
    let after_regular = cache.stats().entries;
    let bold_run = collect(
        &fonts,
        measured_block(&fonts, "AB", &bold),
        "AB",
        &bold,
        &mut cache,
    );

    assert!(
        bold_run
            .iter()
            .any(|glyph| matches!(glyph, SoftwareGlyphAtlasRunGlyph::New(_))),
        "the bold face draws different outlines and must not reuse the regular masks"
    );
    assert!(cache.stats().entries > after_regular);
}

#[test]
fn a_family_whose_files_are_missing_still_draws_in_the_fallback_face() {
    let dir = font_dir("missing");
    let family = FontFamily::file_backed(vec![FontFile::new(
        dir.join("Absent.ttf").to_string_lossy().into_owned(),
    )])
    .expect("a family needs at least one file");
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);

    let rect = measured_block(&fonts, "Fallback", &style);
    assert!(
        rect.width > 0.0,
        "an unloadable family must still measure against the fallback font"
    );
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(
        !collect(&fonts, rect, "Fallback", &style, &mut cache).is_empty(),
        "an unloadable family must still draw with the fallback font"
    );
}

#[test]
fn a_corrupt_font_file_leaves_the_family_on_the_fallback_face() {
    let dir = font_dir("corrupt");
    let family = FontFamily::file_backed(vec![FontFile::new(write_font(
        &dir,
        "Corrupt.ttf",
        b"this is not a font",
    ))])
    .expect("a family needs at least one file");
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);

    let rect = measured_block(&fonts, "Corrupt", &style);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(rect.width > 0.0);
    assert!(!collect(&fonts, rect, "Corrupt", &style, &mut cache).is_empty());
}

#[test]
fn characters_the_app_face_cannot_draw_measure_and_collect_without_panicking() {
    let dir = font_dir("missing-glyphs");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);
    let text = "A\u{E000}\u{10FFFD}B";

    let rect = measured_block(&fonts, text, &style);
    assert!(rect.width > 0.0 && rect.height > 0.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(&fonts, rect, text, &style, &mut cache);
    assert!(!run.is_empty());
    for glyph in &run {
        let placement = glyph.placement();
        assert!(placement.width > 0 && placement.height > 0);
    }
}

#[test]
fn an_app_that_supplies_nothing_keeps_the_embedded_fallback() {
    let fonts = SoftwareTextFontRegistry::new().into_font_set_or_default(&[]);
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            ..Default::default()
        },
        ..Default::default()
    };

    let rect = measured_block(&fonts, "Fallback", &style);
    assert!(rect.width > 0.0);
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(!collect(&fonts, rect, "Fallback", &style, &mut cache).is_empty());
}
