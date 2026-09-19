use super::*;

#[test]
fn a_transparent_window_clears_to_nothing_and_an_opaque_one_to_the_background() {
    assert_eq!(frame_clear_color(true), wgpu::Color::TRANSPARENT);
    let opaque = frame_clear_color(false);
    assert_eq!(
        opaque.a, 1.0,
        "an opaque surface starts from a solid background"
    );
    assert!(
        opaque.r < 0.1 && opaque.g < 0.1 && opaque.b < 0.1,
        "the framework's background is the dark frame colour"
    );
}
