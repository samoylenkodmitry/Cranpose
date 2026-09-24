use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::pipelines_created;
use cranpose_ui_graphics::{
    BlendMode, Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect,
    RuntimeShader, ShaderTarget, ShaderWarmUp,
};

use crate::{shared_test_support, support};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 24;
const BOUNDS: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: WIDTH as f32,
    height: HEIGHT as f32,
};

/// A shader unlike any other in this process, so its pipeline can only come
/// from this test's warm-up or from the frame that first draws it.
fn probe(tag: &str) -> RuntimeShader {
    RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // {tag}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.0, 0.5, 1.0, 1.0);
         }}"
    ))
}

/// A shader whose picture depends on an override, the way a mask pass does:
/// its pipeline carries the override and the general one cannot stand in.
fn override_probe(tag: &str) -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // {tag}
         override MASK: bool = false;
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return select(vec4<f32>(0.0, 0.5, 1.0, 1.0), vec4<f32>(1.0, 1.0, 1.0, 1.0), MASK);
         }}"
    ));
    shader.set_override("MASK", 1.0);
    shader
}

fn page_draw(shader: RuntimeShader) -> RenderGraph {
    let layer = shared_test_support::layer_node(
        BOUNDS,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
            ..Default::default()
        },
        vec![],
    );
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![
            support::solid_rect(BOUNDS, Color::WHITE),
            RenderNode::Layer(Box::new(layer)),
        ],
    )
}

fn layer_draw(shader: RuntimeShader) -> RenderGraph {
    let layer = shared_test_support::layer_node(
        BOUNDS,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            blend_mode: BlendMode::DstOut,
            ..Default::default()
        },
        vec![support::solid_rect(BOUNDS, Color::WHITE)],
    );
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![
            support::solid_rect(BOUNDS, Color::WHITE),
            RenderNode::Layer(Box::new(layer)),
        ],
    )
}

/// Draws a plain page once, so the pipelines a first capture itself needs
/// (the screenshot converter among them) are built before any count starts.
fn prime(renderer: &mut support::LockedRenderer) {
    let _ = support::capture_graph(
        renderer,
        support::page_graph(
            WIDTH,
            HEIGHT,
            vec![support::solid_rect(BOUNDS, Color::WHITE)],
        ),
        WIDTH,
        HEIGHT,
    );
    support::wait_for_background_compiler_idle();
}

/// Pipelines the frame drawing `graph` built on this thread, with the
/// shader draw itself asserted so a count of zero means a ready pipeline
/// and not a draw that never happened. Specialized shape pipelines, which
/// the child rect inside a layer would ask for, compile inside the frame
/// off Vulkan, so the count is only about the shader's pipeline once shape
/// variants are off.
fn frame_thread_compiles_for(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> u64 {
    let before = pipelines_created();
    let _ = support::capture_graph(renderer, graph, WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.shader_pixels > 0 || stats.effect_applies > 0,
        "the shader must draw"
    );
    pipelines_created() - before
}

#[test]
fn warmed_shaders_draw_without_a_frame_thread_compile() {
    let mut renderer = support::headless_renderer().expect("GPU required for shader warm-up");
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SHAPE_VARIANTS", Some("0"));
    let page = probe("page target");
    let layer = probe("layer target");
    let masked = override_probe("override target");
    renderer.warm_shaders([
        ShaderWarmUp {
            shader: page.clone(),
            target: ShaderTarget::Page,
        },
        ShaderWarmUp {
            shader: layer.clone(),
            target: ShaderTarget::Layer,
        },
        ShaderWarmUp {
            shader: masked.clone(),
            target: ShaderTarget::Page,
        },
    ]);
    support::reinit_gpu(&mut renderer).expect("GPU re-init applies the warm-ups");
    prime(&mut renderer);
    assert_eq!(
        frame_thread_compiles_for(&mut renderer, page_draw(page)),
        0,
        "a shader warmed for the page must find its composite pipeline ready"
    );
    let layer_compiles = frame_thread_compiles_for(&mut renderer, layer_draw(layer));
    let masked_compiles = frame_thread_compiles_for(&mut renderer, page_draw(masked));
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SHAPE_VARIANTS", None);
    assert_eq!(
        layer_compiles, 0,
        "a shader warmed for its layer must find its replace pipeline ready"
    );
    assert_eq!(
        masked_compiles, 0,
        "a warm-up carries the shader's overrides: the pipeline the draw needs, not the general one"
    );
}

#[test]
fn an_unregistered_shader_still_compiles_inside_its_first_frame() {
    let mut renderer = support::headless_renderer().expect("GPU required for shader warm-up");
    prime(&mut renderer);
    assert!(
        frame_thread_compiles_for(&mut renderer, page_draw(probe("cold page"))) >= 1,
        "the counter must see an in-frame compile, or the warmed case proves nothing"
    );
}
