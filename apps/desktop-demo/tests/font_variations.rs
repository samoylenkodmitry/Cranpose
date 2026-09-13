use cranpose::text::{FontFamily, FontStyle, FontWeight, TextStyle};
use cranpose_render_common::{
    font_source::SoftwareTextFontRegistry,
    software_text_raster::{
        measure_text_with_font, rasterize_text_to_image, SoftwareTextFont, SoftwareTextFontError,
    },
};
use cranpose_ui_graphics::{Color, Rect, Size};

const FONT: &[u8] = include_bytes!("../assets/leetcodedaily/fonts/MonaspaceKryptonVarVF.ttf");

fn face(axes: &[([u8; 4], f32)]) -> Result<SoftwareTextFont, SoftwareTextFontError> {
    SoftwareTextFont::from_registered_bytes_with_variations(
        &FontFamily::SansSerif,
        FontWeight::MEDIUM,
        FontStyle::Normal,
        FONT,
        axes,
    )
}

#[test]
fn explicit_axes_change_measurement_rasterization_and_cache_identity() {
    let normal = face(&[(*b"wdth", 100.0)]).unwrap();
    let wide = face(&[(*b"wdth", 125.0)]).unwrap();
    let style = TextStyle::default();
    let measure =
        |font: &SoftwareTextFont| measure_text_with_font("AVATAR Browse", &style, 20.0, font).width;
    assert!(measure(&wide) > measure(&normal));
    assert_ne!(normal.content_hash(), wide.content_hash());
    let raster = |font: &SoftwareTextFont| {
        rasterize_text_to_image(
            "AVATAR Browse",
            Rect::from_size(Size::new(300.0, 40.0)),
            &style,
            Color::BLACK,
            20.0,
            1.0,
            font,
        )
        .unwrap()
        .pixels()
        .to_vec()
    };
    assert_ne!(raster(&normal), raster(&wide));
    assert_eq!(wide.weight(), FontWeight::MEDIUM);
}

#[test]
fn explicit_axes_override_semantic_weight_and_have_canonical_cache_identity() {
    let first = face(&[(*b"wght", 600.0), (*b"wdth", 125.0)]).unwrap();
    let reordered = face(&[(*b"wdth", 125.0), (*b"wght", 600.0)]).unwrap();
    let repeated = face(&[(*b"wght", 400.0), (*b"wdth", 125.0), (*b"wght", 600.0)]).unwrap();
    assert_eq!(first.content_hash(), reordered.content_hash());
    assert_eq!(first.content_hash(), repeated.content_hash());
    assert_ne!(
        first.content_hash(),
        face(&[(*b"wdth", 125.0)]).unwrap().content_hash()
    );
    let mut registry = SoftwareTextFontRegistry::new();
    registry
        .register_face_bytes_with_variations(
            &FontFamily::SansSerif,
            FontWeight::MEDIUM,
            FontStyle::Normal,
            FONT,
            &[(*b"wdth", 125.0), (*b"wght", 600.0)],
        )
        .unwrap();
    assert_eq!(registry.faces()[0].content_hash(), first.content_hash());
}

#[test]
fn explicit_axes_reject_unknown_nonfinite_and_out_of_range_coordinates() {
    for (tag, value) in [
        (*b"nope", 1.0),
        (*b"wdth", f32::NAN),
        (*b"wdth", f32::INFINITY),
        (*b"wdth", 99.0),
        (*b"wdth", 126.0),
    ] {
        assert!(
            matches!(face(&[(tag, value)]), Err(SoftwareTextFontError::InvalidVariation { tag: rejected }) if rejected == tag)
        );
    }
    for value in [100.0, 125.0] {
        assert!(face(&[(*b"wdth", value)]).is_ok());
    }
}
