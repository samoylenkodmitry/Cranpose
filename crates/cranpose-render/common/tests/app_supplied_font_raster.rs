#![cfg(feature = "embedded-default-font")]

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use cranpose_render_common::{
    font_source::FontRegistry,
    glyph_mask::GlyphMaskCache,
    text_atlas::{AtlasGlyph, collect_solid_text_atlas_run},
    text_fonts::{TextFonts, default_typeface},
    text_raster::TextRasterRequest,
};
use cranpose_ui::{
    PreparedTextLayout,
    text::{
        AnnotatedString, FontFamily, FontFile, FontWeight, SpanStyle, TextLayoutOptions, TextStyle,
        TextUnit,
    },
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

fn font_set(family: &FontFamily) -> TextFonts {
    font_set_beside(family, &[])
}

/// `family` registered beside faces the app handed over as bytes.
fn font_set_beside(family: &FontFamily, bytes: &[&'static [u8]]) -> TextFonts {
    let mut registry = FontRegistry::new();
    let _ = registry.register_family(family);
    TextFonts::new(registry.into_typefaces(bytes))
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

// `text` in `style` laid out with `fonts`, and the block it measures.
struct Measured {
    rect: Rect,
    prepared: Rc<PreparedTextLayout>,
}

fn measured_block(fonts: &TextFonts, text: &str, style: &TextStyle) -> Measured {
    let prepared = cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts.resolver());
        cranpose_ui::text::prepare_text_layout(
            &AnnotatedString::from(text),
            style,
            TextLayoutOptions::default(),
            None,
        )
    });
    Measured {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: prepared.metrics().width,
            height: prepared.metrics().height,
        },
        prepared,
    }
}

