use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    BlendMode, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect,
    RuntimeShader,
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

#[test]
fn zero_opacity_skips_source_over_effects_but_keeps_destructive_blending() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let plain_passes = renderer
        .last_frame_stats()
        .expect("frame statistics")
        .pass_count;
    for blend_mode in [BlendMode::SrcOver, BlendMode::Src] {
        let mut page = page_with_empty_layer(painting_shader());
        let Some(RenderNode::Layer(layer)) = page.last_mut() else {
            panic!("effect layer");
        };
        layer.graphics_layer.alpha = 0.0;
        layer.graphics_layer.blend_mode = blend_mode;
        layer.graphics_layer.backdrop_effect = Some(RenderEffect::blur(8.0));
        layer.children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                BAR,
                ProjectiveTransform::identity(),
                GraphicsLayer {
                    backdrop_effect: Some(RenderEffect::blur(8.0)),
                    ..Default::default()
                },
                vec![],
            ),
        )));
        let frame = capture(&mut renderer, page);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        if blend_mode == BlendMode::SrcOver {
            assert!(
                frame.pixels == plain.pixels,
                "hidden source-over changed pixels"
            );
            assert_eq!(
                stats.pass_count, plain_passes,
                "hidden effects added passes"
            );
            assert_eq!(stats.shader_pixels, 0, "hidden shader still ran");
            assert_eq!(stats.blur_passes, 0, "hidden descendant backdrop still ran");
        } else {
            assert!(
                frame.pixels != plain.pixels,
                "transparent Src must replace destination pixels"
            );
        }
    }
}

#[test]
fn backdrop_and_foreground_share_group_opacity() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let plain = capture(&mut renderer, support::striped_page(WIDTH, HEIGHT));
    let mut frames = Vec::new();
    for alpha in [0.0, 0.5, 1.0] {
        let mut page = support::striped_page(WIDTH, HEIGHT);
        page.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                BAR,
                ProjectiveTransform::identity(),
                GraphicsLayer {
                    alpha,
                    backdrop_effect: Some(painting_shader()),
                    render_effect: Some(identity_shader(true)),
                    ..Default::default()
                },
                vec![],
            ),
        )));
        frames.push(capture(&mut renderer, page));
    }
    assert_eq!(
        frames[0].pixels, plain.pixels,
        "hidden backdrop changed page"
    );
    let alpha = GraphicsLayer::composite_alpha_8bit(0.5);
    let mut changed = 0;
    for ((base, middle), opaque) in plain
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(frames[1].pixels.as_chunks::<4>().0)
        .zip(frames[2].pixels.as_chunks::<4>().0)
    {
        if base != opaque {
            changed += 1;
        }
        for channel in 0..4 {
            let expected =
                f32::from(base[channel]) * (1.0 - alpha) + f32::from(opaque[channel]) * alpha;
            assert!(
                (f32::from(middle[channel]) - expected).abs() <= 2.0,
                "group opacity differs: {base:?} {middle:?} {opaque:?}"
            );
        }
    }
    assert!(changed > 0, "opaque backdrop must visibly paint the page");
}

#[test]
fn zero_modulated_primitive_alpha_keeps_a_shader_that_paints_from_nothing() {
    let mut renderer = support::headless_renderer().expect("GPU required for opacity probe");
    let expected = capture(&mut renderer, page_with_empty_layer(painting_shader()));
    let mut page = page_with_empty_layer(painting_shader());
    let Some(RenderNode::Layer(layer)) = page.last_mut() else {
        panic!("effect layer");
    };
    layer.graphics_layer.alpha = 0.0;
    layer.graphics_layer.compositing_strategy = CompositingStrategy::ModulateAlpha;
    let actual = capture(&mut renderer, page);
    assert_eq!(actual.pixels, expected.pixels);
    assert!(
        renderer
            .last_frame_stats()
            .expect("frame statistics")
            .shader_pixels
            > 0
    );
}
