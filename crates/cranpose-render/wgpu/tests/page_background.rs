// A background that covers the whole page is the page's clear: the pass
// clears the page to its color and shades none of its pixels.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{Box, BoxSpec, Color, Modifier, composable};

use crate::support;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 160;
const PAGE_BYTES: [u8; 4] = [6, 11, 16, 255];
const PAGE: Color = Color(6.0 / 255.0, 11.0 / 255.0, 16.0 / 255.0, 1.0);
const CARD: f32 = 20.0;
const GPU_STATS: &str = "CRANPOSE_GPU_STATS";

#[composable]
fn BackgroundPage() {
    Box(
        Modifier::empty()
            .size_points(WIDTH as f32, HEIGHT as f32)
            .background(PAGE),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .size_points(CARD, CARD)
                    .background(Color(0.9, 0.3, 0.2, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

// Turns the frame statistics on for as long as it lives.
struct GpuStats;

impl GpuStats {
    fn on() -> Self {
        cranpose_core::debug_toggles::set_debug_toggle(GPU_STATS, Some("1"));
        Self
    }
}

impl Drop for GpuStats {
    fn drop(&mut self) {
        cranpose_core::debug_toggles::set_debug_toggle(GPU_STATS, None);
    }
}

#[test]
fn a_background_over_the_whole_page_is_cleared_to_instead_of_drawn() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let _stats = GpuStats::on();
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        BackgroundPage,
    );
    shell.set_viewport(WIDTH as f32, HEIGHT as f32);
    shell.set_buffer_size(WIDTH, HEIGHT);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(WIDTH, HEIGHT)
        .expect("capture should succeed");
    let stats = shell.renderer().last_frame_stats().expect("stats");
    let page = u64::from(WIDTH) * u64::from(HEIGHT);
    assert!(
        stats.shape_fill_pixels < page / 2,
        "the page's background must not be shaded: {} pixels against a page of {page}",
        stats.shape_fill_pixels
    );
    let corner = &frame.pixels[frame.pixels.len() - 4..];
    for (got, want) in corner.iter().zip(PAGE_BYTES) {
        assert!(
            got.abs_diff(want) <= 1,
            "the page shows its background: {corner:?} against {PAGE_BYTES:?}"
        );
    }
}
