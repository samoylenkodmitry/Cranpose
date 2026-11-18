use cranpose_render_common::graph::{LayerNode, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{
    GradientBlurDirection, GradientBlurLevels, GradientBlurProfile, GraphicsLayer, LayerShape,
    RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RoundedCornerShape, RuntimeShader,
    gradient_blur_effect,
};

use crate::{shared_test_support, support};

const FRAME: u32 = 192;
const REFERENCE: &str = include_str!("fixtures/gradient_blur_reference.wgsl");

fn page(effect: RenderEffect) -> LayerNode {
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
            let RenderEffect::Shader { mut shader } =
                gradient_blur_effect(start, end, direction, GradientBlurProfile::GAUSSIAN)
            else {
                panic!("gradient blur uses a runtime shader");
            };
            if !substrates {
                std::sync::Arc::make_mut(&mut shader).set_substrates(&[]);
            }
            let mut reference =
                RuntimeShader::new(&format!("{RUNTIME_SHADER_PRELUDE_WGSL}{REFERENCE}"));
            for (index, value) in shader.uniforms().iter().enumerate() {
                reference.set_float(index, *value);
            }
            reference.set_input_padding(shader.input_padding());
            reference.set_batched_source(shader.batched_source());
            reference.set_substrates(shader.substrates());
            let expected = support::capture_graph(
                &mut renderer,
                &page(RenderEffect::runtime_shader(reference)),
                FRAME,
                FRAME,
            );
            let actual = support::capture_graph(
                &mut renderer,
                &page(RenderEffect::Shader { shader }),
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

// The effect layer of `page` spans rows 29.5 to 164.5; its upper half, inset
// by a row, holds the start radius under a ramp that starts halfway and asks
// for more than half the radius under a ramp across the whole layer.
const UPPER_HALF: std::ops::Range<usize> = 31..96;

// The pixels of `page` under a gradient blur from 17 pixels at the top to
// each `(end, profile)`.
fn blurred(blurs: [(f32, GradientBlurProfile); 2]) -> [Vec<u8>; 2] {
    let mut renderer = support::headless_renderer().expect("gradient reference needs a GPU");
    blurs.map(|(end, profile)| {
        let effect = gradient_blur_effect(17.0, end, GradientBlurDirection::TopToBottom, profile);
        support::capture_graph(&mut renderer, &page(effect), FRAME, FRAME).pixels
    })
}

fn rows(pixels: &[u8], rows: std::ops::Range<usize>) -> &[u8] {
    let row = FRAME as usize * 4;
    &pixels[rows.start * row..rows.end * row]
}

// How far neighbouring pixels of `row` differ in green inside the effect
// layer: the stripes' contrast left after the blur.
fn contrast(pixels: &[u8], row: usize) -> u32 {
    let start = row * FRAME as usize * 4;
    let greens = || {
        pixels[start + 23 * 4 + 1..start + 162 * 4]
            .iter()
            .step_by(4)
    };
    greens()
        .zip(greens().skip(1))
        .map(|(left, right)| u32::from(left.abs_diff(*right)))
        .sum()
}

#[test]
fn a_ramp_holds_the_start_radius_before_it_begins() {
    let [held, ramped] = blurred([
        (17.0, GradientBlurProfile::GAUSSIAN),
        (
            0.1,
            GradientBlurProfile {
                ramp_start: 0.5,
                ..GradientBlurProfile::GAUSSIAN
            },
        ),
    ]);
    assert!(
        rows(&held, UPPER_HALF) == rows(&ramped, UPPER_HALF),
        "the upper half blurs by the start radius"
    );
    assert!(
        rows(&held, 140..141) != rows(&ramped, 140..141),
        "the lower half eases toward the end radius"
    );
}

#[test]
fn sharp_and_half_levels_keep_fine_detail_below_half_the_radius() {
    let levels = |levels| GradientBlurProfile {
        levels,
        ..GradientBlurProfile::GAUSSIAN
    };
    let [gaussian, sharp_and_half] = blurred([
        (0.1, levels(GradientBlurLevels::Gaussian)),
        (0.1, levels(GradientBlurLevels::SharpAndHalf)),
    ]);
    assert!(
        rows(&gaussian, UPPER_HALF) == rows(&sharp_and_half, UPPER_HALF),
        "rows above half the radius blend the half and the wide level alike"
    );
    // Row 137 asks for about 1.9 of the 17 pixel radius: the Gaussian
    // levels blend the sharp content with the quarter level there, keeping
    // 2,394 of the stripes' contrast, and the sharp and half levels keep
    // 2,919.
    let kept = (contrast(&sharp_and_half, 137), contrast(&gaussian, 137));
    assert!(
        kept.0 * 10 > kept.1 * 11,
        "below half the radius the sharp content shows through: {kept:?}"
    );
}