fn collect(
    measured: &Measured,
    text: &str,
    style: &TextStyle,
    cache: &mut GlyphMaskCache,
) -> Vec<AtlasGlyph> {
    let mut run = Vec::new();
    collect_solid_text_atlas_run(
        TextRasterRequest::new(
            &measured.prepared.layout,
            text,
            measured.rect,
            style,
            Color::WHITE,
            1.0,
        ),
        cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run
}

fn ink_width(run: &[AtlasGlyph]) -> f32 {
    run.iter()
        .map(|glyph| glyph.x as f32 + glyph.width as f32)
        .fold(0.0_f32, f32::max)
}

#[test]
fn an_app_supplied_family_draws_in_the_face_it_was_measured_with() {
    let dir = font_dir("measure-agrees");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);

    let block = measured_block(&fonts, "SCORE 1234", &style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(&block, "SCORE 1234", &style, &mut cache);
    assert!(!run.is_empty(), "the app face must produce glyphs");

    const OVERHANG: f32 = 1.0;
    for glyph in &run {
        let right = (glyph.x + i32::try_from(glyph.width).unwrap_or(i32::MAX)) as f32;
        let bottom = (glyph.y + i32::try_from(glyph.height).unwrap_or(i32::MAX)) as f32;
        assert!(
            glyph.x as f32 >= -OVERHANG && right <= block.rect.width + OVERHANG,
            "glyph spans x {}..{right}, measured block is 0..{}",
            glyph.x,
            block.rect.width
        );
        assert!(
            glyph.y as f32 >= -OVERHANG && bottom <= block.rect.height + OVERHANG,
            "glyph spans y {}..{bottom}, measured block is 0..{}",
            glyph.y,
            block.rect.height
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

    let regular_block = measured_block(&fonts, "Weighted", &regular);
    let bold_block = measured_block(&fonts, "Weighted", &bold);
    assert!(
        bold_block.rect.width > regular_block.rect.width,
        "the bold face must measure wider: regular={:?} bold={:?}",
        regular_block.rect,
        bold_block.rect
    );

    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let regular_ink = ink_width(&collect(&regular_block, "Weighted", &regular, &mut cache));
    let bold_ink = ink_width(&collect(&bold_block, "Weighted", &bold, &mut cache));
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
    let block = measured_block(&fonts, "FPS 60", &style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    let first = collect(&block, "FPS 60", &style, &mut cache);
    assert!(!first.is_empty());
    let after_first = cache.stats();
    assert!(after_first.misses > 0, "the first frame has to rasterize");

    for _ in 0..60 {
        collect(&block, "FPS 60", &style, &mut cache);
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
fn the_two_weights_of_one_family_keep_separate_glyph_cache_entries() {
    let dir = font_dir("cache-keys");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let regular = style_for(&family, FontWeight::NORMAL);
    let bold = style_for(&family, FontWeight::BOLD);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);

    collect(
        &measured_block(&fonts, "AB", &regular),
        "AB",
        &regular,
        &mut cache,
    );
    let after_regular = cache.stats();
    let bold_run = collect(
        &measured_block(&fonts, "AB", &bold),
        "AB",
        &bold,
        &mut cache,
    );

    assert!(!bold_run.is_empty());
    assert!(
        cache.stats().misses > after_regular.misses,
        "the bold face draws different outlines and must not reuse the regular masks"
    );
    assert!(cache.stats().entries > after_regular.entries);
}

#[test]
fn a_family_whose_files_are_missing_still_draws_in_another_app_face() {
    let dir = font_dir("missing");
    let family = FontFamily::file_backed(vec![FontFile::new(
        dir.join("Absent.ttf").to_string_lossy().into_owned(),
    )])
    .expect("a family needs at least one file");
    let fonts = font_set_beside(&family, &[REGULAR]);
    let style = style_for(&family, FontWeight::NORMAL);

    let block = measured_block(&fonts, "Fallback", &style);
    assert!(
        block.rect.width > 0.0,
        "an unloadable family must still measure against the fallback font"
    );
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(
        !collect(&block, "Fallback", &style, &mut cache).is_empty(),
        "an unloadable family must still draw with the fallback font"
    );
}

#[test]
fn a_corrupt_font_file_leaves_the_family_on_another_app_face() {
    let dir = font_dir("corrupt");
    let family = FontFamily::file_backed(vec![FontFile::new(write_font(
        &dir,
        "Corrupt.ttf",
        b"this is not a font",
    ))])
    .expect("a family needs at least one file");
    let fonts = font_set_beside(&family, &[REGULAR]);
    let style = style_for(&family, FontWeight::NORMAL);

    let block = measured_block(&fonts, "Corrupt", &style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(block.rect.width > 0.0);
    assert!(!collect(&block, "Corrupt", &style, &mut cache).is_empty());
}

#[test]
fn characters_the_app_face_cannot_draw_measure_and_collect_without_panicking() {
    let dir = font_dir("missing-glyphs");
    let family = app_family(&dir);
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);
    let text = "A\u{E000}\u{10FFFD}B";

    let block = measured_block(&fonts, text, &style);
    assert!(block.rect.width > 0.0 && block.rect.height > 0.0);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(&block, text, &style, &mut cache);
    assert!(!run.is_empty());
    for glyph in &run {
        assert!(glyph.width > 0 && glyph.height > 0);
    }
}

#[test]
fn the_embedded_face_measures_and_draws_on_its_own() {
    let fonts = TextFonts::new(vec![default_typeface().expect("the embedded face parses")]);
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            ..Default::default()
        },
        ..Default::default()
    };

    let block = measured_block(&fonts, "Fallback", &style);
    assert!(block.rect.width > 0.0);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    assert!(!collect(&block, "Fallback", &style, &mut cache).is_empty());
}

/// One glyph of [`full_glyph_count_font`]: a square of 800 units.
const SQUARE: char = '\u{4E00}';

#[test]
fn a_font_with_the_most_glyphs_a_font_can_hold_draws_from_a_file() {
    let dir = font_dir("full-glyph-count");
    let family = FontFamily::file_backed(vec![FontFile::new(write_font(
        &dir,
        "Full.ttf",
        &full_glyph_count_font(),
    ))])
    .expect("a family needs at least one file");
    let fonts = font_set(&family);
    let style = style_for(&family, FontWeight::NORMAL);

    assert_draws_the_square(&fonts, &style);
}

#[test]
fn a_font_with_the_most_glyphs_a_font_can_hold_draws_from_bytes() {
    let bytes: &'static [u8] = Box::leak(full_glyph_count_font().into_boxed_slice());
    let fonts = TextFonts::new(FontRegistry::new().into_typefaces(&[bytes]));
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            ..Default::default()
        },
        ..Default::default()
    };

    assert_draws_the_square(&fonts, &style);
}

#[test]
fn a_font_with_the_most_glyphs_a_font_can_hold_draws_each_weight() {
    let dir = font_dir("full-glyph-count-weights");
    let path = write_font(&dir, "Full.ttf", &full_glyph_count_font());
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path).with_weight(FontWeight::BOLD),
    ])
    .expect("a family needs at least one file");
    let fonts = font_set(&family);
    let text = SQUARE.to_string();
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let mut ink = |weight| {
        let style = style_for(&family, weight);
        let block = measured_block(&fonts, &text, &style);
        collect(&block, &text, &style, &mut cache)
            .iter()
            .map(|glyph| glyph.width)
            .max()
            .unwrap_or(0)
    };

    let regular = ink(FontWeight::NORMAL);
    let bold = ink(FontWeight::BOLD);
    // 1,025 and 1,250 units of an em of 1000 at 20 px, about 21 and 25 px;
    // the default instance draws 800 units, 16 px, for both.
    assert!(
        bold >= regular + 3,
        "the bold instance must draw wider: regular={regular} bold={bold}"
    );
}

fn assert_draws_the_square(fonts: &TextFonts, style: &TextStyle) {
    let text = SQUARE.to_string();
    let block = measured_block(fonts, &text, style);
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(GLYPH_CACHE_CAPACITY);
    let run = collect(&block, &text, style, &mut cache);
    // 800 units of an em of 1000 at 20 px.
    assert!(
        run.iter()
            .any(|glyph| glyph.width >= 15 && glyph.height >= 15),
        "the square must draw: {:?}",
        run.iter()
            .map(|glyph| (glyph.width, glyph.height))
            .collect::<Vec<_>>()
    );
}

