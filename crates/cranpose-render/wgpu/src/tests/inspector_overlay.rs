use cranpose_core::NodeId;
use cranpose_render_common::{Renderer, graph::LayerNode};
use cranpose_ui_graphics::Size;

use super::*;

fn assert_inspector(renderer: &WgpuRenderer) {
    let overlay = renderer.frontend.dev_overlay.root_layer().expect("overlay");
    assert!(
        overlay
            .children()
            .next_back()
            .is_some_and(|layer| layer.node_id() == Some(NodeId(123)))
    );
    assert!(renderer.frontend.scene.root_layer().is_none());
}

#[test]
fn inspector_survives_fps_overlay_refresh_and_clearing() {
    let mut renderer = WgpuRenderer::new(&[]);
    renderer.set_inspector_overlay(Some(LayerNode {
        node_id: Some(NodeId(123)),
        ..Default::default()
    }));
    assert_inspector(&renderer);
    let overlay = test_support::prepared_text("60 FPS");
    renderer.draw_dev_overlay(&overlay, Size::new(800.0, 600.0));
    assert_inspector(&renderer);
    renderer.frontend.clear_fps_overlay();
    assert_inspector(&renderer);
    renderer.set_inspector_overlay(None);
    assert!(renderer.frontend.dev_overlay.root().is_none());
}
