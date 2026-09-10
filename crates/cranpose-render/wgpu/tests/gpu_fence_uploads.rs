mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};

#[test]
fn profiled_frames_keep_current_uniforms_and_geometry() {
    if std::env::var_os("CRANPOSE_FENCE_TEST_CHILD").is_none() {
        for mode in ["off", "frame", "1"] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "profiled_frames_keep_current_uniforms_and_geometry",
                    "--nocapture",
                ])
                .env("CRANPOSE_FENCE_TEST_CHILD", mode);
            if mode == "off" {
                command.env_remove("CRANPOSE_GPU_FENCE_PROFILE");
            } else {
                command.env("CRANPOSE_GPU_FENCE_PROFILE", mode);
            }
            let result = command.output().expect("run isolated fence mode");
            assert!(
                result.status.success(),
                "mode {mode}:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return;
    }
    let mut renderer = support::headless_renderer().expect("headless renderer");
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 40.0,
    };
    for frame in 0..6 {
        let color = if frame % 2 == 0 {
            Color::RED
        } else {
            Color::GREEN
        };
        let mut children = Vec::new();
        for column in 0..3 {
            children.push(RenderNode::Layer(Box::new(
                shared_test_support::layer_node(
                    bounds,
                    ProjectiveTransform::translation(
                        8.0 + column as f32 * 48.0,
                        8.0 + frame as f32,
                    ),
                    GraphicsLayer {
                        render_effect: Some(RenderEffect::blur(2.0 + column as f32)),
                        ..Default::default()
                    },
                    vec![support::solid_rect(bounds, color)],
                ),
            )));
        }
        let pixels = support::present_and_read(
            &mut renderer,
            160,
            64,
            support::page_graph(160, 64, children),
        );
        for column in 0..3 {
            let offset = ((28 + frame) * 160 + 28 + column * 48) * 4;
            let rgb = [pixels[offset + 2], pixels[offset + 1], pixels[offset]];
            let channel = frame % 2;
            assert!(
                rgb[channel] > 240 && rgb[1 - channel] < 10 && rgb[2] < 10,
                "frame {frame}, tile {column}: current color disappeared or used stale uploads: {rgb:?}"
            );
        }
    }
}
