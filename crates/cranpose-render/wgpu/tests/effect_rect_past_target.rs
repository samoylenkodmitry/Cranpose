//! A runtime shader whose effect rect reaches past its target draws what it
//! draws on a target that holds the whole rect. Safari's WebGPU drops a
//! frame whose pass sets a viewport reaching past its attachment, so the
//! viewport stays inside the target and the prelude's vertex stage places
//! the rect in it.

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader,
};

use crate::{shared_test_support, support};

const FRAME_WIDTH: u32 = 96;
const FRAME_HEIGHT: u32 = 64;
/// How far the larger frame reaches past the small one on each side.
const MARGIN: u32 = 48;

/// Paints each pixel of the effect rect with its `uv`: red across, green
/// down.
fn uv_effect() -> RenderEffect {
    RenderEffect::runtime_shader(RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(input.uv, 0.25, 1.0);
         }}"
    )))
}

/// A page of `width` by `height` with the shader's layer at `origin`.
fn page(width: u32, height: u32, origin: (f32, f32), size: (f32, f32)) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: size.0,
        height: size.1,
    };
    let layer = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(origin.0, origin.1),
        GraphicsLayer {
            render_effect: Some(uv_effect()),
            ..GraphicsLayer::default()
        },
        vec![support::solid_rect(bounds, Color::WHITE)],
    );
    support::page_graph(width, height, vec![RenderNode::Layer(Box::new(layer))])
}

#[test]
fn effect_rects_past_the_targets_edges_keep_their_mapping() {
    let mut renderer = support::headless_renderer().expect("the effect rect test needs a GPU");
    for (origin, size) in [
        ((-21.0, 9.0), (64.0, 32.0)),
        ((50.0, -13.0), (40.0, 30.0)),
        ((61.5, 40.25), (64.0, 48.0)),
        ((-10.0, -12.0), (120.0, 90.0)),
    ] {
        let (cropped, _) = support::capture_drawn(
            &mut renderer,
            &page(FRAME_WIDTH, FRAME_HEIGHT, origin, size),
            FRAME_WIDTH,
            FRAME_HEIGHT,
        );
        let margin = MARGIN as f32;
        let (whole, _) = support::capture_drawn(
            &mut renderer,
            &page(
                FRAME_WIDTH + 2 * MARGIN,
                FRAME_HEIGHT + 2 * MARGIN,
                (origin.0 + margin, origin.1 + margin),
                size,
            ),
            FRAME_WIDTH + 2 * MARGIN,
            FRAME_HEIGHT + 2 * MARGIN,
        );
        assert!(
            support::distinct_colors(&cropped.pixels) > 64,
            "the effect at {origin:?} drew no gradient"
        );
        let whole_width = (FRAME_WIDTH + 2 * MARGIN) as usize;
        let mut differing = Vec::new();
        for y in 0..FRAME_HEIGHT as usize {
            for x in 0..FRAME_WIDTH as usize {
                let at = (y * FRAME_WIDTH as usize + x) * 4;
                let whole_at = ((y + MARGIN as usize) * whole_width + x + MARGIN as usize) * 4;
                let pixel = &cropped.pixels[at..at + 4];
                let expected = &whole.pixels[whole_at..whole_at + 4];
                if pixel.iter().zip(expected).any(|(a, b)| a.abs_diff(*b) > 1) {
                    differing.push((x, y, pixel.to_vec(), expected.to_vec()));
                }
            }
        }
        assert!(
            differing.is_empty(),
            "an effect rect at {origin:?} of {size:?} drew differently past the target's edge: {} pixels, first {:?}",
            differing.len(),
            differing.iter().take(4).collect::<Vec<_>>()
        );
    }
}
