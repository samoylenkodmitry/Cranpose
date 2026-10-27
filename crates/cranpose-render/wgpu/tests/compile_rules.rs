// #1947's rules for shader compiles: a pipeline still compiling may change
// how fast a frame draws, never what it shows.

use cranpose_render_common::graph::{DrawRunNode, PrimitivePhase, RenderGraph, RenderNode};
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
