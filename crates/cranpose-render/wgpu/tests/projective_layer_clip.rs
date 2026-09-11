mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Point, Rect};

const FRAME: u32 = 200;
/// The clipping band covers the frame below this row.
const CLIP_TOP: usize = 80;
const CHILD: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 120.0,
    height: 120.0,
};
/// Where the child sits in the band: 60 of its 120 rows above the band's top.
const PLACEMENT: Point = Point { x: 40.0, y: -60.0 };

/// A clipped band holding one red child with `layer`, placed the way the
/// graph builder places a `graphics_layer` node.
fn page(layer: GraphicsLayer) -> RenderGraph {
    let child = shared_test_support::layer_node(
        CHILD,
        layer_transform_to_parent(CHILD, PLACEMENT, &layer),
        layer,
        vec![support::solid_rect(CHILD, Color::from_rgb_u8(220, 30, 30))],
    );
    let band = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32 - CLIP_TOP as f32,
        },
        ProjectiveTransform::translation(0.0, CLIP_TOP as f32),
        GraphicsLayer {
            clip: true,
            ..GraphicsLayer::default()
        },
        vec![RenderNode::Layer(Box::new(child))],
    );
    support::page_graph(FRAME, FRAME, vec![RenderNode::Layer(Box::new(band))])
}

/// Red pixels above and inside the band.
fn red_pixels(pixels: &[u8]) -> (usize, usize) {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, px)| px[0] > 160 && px[1] < 90 && px[2] < 90)
        .fold((0, 0), |(above, inside), (index, _)| {
            if (index / FRAME as usize) < CLIP_TOP {
                (above + 1, inside)
            } else {
                (above, inside + 1)
            }
        })
}

fn capture(layer: GraphicsLayer) -> Option<(usize, usize)> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping projective layer clip: {err}");
            return None;
        }
    };
    let frame = support::capture_graph(&mut renderer, page(layer), FRAME, FRAME);
    let counted = red_pixels(&frame.pixels);
    eprintln!("[probe] red (above, inside) = {counted:?}");
    assert!(
        counted.1 > 1000,
        "the child must be drawn inside the band for this to test anything: {counted:?}"
    );
    Some(counted)
}

#[test]
fn an_upright_child_reaching_past_the_band_is_clipped() {
    let Some((above, _)) = capture(GraphicsLayer::default()) else {
        return;
    };
    assert_eq!(
        above, 0,
        "an upright child is drawn past the top of the layer that clips it"
    );
}

/// The plane from the demo's Shaders page: rotated about X and Y under a
/// camera, so its quad is a true perspective.
#[test]
fn a_tilted_child_reaching_past_the_band_is_clipped() {
    let tilted = GraphicsLayer {
        rotation_x: 26.0,
        rotation_y: -18.0,
        camera_distance: 8.0,
        ..GraphicsLayer::default()
    };
    let Some((above, _)) = capture(tilted) else {
        return;
    };
    assert_eq!(
        above, 0,
        "{above} pixels of a child leaning out of the layer that clips it landed past the \
         layer's edge: the plane a person tilts inside a scroll list is drawn over whatever \
         sits above the list"
    );
}
