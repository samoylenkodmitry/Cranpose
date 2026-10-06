//! A frame that draws more distinct images than the image texture cache
//! holds draws every one of them, frame after frame: an image the cache
//! evicts while later images of the same frame are prepared stays bound for
//! the draw that took it.

use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, Rect, Size,
};

use crate::{shared_test_support, support};

/// Cells a side: 400 images, past the cache's 256 entries.
const CELLS: u32 = 20;
const CELL: u32 = 3;
const FRAME: u32 = CELLS * CELL;

/// A colour no other cell shares: an image's id follows its pixels.
fn colour(index: u32) -> [u8; 4] {
    [
        (index % 256) as u8,
        (index / 256 * 97 + 13) as u8,
        (index * 53 % 256) as u8,
        255,
    ]
}

fn frame() -> RenderGraph {
    let mut primitives = vec![DrawPrimitive::Rect {
        rect: Rect::from_size(Size::new(FRAME as f32, FRAME as f32)),
        brush: Brush::solid(Color::BLACK),
        stroke: None,
    }];
    primitives.extend((0..CELLS * CELLS).map(|index| {
        let image = ImageBitmap::from_rgba8(1, 1, colour(index).to_vec()).expect("one pixel");
        DrawPrimitive::Image {
            rect: Rect {
                x: (index % CELLS * CELL) as f32,
                y: (index / CELLS * CELL) as f32,
                width: CELL as f32,
                height: CELL as f32,
            },
            image,
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            src_rect: None,
        }
    }));
    RenderGraph::new(shared_test_support::layer_node(
        Rect::from_size(Size::new(FRAME as f32, FRAME as f32)),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))],
    ))
}

#[test]
fn a_frame_of_more_images_than_the_cache_holds_draws_them_all() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping many images: headless WGPU init failed: {err}");
            return;
        }
    };
    // New images every frame, as an animated mask makes them.
    for round in 0..3 {
        renderer.scene_mut().graph = Some(frame());
        let pixels = renderer
            .capture_frame_with_scale(FRAME, FRAME, 1.0)
            .unwrap_or_else(|err| panic!("frame {round} failed: {err:?}"))
            .pixels;
        for index in 0..CELLS * CELLS {
            let (x, y) = (index % CELLS * CELL + 1, index / CELLS * CELL + 1);
            let at = ((y * FRAME + x) * 4) as usize;
            let expected = colour(index);
            assert!(
                pixels[at..at + 3]
                    .iter()
                    .zip(&expected[..3])
                    .all(|(actual, expected)| actual.abs_diff(*expected) <= 1),
                "frame {round}: image {index} shows {:?}, not {:?}",
                &pixels[at..at + 4],
                expected
            );
        }
    }
}
