// #1947's rules for shader compiles: a pipeline still compiling may change
// how fast a frame draws, never what it shows.

use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Size};

use crate::support::{self, LockedRenderer};

// Shapes of every kind the support scenes record: solid, gradient and
// stroked rects, rounded rects in each tile mode, on one 256 by 256 page.
fn every_shape() -> RenderGraph {
    let mut recording = cranpose_ui_graphics::CommandRecording::default();
    let mut scope = DrawScopeDefault::new(Size::new(256.0, 256.0), &mut recording);
    support::record_gradient_fill_scene(&mut scope);
    support::record_mixed_scene(&mut scope);
    support::page_graph(
        256,
        256,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
    )
}

// A specialized shape pipeline is the general program with constants folded,
// so it draws the general program's bytes, and putting one in place of the
// other under a frame on screen changes nothing.
#[test]
fn a_shape_specialization_draws_the_bytes_of_the_general_program() {
    let graph = every_shape();
    let capture = || {
        let mut renderer = LockedRenderer::beside_locked().expect("renderer");
        support::capture_graph(&mut renderer, graph.clone(), 256, 256).pixels
    };
    let specialized = capture();
    cranpose_core::debug_toggles::set_debug_toggle("CRANPOSE_SHAPE_VARIANTS", Some("0"));
    let general = capture();
    cranpose_core::debug_toggles::set_debug_toggle("CRANPOSE_SHAPE_VARIANTS", None);
    support::assert_same_bytes("specialized against general", 256, &specialized, &general);
}

// A renderer builds its base tier once its first frame is drawn. From then
// on, shapes no frame drew before draw through it while their own
// pipelines compile: the bytes a renderer that builds every pipeline where
// it needs it draws, and no frame after is needed to correct them.
#[test]
fn shapes_new_after_the_base_landed_draw_their_exact_bytes() {
    let graph = every_shape();
    let exact = {
        let mut reference = LockedRenderer::beside_locked().expect("reference renderer");
        support::capture_graph(&mut reference, graph.clone(), 256, 256).pixels
    };
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    let empty = support::page_graph(256, 256, Vec::new());
    for _ in 0..2 {
        support::capture_graph(&mut renderer, empty.clone(), 256, 256);
    }
    support::wait_for_background_compiler_idle();
    let frame = support::capture_graph(&mut renderer, graph, 256, 256);
    let asks_again = renderer.needs_frame_warmup();
    drop(renderer);
    support::assert_same_bytes("new shapes over the base tier", 256, &frame.pixels, &exact);
    assert!(
        !asks_again,
        "a frame drawn through the base tier asks for no frame after it"
    );
}
