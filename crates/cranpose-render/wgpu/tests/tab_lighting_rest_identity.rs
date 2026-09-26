use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader,
};

use crate::{
    shared_test_support, support,
    support::bar_scene::{BAR, HEIGHT, WIDTH},
};

/// The liquid tab bar's lighting shader, assembled the way the bar builds
/// it: the source with no uniform set.
fn lighting_shader() -> RuntimeShader {
    RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}",
        include_str!("../../../cranpose-liquid/src/widgets/tab_lighting.wgsl")
    ))
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
