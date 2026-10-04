//! A frame never waits for an effect's pipelines to compile: until they
//! land, a backdrop fills its shape with its shader's placeholder, a layer
//! effect leaves the layer's content as it is, and the renderer asks for the
//! frame that replaces them.

use cranpose_render_common::{
    Renderer,
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader,
};

use crate::{shared_test_support, support};

const SIZE: u32 = 32;
const PANE: Rect = Rect {
    x: 8.0,
    y: 8.0,
    width: 16.0,
    height: 16.0,
};
const RED: Color = Color(1.0, 0.0, 0.0, 1.0);

/// A shader no other test draws, painting the pane blue, filled with
/// `placeholder` while it compiles.
fn shader(tag: &str, placeholder: Option<Color>) -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // {tag} {}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.0, 0.0, 1.0, 1.0);
         }}",
        std::process::id()
    ));
    shader.set_placeholder(placeholder);
    shader
}

/// A white page with a pane drawn by `layer` over `content`.
fn page(layer: GraphicsLayer, content: Vec<RenderNode>) -> RenderGraph {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    let pane = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: PANE.width,
            height: PANE.height,
        },
        ProjectiveTransform::translation(PANE.x, PANE.y),
        layer,
        content,
    );
    support::page_graph(
        SIZE,
        SIZE,
        vec![
            support::solid_rect(full, Color::WHITE),
            RenderNode::Layer(Box::new(pane)),
        ],
    )
}

fn backdrop_page(shader: RuntimeShader) -> RenderGraph {
    page(
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::runtime_shader(shader)),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )
}

/// A green square drawn as the pane's content, through `effect`.
fn effect_page(effect: Option<RenderEffect>) -> RenderGraph {
    let square = support::solid_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: PANE.width,
            height: PANE.height,
        },
        Color(0.0, 1.0, 0.0, 1.0),
    );
    page(
        GraphicsLayer {
            render_effect: effect,
            ..GraphicsLayer::default()
        },
        vec![square],
    )
}

/// What an app's renderer draws for `graph` once every pipeline is built.
fn settled(graph: RenderGraph) -> CapturedFrame {
    let mut reference = support::LockedRenderer::beside_locked().expect("reference renderer");
    support::capture_graph(&mut reference, graph, SIZE, SIZE)
}

/// The first frame of `graph` on a renderer whose pipelines compile in the
/// background, asserted to have drawn one placeholder, and that renderer.
fn first_frame(graph: &RenderGraph) -> (support::LockedRenderer, CapturedFrame) {
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    let frame = support::capture_graph(&mut renderer, graph.clone(), SIZE, SIZE);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(
        stats.placeholder_draws, 1,
        "the effect must wait for its pipeline"
    );
    assert!(
        renderer.needs_frame_warmup(),
        "a frame drawn without an effect asks for the frame that replaces it"
    );
    (renderer, frame)
}

/// Captures `graph` until the effect draws, then asserts that frame is the
/// settled picture and that the renderer stops asking for frames.
fn assert_lands(renderer: &mut support::LockedRenderer, graph: RenderGraph) {
    let (drawn, _) = support::capture_drawn(renderer, &graph, SIZE, SIZE);
    support::assert_same_bytes(
        "the frame after the pipeline lands",
        SIZE,
        &drawn.pixels,
        &settled(graph.clone()).pixels,
    );
    for _ in 0..4 {
        if !renderer.needs_frame_warmup() {
            return;
        }
        support::capture_graph(renderer, graph.clone(), SIZE, SIZE);
    }
    panic!("the renderer kept asking for frames after the effect drew");
}

fn pane_colors(frame: &CapturedFrame) -> Vec<[u8; 4]> {
    let mut colors = support::region_pixels(frame, PANE)
        .as_chunks::<4>()
        .0
        .to_vec();
    colors.sort_unstable();
    colors.dedup();
    colors
}

#[test]
fn a_backdrop_fills_its_shape_with_its_placeholder_until_its_pipeline_lands() {
    let graph = backdrop_page(shader("placeholder backdrop", Some(RED)));
    let (mut renderer, first) = first_frame(&graph);
    assert_eq!(pane_colors(&first), [[255, 0, 0, 255]]);
    let without_pane = settled(support::page_graph(
        SIZE,
        SIZE,
        vec![support::solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: SIZE as f32,
                height: SIZE as f32,
            },
            Color::WHITE,
        )],
    ));
    let outside = |frame: &CapturedFrame| {
        let mut pixels = frame.pixels.clone();
        for y in PANE.y as u32..(PANE.y + PANE.height) as u32 {
            let start = ((y * SIZE + PANE.x as u32) * 4) as usize;
            pixels[start..start + (PANE.width as usize) * 4].fill(0);
        }
        pixels
    };
    assert_eq!(
        outside(&first),
        outside(&without_pane),
        "the placeholder stays inside its shape"
    );
    assert_lands(&mut renderer, graph);
}

#[test]
fn a_backdrop_without_a_placeholder_leaves_its_shape_to_what_is_beneath() {
    let graph = backdrop_page(shader("bare backdrop", None));
    let (mut renderer, first) = first_frame(&graph);
    assert_eq!(pane_colors(&first), [[255, 255, 255, 255]]);
    assert_lands(&mut renderer, graph);
}

#[test]
fn a_layer_effect_leaves_the_content_as_it_is_until_its_pipeline_lands() {
    let effect = RenderEffect::runtime_shader(shader("layer effect", Some(RED)));
    let graph = effect_page(Some(effect));
    let (mut renderer, first) = first_frame(&graph);
    support::assert_same_bytes(
        "the content without its effect",
        SIZE,
        &first.pixels,
        &settled(effect_page(None)).pixels,
    );
    assert_lands(&mut renderer, graph);
}

#[test]
fn a_layer_drawn_only_by_its_shader_shows_the_placeholder_until_its_pipeline_lands() {
    let mut only = shader("shader-only layer", Some(RED));
    only.set_position_independent(true);
    let graph = page(
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(only)),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    );
    let (mut renderer, first) = first_frame(&graph);
    assert_eq!(pane_colors(&first), [[255, 0, 0, 255]]);
    assert_lands(&mut renderer, graph);
}
