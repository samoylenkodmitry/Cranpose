use cranpose_render_common::render_contract::ALL_SHARED_RENDER_CASES;
use cranpose_render_pixels::{draw_scene, Scene};

#[test]
fn pixels_renderer_matches_shared_render_contracts() {
    for case in ALL_SHARED_RENDER_CASES {
        let fixture = case.fixture();
        let mut scene = Scene::new();
        scene.graph = Some(fixture.graph);
        let mut frame = vec![0u8; (fixture.width * fixture.height * 4) as usize];
        draw_scene(&mut frame, fixture.width, fixture.height, &scene);
        case.assert_frame(&frame, fixture.width, fixture.height);
    }
}
