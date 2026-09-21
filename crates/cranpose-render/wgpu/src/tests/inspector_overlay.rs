use cranpose_render_common::graph::LayerNode;

use super::*;

fn assert_inspector(renderer: &WgpuRenderer) {
    let graph = renderer
        .frontend
        .dev_overlay_graph
        .as_ref()
        .expect("overlay");
    assert!(
        matches!(graph.root.children.last(), Some(RenderNode::Layer(layer)) if layer.node_id == Some(123))
    );
    assert!(renderer.frontend.scene.graph.is_none());
}

#[test]
fn inspector_survives_fps_overlay_refresh_and_clearing() {
    let mut renderer = WgpuRenderer::new(&[]);
    renderer.set_inspector_overlay(Some(RenderGraph::new(LayerNode {
        node_id: Some(123),
        ..Default::default()
    })));
    assert_inspector(&renderer);
    renderer.draw_dev_overlay("60 FPS", Size::new(800.0, 600.0));
    assert_inspector(&renderer);
    renderer.frontend.clear_fps_overlay();
    assert_inspector(&renderer);
    renderer.set_inspector_overlay(None);
    assert!(renderer.frontend.dev_overlay_graph.is_none());
}
