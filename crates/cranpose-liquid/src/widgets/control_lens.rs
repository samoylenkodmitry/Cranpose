use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Size};
use cranpose_ui_layout::Alignment;

pub(super) fn node_size(rest: Size, held: Size, padding: f32) -> Size {
    let projection = super::lens_motion::MAX_LENS_PROJECTION;
    Size::new(
        (rest.width + (held.width - rest.width) * 1.2) * projection + padding * 2.0,
        (rest.height + (held.height - rest.height) * 1.2) * projection + padding * 2.0,
    )
}

#[composable]
pub(super) fn WhiteControlThumb(modifier: Modifier, size: Size) {
    let face = Modifier::empty()
        .size(size)
        .drop_shadow(
            cranpose_ui_graphics::LayerShape::Rounded(
                cranpose_ui_graphics::RoundedCornerShape::uniform(size.height * 0.5),
            ),
            |scope| {
                scope.radius = 6.0;
                scope.offset.y = 2.0;
                scope.color = Color::BLACK.with_alpha(0.16);
            },
        )
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::solid(Color::WHITE),
                CornerRadii::uniform(size.height * 0.5),
            );
        });
    Box(
        modifier,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Box(face.clone(), BoxSpec::default(), || {});
        },
    );
}
