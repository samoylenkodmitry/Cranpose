mod support;

use cranpose_render_common::render_contract::{RenderedFrame, ALL_SHARED_RENDER_CASES};
use cranpose_render_common::Renderer;

#[test]
fn wgpu_renderer_matches_shared_render_contracts() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shared render contract assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    for case in ALL_SHARED_RENDER_CASES {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            renderer.scene_mut().graph = Some(fixture.graph);
            let frame = renderer
                .capture_frame(fixture.width, fixture.height)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to capture shared render case {}: {err:?}",
                        case.name()
                    )
                });
            frames.push(RenderedFrame {
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}
