use cranpose_render_common::{
    font_source::SoftwareTextFontRegistry,
    software_text_raster::{
        SoftwareGlyphRasterCache, SoftwareTextFontSet, SoftwareTextMeasurer,
        rasterize_annotated_text_to_image_with_glyph_cache,
    },
};
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontStyle, FontSynthesis, FontWeight, SpanStyle, TextMeasurer,
    TextStyle, TextUnit,
};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect};

const REGULAR: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");
const BOLD: &[u8] = include_bytes!("../assets/NotoSansBold.ttf");

fn font_set(faces: &[(u16, &'static [u8])]) -> SoftwareTextFontSet {
    let mut registry = SoftwareTextFontRegistry::new();
    for &(weight, bytes) in faces {
        registry
            .register_face_bytes(
                &FontFamily::named("Weight matching"),
                FontWeight(weight),
                FontStyle::Normal,
                bytes,
            )
            .expect("test font registration");
    }
    registry.into_font_set(&[])
}

fn output(fonts: &SoftwareTextFontSet, requested: u16) -> (ImageBitmap, f32) {
    let text = AnnotatedString::from("Trading workspace 264.000");
    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(24.0),
            font_family: Some(FontFamily::named("Weight matching")),
            font_weight: Some(FontWeight(requested)),
            font_synthesis: Some(FontSynthesis::None),
            ..Default::default()
        },
        ..Default::default()
    };
    let measured = SoftwareTextMeasurer::from_font_set(fonts.clone(), 16).measure(&text, &style);
    let image = rasterize_annotated_text_to_image_with_glyph_cache(
        &text,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 48.0,
        },
        &style,
        Color::BLACK,
        24.0,
        1.0,
        fonts,
        &mut SoftwareGlyphRasterCache::with_capacity_at_least_one(128),
    )
    .expect("visible glyphs");
    (image, measured.width)
}

fn assert_rendered_weight(requested: u16, first: u16, second: u16, expected: u16) {
    let faces = [(first, REGULAR), (second, BOLD)];
    let expected_face = if expected == first {
        faces[0]
    } else {
        faces[1]
    };
    let unexpected_face = if expected == first {
        faces[1]
    } else {
        faces[0]
    };
    let expected_output = output(&font_set(&[expected_face]), requested);
    let wrong_output = output(&font_set(&[unexpected_face]), requested);
    assert!(expected_output.0.pixels() != wrong_output.0.pixels());
    assert_ne!(expected_output.1, wrong_output.1);
    for ordered_faces in [faces, [faces[1], faces[0]]] {
        let actual = output(&font_set(&ordered_faces), requested);
        assert!(
            actual.0.pixels() == expected_output.0.pixels(),
            "weight {requested} must render as {expected} among {first}/{second}"
        );
        assert_eq!(
            actual.1, expected_output.1,
            "measurement must use the rendered face"
        );
    }
}

#[test]
fn heavy_requests_search_heavier_faces_before_lighter_faces() {
    for (requested, first, second, expected) in [
        (600, 500, 700, 700),
        (600, 500, 900, 900),
        (800, 500, 700, 700),
        (700, 500, 700, 700),
    ] {
        assert_rendered_weight(requested, first, second, expected);
    }
}

#[test]
fn light_requests_search_lighter_faces_before_heavier_faces() {
    for (requested, first, second, expected) in [
        (350, 100, 400, 100),
        (100, 300, 400, 300),
        (300, 300, 400, 300),
    ] {
        assert_rendered_weight(requested, first, second, expected);
    }
}

#[test]
fn regular_interval_searches_up_to_medium_then_lighter_then_heavier() {
    for (requested, first, second, expected) in [
        (400, 300, 500, 500),
        (450, 400, 500, 500),
        (450, 300, 600, 300),
        (500, 400, 600, 400),
        (450, 600, 700, 600),
        (400, 400, 500, 400),
        (500, 400, 500, 500),
    ] {
        assert_rendered_weight(requested, first, second, expected);
    }
}
