use cranpose_render_common::render_contract::{RenderedFrame, ALL_SHARED_RENDER_CASES};
use cranpose_render_pixels::{draw_scene, Scene};

#[test]
fn pixels_warning_state_is_scene_owned() {
    let draw_source = std::fs::read_to_string("src/draw.rs").expect("read pixels draw source");
    let pipeline_source =
        std::fs::read_to_string("src/pipeline.rs").expect("read pixels pipeline source");

    for source in [draw_source.as_str(), pipeline_source.as_str()] {
        assert!(
            !source.contains("static REPORTED_UNSUPPORTED_PIXELS_"),
            "pixels renderer warning state must be owned by the renderer scene, not process globals"
        );
        assert!(
            !source.contains("AtomicBool"),
            "pixels renderer warning suppression must not use process-global atomics"
        );
    }
}

#[test]
fn pixels_text_font_state_is_renderer_owned() {
    let draw_source = std::fs::read_to_string("src/draw.rs").expect("read pixels draw source");

    assert!(
        !draw_source.contains("static FONT")
            && !draw_source.contains("Lazy<Option<SoftwareTextFont>>"),
        "pixels renderer text font state must not be a process-global cache"
    );
    assert!(
        draw_source.contains("pub struct PixelsTextResources")
            && draw_source.contains("draw_scene_with_text_resources"),
        "pixels text resources should be explicit renderer-owned draw inputs"
    );
}

#[test]
fn pixels_renderer_matches_shared_render_contracts() {
    let app_context = cranpose_ui::AppContext::new();
    for case in ALL_SHARED_RENDER_CASES {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            let mut scene = Scene::new();
            scene.graph = Some(fixture.graph);
            let mut frame = vec![0u8; (fixture.width * fixture.height * 4) as usize];
            app_context.enter(|| draw_scene(&mut frame, fixture.width, fixture.height, &scene));
            frames.push(RenderedFrame {
                width: fixture.width,
                height: fixture.height,
                pixels: frame,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}
