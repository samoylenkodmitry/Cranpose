mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{GraphicsLayer, Rect, RenderEffect, RuntimeShader};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;
const BAR: Rect = Rect {
    x: 20.0,
    y: 24.0,
    width: 120.0,
    height: 48.0,
};

/// The liquid tab bar's lighting shader, the way the bar registers it for
/// warm-up: the source with no uniform set.
fn lighting_shader() -> RuntimeShader {
    cranpose_liquid::shader_warm_ups()
        .into_iter()
        .map(|warm_up| warm_up.shader)
        .find(|shader| shader.source().contains("blurred_disk"))
        .expect("the tab lighting shader is registered for warm-up")
}

/// The lighting composited over a page at zero glow must leave every byte
/// of the page as it was: the bar draws no lighting layer at rest on that
/// account, so this is what that omission is worth.
#[test]
fn tab_lighting_at_zero_glow_leaves_the_page_untouched() {
    let mut renderer = support::headless_renderer().expect("GPU required for the lighting probe");
    let page = support::striped_page(WIDTH, HEIGHT);
    let plain = support::capture_graph(
        &mut renderer,
        support::page_graph(WIDTH, HEIGHT, page.clone()),
        WIDTH,
        HEIGHT,
    );
    let mut shader = lighting_shader();
    shader.set_float4(0, BAR.width, BAR.height, 30.0, 20.0);
    shader.set_float4(4, 0.0, 0.0, 46.5, 0.0);
    let mut lit_page = page;
    lit_page.push(RenderNode::Layer(Box::new(
        shared_test_support::layer_node(
            BAR,
            ProjectiveTransform::identity(),
            GraphicsLayer {
                backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
                ..Default::default()
            },
            vec![],
        ),
    )));
    let lit = support::capture_graph_settled(
        &mut renderer,
        support::page_graph(WIDTH, HEIGHT, lit_page),
        WIDTH,
        HEIGHT,
    );
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(stats.shader_pixels > 0, "the lighting must have drawn");
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &lit.pixels);
    assert!(
        differing.is_empty(),
        "lighting at zero glow changed the page: {}",
        support::describe_differing(&differing)
    );
}
