//! A frame whose passes hand the pipelines they are missing to the
//! compiler's threads draws what a frame that builds each where it is first
//! needed draws: the shapes beyond a pass's first, and a layer's composite
//! blit, which the pass queues as it prepares the composite and builds or
//! waits for where it draws it. A turned layer also queues the blit it draws
//! with once a later frame turns it back onto the pixel grid.

use cranpose_render_common::{
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect, Size};

use crate::{
    shared_test_support,
    support::{self, LockedRenderer, SIZE, record_mixed_scene},
};

/// Solid, gradient and stroked shapes, each kind with a pipeline of its own.
fn shapes() -> RenderNode {
    let mut scope = DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32));
    record_mixed_scene(&mut scope);
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        scope.into_primitives(),
    ))
}

fn page_bounds() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    }
}

/// The shapes on the page, and again in a layer at half opacity, which
/// draws into a target of its own and is composited from it.
fn scene() -> RenderGraph {
    let faded = shared_test_support::layer_node(
        page_bounds(),
        ProjectiveTransform::translation(0.0, 0.0),
        GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        },
        vec![shapes()],
    );
    support::page_graph(
        SIZE,
        SIZE,
        vec![shapes(), RenderNode::Layer(Box::new(faded))],
    )
}

#[test]
fn a_frame_building_its_missing_pipelines_side_by_side_draws_as_one_building_them_in_turn() {
    let expected = {
        let Ok(mut inline) = support::headless_renderer() else {
            return;
        };
        support::capture_graph(&mut inline, scene(), SIZE, SIZE)
    };
    let Ok(mut background) = support::headless_renderer_compiling_in_background() else {
        return;
    };
    let frame = support::capture_graph(&mut background, scene(), SIZE, SIZE);
    let differing = support::differing_pixels(SIZE, &expected.pixels, &frame.pixels);
    assert!(
        differing.is_empty(),
        "the first frame differs where its pipelines were built side by side: {}",
        support::describe_differing(&differing)
    );
}

/// The shapes on the page, and again in a layer at half opacity turned by
/// `degrees` about its centre: projected from its target while turned,
/// blitted one to one at zero.
fn turned(degrees: f32) -> RenderGraph {
    let turn = GraphicsLayer {
        alpha: 0.5,
        rotation_z: degrees,
        ..GraphicsLayer::default()
    };
    let half = SIZE as f32 / 2.0;
    let layer = shared_test_support::layer_node(
        page_bounds(),
        layer_transform_to_parent(page_bounds(), Point::new(half, half), &turn),
        turn,
        vec![shapes()],
    );
    support::page_graph(
        SIZE,
        SIZE,
        vec![shapes(), RenderNode::Layer(Box::new(layer))],
    )
}

/// A frame with the layer turned, then one with it back on the grid.
fn turn_back(renderer: &mut LockedRenderer) -> Vec<u8> {
    support::capture_graph(renderer, turned(12.0), SIZE, SIZE);
    support::capture_graph(renderer, turned(0.0), SIZE, SIZE).pixels
}

#[test]
fn a_layer_turned_back_onto_the_pixel_grid_draws_as_one_building_its_blit_in_turn() {
    let expected = {
        let Ok(mut inline) = support::headless_renderer() else {
            return;
        };
        turn_back(&mut inline)
    };
    let Ok(mut background) = support::headless_renderer_compiling_in_background() else {
        return;
    };
    let frame = turn_back(&mut background);
    let differing = support::differing_pixels(SIZE, &expected, &frame);
    assert!(
        differing.is_empty(),
        "the frame back on the grid differs where its blit was queued while the layer turned: {}",
        support::describe_differing(&differing)
    );
}
