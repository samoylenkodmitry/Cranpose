use cranpose_core::NodeId;
use cranpose_render_common::graph::{CachePolicy, LayerNode, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader,
};

use crate::{
    shared_test_support, support,
    support::bar_scene::{BAR, HEIGHT, WIDTH},
};

const INK: Rect = Rect {
    x: 40.0,
    y: 36.0,
    width: 60.0,
    height: 20.0,
};

// A shader that stands for costly work over still content: it swaps the
// red and green of what it is given.
fn bake() -> RuntimeShader {
    RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{ let c = textureSampleLevel(input_texture, input_sampler, input.uv, 0.0); return vec4<f32>(c.g, c.r, c.b, 1.0); }}"
    ))
}

// A shader whose uniform changes every frame, as a light that moves.
fn light(frame: u32) -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{ let c = textureSampleLevel(input_texture, input_sampler, input.uv, 0.0); return vec4<f32>(c.rgb * u[0].x, c.a); }}"
    ));
    shader.set_float(0, 0.5 + 0.01 * frame as f32);
    shader
}

// The striped page with a layer whose shader moves every frame over a
// layer whose shader works over one still rectangle, the outer layer
// moved down by `shift` pixels each frame.
fn page(frame: u32, shift: f32) -> LayerNode {
    let mut page = support::striped_page(WIDTH, HEIGHT);
    let mut baked = shared_test_support::layer_node(
        BAR,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(bake())),
            ..Default::default()
        },
        vec![support::solid_rect(INK, Color::WHITE)],
    );
    baked.node_id = Some(NodeId(7));
    baked.cache_policy = CachePolicy::Auto;
    let mut lit = shared_test_support::layer_node(
        BAR,
        ProjectiveTransform::translation(0.0, shift * frame as f32),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(light(frame))),
            ..Default::default()
        },
        vec![RenderNode::Layer(Box::new(baked))],
    );
    lit.node_id = Some(NodeId(8));
    lit.cache_policy = CachePolicy::Auto;
    page.push(RenderNode::Layer(Box::new(lit)));
    support::page_graph(WIDTH, HEIGHT, page)
}

// The renderer keeps the surface of a layer whose content holds still,
// the output of an effect inside it included: under a shader that changes
// every frame, the still layer's shader runs in the first frame only,
// also while the outer layer moves by fractions of a pixel. The Explainer
// paints its oil painting this way once and lights it every frame.
#[test]
fn an_effect_under_a_moving_effect_runs_once() {
    let mut renderer = support::headless_renderer().expect("GPU required for the cache probe");
    support::capture_graph(&mut renderer, &page(0, 0.0), WIDTH, HEIGHT);
    let first = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        first.isolated_layer_renders >= 2,
        "the first frame draws both layers"
    );
    for frame in 1..8 {
        let shift = if frame < 4 { 0.0 } else { 0.37 };
        support::capture_graph(&mut renderer, &page(frame, shift), WIDTH, HEIGHT);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame} drew a still layer again"
        );
        assert_eq!(
            stats.shader_pixels * 2,
            first.shader_pixels,
            "frame {frame} ran the still layer's shader again"
        );
    }
    assert_eq!(renderer.device_error_count_for_tests(), 0);
}
