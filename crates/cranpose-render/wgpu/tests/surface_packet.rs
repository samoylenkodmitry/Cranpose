mod support;

use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, ProjectiveTransform, RenderGraph},
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{Color, Rect, Size};

const FRAME_WIDTH: u32 = 128;
const FRAME_HEIGHT: u32 = 96;

fn shadowed_root_graph(cache_policy: CachePolicy) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };
    let mut root = support::contract_layer(
        Some(4_100),
        cache_policy,
        bounds,
        ProjectiveTransform::identity(),
        vec![support::rect_primitive(
            Rect {
                x: 24.0,
                y: 20.0,
                width: 80.0,
                height: 56.0,
            },
            Color(0.9, 0.2, 0.15, 1.0),
        )],
    );
    root.graphics_layer.shadow_elevation = 4.0;
    RenderGraph::new(root)
}

fn rgba(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * frame.width + x) * 4) as usize;
    [
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
        frame.pixels[index + 3],
    ]
}

#[test]
fn surface_packet_root_renders_from_packet_source() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping surface packet render assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(shadowed_root_graph(CachePolicy::None));
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("surface packet frame should render");

    let inside = rgba(&frame, 64, 48);
    let outside = rgba(&frame, 4, 4);
    assert!(
        inside[0] > 120 && inside[0] > inside[2],
        "the packet-lowered root content must reach the frame: inside={inside:?}"
    );
    assert!(
        outside[0] < 60,
        "outside the rect the clear color must remain: outside={outside:?}"
    );
}

#[test]
fn surface_packet_unchanged_root_reuses_its_cached_shadow() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping surface packet cache assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(shadowed_root_graph(CachePolicy::Auto));
    let first = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("first surface frame should render");
    let first_stats = renderer.last_frame_stats().expect("first frame stats");

    renderer.scene_mut().graph = Some(shadowed_root_graph(CachePolicy::Auto));
    let second = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("second surface frame should render");
    let second_stats = renderer.last_frame_stats().expect("second frame stats");

    assert!(
        first_stats.blur_passes > 0,
        "the first surface frame must resolve the root's blurred shadow: {first_stats:?}"
    );
    assert_eq!(
        second_stats.blur_passes, 0,
        "an unchanged root must serve its shadow from the shadow cache: {second_stats:?}"
    );
    assert_eq!(
        first.pixels, second.pixels,
        "cache-hit surface frames must render identically to the filling frame"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dev_overlay_packet_renders_over_both_root_kinds() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping dev overlay packet assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };
    let viewport = Size {
        width: FRAME_WIDTH as f32,
        height: FRAME_HEIGHT as f32,
    };

    let direct_graph = || {
        RenderGraph::new(support::contract_layer(
            Some(4_200),
            CachePolicy::None,
            Rect {
                x: 0.0,
                y: 0.0,
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            },
            ProjectiveTransform::identity(),
            vec![support::rect_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: FRAME_WIDTH as f32,
                    height: FRAME_HEIGHT as f32,
                },
                Color(0.1, 0.1, 0.1, 1.0),
            )],
        ))
    };
    renderer.scene_mut().graph = Some(direct_graph());
    let plain = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("direct frame should render");

    renderer.scene_mut().graph = Some(direct_graph());
    renderer.draw_dev_overlay("FPS 60.0", viewport);
    let with_overlay = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("direct frame with overlay should render");
    assert_ne!(
        plain.pixels, with_overlay.pixels,
        "the packet-carried dev overlay must draw over a direct root"
    );

    renderer.scene_mut().graph = Some(shadowed_root_graph(CachePolicy::None));
    renderer.draw_dev_overlay("FPS 60.0", viewport);
    renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("surface frame with overlay should render");
}