/// A variable TrueType font with 65,535 glyphs, the most its `maxp` table can
/// count: glyph 1 is a square that [`SQUARE`] maps to and that widens along
/// the weight axis, from 800 units at its default weight of 100; the others
/// are empty.
fn full_glyph_count_font() -> Vec<u8> {
    const GLYPHS: u16 = u16::MAX;
    fn be16(out: &mut Vec<u8>, values: &[i32]) {
        for value in values {
            out.extend_from_slice(&(*value as u16).to_be_bytes());
        }
    }
    let mut square = Vec::new();
    // One contour, its bounds, the last point's index, no instructions.
    be16(&mut square, &[1, 100, 0, 900, 800, 3, 0]);
    square.extend_from_slice(&[1, 1, 1, 1]);
    be16(&mut square, &[100, 800, 0, -800, 0, 0, 800, 0]);

    let mut loca = Vec::new();
    for glyph in 0..=u32::from(GLYPHS) {
        let start = if glyph < 2 { 0 } else { square.len() as u32 };
        loca.extend_from_slice(&start.to_be_bytes());
    }
    let mut hmtx = Vec::new();
    be16(&mut hmtx, &[1000, 0]);
    hmtx.resize(hmtx.len() + 2 * (usize::from(GLYPHS) - 1), 0);

    let mut head = Vec::new();
    be16(&mut head, &[1, 0, 1, 0, 0, 0, 0x5F0F, 0x3CF5, 0, 1000]);
    head.resize(head.len() + 16, 0);
    be16(&mut head, &[0, -200, 1000, 800, 0, 8, 2, 1, 0]);
    let mut hhea = Vec::new();
    be16(&mut hhea, &[1, 0, 800, -200, 0, 1000, 0, 0, 1000, 1, 0, 0]);
    be16(&mut hhea, &[0, 0, 0, 0, 0, 1]);
    let mut maxp = Vec::new();
    be16(&mut maxp, &[0, 0x5000, i32::from(GLYPHS)]);
    let mut cmap = Vec::new();
    let code = SQUARE as i32;
    be16(&mut cmap, &[0, 1, 3, 1, 0, 12]);
    be16(
        &mut cmap,
        &[4, 32, 0, 4, 4, 1, 0, code, 0xFFFF, 0, code, 0xFFFF],
    );
    be16(&mut cmap, &[1 - code, 1, 0, 0]);

    // One weight axis, 100 to 900 with 100 by default.
    let mut fvar = Vec::new();
    be16(&mut fvar, &[1, 0, 16, 2, 1, 20, 0, 8]);
    fvar.extend_from_slice(b"wght");
    be16(&mut fvar, &[100, 0, 100, 0, 900, 0, 0, 256]);
    // At weight 900 the square's left side moves 300 units left and its right
    // side 300 units right; the four phantom points stay.
    let mut variation = Vec::new();
    // One tuple with its own point numbers, its 19 bytes of point numbers
    // and deltas starting at byte 10, at the axis' end.
    be16(&mut variation, &[1, 10, 19, 0xA000, 0x4000]);
    // A point count of zero: every point moves.
    variation.push(0);
    // Eight x deltas in words, then eight y deltas of zero.
    variation.push(0x40 | 7);
    be16(&mut variation, &[-300, 300, 300, -300, 0, 0, 0, 0]);
    variation.push(0x80 | 7);
    let mut gvar = Vec::new();
    let offsets = 4 * (usize::from(GLYPHS) + 1);
    be16(&mut gvar, &[1, 0, 1, 0, 0, 20, i32::from(GLYPHS), 1]);
    gvar.extend_from_slice(&((20 + offsets) as u32).to_be_bytes());
    for glyph in 0..=u32::from(GLYPHS) {
        let start = if glyph < 2 { 0 } else { variation.len() as u32 };
        gvar.extend_from_slice(&start.to_be_bytes());
    }
    gvar.extend_from_slice(&variation);

    let tables: [(&[u8; 4], Vec<u8>); 9] = [
        (b"cmap", cmap),
        (b"fvar", fvar),
        (b"glyf", square),
        (b"gvar", gvar),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let mut font = Vec::new();
    be16(&mut font, &[1, 0, tables.len() as i32, 0, 0, 0]);
    let mut offset = 12 + 16 * tables.len();
    for (tag, table) in &tables {
        font.extend_from_slice(*tag);
        font.extend_from_slice(&[0; 4]);
        font.extend_from_slice(&(offset as u32).to_be_bytes());
        font.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += table.len().next_multiple_of(4);
    }
    for (_, table) in &tables {
        font.extend_from_slice(table);
        font.resize(font.len().next_multiple_of(4), 0);
    }
    font
}
