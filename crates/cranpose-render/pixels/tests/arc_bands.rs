// Arcs the pixels renderer draws cover their band whatever their sweep.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, Point, Stroke, StrokeCap, TAU};

const SIDE: u32 = 120;

#[test]
fn a_butt_ring_short_of_a_whole_turn_covers_the_side_across_its_gap() {
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    scope.draw_arc(
                        Brush::solid(Color::from_rgb_u8(98, 160, 255)),
                        Point::new(60.5, 60.5),
                        30.0,
                        0.0,
                        TAU * 0.999,
                        Stroke::new(9.0).with_cap(StrokeCap::Butt),
                    );
                }),
                BoxSpec::default(),
                || {},
            );
        },
        (SIDE, SIDE),
        (SIDE as f32, SIDE as f32),
    );
    shell.update();
    let mut frame = vec![0u8; (SIDE * SIDE * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw(&mut frame, SIDE, SIDE));
    // Half a turn from the gap, on the band's middle line.
    let at = ((60 * SIDE + 30) * 4) as usize;
    assert_eq!(frame[at..at + 4], [98, 160, 255, 255]);
}
