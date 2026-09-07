mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, DrawRunNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
    image_compare::image_difference_stats,
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, DrawScope, DrawScopeDefault, GraphicsLayer, Point,
    Rect, Size, Stroke, StrokeCap, StrokeJoin,
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

/// A band is only a raster extent: the fragment stage decides every pixel,
/// so the band-drawn ring and the same ring split into two clipped halves
/// hold the same pixels, and both hold fewer raster pixels than the disc.
#[test]
fn a_banded_ring_matches_the_ring_drawn_in_clipped_halves() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band parity: {err}");
            return;
        }
    };
    let whole = capture(&mut renderer, frame_of(vec![ring(None)]));
    let half = FRAME as f32 / 2.0;
    let halves = capture(
        &mut renderer,
        frame_of(vec![
            ring(Some(rect(0.0, 0.0, half, FRAME as f32))),
            ring(Some(rect(half, 0.0, half, FRAME as f32))),
        ]),
    );
    let distinct = support::distinct_colors(&whole.pixels);
    assert!(
        distinct > 2,
        "the ring must be visible, saw {distinct} colours"
    );
    let stats = image_difference_stats(&whole.pixels, &halves.pixels, FRAME, FRAME, 0);
    assert_eq!(
        stats.differing_pixels, 0,
        "the whole ring and the clipped-half ring differ: differing={} max={} first={:?}",
        stats.differing_pixels, stats.max_difference, stats.first_difference
    );
}

/// A quad that shares a draw with a wide ring is instanced over the ring's
/// strip pattern, its vertices past its four corners pinned onto its last
/// corner; a pinned vertex anywhere else would draw a real triangle from
/// the quad's edge and blend the translucent quad's pixels twice.
#[test]
fn a_translucent_quad_sharing_a_draw_with_a_wide_ring_blends_once() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band pinning: {err}");
            return;
        }
    };
    let veil = || {
        primitive(
            DrawPrimitive::Rect {
                rect: rect(20.0, 20.0, 120.0, 90.0),
                brush: Brush::solid(Color(0.9, 0.3, 0.2, 0.5)),
                stroke: None,
            },
            None,
        )
    };
    let with_ring = capture(&mut renderer, frame_of(vec![ring(None), veil()]));
    let alone = capture(&mut renderer, frame_of(vec![veil()]));
    let mut differing = 0u32;
    for y in 0..FRAME {
        for x in 0..FRAME {
            let dx = x as f32 + 0.5 - CENTER;
            let dy = y as f32 + 0.5 - CENTER;
            let on_ring = ((dx * dx + dy * dy).sqrt() - RADIUS).abs() < STROKE + 2.0;
            if !on_ring && pixel(&with_ring, x, y) != pixel(&alone, x, y) {
                differing += 1;
            }
        }
    }
    assert_eq!(
        differing, 0,
        "pixels off the ring differ between the veil drawn beside the ring and alone"
    );
}

fn pixel(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * FRAME + x) * 4) as usize;
    [
        frame.pixels[at],
        frame.pixels[at + 1],
        frame.pixels[at + 2],
        frame.pixels[at + 3],
    ]
}

fn short_arcs() -> Vec<RenderNode> {
    let mut scope = DrawScopeDefault::new(Size::new(FRAME as f32, FRAME as f32));
    for radius in [32.0, 100.0, 200.0] {
        for index in 0..36 {
            let cap = [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][index % 3];
            let width = [0.25, 1.0, 3.5][(index / 3) % 3];
            scope.draw_arc(
                Brush::solid(Color(0.2 + index as f32 / 60.0, 0.7, 0.9, 0.6)),
                Point::new(CENTER + 0.125, CENTER - 0.375),
                radius,
                index as f32 * cranpose_ui_graphics::TAU / 36.0,
                [0.012, -0.055, 0.095][(index / 9) % 3],
                Stroke::new(width).with_cap(cap),
            );
        }
    }
    let recording = scope.finish();
    assert!(
        recording
            .shapes()
            .iter()
            .filter(|r| r.is_banded() && r.band_segments() == 1)
            .count()
            > 60
    );
    let segments = recording.all_segments();
    vec![RenderNode::DrawRun(DrawRunNode::for_command_shared(
        PrimitivePhase::BeforeChildren,
        None,
        std::rc::Rc::new(recording),
        segments,
    ))]
}

#[test]
fn short_arc_quads_preserve_the_pixels_of_full_disc_rasterization() {
    let graph = frame_of(short_arcs());
    let frames = [
        wgpu::Limits::default(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: 0,
            ..wgpu::Limits::default()
        },
    ]
    .map(|limits| {
        let mut renderer = support::headless_renderer_with_limits(limits).expect("arc parity GPU");
        renderer.scene_mut().graph = Some(graph.clone());
        [0.5, 1.0, 2.75].map(|scale| {
            let side = (FRAME as f32 * scale) as u32;
            renderer
                .capture_frame_with_scale(side, side, scale)
                .expect("scaled arc capture")
        })
    });
    for (actual, expected) in frames[0].iter().zip(&frames[1]) {
        let stats = image_difference_stats(
            &actual.pixels,
            &expected.pixels,
            actual.width,
            actual.height,
            2,
        );
        assert_eq!(
            stats.differing_pixels, 0,
            "short arcs lost coverage: {stats:?}"
        );
    }
}

/// Records draw in the order they were recorded whatever geometry each
/// takes: a rect recorded after a band-drawn ring covers the ring where
/// they overlap, exactly as it does when the ring draws as its disc.
#[test]
fn a_rect_recorded_after_a_banded_ring_covers_it() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping band order: {err}");
            return;
        }
    };
    let cover = primitive(
        DrawPrimitive::Rect {
            rect: rect(CENTER + RADIUS - 40.0, CENTER - 40.0, 80.0, 80.0),
            brush: Brush::solid(Color::from_rgb_u8(200, 40, 40)),
            stroke: None,
        },
        None,
    );
    let frame = capture(&mut renderer, frame_of(vec![ring(None), cover]));
    let on_ring = pixel(&frame, (CENTER + RADIUS) as u32, CENTER as u32);
    assert!(
        on_ring[0] > 150 && on_ring[1] < 100,
        "the rect recorded after the ring must cover it: saw {on_ring:?}"
    );
    let ring_only = pixel(&frame, (CENTER - RADIUS) as u32, CENTER as u32);
    assert!(
        ring_only[1] > 150,
        "the ring must still draw where nothing covers it: saw {ring_only:?}"
    );
}
