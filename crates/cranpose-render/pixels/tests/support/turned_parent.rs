// A parent turned by its graphics layer, which draws through a surface of
// its own.

use cranpose_ui::Modifier;
use cranpose_ui_graphics::{Color, GraphicsLayer};

// A dark box of 80 by 60 at `(x, 50)`, turned about its center by the
// degrees `rotation` reads.
pub(crate) fn turned_parent(x: f32, rotation: impl Fn() -> f32 + 'static) -> Modifier {
    Modifier::empty()
        .offset(x, 50.0)
        .width(80.0)
        .height(60.0)
        .graphics_layer(move || GraphicsLayer {
            rotation_z: rotation(),
            ..GraphicsLayer::default()
        })
        .background(Color::from_rgb_u8(30, 40, 50))
}
