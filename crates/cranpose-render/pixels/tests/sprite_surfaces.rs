//! A layer the sprite compositor's shapes cannot follow, as a turned draw
//! group, reaches it as a texture that holds the layer's pixels.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::{
    PixelsRenderer,
    sprites::{SPRITE_FLOATS, SpriteFrame, SpriteKind, field},
};
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, Point, Rect, Size};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 60;

/// A bar across the right half that a draw group turns upright about its
/// centre.
#[composable]
fn TurnedBar() {
    Box(
        Modifier::empty()
            .size(Size::new(WIDTH as f32, HEIGHT as f32))
            .draw_behind(|scope| {
                let bar = Rect {
                    x: 64.0,
                    y: 26.0,
                    width: 52.0,
                    height: 8.0,
                };
                scope.rotate(90.0, Point::new(90.0, 30.0), &mut |scope| {
                    scope.draw_rect_at(bar, Brush::solid(Color::WHITE));
                });
            }),
        BoxSpec::default(),
        || {},
    );
}

/// The alpha the texture sprites of `frame` put at device pixel `x`, `y`.
fn texture_alpha_at(frame: &SpriteFrame, x: f32, y: f32) -> Option<u8> {
    frame
        .sprites()
        .as_chunks::<SPRITE_FLOATS>()
        .0
        .iter()
        .filter(|sprite| sprite[field::KIND] == SpriteKind::Texture as u8 as f32)
        .find_map(|sprite| {
            let id = sprite[field::KIND + 1] as u32;
            let [left, top, width, height] = [0, 1, 2, 3].map(|at| sprite[field::GEOMETRY + at]);
            let inside = x >= left && x < left + width && y >= top && y < top + height;
            let upload = frame.uploads().iter().find(|upload| upload.id == id)?;
            inside.then(|| {
                let column = (x - left) as usize;
                let row = (y - top) as usize;
                upload.pixels[(row * upload.width as usize + column) * 4 + 3]
            })
        })
}

#[test]
fn a_turned_draw_group_reaches_the_sprite_compositor_as_a_texture() {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        TurnedBar,
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    shell.update();
    let mut frame = SpriteFrame::default();
    shell
        .primary()
        .with_renderer(|renderer| renderer.compose_sprites(&mut frame, WIDTH, HEIGHT));
    let upright = texture_alpha_at(&frame, 90.5, 8.5);
    assert!(
        upright.is_some_and(|alpha| alpha > 200),
        "the turned bar stands upright in a texture, got {upright:?}"
    );
    let across = texture_alpha_at(&frame, 68.5, 30.5);
    assert!(
        across.is_none_or(|alpha| alpha < 40),
        "the turned bar no longer lies across, got {across:?}"
    );
}
