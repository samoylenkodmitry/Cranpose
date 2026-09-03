mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Color, Modifier, RenderEffect, TextStyle, composable,
    widgets::{Box, BoxSpec, Text},
};
use support::{FramePage, rect_modifier};

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
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, CardsPage);
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    shell.update();
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
    assert_eq!(lowered.len(), kept.len());
    let mut differing = 0usize;
    let mut worst = 0u8;
    let mut first = None;
    for (index, (a, b)) in lowered.iter().zip(&kept).enumerate() {
        let diff = a.abs_diff(*b);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
            first.get_or_insert((
                index / 4 % FRAME_WIDTH as usize,
                index / 4 / FRAME_WIDTH as usize,
            ));
        }
    }
    assert_eq!(
        differing, 0,
        "{differing} bytes diverged (worst {worst}, first at {first:?}) between a lowered \
         texel fetch and the box resolve it replaced — the lowering may only fire when the \
         box covers exactly one texel with weight one"
    );
}
