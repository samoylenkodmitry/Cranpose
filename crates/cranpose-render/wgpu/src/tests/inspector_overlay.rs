use cranpose_render_common::{Renderer, scene_store::OverlayLayer};
use cranpose_ui_graphics::{Point, Rect, Size};

use super::*;

const INSPECTOR: Size = Size {
    width: 123.0,
    height: 45.0,
};

fn assert_inspector(renderer: &WgpuRenderer) {
    let overlay = renderer.frontend.dev_overlay.root_layer().expect("overlay");
    assert!(
        overlay
            .children()
            .next_back()
            .is_some_and(|layer| layer.node_rect() == Rect::from_size(INSPECTOR))
    );
    assert!(renderer.frontend.scene.root_layer().is_none());
}

#[test]
fn inspector_survives_fps_overlay_refresh_and_clearing() {
    let mut renderer = WgpuRenderer::new(&[]);
    renderer.set_inspector_overlay(Some(OverlayLayer {
        size: INSPECTOR,
        origin: Point::default(),
        primitives: Vec::new(),
    }));
    assert_inspector(&renderer);
    let overlay = test_support::prepared_text("60 FPS");
    renderer.draw_dev_overlay(&overlay, Size::new(800.0, 600.0));
    assert_inspector(&renderer);
    assert_eq!(
        renderer
            .frontend
            .dev_overlay
            .root_layer()
            .expect("overlay")
            .children()
            .count(),
        2,
        "the frame counter and the inspector"
    );
    renderer.frontend.clear_fps_overlay();
    assert_inspector(&renderer);
    renderer.set_inspector_overlay(None);
    assert!(renderer.frontend.dev_overlay.root().is_none());
}
