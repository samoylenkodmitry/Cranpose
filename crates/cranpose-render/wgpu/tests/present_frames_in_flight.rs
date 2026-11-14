// How many frames a producer hands the present thread at once, and the
// frames that go while another is in flight.

use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, ProjectiveTransform, RenderNode},
};
use cranpose_render_wgpu::{
    InlinePresentRuntime, PresentOutcome, PublishOutcome, TestImages, WgpuRenderer,
};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader,
    ShaderPlaceholder,
};

use crate::{
    present_runtime_contract::{
        HEIGHT, WIDTH, direct_graph, drain_outcomes, publish, shadowed_child_graph, threaded_parts,
    },
    shared_test_support, support,
};

// The renderer on an inline runtime with a test swapchain of `images`
// attached; `None` when the machine has no headless device.
fn attached(
    name: &str,
    images: TestImages,
) -> Option<(
    std::sync::MutexGuard<'static, ()>,
    WgpuRenderer,
    InlinePresentRuntime,
)> {
    let (lock, mut renderer, target) = match threaded_parts(wgpu::TextureFormat::Bgra8UnormSrgb) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping {name}: headless WGPU init failed: {err}");
            return None;
        }
    };
    let mut runtime = renderer.init_gpu_inline_for_tests(target);
    attach(&mut renderer, &mut runtime, images);
    Some((lock, renderer, runtime))
}

fn attach(renderer: &mut WgpuRenderer, runtime: &mut InlinePresentRuntime, images: TestImages) {
    let ack = renderer
        .present_thread()
        .and_then(|thread| thread.send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT, images))
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("the test swapchain is attached");
}

// Draws changing frames until two frames in a row drew with every pipeline:
// the pipelines land on the background compiler whenever they do. Leaves no
// frame in flight and the shadowed child described, so a frame of the
// direct graph draws next.
fn hold_frames(renderer: &mut WgpuRenderer, runtime: &mut InlinePresentRuntime) {
    let graphs = [direct_graph(), shadowed_child_graph()];
    let mut settled_frames = 0;
    for frame in 0.. {
        renderer.scene_mut().describe(&graphs[frame % graphs.len()]);
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published,
            "with no frame in flight every frame goes"
        );
        runtime.pump();
        drain_outcomes(renderer);
        let settled = renderer
            .last_frame_stats()
            .is_some_and(|stats| support::pipelines_settled(&stats));
        settled_frames = if settled { settled_frames + 1 } else { 0 };
        if settled_frames >= 2 && frame % graphs.len() == 1 {
            return;
        }
        if !settled {
            support::wait_for_background_compiler_idle();
        }
        assert!(frame < 64, "the pipelines never settled");
    }
}

#[test]
fn depth_one_credit_gates_publish_before_lowering() {
    let Some((_lock, mut renderer, mut runtime)) =
        attached("depth-one credit", TestImages::Kept(3))
    else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    let first = renderer.last_published_frame_id() + 1;
    renderer.scene_mut().describe(&direct_graph());
    assert!(renderer.has_frame_credit());
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first);
    assert!(
        renderer.has_frame_credit(),
        "one rendering plus one waiting: the second credit is what makes \
         the producer and present stages overlap"
    );
    renderer.scene_mut().describe(&shadowed_child_graph());
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first + 1);
    assert!(
        !renderer.has_frame_credit(),
        "two packets in flight is the bound; the producer stalls here so \
         it can never run away from the screen"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::NoCredit,
        "both slots are occupied"
    );
    assert_eq!(
        renderer.last_published_frame_id(),
        first + 1,
        "a NoCredit publish must not build a packet: backpressure lands \
         before the lowering work"
    );

    runtime.pump();
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![
            (first, PresentOutcome::Presented),
            (first + 1, PresentOutcome::Presented)
        ],
        "the offscreen surrogate must render both packets, in publish order"
    );
    assert!(
        renderer.has_frame_credit(),
        "drained returns free the publish credit"
    );
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first + 2);
}

#[test]
fn a_returned_frame_frees_a_credit_it_reports_until_drained() {
    let Some((_lock, mut renderer, mut runtime)) = attached("returned credit", TestImages::Kept(3))
    else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    assert!(!renderer.frame_credit_returned(), "every return is drained");
    for graph in [direct_graph(), shadowed_child_graph()] {
        renderer.scene_mut().describe(&graph);
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published
        );
    }
    assert!(!renderer.has_frame_credit());
    assert!(
        !renderer.frame_credit_returned(),
        "a packet not yet rendered holds its credit"
    );

    runtime.pump();
    assert!(
        !renderer.has_frame_credit(),
        "a return frees its credit only once drained"
    );
    assert!(
        renderer.frame_credit_returned(),
        "the rendered packet's return is waiting to be drained"
    );
    assert!(renderer.drain_present_returns() >= 1);
    assert!(renderer.has_frame_credit());
    assert!(
        !renderer.frame_credit_returned(),
        "a drained return is no longer reported"
    );
}

