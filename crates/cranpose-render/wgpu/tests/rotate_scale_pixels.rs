mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Color, Modifier,
    widgets::{Box, BoxSpec},
};

const FRAME_SIZE: u32 = 400;
const RECT_WIDTH: f32 = 200.0;
const RECT_HEIGHT: f32 = 40.0;
const RECT_LEFT: f32 = (FRAME_SIZE as f32 - RECT_WIDTH) / 2.0;
const RECT_TOP: f32 = (FRAME_SIZE as f32 - RECT_HEIGHT) / 2.0;

fn is_marker(frame: &cranpose_render_wgpu::CapturedFrame, x: u32, y: u32) -> bool {
    let offset = ((y * frame.width + x) * 4) as usize;
    let (r, g, b) = (
        frame.pixels[offset],
        frame.pixels[offset + 1],
        frame.pixels[offset + 2],
    );
    r > 200 && g < 80 && b < 80
}

fn render_layered_rect(modifier: Modifier) -> cranpose_render_wgpu::CapturedFrame {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, move || {
        let modifier = modifier.clone();
        Box(
            Modifier::empty()
                .size_points(FRAME_SIZE as f32, FRAME_SIZE as f32)
                .background(Color(0.05, 0.05, 0.06, 1.0)),
            BoxSpec::new(),
            move || {
                Box(
                    modifier
                        .clone()
                        .offset(RECT_LEFT, RECT_TOP)
                        .size_points(RECT_WIDTH, RECT_HEIGHT)
                        .background(Color(0.9, 0.1, 0.1, 1.0)),
                    BoxSpec::new(),
                    || {},
                );
            },
        );
    });
    shell.set_viewport(FRAME_SIZE as f32, FRAME_SIZE as f32);
    shell.set_buffer_size(FRAME_SIZE, FRAME_SIZE);
    shell.update();
    shell
        .renderer()
        .capture_frame(FRAME_SIZE, FRAME_SIZE)
        .expect("frame capture should succeed")
}

#[test]
fn rotate_ninety_degrees_swaps_the_visible_bounding_box_about_the_center() {
    let center = (FRAME_SIZE / 2, FRAME_SIZE / 2);
    let unrotated = render_layered_rect(Modifier::empty());

    assert!(
        is_marker(&unrotated, center.0 - 90, center.1),
        "the unrotated wide rectangle must cover a point 90px left of center"
    );
    assert!(
        !is_marker(&unrotated, center.0, center.1 - 90),
        "the unrotated rectangle must not reach 90px above center (it is only 40px tall)"
    );

    let rotated = render_layered_rect(Modifier::empty().rotate(90.0));

    assert!(
        !is_marker(&rotated, center.0 - 90, center.1),
        "after a 90 degree rotation the rectangle must no longer reach 90px to the left"
    );
    assert!(
        is_marker(&rotated, center.0, center.1 - 90),
        "after a 90 degree rotation the rectangle must now reach 90px above center"
    );
}

#[test]
fn scale_about_center_grows_the_visible_bounding_box_symmetrically() {
    let center = (FRAME_SIZE / 2, FRAME_SIZE / 2);
    let unscaled = render_layered_rect(Modifier::empty());

    assert!(
        !is_marker(&unscaled, center.0, center.1 - 30),
        "the unscaled rectangle is only 40px tall, so 30px above center must be background"
    );

    let scaled = render_layered_rect(Modifier::empty().scale(2.0));

    assert!(
        is_marker(&scaled, center.0, center.1 - 30),
        "scaling by 2x about the center must extend the rectangle's half-height from 20px to 40px"
    );
    assert!(
        !is_marker(&scaled, center.0, center.1 - 45),
        "a 2x scale must not extend the rectangle's half-height past 40px"
    );
}

#[test]
fn scale_xy_grows_axes_independently() {
    let center = (FRAME_SIZE / 2, FRAME_SIZE / 2);

    let scaled = render_layered_rect(Modifier::empty().scale_xy(1.0, 3.0));

    assert!(
        is_marker(&scaled, center.0, center.1 - 50),
        "scaling y by 3x about the center must extend the half-height from 20px to 60px"
    );
    assert!(
        !is_marker(&scaled, center.0 + 105, center.1),
        "scaling only y must leave the half-width at 100px"
    );
}
