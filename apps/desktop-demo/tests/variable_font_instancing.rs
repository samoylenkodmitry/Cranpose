use std::path::PathBuf;

use cranpose_render_common::{
    font_source::FontRegistry,
    glyph_mask::GlyphMaskCache,
    text_atlas::{collect_solid_text_atlas_run, AtlasGlyph},
    text_fonts::TextFonts,
    text_raster::TextRasterRequest,
};
use cranpose_text::{FontRequest, Typeface};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontFile, FontStyle, FontWeight, SpanStyle, TextLayoutOptions,
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

fn font_set(family: &FontFamily) -> TextFonts {
    let mut registry = FontRegistry::new();
    registry
        .register_family(family)
        .expect("the demo's variable font must load");
    TextFonts::new(registry.into_typefaces(&[]))
}

// The typeface `fonts` draw `family` at `weight` with.
fn resolved(fonts: &TextFonts, family: &FontFamily, weight: FontWeight) -> Typeface {
    fonts
        .resolver()
        .resolve(FontRequest {
            family,
            weight,
            style: FontStyle::Normal,
        })
        .cloned()
        .expect("the family resolves")
}

fn ink_area(fonts: &TextFonts, text: &str, style: &TextStyle) -> usize {
    let text = AnnotatedString::from(text);
    let prepared = cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts.resolver());
        cranpose_ui::text::prepare_text_layout(&text, style, TextLayoutOptions::default(), None)
    });
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: prepared.metrics.width,
        height: prepared.metrics.height,
    };
    let mut cache = GlyphMaskCache::with_capacity_at_least_one(256);
    let mut run: Vec<AtlasGlyph> = Vec::new();
    collect_solid_text_atlas_run(
        TextRasterRequest::new(&prepared.layout, &text, rect, style, Color::WHITE, 1.0),
        &mut cache,
        &mut run,
    )
    .expect("a solid single-style run must be atlasable");
    run.iter().map(|glyph| glyph.width * glyph.height).sum()
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

    let regular = resolved(&fonts, &family, FontWeight::NORMAL);
    let bold = resolved(&fonts, &family, FontWeight::BOLD);

    assert_ne!(
        regular.content_hash(),
        bold.content_hash(),
        "instances share bytes but not outlines, so they must key the glyph atlas apart"
    );
}

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

fn system_font_set(dir: &SystemFontDir, weight: FontWeight) -> TextFonts {
    let mut registry = FontRegistry::new();
    registry
        .register_system_face(&dir.0, &FontFamily::SansSerif, weight, FontStyle::Normal)
        .expect("the system face must load");
    TextFonts::new(registry.into_typefaces(&[]))
}

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

#[test]
fn an_app_supplied_variable_face_still_instances_an_arbitrary_weight() {
    let path = variable_font_path();
    let family = FontFamily::file_backed(vec![
        FontFile::new(path.clone()),
        FontFile::new(path).with_weight(FontWeight(450)),
    ])
    .expect("a family needs at least one file");

    let fonts = font_set(&family);
    let regular = resolved(&fonts, &family, FontWeight::NORMAL);
    let off_grid = resolved(&fonts, &family, FontWeight(450));

    assert_eq!(off_grid.weight(), FontWeight(450));
    assert_ne!(
        regular.content_hash(),
        off_grid.content_hash(),
        "an app's own variable font must still instance at the weight it declares"
    );
}

#[test]
fn a_static_face_declared_at_a_weight_it_does_not_have_keeps_its_own_outlines() {
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
    let regular = resolved(&fonts, &family, FontWeight::NORMAL);
    let bold = resolved(&fonts, &family, FontWeight::BOLD);

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
    let upright = resolved(&fonts, &family, FontWeight::NORMAL);
    let italic = fonts
        .resolver()
        .resolve(FontRequest {
            family: &family,
            weight: FontWeight::NORMAL,
            style: FontStyle::Italic,
        })
        .cloned()
        .expect("italic face");

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
