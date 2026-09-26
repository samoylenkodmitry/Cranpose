use cranpose_ui_graphics::BlendMode;

use crate::render::supported_blend_mode;

#[test]
fn an_unsupported_mode_folds_onto_one_that_is_built() {
    assert_eq!(
        supported_blend_mode(BlendMode::Multiply),
        BlendMode::SrcOver
    );
    assert_eq!(supported_blend_mode(BlendMode::Clear), BlendMode::SrcOver);
    assert_eq!(supported_blend_mode(BlendMode::Src), BlendMode::Src);
    assert_eq!(supported_blend_mode(BlendMode::DstOut), BlendMode::DstOut);
}
