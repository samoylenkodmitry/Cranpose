mod support;

use support::page::*;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;
const TOGGLE: &str = "CRANPOSE_KEEP_BOX4";

#[composable]
#[allow(non_snake_case)]
fn CardsPage() {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.06, 0.05, 0.12, 1.0),
        || {
            for i in 0..12u32 {
                let x = (i as f32 * 71.0) % 300.0;
                let y = (i as f32 * 43.0) % 220.0;
                Box(
                    rect_modifier([x, y, 18.0, 18.0])
                        .background(Color(0.9, 0.8 - (i % 3) as f32 * 0.2, 0.4, 1.0))
                        .rounded_corners(9.0),
                    BoxSpec::new(),
                    || {},
                );
            }
            for row in 0..2u32 {
                let card = [24.0, 30.0 + row as f32 * 100.0, 272.0, 80.0];
                Box(
                    rect_modifier(card)
                        .backdrop_effect(RenderEffect::blur(6.0))
                        .background(Color(1.0, 1.0, 1.0, 0.16))
                        .rounded_corners(14.0),
                    BoxSpec::new(),
                    move || {
                        Text(
                            if row == 0 {
                                "Box resolve"
                            } else {
                                "Texel fetch"
                            },
                            Modifier::empty().offset(16.0, 14.0),
                            TextStyle::default(),
                        );
                    },
                );
            }
        },
    );
}

fn capture(keep_box4: bool) -> Option<Vec<u8>> {
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, keep_box4.then_some("1"));
    let captured = capture_with_current_toggles();
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    captured
}

fn capture_with_current_toggles() -> Option<Vec<u8>> {
    let (_lock, mut shell) = support::app_shell_for(
        CardsPage,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )?;
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    Some(frame.pixels)
}

#[test]
fn a_lowered_box_resolve_matches_the_full_walk_byte_for_byte() {
    let Some(lowered) = capture(false) else {
        return;
    };
    let kept = capture(true).expect("headless WGPU init failed mid-suite");
    support::assert_same_bytes(
        "lowered texel fetch vs box resolve, which may only be lowered where the box covers \
         exactly one texel with weight one",
        FRAME_WIDTH,
        &lowered,
        &kept,
    );
}
