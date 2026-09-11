mod support;

use cranpose_ui_graphics::{Color, GraphicsLayer, Point};
use support::clip_band::{self, CHILD, FRAME};

/// Where the child sits in the band: 60 of its 120 rows above the band's top.
const PLACEMENT: Point = Point { x: 40.0, y: -60.0 };

fn capture(layer: GraphicsLayer) -> Option<(usize, usize)> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping projective layer clip: {err}");
            return None;
        }
    };
    let fill = support::solid_rect(CHILD, Color::from_rgb_u8(220, 30, 30));
    let frame = support::capture_graph(
        &mut renderer,
        clip_band::page(layer, PLACEMENT, vec![fill]),
        FRAME,
        FRAME,
    );
    let counted = clip_band::red_pixels(&frame.pixels);
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
