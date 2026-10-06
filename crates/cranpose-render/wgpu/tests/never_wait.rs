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
    Color, GraphicsLayer, PlaceholderShape, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect,
    RuntimeShader, ShaderPlaceholder,
};

use crate::{shared_test_support, support};

const SIZE: u32 = 32;
const PANE: Rect = Rect {
    x: 8.0,
    y: 8.0,
    width: 16.0,
    height: 16.0,
};
const RED: ShaderPlaceholder = ShaderPlaceholder {
    color: Color(1.0, 0.0, 0.0, 1.0),
    shape: None,
};

/// A shader no other test draws, painting the pane blue, filled with
/// `placeholder` while it compiles.
fn shader(tag: &str, placeholder: Option<ShaderPlaceholder>) -> RuntimeShader {
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
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: PANE.width,
        height: PANE.height,
    };
    page_with(
        bounds,
        ProjectiveTransform::translation(PANE.x, PANE.y),
        layer,
        content,
    )
}

/// A white page with a layer of `bounds` placed by `transform`.
fn page_with(
    bounds: Rect,
    transform: ProjectiveTransform,
    layer: GraphicsLayer,
    content: Vec<RenderNode>,
) -> RenderGraph {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    let pane = shared_test_support::layer_node(bounds, transform, layer, content);
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
    effect_page_with(GraphicsLayer::default(), effect)
}

/// [`effect_page`] on a pane `layer` describes.
fn effect_page_with(layer: GraphicsLayer, effect: Option<RenderEffect>) -> RenderGraph {
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
            ..layer
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

/// A layer drawn only by a shader no other test draws, with the red
/// placeholder.
fn shader_only(tag: &str) -> GraphicsLayer {
    let mut only = shader(tag, Some(RED));
    only.set_position_independent(true);
    GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(only)),
        ..GraphicsLayer::default()
    }
}

/// Asserts `graph`'s first frame shows the red placeholder over the pane,
/// and that the shader lands.
fn assert_placeholder_then_lands(graph: RenderGraph) {
    let (mut renderer, first) = first_frame(&graph);
    assert_eq!(pane_colors(&first), [[255, 0, 0, 255]]);
    assert_lands(&mut renderer, graph);
}

#[test]
fn a_layer_drawn_only_by_its_shader_shows_the_placeholder_until_its_pipeline_lands() {
    assert_placeholder_then_lands(page(shader_only("shader-only layer"), Vec::new()));
}

/// The red placeholder over the middle half of the pane, its corners
/// rounded by a quarter of the pane.
const MIDDLE: ShaderPlaceholder = ShaderPlaceholder {
    shape: Some(PlaceholderShape {
        bounds: Rect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        corner_radius: PANE.width * 0.25,
    }),
    ..RED
};

/// Asserts `frame` shows `inside` in the middle of the pane and the white
/// page outside [`MIDDLE`]'s shape, its rounded corner included.
fn assert_in_middle(frame: &CapturedFrame, inside: [u8; 4]) {
    let pixel = |x: f32, y: f32| {
        let start = (((PANE.y + y) as u32 * SIZE + (PANE.x + x) as u32) * 4) as usize;
        [0, 1, 2, 3].map(|channel| frame.pixels[start + channel])
    };
    assert_eq!(pixel(8.0, 8.0), inside, "the shape's middle");
    assert_eq!(pixel(2.0, 2.0), [255; 4], "outside the shape");
    assert_eq!(pixel(4.0, 4.0), [255; 4], "the shape's rounded corner");
}

#[test]
fn a_placeholder_with_a_shape_fills_only_that_rounded_rectangle() {
    let graph = backdrop_page(shader("shaped placeholder", Some(MIDDLE)));
    let (mut renderer, first) = first_frame(&graph);
    assert_in_middle(&first, [255, 0, 0, 255]);
    assert_lands(&mut renderer, graph);
}

/// A glass that masks its layer's content keeps the content in its shape
/// while it compiles.
#[test]
fn a_layer_effect_with_a_shaped_placeholder_keeps_the_content_in_that_shape() {
    let effect = RenderEffect::runtime_shader(shader("shaped layer effect", Some(MIDDLE)));
    let graph = effect_page(Some(effect));
    let (mut renderer, first) = first_frame(&graph);
    assert_in_middle(&first, [0, 255, 0, 255]);
    assert_lands(&mut renderer, graph);
}

#[test]
fn a_shadow_is_left_out_until_its_blur_lands() {
    let square = || {
        support::solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: PANE.width,
                height: PANE.height,
            },
            Color(0.0, 1.0, 0.0, 1.0),
        )
    };
    let shadow = support::drop_shadow(
        Rect {
            x: 6.0,
            y: 6.0,
            width: PANE.width,
            height: PANE.height,
        },
        Color::BLACK,
        6.0,
    );
    let graph = page(GraphicsLayer::default(), vec![shadow, square()]);
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    let first = support::capture_graph(&mut renderer, graph.clone(), SIZE, SIZE);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.placeholder_draws > 0,
        "the shadow must wait for its blur"
    );
    assert!(renderer.needs_frame_warmup());
    support::assert_same_bytes(
        "the page without its shadow",
        SIZE,
        &first.pixels,
        &settled(page(GraphicsLayer::default(), vec![square()])).pixels,
    );
    assert_lands(&mut renderer, graph);
}

/// A frame that drew placeholders is not the final picture while their
/// pipelines compile; the renderer stops saying so once they land, with no
/// frame drawn meanwhile, and the next frame draws the effects.
#[test]
fn a_frame_with_placeholders_awaits_their_pipelines_until_they_land() {
    let graph = backdrop_page(shader("awaited backdrop", Some(RED)));
    let (mut renderer, _) = first_frame(&graph);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while renderer.awaits_pipelines() {
        assert!(
            std::time::Instant::now() < deadline,
            "the pipelines never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    support::capture_graph(&mut renderer, graph, SIZE, SIZE);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(stats.placeholder_draws, 0);
    assert!(!renderer.awaits_pipelines());
}

/// A scaled layer that is all its shader draws through a surface of its own,
/// and still shows the placeholder while the shader compiles.
#[test]
fn a_scaled_layer_drawn_only_by_its_shader_shows_the_placeholder() {
    assert_placeholder_then_lands(page_with(
        Rect {
            x: 0.0,
            y: 0.0,
            width: PANE.width * 0.5,
            height: PANE.height * 0.5,
        },
        ProjectiveTransform::uniform_scale(2.0)
            .then(ProjectiveTransform::translation(PANE.x, PANE.y)),
        shader_only("scaled shader-only layer"),
        Vec::new(),
    ));
}
