use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Size};

pub(super) fn node_size(rest: Size, held: Size, padding: f32) -> Size {
    let projection = super::lens_motion::MAX_LENS_PROJECTION;
    Size::new(
        (rest.width + (held.width - rest.width) * 1.2) * projection + padding * 2.0,
        (rest.height + (held.height - rest.height) * 1.2) * projection + padding * 2.0,
    )
}

#[composable]
pub(super) fn WhiteControlThumb(modifier: Modifier, height: f32) {
    Box(
        modifier.draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::solid(Color::WHITE),
                CornerRadii::uniform(height * 0.5),
            );
        }),
        BoxSpec::default(),
        || {},
    );
}