// A page with a rect of `color` and, when `pane` names one, a small pane
// drawn by that shader, which no other test draws, filled red while it
// compiles.
fn shader_page(color: Color, pane: Option<&str>) -> LayerNode {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // frames in flight stand-in {} {}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.1, 0.8, input.uv.y, 1.0);
         }}",
        pane.unwrap_or_default(),
        std::process::id()
    ));
    shader.set_placeholder(Some(ShaderPlaceholder {
        color: Color(1.0, 0.0, 0.0, 1.0),
        shape: None,
    }));
    shader.set_position_independent(true);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
    };
    let layer = GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(shader)),
        ..GraphicsLayer::default()
    };
    let rect = Rect {
        x: 16.0,
        y: 12.0,
        width: 64.0,
        height: 48.0,
    };
    let mut nodes = vec![support::solid_rect(rect, color)];
    if pane.is_some() {
        let transform = ProjectiveTransform::translation(96.0, 64.0);
        let pane = shared_test_support::layer_node(bounds, transform, layer, Vec::new());
        nodes.push(RenderNode::Layer(Box::new(pane)));
    }
    support::page_graph(WIDTH, HEIGHT, nodes)
}

// Publishes two frames of `pages`, the second while the first is in flight,
// and draws both; whether each went and both presented.
fn overlapping_pair(
    renderer: &mut WgpuRenderer,
    runtime: &mut InlinePresentRuntime,
    pages: [&LayerNode; 2],
) -> bool {
    renderer.scene_mut().describe(pages[0]);
    publish(renderer);
    let overlaps = renderer.has_frame_credit();
    if overlaps {
        renderer.scene_mut().describe(pages[1]);
        publish(renderer);
    }
    runtime.pump();
    runtime.pump();
    let outcomes = drain_outcomes(renderer);
    overlaps
        && outcomes.len() == 2
        && outcomes
            .iter()
            .all(|(_, outcome)| *outcome == PresentOutcome::Presented)
}

// A frame that draws a stand-in while a pipeline compiles keeps its image,
// the part it drew marked: the frames after it, through the landing of the
// pipeline, each go while the frame before is in flight, and none is
// refused for drawing more than its walk took.
#[test]
fn the_frames_after_a_stand_in_go_while_another_is_in_flight() {
    let Some((_lock, mut renderer, mut runtime)) = attached("stand-in frames", TestImages::Kept(3))
    else {
        return;
    };
    let colors = [Color(0.2, 0.7, 0.3, 1.0), Color(0.7, 0.2, 0.3, 1.0)];
    let pages = colors.map(|color| shader_page(color, None));
    // From the window's start until the page's own pipelines landed and its
    // frames overlap, each drawing the part of the window it changes.
    let mut overlapped = 0;
    for _ in 0..80 {
        let pair = overlapping_pair(&mut renderer, &mut runtime, [&pages[0], &pages[1]]);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        let settled = support::pipelines_settled(&stats);
        if !settled {
            support::wait_for_background_compiler_idle();
        }
        let small = settled && stats.repainted_pixels < u64::from(WIDTH * HEIGHT);
        overlapped = if pair && small { overlapped + 1 } else { 0 };
        if overlapped == 3 {
            break;
        }
    }
    assert_eq!(
        overlapped, 3,
        "the page's frames overlap, each drawing a part"
    );
    renderer
        .scene_mut()
        .describe(&shader_page(colors[0], Some("part")));
    publish(&mut renderer);
    runtime.pump();
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.placeholder_draws > 0 && stats.repainted_pixels < u64::from(WIDTH * HEIGHT),
        "the pane's first frame draws its placeholder over a part of the window: {stats:?}"
    );
    assert_eq!(drain_outcomes(&mut renderer).len(), 1);
    let pages = colors.map(|color| shader_page(color, Some("part")));
    let mut settled = 0;
    for pair in 0.. {
        assert!(
            overlapping_pair(&mut renderer, &mut runtime, [&pages[1], &pages[0]]),
            "pair {pair} after the stand-in: a frame waited, or one was refused"
        );
        let stats = renderer.last_frame_stats().expect("frame statistics");
        if support::pipelines_settled(&stats) {
            settled += 1;
        } else {
            assert!(pair < 16, "the shader never landed: {stats:?}");
            settled = 0;
            support::wait_for_background_compiler_idle();
        }
        if settled == 3 {
            break;
        }
    }
}

// A window's first frame draws whole while the shader of its pane compiles,
// a stand-in in the pane's place. The image keeps that frame with the whole
// window marked: the frame after it goes while another is in flight, and
// none is refused for drawing more than its walk took.
#[test]
fn the_frames_after_a_whole_frame_with_a_stand_in_go_while_another_is_in_flight() {
    let Some((_lock, mut renderer, mut runtime)) =
        attached("whole stand-in frames", TestImages::Kept(1))
    else {
        return;
    };
    let colors = [Color(0.2, 0.7, 0.3, 1.0), Color(0.7, 0.2, 0.3, 1.0)];
    let pages = colors.map(|color| shader_page(color, Some("whole")));
    renderer.scene_mut().describe(&pages[0]);
    publish(&mut renderer);
    runtime.pump();
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        stats.placeholder_draws > 0 && stats.repainted_pixels == u64::from(WIDTH * HEIGHT),
        "the window's first frame draws whole with the pane's placeholder: {stats:?}"
    );
    assert_eq!(drain_outcomes(&mut renderer).len(), 1);
    assert!(
        overlapping_pair(&mut renderer, &mut runtime, [&pages[1], &pages[0]]),
        "after the whole frame with a stand-in, a frame waited, or one was refused"
    );
}
