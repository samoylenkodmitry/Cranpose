mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
    image_compare::image_difference_stats,
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, GraphicsLayer, Point, Rect, Stroke, StrokeCap,
    StrokeJoin,
};

const FRAME: u32 = 480;
const RADIUS: f32 = 200.0;
const CENTER: f32 = 240.0;
const STROKE: f32 = 4.0;
/// The pixels the band of a stroked circle of `RADIUS` and `STROKE` covers
/// with a two-pixel anti-aliasing margin on each side, times two for the
/// polygon's slack; the disc is 160 000.
const RING_BUDGET: u64 = 2 * 6284 * 8;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn primitive(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode { primitive, clip }),
    })
}

fn stroke() -> Stroke {
    Stroke {
        width: STROKE,
        cap: StrokeCap::Butt,
        join: StrokeJoin::Miter,
    }
}

fn ring(clip: Option<Rect>) -> RenderNode {
    primitive(
        DrawPrimitive::RoundRect {
            rect: rect(CENTER - RADIUS, CENTER - RADIUS, 2.0 * RADIUS, 2.0 * RADIUS),
            brush: Brush::solid(Color::from_rgb_u8(240, 200, 80)),
            radii: CornerRadii::uniform(RADIUS),
            stroke: Some(stroke()),
        },
        clip,
    )
}

fn arc_band() -> RenderNode {
    primitive(
        DrawPrimitive::Arc {
            rect: rect(CENTER - RADIUS, CENTER - RADIUS, 2.0 * RADIUS, 2.0 * RADIUS),
            brush: Brush::solid(Color::from_rgb_u8(80, 200, 240)),
            center: Point::new(CENTER, CENTER),
            radius: RADIUS,
            start_angle: 0.3,
            sweep_angle: 1.2,
            stroke: None,
            inner_radius: RADIUS - 12.0,
        },
        None,
    )
}

fn frame_of(children: Vec<RenderNode>) -> RenderGraph {
    let mut nodes = vec![primitive(
        DrawPrimitive::Rect {
            rect: rect(0.0, 0.0, FRAME as f32, FRAME as f32),
            brush: Brush::solid(Color::from_rgb_u8(20, 24, 40)),
            stroke: None,
        },
        None,
    )];
    nodes.extend(children);
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    ))
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> CapturedFrame {
    renderer.scene_mut().graph = Some(graph);
    renderer
        .capture_frame(FRAME, FRAME)
        .expect("capture should succeed")
}

fn background_fill_pixels(renderer: &mut support::LockedRenderer) -> u64 {
    capture(renderer, frame_of(Vec::new()));
    renderer
        .last_frame_stats()
        .expect("stats")
        .shape_fill_pixels
}

/// A stroked circle rasterizes its band, not the disc inside it.
#[test]
fn a_stroked_circle_rasterizes_its_band_not_its_disc() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band fill: {err}");
            return;
        }
    };
    let background = background_fill_pixels(&mut renderer);
    capture(&mut renderer, frame_of(vec![ring(None)]));
    let stats = renderer.last_frame_stats().expect("stats");
    let ring_pixels = stats.shape_fill_pixels - background;
    assert!(
        ring_pixels <= RING_BUDGET,
        "the ring rasterized {ring_pixels} pixels, budget {RING_BUDGET}: {stats:?}"
    );
}

/// An arc band rasterizes its sector, not the disc of its circle.
#[test]
fn an_arc_band_rasterizes_its_sector_not_its_disc() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band fill: {err}");
            return;
        }
    };
    let background = background_fill_pixels(&mut renderer);
    capture(&mut renderer, frame_of(vec![arc_band()]));
    let stats = renderer.last_frame_stats().expect("stats");
    let sector_pixels = stats.shape_fill_pixels - background;
    assert!(
        sector_pixels <= RING_BUDGET,
        "the arc band rasterized {sector_pixels} pixels, budget {RING_BUDGET}: {stats:?}"
    );
}

/// The rasterizer interpolates the world position across a 400-pixel quad
/// and across a 6-pixel triangle with different rounding, which moves an
/// anti-aliased edge pixel by a step or two; a pixel the mesh missed would
/// differ by the ring's whole intensity.
const INTERPOLATION_TOLERANCE: u32 = 2;

/// The mesh only restricts where the shape shader runs, so a ring drawn
/// through it matches the same ring drawn as two clipped halves, which take
/// the quad path, to interpolation rounding.
#[test]
fn a_meshed_ring_matches_the_ring_drawn_as_clipped_quads() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band parity: {err}");
            return;
        }
    };
    let meshed = capture(&mut renderer, frame_of(vec![ring(None)]));
    let half = FRAME as f32 / 2.0;
    let quads = capture(
        &mut renderer,
        frame_of(vec![
            ring(Some(rect(0.0, 0.0, half, FRAME as f32))),
            ring(Some(rect(half, 0.0, half, FRAME as f32))),
        ]),
    );
    let distinct = support::distinct_colors(&meshed.pixels);
    assert!(
        distinct > 2,
        "the ring must be visible, saw {distinct} colours"
    );
    let stats = image_difference_stats(
        &meshed.pixels,
        &quads.pixels,
        FRAME,
        FRAME,
        INTERPOLATION_TOLERANCE,
    );
    assert_eq!(
        stats.differing_pixels, 0,
        "the meshed ring and the clipped-quad ring differ: differing={} max={} first={:?}",
        stats.differing_pixels, stats.max_difference, stats.first_difference
    );
}
