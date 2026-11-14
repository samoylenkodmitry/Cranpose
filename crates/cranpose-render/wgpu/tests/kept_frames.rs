// The frame a renderer collects to tell whether a surface shows its scene,
// and keeps for the next draw.

use cranpose_render_common::{Renderer, graph::LayerNode};
use cranpose_ui_graphics::{Color, Rect};

use crate::support;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

fn page(color: Color) -> LayerNode {
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![support::solid_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            color,
        )],
    )
}

// A scene put in place of the one a kept frame was collected from draws
// itself: the kept frame belongs to the scene before, even where both
// scenes are at the same update.
#[test]
fn a_replaced_scene_draws_itself_and_not_the_frame_kept_for_the_one_before() {
    let (_lock, mut renderer) =
        match support::headless_renderer_parts_with_display_format(wgpu::TextureFormat::Rgba8Unorm)
        {
            Ok(parts) => parts,
            Err(error) => {
                eprintln!("skipping kept frame check: {error}");
                return;
            }
        };
    let device = renderer.try_device().expect("device").clone();
    let queue = renderer.try_queue_for_tests().expect("queue").clone();
    let (texture, view) = support::render_target(
        &device,
        WIDTH,
        HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let drawn = |renderer: &mut cranpose_render_wgpu::WgpuRenderer| {
        renderer
            .render(&texture, &view, WIDTH, HEIGHT)
            .expect("the frame renders");
        support::read_texture(&device, &queue, &texture)
    };

    renderer.scene_mut().describe(&page(Color::RED));
    let red = drawn(&mut renderer);
    renderer.scene_mut().describe(&page(Color::BLUE));
    let blue = drawn(&mut renderer);
    assert_ne!(red, blue, "the two pages draw apart");

    renderer.scene_mut().describe(&page(Color::RED));
    assert!(
        !renderer.shows_scene(WIDTH, HEIGHT),
        "the surface shows the blue page"
    );
    renderer.scene_mut().describe(&page(Color::BLUE));
    assert_eq!(
        drawn(&mut renderer),
        blue,
        "the blue page draws itself, not the red frame kept before it"
    );
}
