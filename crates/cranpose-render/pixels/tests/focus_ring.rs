// The ring `Modifier::focusable` draws follows the keyboard's focus in the
// picture a host shows: it leaves the control focus left and appears on the
// one focus reached, in the frame after the key, and a pointer press takes
// it away.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Key, KeyEvent, Modifier, Size};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 100;
const RING: [u8; 4] = [0, 95, 204, 255];

#[composable]
fn Controls() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        for _ in 0..2 {
            Box(
                Modifier::empty().size(Size::new(100.0, 40.0)).focusable(),
                BoxSpec::default(),
                || {},
            );
        }
    });
}

// The pixel on the left edge of each control, where its ring's outer line
// runs, after a frame.
fn left_edges(shell: &mut AppShell<PixelsRenderer>) -> [[u8; 4]; 2] {
    shell.update();
    let mut image = vec![0; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw_scaled(&mut image, WIDTH, HEIGHT, 1.0));
    [20, 60].map(|y| {
        image[y * WIDTH as usize * 4..][..4]
            .try_into()
            .expect("RGBA pixel")
    })
}

#[test]
fn the_ring_moves_with_the_keyboard_focus() {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        Controls,
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    let [first, second] = left_edges(&mut shell);
    assert!(first != RING && second != RING, "no ring before a key");

    assert!(shell.on_key_event(&KeyEvent::key_down(Key::DirectionDown, "")));
    assert_eq!(left_edges(&mut shell)[0], RING, "the first control's ring");

    assert!(shell.on_key_event(&KeyEvent::key_down(Key::DirectionDown, "")));
    let [first, second] = left_edges(&mut shell);
    assert_eq!(second, RING, "the second control's ring");
    assert_ne!(first, RING, "the first control's ring went away");

    shell.set_cursor(110.0, 90.0);
    shell.pointer_pressed();
    shell.pointer_released();
    assert_ne!(
        left_edges(&mut shell)[1],
        RING,
        "a pointer press takes the ring away"
    );
}
