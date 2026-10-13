//! A pipeline built from its two stages specialized by their override
//! constants and folded draws what the same pipeline draws when the device
//! specializes its shared module: shapes of each kind, the blit of a layer
//! at half opacity and the projective blit of a turned one.

use cranpose_render_common::{
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect, Size};

use crate::{
    shared_test_support,
    support::{self, SIZE, record_mixed_scene},
};

const NO_SHADER_FOLD: &str = "CRANPOSE_NO_SHADER_FOLD";

fn shapes() -> RenderNode {
    let mut scope = DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32));
    record_mixed_scene(&mut scope);
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        scope.into_primitives(),
    ))
}

/// The shapes on the page, again in a layer at half opacity, and again in
/// that layer turned about its centre.
fn scene() -> RenderGraph {
    let bounds = Rect::from_size(Size::new(SIZE as f32, SIZE as f32));
    let faded = GraphicsLayer {
        alpha: 0.5,
        ..GraphicsLayer::default()
    };
    let turned = GraphicsLayer {
        alpha: 0.5,
        rotation_z: 12.0,
        ..GraphicsLayer::default()
    };
    let half = SIZE as f32 / 2.0;
    let turn = layer_transform_to_parent(bounds, Point::new(half, half), &turned);
    let layer = |graphics: GraphicsLayer, transform| {
        RenderNode::Layer(Box::new(shared_test_support::layer_node(
            bounds,
            transform,
            graphics,
            vec![shapes()],
        )))
    };
    support::page_graph(
        SIZE,
        SIZE,
        vec![
            shapes(),
            layer(faded, ProjectiveTransform::translation(0.0, 0.0)),
            layer(turned, turn),
        ],
    )
}

#[test]
fn folded_stages_draw_what_stages_the_device_specializes_draw() {
    let before = cranpose_render_wgpu::shader_stages_folded();
    let flattened_before = cranpose_render_wgpu::shader_stages_flattened();
    let folded = {
        let Ok(mut renderer) = support::headless_renderer() else {
            return;
        };
        support::capture_graph(&mut renderer, scene(), SIZE, SIZE).pixels
    };
    assert!(
        cranpose_render_wgpu::shader_stages_folded() > before,
        "the frame's pipelines took folded stages"
    );
    assert!(
        cranpose_render_wgpu::shader_stages_flattened() > flattened_before,
        "the frame's pipelines took flattened stages"
    );
    cranpose_render_wgpu::set_debug_toggle(NO_SHADER_FOLD, Some("1"));
    let shared = support::headless_renderer()
        .map(|mut renderer| support::capture_graph(&mut renderer, scene(), SIZE, SIZE).pixels);
    cranpose_render_wgpu::set_debug_toggle(NO_SHADER_FOLD, None);
    let Ok(shared) = shared else {
        return;
    };
    let differing = support::differing_pixels(SIZE, &folded, &shared);
    assert!(
        differing.is_empty(),
        "folded stages changed pixels: {}",
        support::describe_differing(&differing)
    );
}
