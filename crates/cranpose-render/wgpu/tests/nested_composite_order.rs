mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Color, GraphicsLayer, RenderEffect, composable,
    widgets::{Box, BoxSpec},
};
use support::{FramePage, rect_modifier};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 180;
const CARD: [f32; 4] = [20.0, 20.0, 300.0, 140.0];
const BLUR_CHILD: [f32; 4] = [40.0, 30.0, 150.0, 60.0];
const CARD_ALPHA: f32 = 0.9;
const CHILD_ALPHA: f32 = 0.5;
const BACKGROUND: Color = Color(0.1, 0.1, 0.1, 1.0);

fn alpha_layer(alpha: f32) -> GraphicsLayer {
    GraphicsLayer {
        alpha,
        ..GraphicsLayer::default()
    }
}

/// A translucent card whose content interleaves direct draws with an
/// isolated child: red below, a half-transparent green child over it, blue on
/// top of both. The child composites plainly into the card and reads nothing
/// from it, so the card's content and the child's composite render in one
/// pass; that pass must still draw them in z order. With `with_blur_child`
/// a blur backdrop child sits between the green child and the blue rect: it
/// reads the card, so everything queued before it has to land first.
#[composable]
#[allow(non_snake_case)]
fn LayeredCardPage(with_blur_child: bool) {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, BACKGROUND, move || {
        Box(
            rect_modifier(CARD).graphics_layer_value(alpha_layer(CARD_ALPHA)),
            BoxSpec::new(),
            move || {
                Box(
                    rect_modifier([0.0, 0.0, 200.0, 100.0]).background(Color(1.0, 0.0, 0.0, 1.0)),
                    BoxSpec::new(),
                    || {},
                );
                Box(
                    rect_modifier([100.0, 20.0, 120.0, 60.0])
                        .graphics_layer_value(alpha_layer(CHILD_ALPHA))
                        .background(Color(0.0, 1.0, 0.0, 1.0)),
                    BoxSpec::new(),
                    || {
                        Box(
                            rect_modifier([10.0, 10.0, 20.0, 20.0])
                                .background(Color(1.0, 1.0, 0.0, 1.0)),
                            BoxSpec::new(),
                            || {},
                        );
                    },
                );
                if with_blur_child {
                    Box(
                        rect_modifier(BLUR_CHILD).backdrop_effect(RenderEffect::blur(3.0)),
                        BoxSpec::new(),
                        || {},
                    );
                }
                Box(
                    rect_modifier([150.0, 0.0, 100.0, 100.0]).background(Color(0.0, 0.0, 1.0, 1.0)),
                    BoxSpec::new(),
                    || {},
                );
            },
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn PlainCardPage() {
    LayeredCardPage(false);
}

#[composable]
#[allow(non_snake_case)]
fn BlurCardPage() {
    LayeredCardPage(true);
}

fn over(top: [f32; 3], alpha: f32, under: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| top[i] * alpha + under[i] * (1.0 - alpha))
}

fn expected_pixel(card_pixel: [f32; 3]) -> [u8; 3] {
    over(
        card_pixel,
        CARD_ALPHA,
        [BACKGROUND.0, BACKGROUND.1, BACKGROUND.2],
    )
    .map(|channel| (channel * 255.0).round() as u8)
}

fn sample(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let index = ((y * FRAME_WIDTH + x) * 4) as usize;
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

fn assert_pixel(pixels: &[u8], x: u32, y: u32, expected: [u8; 3], what: &str) {
    let actual = sample(pixels, x, y);
    let off = actual
        .iter()
        .zip(expected)
        .map(|(a, e)| a.abs_diff(e))
        .max()
        .unwrap_or(0);
    assert!(
        off <= 2,
        "{what} at ({x},{y}): expected {expected:?}, got {actual:?}"
    );
}

fn render_frames(page: fn()) -> Option<Vec<Vec<u8>>> {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, page);
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    Some(
        (0..2)
            .map(|_| {
                shell.update();
                shell
                    .renderer()
                    .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
                    .expect("frame capture should succeed")
                    .pixels
            })
            .collect(),
    )
}

fn assert_stack_order(pixels: &[u8], frame: usize) {
    let red = [1.0, 0.0, 0.0];
    let green_over_red = over([0.0, 1.0, 0.0], CHILD_ALPHA, red);
    let blue = [0.0, 0.0, 1.0];
    assert_pixel(
        pixels,
        60,
        60,
        expected_pixel(red),
        &format!("frame {frame}: red alone"),
    );
    assert_pixel(
        pixels,
        140,
        85,
        expected_pixel(green_over_red),
        &format!("frame {frame}: the child over red"),
    );
    assert_pixel(
        pixels,
        200,
        70,
        expected_pixel(blue),
        &format!("frame {frame}: blue over the child and red"),
    );
}

#[test]
fn a_deferred_content_range_still_draws_an_isolated_child_between_the_ops_around_it() {
    let Some(frames) = render_frames(PlainCardPage) else {
        return;
    };
    for (frame, pixels) in frames.iter().enumerate() {
        assert_stack_order(pixels, frame);
    }
}

#[test]
fn a_backdrop_child_reads_the_deferred_ops_and_the_isolated_child_beneath_it() {
    let Some(frames) = render_frames(BlurCardPage) else {
        return;
    };
    for (frame, pixels) in frames.iter().enumerate() {
        assert_stack_order(pixels, frame);
        let red = [1.0, 0.0, 0.0];
        let green_over_red = over([0.0, 1.0, 0.0], CHILD_ALPHA, red);
        assert_pixel(
            pixels,
            80,
            70,
            expected_pixel(red),
            &format!("frame {frame}: the blur child over uniform red"),
        );
        assert_pixel(
            pixels,
            160,
            75,
            expected_pixel(green_over_red),
            &format!("frame {frame}: the blur child over the isolated child"),
        );
    }
}
