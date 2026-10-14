//! While a shape's pipeline compiles, its draw takes triangles the CPU
//! tessellated: the first frame shows every shape, each with its colours
//! and within the antialiasing at its edges, and the frames after it draw
//! the shapes with their pipelines as those land.

use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, DrawRunNode, LayerNode, PrimitivePhase, RenderGraph, RenderNode},
};
use cranpose_render_wgpu::RenderStatsSnapshot;
use cranpose_ui_graphics::{
    Brush, Color, DrawScope, DrawScopeDefault, Point, Rect, Size, Stroke, StrokeCap, StrokeJoin,
};
use support::SIZE;

use crate::support;

/// A root scale that puts edges between whole pixels.
const ROOT_SCALE: f32 = 1.25;

/// A clipped layer, smaller than the frame, whose draw run holds the shapes
/// `record` draws: the clip cuts some of them.
fn graph(record: impl FnOnce(&mut DrawScopeDefault)) -> RenderGraph {
    let mut scope = DrawScopeDefault::new(Size::new(SIZE as f32, SIZE as f32));
    record(&mut scope);
    RenderGraph::new(LayerNode {
        cache_policy: CachePolicy::None,
        local_bounds: Rect {
            x: 11.25,
            y: 7.5,
            width: 183.0,
            height: 171.0,
        },
        clip_to_bounds: true,
        children: vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
        ..LayerNode::default()
    })
}

/// Solid arcs, discs, rounded rects and strokes, with lines of every cap
/// and a stroked circle and arc.
fn solid_shapes(scope: &mut DrawScopeDefault) {
    support::record_solid_scene(scope);
    let brush = Brush::solid(Color(0.95, 0.6, 0.2, 1.0));
    for (index, cap) in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square]
        .into_iter()
        .enumerate()
    {
        let x = 70.0 + index as f32 * 17.0;
        scope.draw_line(
            brush.clone(),
            Point::new(x, 60.0),
            Point::new(x + 30.0, 150.0),
            Stroke {
                width: 1.0 + index as f32 * 2.5,
                cap,
                join: StrokeJoin::Miter,
            },
        );
    }
    let ring = Stroke {
        width: 2.5,
        ..Stroke::default()
    };
    scope.draw_circle_stroked(brush.clone(), Point::new(170.0, 160.0), 30.0, ring);
    scope.draw_arc(brush, Point::new(60.0, 175.0), 24.0, 0.4, 3.9, ring);
}

/// The first frame of `graph` on a renderer that has built what every
/// screen draws with, but none of the graph's own shape pipelines, with its
/// statistics, and the frame once those landed.
fn first_and_landed(graph: &RenderGraph) -> (Vec<u8>, RenderStatsSnapshot, Vec<u8>) {
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    // The fallback pipelines and those of every screen build while an app
    // composes its first screen.
    support::wait_for_background_compiler_idle();
    let first = capture(&mut renderer, graph);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    let landed = support::capture_settled(&mut renderer, |renderer| {
        renderer.scene_mut().graph = Some(graph.clone());
        renderer
            .capture_frame_with_scale(SIZE, SIZE, ROOT_SCALE)
            .expect("capture")
    });
    (first, stats, landed.pixels)
}

fn capture(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    renderer.scene_mut().graph = Some(graph.clone());
    renderer
        .capture_frame_with_scale(SIZE, SIZE, ROOT_SCALE)
        .expect("capture")
        .pixels
}

/// Levels a pixel inside a shape or between shapes may differ by.
const INSIDE: u8 = 32;

/// Levels an edge pixel may differ by: its coverage, not a whole pixel of
/// the shape, as an edge drawn a pixel off would.
const EDGE: u8 = 160;

/// The pixels where `first` differs from `landed` by more than [`INSIDE`]
/// and no neighbour of the pixel in `landed` differs from it by as much:
/// differences away from every edge.
fn differing_inside(first: &[u8], landed: &[u8]) -> Vec<(usize, usize)> {
    let side = SIZE as usize;
    let pixel = |pixels: &[u8], x: usize, y: usize| {
        let at = (y * side + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    };
    let apart = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).any(|(a, b)| a.abs_diff(b) > INSIDE);
    let mut inside = Vec::new();
    for y in 0..side {
        for x in 0..side {
            let drawn = pixel(landed, x, y);
            if !apart(pixel(first, x, y), drawn) {
                continue;
            }
            let on_edge = (y.saturating_sub(1)..(y + 2).min(side)).any(|ny| {
                (x.saturating_sub(1)..(x + 2).min(side))
                    .any(|nx| apart(pixel(landed, nx, ny), drawn))
            });
            if !on_edge {
                inside.push((x, y));
            }
        }
    }
    inside
}

/// Asserts the first frame drew the graph's shapes as fallback triangles
/// and that those show what the shapes' own pipelines show.
fn assert_fallback_matches(record: impl FnOnce(&mut DrawScopeDefault)) {
    let (first, stats, landed) = first_and_landed(&graph(record));
    assert!(
        stats.shape_pipeline_fallback_draws > 0,
        "the first frame must draw shapes while their pipelines compile: {stats:?}"
    );
    assert_eq!(stats.placeholder_draws, 0, "no shape is left out");
    let inside = differing_inside(&first, &landed);
    assert!(
        inside.is_empty(),
        "the fallback differs away from the edges at {inside:?}"
    );
    let moved = support::pixels_differing_beyond(SIZE, &first, &landed, EDGE);
    assert!(
        moved.is_empty(),
        "edges moved: {}",
        support::describe_differing(&moved)
    );
}

#[test]
fn a_first_frame_draws_solid_shapes_as_their_pipelines_do() {
    assert_fallback_matches(solid_shapes);
}

#[test]
fn a_first_frame_draws_gradients_with_their_colours() {
    assert_fallback_matches(support::record_mixed_scene);
}
