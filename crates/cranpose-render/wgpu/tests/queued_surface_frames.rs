mod support;

use cranpose_render_common::{Renderer, graph::RenderGraph};
use cranpose_ui_graphics::{Color, Rect};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn frame_graph(index: usize) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: WIDTH as f32,
        height: HEIGHT as f32,
    };
    RenderGraph::new(support::layer_node(
        Some(71),
        WIDTH as f32,
        HEIGHT as f32,
        vec![
            support::rect_primitive(bounds, Color::BLACK),
            support::rect_primitive(
                Rect {
                    x: 8.0 + index as f32 * 9.0,
                    y: 16.0,
                    width: 24.0,
                    height: 48.0,
                },
                if index.is_multiple_of(2) {
                    Color::RED
                } else {
                    Color::BLUE
                },
            ),
        ],
    ))
}

#[test]
fn queued_surface_images_keep_their_own_complete_frame_after_image_reuse() {
    let (_lock, mut renderer) = support::headless_renderer_parts_with_display_format(FORMAT)
        .expect("queued surface guard requires a GPU adapter");
    let device = renderer.try_device().expect("device").clone();
    let queue = renderer.try_queue_for_tests().expect("queue").clone();
    let target = || {
        support::render_target(
            &device,
            WIDTH,
            HEIGHT,
            FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        )
    };
    let images: Vec<_> = (0..3).map(|_| target()).collect();
    let mut snapshots = Vec::new();
    for index in 0..9 {
        renderer.scene_mut().graph = Some(frame_graph(index));
        let (texture, view) = &images[index % images.len()];
        renderer
            .render_surface_texture(texture, view, WIDTH, HEIGHT)
            .expect("queued surface render");
        let (snapshot, _) = target();
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            texture.as_image_copy(),
            snapshot.as_image_copy(),
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        snapshots.push(snapshot);
    }
    for (index, snapshot) in snapshots.iter().enumerate() {
        let presented = support::read_texture(&device, &queue, snapshot);
        renderer.scene_mut().graph = Some(frame_graph(index));
        let expected = renderer.capture_frame(WIDTH, HEIGHT).expect("serial frame");
        assert!(expected.pixels.as_chunks::<4>().0.iter().any(|pixel| {
            if index.is_multiple_of(2) {
                pixel[0] > 240 && pixel[2] < 10
            } else {
                pixel[2] > 240 && pixel[0] < 10
            }
        }));
        support::assert_same_bytes(
            &format!("queued surface frame {index}"),
            WIDTH,
            &presented,
            &expected.pixels,
        );
    }
    assert_eq!(renderer.device_error_count_for_tests(), 0);
}
