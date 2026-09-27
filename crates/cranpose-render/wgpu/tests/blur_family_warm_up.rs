use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::pipelines_created;
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

/// Pipelines the frame drawing a blur of `radius` built on this thread,
/// with the blur asserted so a count of zero means ready pipelines.
fn frame_thread_compiles_for(renderer: &mut support::LockedRenderer, radius: f32) -> u64 {
    let before = pipelines_created();
    let _ = support::capture_graph(renderer, blurred_page(radius), FRAME, FRAME);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(stats.blur_passes > 0, "the backdrop must blur");
    pipelines_created() - before
}

#[test]
fn a_blur_finds_its_other_downsample_block_warmed_by_the_first() {
    let mut renderer = support::headless_renderer().expect("GPU required for blur warm-up");
    assert!(
        frame_thread_compiles_for(&mut renderer, HALF_SCALE_RADIUS) >= 1,
        "the first blur compiles its own pipelines inside its frame"
    );
    support::wait_for_background_compiler_idle();
    assert_eq!(
        frame_thread_compiles_for(&mut renderer, QUARTER_SCALE_RADIUS),
        0,
        "a wider radius's downsample must be ready once its family was first drawn"
    );
}
