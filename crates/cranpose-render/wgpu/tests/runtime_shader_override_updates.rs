mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{ProjectiveTransform, RenderNode},
};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader,
};

#[test]
fn changing_cloned_overrides_replaces_the_pipeline_without_changing_the_original() {
    let mut renderer = support::headless_renderer().expect("GPU required for override pixel guard");
    let mut original = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         override RED: bool = false;
         override SCALE: f32 = 1.0;
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return select(vec4<f32>(0.0, 0.0, SCALE, 1.0), vec4<f32>(SCALE, 0.0, 0.0, 1.0), RED);
         }}"
    ));
    original.set_override("RED", 0.0);
    original.set_override("SCALE", 1.0);
    let _ = original.overrides_hash();
    let mut raised = original.clone();
    raised.set_override("RED", 1.0);
    let _ = raised.overrides_hash();
    let mut cleared = raised.clone();
    assert!(cleared.clear_override("RED"));
    let mut lowered = raised.clone();
    lowered.set_override("RED", 0.0);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 32.0,
        height: 24.0,
    };
    for (shader, expected) in [
        (original.clone(), [0, 0, 255, 255]),
        (raised.clone(), [255, 0, 0, 255]),
        (cleared, [0, 0, 255, 255]),
        (lowered, [0, 0, 255, 255]),
        (raised, [255, 0, 0, 255]),
        (original, [0, 0, 255, 255]),
    ] {
        let layer = shared_test_support::layer_node(
            bounds,
            ProjectiveTransform::identity(),
            GraphicsLayer {
                render_effect: Some(RenderEffect::runtime_shader(shader)),
                ..Default::default()
            },
            vec![support::solid_rect(bounds, Color::WHITE)],
        );
        renderer.scene_mut().graph = Some(support::page_graph(
            32,
            24,
            vec![RenderNode::Layer(Box::new(layer))],
        ));
        let frame = renderer.capture_frame(32, 24).expect("override frame");
        assert_eq!(frame.pixels.len(), 32 * 24 * 4);
        assert!(
            frame
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == expected),
            "cached pipeline must follow the current shader overrides: expected {expected:?}"
        );
    }
}
