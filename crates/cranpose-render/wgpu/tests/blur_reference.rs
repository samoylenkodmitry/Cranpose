mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};
use support::{ReferenceEdge, region_pixels, solid_rect};

const FRAME: u32 = 160;
const RADIUS: f32 = 4.0;
const WIDE_RADIUS: f32 = 20.0;
const GLASS: Rect = Rect {
    x: 40.0,
    y: 40.0,
    width: 80.0,
    height: 80.0,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Bars and blocks of distinct colors under the glass, so every tap of the
/// kernel meets an edge somewhere.
fn page(blur: Option<f32>) -> RenderGraph {
    let mut children = vec![
        solid_rect(
            rect(0.0, 0.0, FRAME as f32, FRAME as f32),
            Color::from_rgb_u8(30, 40, 60),
        ),
        solid_rect(
            rect(50.0, 0.0, 9.0, FRAME as f32),
            Color::from_rgb_u8(240, 80, 40),
        ),
        solid_rect(
            rect(0.0, 70.0, FRAME as f32, 7.0),
            Color::from_rgb_u8(60, 220, 90),
        ),
        solid_rect(
            rect(80.0, 90.0, 25.0, 13.0),
            Color::from_rgb_u8(250, 240, 120),
        ),
        solid_rect(
            rect(95.0, 45.0, 3.0, 40.0),
            Color::from_rgb_u8(255, 255, 255),
        ),
    ];
    if let Some(radius) = blur {
        children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                rect(0.0, 0.0, GLASS.width, GLASS.height),
                ProjectiveTransform::translation(GLASS.x, GLASS.y),
                GraphicsLayer {
                    backdrop_effect: Some(RenderEffect::blur(radius)),
                    ..GraphicsLayer::default()
                },
                Vec::new(),
            ),
        )));
    }
    support::page_graph(FRAME, FRAME, children)
}

/// The worst channel deviation of the blurred glass interior from the CPU
/// kernel, where it is, and how many of the interior's channels the blur
/// changed.
struct KernelDeviation {
    worst: f32,
    worst_at: (usize, usize, usize),
    changed: usize,
    channels: usize,
}

fn worst_kernel_deviation(radius: f32) -> Option<KernelDeviation> {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return None;
    };
    let plain = support::capture_graph(&mut renderer, page(None), FRAME, FRAME);
    let blurred = support::capture_graph(&mut renderer, page(Some(radius)), FRAME, FRAME);
    let plain_values: Vec<f32> = plain.pixels.iter().map(|value| f32::from(*value)).collect();
    let expected = support::reference_blur(
        &plain_values,
        FRAME as usize,
        FRAME as usize,
        4,
        radius,
        ReferenceEdge::Clamp,
    );
    let inside = rect(
        GLASS.x + 2.0,
        GLASS.y + 2.0,
        GLASS.width - 4.0,
        GLASS.height - 4.0,
    );
    let actual = region_pixels(&blurred, inside);
    let mut worst = 0.0f32;
    let mut worst_at = (0, 0, 0);
    let mut changed = 0usize;
    for (index, value) in actual.iter().enumerate() {
        let x = inside.x as usize + index / 4 % inside.width as usize;
        let y = inside.y as usize + index / 4 / inside.width as usize;
        let c = index % 4;
        let want = expected[(y * FRAME as usize + x) * 4 + c];
        let delta = (*value as f32 - want).abs();
        if delta > worst {
            worst = delta;
            worst_at = (x, y, c);
        }
        if *value != plain.pixels[(y * FRAME as usize + x) * 4 + c] {
            changed += 1;
        }
    }
    Some(KernelDeviation {
        worst,
        worst_at,
        changed,
        channels: actual.len(),
    })
}

fn assert_blur_follows_its_kernel(radius: f32, budget: f32) {
    let Some(KernelDeviation {
        worst,
        worst_at,
        changed,
        channels,
    }) = worst_kernel_deviation(radius)
    else {
        return;
    };
    assert!(
        changed > channels / 4,
        "the blur must change most pixels under the glass"
    );
    assert!(
        worst <= budget,
        "the radius-{radius} blur diverges from its kernel by {worst} at {worst_at:?}; every \
         weight and tap offset must reproduce the kernel within {budget}"
    );
}

#[test]
fn a_blur_matches_its_kernel_applied_by_the_cpu_within_one_step() {
    assert_blur_follows_its_kernel(RADIUS, 1.0);
}

/// A wide blur averages each block of four texels, runs both passes at a
/// quarter of the capture's size and interpolates the pixels between: a CPU
/// model of exactly that lands 5.3 levels from the kernel on this page and
/// the GPU 5.9, while a pass at the wrong pitch or a tap that skips texels
/// lands 30 to 95 away.
const DOWNSCALE_BUDGET: f32 = 8.0;

#[test]
fn a_wide_blur_matches_its_kernel_within_the_downscale_budget() {
    assert_blur_follows_its_kernel(WIDE_RADIUS, DOWNSCALE_BUDGET);
}

/// A wide blur's two passes each cover a sixteenth of its capture, while a
/// narrow blur's cover the capture whole; the rest of the page renders the
/// same for both. The narrow page therefore spends more than half a wide
/// capture more on its blur than the wide page, which a wide vertical pass
/// left at the capture's size would erase.
#[test]
fn a_wide_blur_runs_both_passes_at_the_scratch_size() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let pass_pixels = |renderer: &mut support::LockedRenderer, radius: f32| {
        support::capture_graph(renderer, page(Some(radius)), FRAME, FRAME);
        renderer.last_frame_stats().expect("stats").pass_pixels
    };
    let narrow = pass_pixels(&mut renderer, RADIUS);
    let wide = pass_pixels(&mut renderer, WIDE_RADIUS);
    let wide_capture = (GLASS.width + 2.0 * WIDE_RADIUS) * (GLASS.height + 2.0 * WIDE_RADIUS);
    let saved = narrow.saturating_sub(wide);
    assert!(
        saved as f32 > wide_capture / 2.0,
        "the wide blur must run both passes at the scratch size: narrow={narrow} wide={wide} \
         saved={saved} wide capture={wide_capture}"
    );
}
