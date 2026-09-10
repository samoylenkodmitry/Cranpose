mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{
    GradientBlurDirection, GraphicsLayer, LayerShape, Rect, RenderEffect, RoundedCornerShape,
    RuntimeShader, gradient_blur_effect,
};

const FRAME: u32 = 192;
const REFERENCE: &str = include_str!("fixtures/gradient_blur_reference.wgsl");

fn page(effect: RenderEffect) -> RenderGraph {
    let mut children = support::striped_page(FRAME, FRAME);
    children.push(RenderNode::Layer(Box::new(
        shared_test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 143.0,
                height: 135.0,
            },
            ProjectiveTransform::translation(21.25, 29.5),
            GraphicsLayer {
                backdrop_effect: Some(effect),
                clip: true,
                shape: LayerShape::Rounded(RoundedCornerShape::uniform(11.0)),
                ..GraphicsLayer::default()
            },
            Vec::new(),
        ),
    )));
    support::page_graph(FRAME, FRAME, children)
}

fn compare_ladder(substrates: bool) {
    let mut renderer = support::headless_renderer().expect("gradient reference needs a GPU");
    for direction in [
        GradientBlurDirection::LeftToRight,
        GradientBlurDirection::RightToLeft,
        GradientBlurDirection::TopToBottom,
        GradientBlurDirection::BottomToTop,
    ] {
        for (start, end) in [(0.0, 24.0), (17.0, 0.1), (3.0, 3.0)] {
            let RenderEffect::Shader { mut shader } = gradient_blur_effect(start, end, direction)
            else {
                panic!("gradient blur uses a runtime shader");
            };
            if !substrates {
                shader.set_substrates(&[]);
            }
            let mut reference = RuntimeShader::new(REFERENCE);
            for (index, value) in shader.uniforms().iter().enumerate() {
                reference.set_float(index, *value);
            }
            reference.set_input_padding(shader.input_padding());
            reference.set_batched_source(shader.batched_source());
            reference.set_substrates(shader.substrates());
            let expected = support::capture_graph(
                &mut renderer,
                page(RenderEffect::runtime_shader(reference)),
                FRAME,
                FRAME,
            );
            let actual = support::capture_graph(
                &mut renderer,
                page(RenderEffect::runtime_shader(shader)),
                FRAME,
                FRAME,
            );
            let stats = renderer
                .last_frame_stats()
                .expect("gradient frame statistics");
            assert_eq!(stats.substrates > 0, substrates);
            support::assert_same_bytes(
                &format!("{direction:?} radii={start}..{end} substrates={substrates}"),
                FRAME,
                &expected.pixels,
                &actual.pixels,
            );
        }
    }
}

#[test]
fn gradient_ladder_matches_reference_with_substrates() {
    compare_ladder(true);
}

#[test]
fn gradient_ladder_preserves_fallback_without_substrates() {
    compare_ladder(false);
}
