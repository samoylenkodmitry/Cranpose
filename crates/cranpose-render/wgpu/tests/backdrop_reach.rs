mod support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};

const FRAME: u32 = 200;
/// The list's band: the frame below this row, clipped.
const BAND_TOP: f32 = 80.0;
const BLUR_RADIUS: f32 = 20.0;
/// The blurred control: the band's full width from the band's top edge.
const CONTROL: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: FRAME as f32,
    height: 80.0,
};
/// A dark mark under the control's middle, far enough from every edge that
/// its blur never reaches the strips compared below.
const MARK: Rect = Rect {
    x: 95.0,
    y: 45.0,
    width: 10.0,
    height: 10.0,
};
/// Rows and columns along the control's top, left and right edges.
const STRIP: usize = 10;
const PAGE: u8 = 240;
const CHROME: u8 = 20;
const TOLERANCE: i32 = 2;

fn grey(value: u8) -> Color {
    Color::from_rgb_u8(value, value, value)
}

/// Dark chrome fills the frame; a clipped light band covers its lower part
/// and holds one blurred control along its top edge, the way a list holds a
/// nav bar under the tab strip above the list.
fn page() -> RenderGraph {
    let chrome = support::solid_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        grey(CHROME),
    );
    let band_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: FRAME as f32,
        height: FRAME as f32 - BAND_TOP,
    };
    let control = support::layer_node(None, CONTROL.width, CONTROL.height, vec![]);
    let control = cranpose_render_common::graph::LayerNode {
        graphics_layer: GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(BLUR_RADIUS)),
            ..GraphicsLayer::default()
        },
        ..control
    };
    let band = cranpose_render_common::graph::LayerNode {
        transform_to_parent: ProjectiveTransform::translation(0.0, BAND_TOP),
        graphics_layer: GraphicsLayer {
            clip: true,
            ..GraphicsLayer::default()
        },
        ..support::layer_node(
            None,
            band_bounds.width,
            band_bounds.height,
            vec![
                support::solid_rect(band_bounds, grey(PAGE)),
                support::solid_rect(MARK, grey(CHROME)),
                RenderNode::Layer(Box::new(control)),
            ],
        )
    };
    support::page_graph(
        FRAME,
        FRAME,
        vec![chrome, RenderNode::Layer(Box::new(band))],
    )
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let index = (y * FRAME as usize + x) * 4;
    [pixels[index], pixels[index + 1], pixels[index + 2]]
}

fn off_page(px: [u8; 3]) -> bool {
    px.iter()
        .any(|channel| (i32::from(*channel) - i32::from(PAGE)).abs() > TOLERANCE)
}

/// A control's blur reads what lies beneath it within the clip it is drawn
/// in: at the list's edge it holds the list's own edge, the way the screen's
/// edge holds, and never the chrome beyond the list.
#[test]
fn a_blur_at_the_edge_of_its_clip_does_not_read_past_the_clip() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping backdrop reach: {err}");
            return;
        }
    };
    let frame = support::capture_graph(&mut renderer, page(), FRAME, FRAME);
    let pixels = &frame.pixels;

    let top = BAND_TOP as usize;
    let chrome = pixel(pixels, 100, top / 2);
    assert!(
        chrome.iter().all(|channel| *channel < 60),
        "the chrome above the band must be dark for this to test anything: {chrome:?}"
    );
    let centre = pixel(
        pixels,
        (MARK.x + MARK.width * 0.5) as usize,
        top + (MARK.y + MARK.height * 0.5) as usize,
    );
    assert!(
        off_page(centre) && centre.iter().all(|channel| *channel > CHROME + 40),
        "the mark under the control must read blurred, neither the page nor itself: {centre:?}"
    );

    let bottom = top + CONTROL.height as usize;
    let mut poisoned = Vec::new();
    for y in top..bottom {
        for x in 0..FRAME as usize {
            let on_edge = y < top + STRIP || x < STRIP || x >= FRAME as usize - STRIP;
            if on_edge && off_page(pixel(pixels, x, y)) {
                poisoned.push((x, y, pixel(pixels, x, y)));
            }
        }
    }
    assert!(
        poisoned.is_empty(),
        "{} pixels along the control's top, left and right edges read the chrome past the \
         list's clip; first {:?}",
        poisoned.len(),
        &poisoned[..poisoned.len().min(6)]
    );
}
