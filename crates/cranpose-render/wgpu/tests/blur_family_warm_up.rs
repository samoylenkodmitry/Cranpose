use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};

use crate::{shared_test_support, support};

const FRAME: u32 = 160;
/// A radius the blur downsamples two to one, and one it downsamples four to
/// one: the same tile mode's family, different downsample pipelines.
const HALF_SCALE_RADIUS: f32 = 8.0;
const QUARTER_SCALE_RADIUS: f32 = 30.0;
const GLASS: Rect = Rect {
    x: 40.0,
    y: 40.0,
    width: 80.0,
    height: 80.0,
};

fn blurred_page(radius: f32) -> RenderGraph {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME as f32,
        height: FRAME as f32,
    };
    let glass = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: GLASS.width,
            height: GLASS.height,
        },
        ProjectiveTransform::translation(GLASS.x, GLASS.y),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(radius)),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    );
    support::page_graph(
        FRAME,
        FRAME,
        vec![
            support::solid_rect(full, Color::from_rgb_u8(30, 40, 60)),
            support::solid_rect(
                Rect {
                    x: 50.0,
                    y: 0.0,
                    width: 9.0,
                    height: FRAME as f32,
                },
                Color::from_rgb_u8(240, 80, 40),
            ),
            RenderNode::Layer(Box::new(glass)),
        ],
    )
}

/// What the frame drawing a blur of `radius` waited for, with the blur
/// asserted so no waits means ready pipelines.
fn frame_waits_for(renderer: &mut support::LockedRenderer, radius: f32) -> u64 {
    let (stats, waits) = support::capture_waits(renderer, blurred_page(radius), FRAME, FRAME);
    assert!(
        stats.blur_passes > 0 || stats.placeholder_draws > 0,
        "the backdrop must blur, or wait for its blur"
    );
    waits
}

#[test]
fn a_blur_finds_its_other_downsample_block_warmed_by_the_first() {
    let mut renderer = support::headless_renderer_compiling_in_background()
        .expect("GPU required for blur warm-up");
    assert!(
        frame_waits_for(&mut renderer, HALF_SCALE_RADIUS) >= 1,
        "the first blur waits for its own pipelines"
    );
    support::capture_drawn(
        &mut renderer,
        &blurred_page(HALF_SCALE_RADIUS),
        FRAME,
        FRAME,
    );
    support::wait_for_background_compiler_idle();
    assert_eq!(
        frame_waits_for(&mut renderer, QUARTER_SCALE_RADIUS),
        0,
        "a wider radius's downsample must be ready once its family was first drawn"
    );
}
