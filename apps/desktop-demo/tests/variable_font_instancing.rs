//! Variable fonts registered at several weights.
//!
//! Android backs `sans-serif` with one variable `Roboto-Regular.ttf` and
//! describes every weight of the family as a `wght` axis position on it, so a
//! family that instances a single file has to produce genuinely different
//! outlines per weight — otherwise a Compose port asking for Medium draws
//! Regular. The demo bundles a variable face, which is what this exercises;
//! `cranpose-render-common` ships only static faces of its own.

use std::path::PathBuf;

use cranpose_render_common::{
    font_source::SoftwareTextFontRegistry,
    software_text_raster::{
        collect_solid_text_atlas_run, SoftwareGlyphAtlasRunGlyph, SoftwareGlyphRasterCache,
        SoftwareTextFontSet, SoftwareTextMeasurer,
    },
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

/// A directory shaped like `/system/fonts`, holding the variable file under the
/// name Android's `sans-serif` names.
struct SystemFontDir(PathBuf);

impl SystemFontDir {
    fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-output/cranpose-system-font-weights")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        std::fs::copy(variable_font_path(), path.join("Roboto-Regular.ttf"))
            .expect("the demo's variable font must copy");
        Self(path)
    }
}

impl Drop for SystemFontDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn system_font_set(dir: &SystemFontDir, weight: FontWeight) -> SoftwareTextFontSet {
    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_system_face(&dir.0, &FontFamily::SansSerif, weight, FontStyle::Normal)
        .expect("the system face must load");
    registry.into_font_set_or_default(&[])
}

/// The defect this guards against: Wear Material 3 asks `sans-serif` for body
/// 450 and title 550, `/system/etc/fonts.xml` declares the family only in
/// hundreds, and Minikin resolves both to the hundred below. A registry that
/// instead instanced the `wght` axis at 450 drew a face the platform cannot
/// draw — measured at +12.3% ink per glyph at 24 px on a Wear device, text that
/// measures and lays out perfectly and is simply heavier than every other app.
#[test]
fn a_system_weight_the_font_config_does_not_declare_draws_the_declared_face() {
    let dir = SystemFontDir::new("sans-serif");
    let family = FontFamily::SansSerif;

    let regular = ink_area(
        &system_font_set(&dir, FontWeight::NORMAL),
        "Weighted",
        &style_for(&family, FontWeight::NORMAL),
    );
    let medium = ink_area(
        &system_font_set(&dir, FontWeight::MEDIUM),
        "Weighted",
        &style_for(&family, FontWeight::MEDIUM),
    );
    // Asked for off-grid; must draw exactly what the platform's matcher returns.
    let off_grid = ink_area(
        &system_font_set(&dir, FontWeight(450)),
        "Weighted",
        &style_for(&family, FontWeight(450)),
    );

    assert!(
        medium > regular,
        "the fixture must have a wght axis worth instancing, \
         or this test cannot fail: regular={regular} medium={medium}"
    );
    assert_eq!(
        off_grid, regular,
        "450 must draw the 400 entry: regular={regular} medium={medium} off_grid={off_grid}"
    );
}

/// The same request against an app's own file must keep instancing freely — a
/// variable axis belongs to the font, and only the platform's aliases are
/// limited to the platform's declared entries.
#[test]
fn an_app_supplied_variable_face_still_instances_an_arbitrary_weight() {
    let path = variable_font_path();
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path).with_weight(FontWeight(450)),
    ])
    .expect("a family needs at least one file");

    let fonts = font_set(&family);
    let regular = fonts
        .resolve(&style_for(&family, FontWeight::NORMAL))
        .expect("regular face");
    let off_grid = fonts
        .resolve(&style_for(&family, FontWeight(450)))
        .expect("off-grid face");

    assert_eq!(off_grid.weight(), FontWeight(450));
    assert_ne!(
        regular.content_hash(),
        off_grid.content_hash(),
        "an app's own variable font must still instance at the weight it declares"
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
