use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader,
};

use crate::{
    shared_test_support, support,
    support::bar_scene::{BAR, HEIGHT, WIDTH},
};

/// A shader returning its source as it is, so zero over a transparent
/// layer; declared so or not.
fn identity_shader(declared: bool) -> RenderEffect {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> \
         @location(0) vec4<f32> {{\n    return textureSample(input_texture, input_sampler, \
         input.uv);\n}}\n"
    ));
    shader.set_preserves_transparency(declared);
    RenderEffect::runtime_shader(shader)
}

/// A shader painting red whatever its source holds.
fn painting_shader() -> RenderEffect {
    RenderEffect::runtime_shader(RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> \
         @location(0) vec4<f32> {{\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}}\n"
    )))
}

/// The striped page with an empty layer over `BAR` carrying `effect`.
fn page_with_empty_layer(effect: RenderEffect) -> Vec<RenderNode> {
    let mut page = support::striped_page(WIDTH, HEIGHT);
    page.push(RenderNode::Layer(Box::new(
        shared_test_support::layer_node(
            BAR,
            ProjectiveTransform::identity(),
            GraphicsLayer {
                render_effect: Some(effect),
                ..Default::default()
            },
            vec![],
        ),
    )));
    page
}

fn capture(renderer: &mut support::LockedRenderer, page: Vec<RenderNode>) -> CapturedFrame {
    support::capture_graph_settled(
        renderer,
        support::page_graph(WIDTH, HEIGHT, page),
        WIDTH,
        HEIGHT,
    )
}

/// An empty layer under a shader that keeps a transparent source
/// transparent composites nothing, so the renderer draws nothing for it;
/// the same shader undeclared shades the layer's pixels to the same page,
/// and a shader that paints from nothing is drawn as declared.
#[test]
fn an_empty_layer_under_a_transparency_preserving_shader_draws_nothing() {
    let mut renderer = support::headless_renderer().expect("GPU required for the layer probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let plain_passes = renderer
        .last_frame_stats()
        .expect("frame statistics")
        .pass_count;

    let undeclared = capture(&mut renderer, page_with_empty_layer(identity_shader(false)));
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.shader_pixels > 0,
        "an undeclared shader over an empty layer is drawn"
    );
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &undeclared.pixels);
    assert!(
        differing.is_empty(),
        "the identity shader over nothing changed the page: {}",
        support::describe_differing(&differing)
    );

    let declared = capture(&mut renderer, page_with_empty_layer(identity_shader(true)));
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(stats.shader_pixels, 0, "the declared shader is not drawn");
    assert_eq!(
        stats.pass_count, plain_passes,
        "the empty layer costs the page no pass"
    );
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &declared.pixels);
    assert!(
        differing.is_empty(),
        "skipping the empty layer changed the page: {}",
        support::describe_differing(&differing)
    );

    let painted = capture(&mut renderer, page_with_empty_layer(painting_shader()));
    let differing = support::differing_pixels(WIDTH, &plain.pixels, &painted.pixels);
    assert_eq!(
        differing.len(),
        (BAR.width * BAR.height) as usize,
        "a shader painting from nothing paints the whole layer"
    );
}
