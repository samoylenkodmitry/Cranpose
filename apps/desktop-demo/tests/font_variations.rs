use std::rc::Rc;

use cranpose::text::{FontFamily, FontStyle, FontWeight, TextStyle, TextUnit};
use cranpose_render_common::{
    font_source::FontRegistry,
    text_canvas::TextPixels,
    text_fonts::TextFonts,
    text_raster::{rasterize_text, TextRasterRequest},
};
use cranpose_text::{Typeface, TypefaceError};
use cranpose_ui::PreparedTextLayout;
use cranpose_ui_graphics::{Color, Rect, Size};

const FONT: &[u8] = include_bytes!("../assets/leetcodedaily/fonts/MonaspaceKryptonVarVF.ttf");
const TEXT: &str = "AVATAR Browse";

fn face(axes: &[([u8; 4], f32)]) -> Result<Typeface, TypefaceError> {
    Typeface::from_registered(
        &FontFamily::SansSerif,
        FontWeight::MEDIUM,
        FontStyle::Normal,
        FONT,
        axes,
    )
}

// `TEXT` at 20 pixels laid out in `face` alone.
fn laid_out(face: &Typeface, style: &TextStyle) -> Rc<PreparedTextLayout> {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(TextFonts::new(vec![face.clone()]).resolver());
        cranpose_ui::text::prepare_text_layout(&TEXT.into(), style, Default::default(), None)
    })
}

#[test]
fn explicit_axes_change_measurement_rasterization_and_cache_identity() {
    let normal = face(&[(*b"wdth", 100.0)]).unwrap();
    let wide = face(&[(*b"wdth", 125.0)]).unwrap();
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(20.0);
    let measure = |font: &Typeface| {
        let prepared = laid_out(font, &style);
        prepared.layout.line_right(0) - prepared.layout.line_left(0)
    };
    assert!(measure(&wide) > measure(&normal));
    assert_ne!(normal.content_hash(), wide.content_hash());
    let raster = |font: &Typeface| {
        let prepared = laid_out(font, &style);
        rasterize_text(
            TextRasterRequest::new(
                &prepared.layout,
                TEXT,
                Rect::from_size(Size::new(300.0, 40.0)),
                &style,
                Color::BLACK,
                1.0,
            ),
            None,
        )
        .and_then(TextPixels::into_image)
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
    let mut registry = FontRegistry::new();
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
            matches!(face(&[(tag, value)]), Err(TypefaceError::InvalidVariation { tag: rejected }) if rejected == tag)
        );
    }
    for value in [100.0, 125.0] {
        assert!(face(&[(*b"wdth", value)]).is_ok());
    }
}
