//! Variable fonts registered at several weights.
//!
//! Android backs `sans-serif` with one variable `Roboto-Regular.ttf` and
//! describes every weight of the family as a `wght` axis position on it, so a
//! family that instances a single file has to produce genuinely different
//! outlines per weight — otherwise a Compose port asking for Medium draws
//! Regular. The demo bundles a variable face, which is what this exercises;
//! `cranpose-render-common` ships only static faces of its own.

use std::path::PathBuf;

use cranpose_render_common::font_source::SoftwareTextFontRegistry;
use cranpose_render_common::software_text_raster::{
    collect_solid_text_atlas_run, SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache,
    SoftwareTextFontSet, SoftwareTextMeasurer,
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontFile, FontStyle, FontWeight, SpanStyle, TextMeasurer,
    TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, Rect};

const FONT_SIZE: f32 = 32.0;

fn variable_font_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/leetcodedaily/fonts/MonaspaceKryptonVarVF.ttf")
        .to_string_lossy()
        .into_owned()
}

/// One variable file declared at three weights — the shape
/// `register_system_family` produces for Android's `sans-serif`.
fn instanced_family() -> FontFamily {
    let path = variable_font_path();
    FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path.clone()).with_weight(FontWeight::MEDIUM),
        FontFile::new(path).with_weight(FontWeight::BOLD),
    ])
    .expect("a family needs at least one file")
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

fn font_set(family: &FontFamily) -> SoftwareTextFontSet {
    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_family(family)
        .expect("the demo's variable font must load");
    registry.into_font_set_or_default(&[])
}

/// Total glyph coverage of a run — the signal that outlines actually got
/// heavier, which is what a monospaced variable face changes without changing
/// its advances.
fn ink_area(fonts: &SoftwareTextFontSet, text: &str, style: &TextStyle) -> usize {
    let metrics =
        SoftwareTextMeasurer::from_font_set(fonts.clone(), 64).measure(&text.into(), style);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: metrics.width,
        height: metrics.line_count.max(1) as f32 * metrics.line_height,
    };
    let mut cache = SoftwareGlyphRasterCache::with_capacity_at_least_one(256);
    let mut run: Vec<SoftwareGlyphAtlasRunGlyph> = Vec::new();
    collect_solid_text_atlas_run(
        &AnnotatedString::from(text),
        rect,
        style,
        Color::WHITE,
        FONT_SIZE,
        1.0,
        fonts,
        &mut cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run.iter()
        .map(|glyph| {
            let placement = glyph.placement();
            placement.width * placement.height
        })
        .sum()
}

#[test]
fn one_variable_file_draws_heavier_outlines_at_each_registered_weight() {
    let family = instanced_family();
    let fonts = font_set(&family);

    let regular = ink_area(&fonts, "Weighted", &style_for(&family, FontWeight::NORMAL));
    let medium = ink_area(&fonts, "Weighted", &style_for(&family, FontWeight::MEDIUM));
    let bold = ink_area(&fonts, "Weighted", &style_for(&family, FontWeight::BOLD));

    assert!(
        medium > regular && bold > medium,
        "the wght axis must be instanced per registered weight: \
         regular={regular} medium={medium} bold={bold}"
    );
}

#[test]
fn instances_of_one_variable_file_do_not_share_glyph_cache_entries() {
    let family = instanced_family();
    let fonts = font_set(&family);

    let regular = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("regular instance");
    let bold = fonts
        .resolve(&style_for(&family, FontWeight::BOLD))
        .expect("bold instance");

    assert_ne!(
        regular.content_hash(),
        bold.content_hash(),
        "instances share bytes but not outlines, so they must key the glyph atlas apart"
    );
}

#[test]
fn a_static_face_declared_at_a_weight_it_does_not_have_keeps_its_own_outlines() {
    // No `wght` axis to instance, so the declaration only affects matching —
    // the outlines are whatever the file holds, and synthesis takes over from
    // there. Registering the same static file twice must therefore reuse one
    // set of glyph masks rather than duplicating the atlas.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets/leetcodedaily/fonts/DejaVuSans.ttf")
        .to_string_lossy()
        .into_owned();
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path).with_weight(FontWeight::BOLD),
    ])
    .expect("a family needs at least one file");

    let fonts = font_set(&family);
    let regular = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("regular face");
    let bold = fonts
        .resolve(&style_for(&family, FontWeight::BOLD))
        .expect("declared bold face");

    assert_eq!(regular.weight(), FontWeight::NORMAL);
    assert_eq!(bold.weight(), FontWeight::BOLD);
    assert_eq!(
        regular.content_hash(),
        bold.content_hash(),
        "a static file instanced no differently must not duplicate glyph masks"
    );
}

#[test]
fn an_italic_face_of_a_variable_family_instances_the_slant_axis() {
    let path = variable_font_path();
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path).with_style(FontStyle::Italic),
    ])
    .expect("a family needs at least one file");

    let fonts = font_set(&family);
    let italic_style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(FONT_SIZE),
            font_family: Some(family.clone()),
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        },
        ..Default::default()
    };
    let upright = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("upright face");
    let italic = fonts.resolve(&italic_style).expect("italic face");

    assert_eq!(italic.style(), FontStyle::Italic);
    assert_ne!(
        upright.content_hash(),
        italic.content_hash(),
        "this face carries a `slnt` axis, so its italic is real rather than synthesized"
    );
    assert!(
        ink_area(&fonts, "Slanted", &italic_style)
            > ink_area(&fonts, "Slanted", &style_for(&family, FontWeight::NORMAL)),
        "leaning the outlines must widen their coverage"
    );
}
