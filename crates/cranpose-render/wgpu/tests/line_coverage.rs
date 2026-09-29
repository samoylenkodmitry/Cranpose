//! A line the GPU draws covers each pixel as `LineGeometry::coverage` says,
//! for every cap, level, upright and slanted, with ends on whole and half
//! pixels, whether a draw scope recorded it or it arrived as a loose
//! primitive.

use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, DrawScope, DrawScopeDefault, GraphicsLayer, LineGeometry, Point,
    Rect, Size, Stroke, StrokeCap, StrokeJoin,
};

use crate::{shared_test_support, support};

const FRAME: u32 = 48;

/// Levels either side of the expected value a pixel may land on: the
/// target's rounding and the GPU's float arithmetic.
const TOLERANCE: i32 = 2;

fn stroke(width: f32, cap: StrokeCap) -> Stroke {
    Stroke {
        width,
        cap,
        join: StrokeJoin::Miter,
    }
}

fn cases() -> Vec<(&'static str, Point, Point, Stroke)> {
    let mut cases = Vec::new();
    for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
        cases.push((
            "level on whole pixels",
            Point::new(8.0, 12.0),
            Point::new(40.0, 12.0),
            stroke(2.0, cap),
        ));
        cases.push((
            "level on half pixels",
            Point::new(8.5, 20.5),
            Point::new(39.5, 20.5),
            stroke(1.0, cap),
        ));
        cases.push((
            "upright and thin",
            Point::new(24.25, 6.0),
            Point::new(24.25, 42.0),
            stroke(0.5, cap),
        ));
        cases.push((
            "slanted and wide",
            Point::new(10.0, 38.0),
            Point::new(38.0, 14.0),
            stroke(6.0, cap),
        ));
    }
    cases.push((
        "a dot of no length",
        Point::new(24.0, 24.0),
        Point::new(24.0, 24.0),
        stroke(8.0, StrokeCap::Round),
    ));
    cases
}

fn background() -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        brush: Brush::solid(Color::BLACK),
        stroke: None,
    }
}

fn frame_of(children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME as f32,
            height: FRAME as f32,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

/// The line as a draw scope records it, in one run with its background.
fn recorded(start: Point, end: Point, stroke: Stroke) -> RenderGraph {
    let mut scope = DrawScopeDefault::new(Size::new(FRAME as f32, FRAME as f32));
    scope.draw_rect(Brush::solid(Color::BLACK));
    scope.draw_line(Brush::solid(Color::WHITE), start, end, stroke);
    frame_of(vec![RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        scope.into_primitives(),
    ))])
}

/// The line as a loose primitive node beside its background.
fn loose(start: Point, end: Point, stroke: Stroke) -> RenderGraph {
    let line = LineGeometry::new(start, end, stroke);
    frame_of(vec![
        support::draw_node(background(), None),
        support::draw_node(
            DrawPrimitive::Line {
                rect: line.end_bounds(),
                brush: Brush::solid(Color::WHITE),
                start,
                end,
                stroke,
            },
            None,
        ),
    ])
}

fn check(
    renderer: &mut support::LockedRenderer,
    path: &str,
    graph: RenderGraph,
    case: &str,
    line: LineGeometry,
) {
    renderer.scene_mut().graph = Some(graph);
    let captured = renderer
        .capture_frame_with_scale(FRAME, FRAME, 1.0)
        .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
    let inked: Vec<(u32, u32)> = (0..FRAME * FRAME)
        .filter(|index| captured.pixels[(index * 4) as usize] > 0)
        .map(|index| (index % FRAME, index / FRAME))
        .collect();
    let inked_bounds = inked
        .iter()
        .fold(None, |bounds: Option<(u32, u32, u32, u32)>, &(x, y)| {
            Some(bounds.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
            }))
        });
    let mut drawn = 0;
    for y in 0..FRAME {
        for x in 0..FRAME {
            let expected =
                (line.coverage(Point::new(x as f32 + 0.5, y as f32 + 0.5)) * 255.0).round() as i32;
            let actual = i32::from(captured.pixels[((y * FRAME + x) * 4) as usize]);
            drawn += i32::from(actual > 0);
            assert!(
                (actual - expected).abs() <= TOLERANCE,
                "{path}, {case}, cap {:?}: pixel ({x}, {y}) is {actual}, its coverage says {expected}; \
                 the drawn pixels span {inked_bounds:?}",
                line.cap
            );
        }
    }
    assert!(drawn > 0, "{path}, {case}: the line must draw something");
}

#[test]
fn a_gpu_line_covers_each_pixel_as_its_geometry_says() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping line coverage: headless WGPU init failed: {err}");
            return;
        }
    };
    for (case, start, end, stroke) in cases() {
        let line = LineGeometry::new(start, end, stroke);
        check(
            &mut renderer,
            "recorded",
            recorded(start, end, stroke),
            case,
            line,
        );
        check(
            &mut renderer,
            "loose",
            loose(start, end, stroke),
            case,
            line,
        );
    }
}

#[test]
fn a_butt_capped_segment_of_no_length_draws_nothing() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping line coverage: headless WGPU init failed: {err}");
            return;
        }
    };
    let point = Point::new(24.0, 24.0);
    for graph in [
        recorded(point, point, stroke(8.0, StrokeCap::Butt)),
        loose(point, point, stroke(8.0, StrokeCap::Butt)),
    ] {
        renderer.scene_mut().graph = Some(graph);
        let captured = renderer
            .capture_frame_with_scale(FRAME, FRAME, 1.0)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert!(
            captured
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[0] == 0),
            "Compose's drawLine draws nothing for a zero-length butt-capped line"
        );
    }
}

/// A quarter turn of the frame about its centre: `(x, y)` lands on
/// `(FRAME - y, x)`, so every pixel centre lands on a pixel centre.
fn quarter_turned(point: Point) -> Point {
    Point::new(FRAME as f32 - point.y, point.x)
}

#[test]
fn a_line_in_a_turned_layer_covers_the_pixels_of_the_turned_line() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping line coverage: headless WGPU init failed: {err}");
            return;
        }
    };
    let side = FRAME as f32;
    let frame = Rect {
        x: 0.0,
        y: 0.0,
        width: side,
        height: side,
    };
    let turn = ProjectiveTransform::from_rect_to_quad(
        frame,
        // Top left, top right, bottom left and bottom right, turned.
        [[side, 0.0], [side, side], [0.0, 0.0], [0.0, side]],
    );
    for (case, start, end, stroke) in cases() {
        let mut scope = DrawScopeDefault::new(Size::new(side, side));
        scope.draw_line(Brush::solid(Color::WHITE), start, end, stroke);
        let turned_layer = shared_test_support::layer_node(
            frame,
            turn,
            GraphicsLayer::default(),
            vec![RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                scope.into_primitives(),
            ))],
        );
        let graph = frame_of(vec![
            support::draw_node(background(), None),
            RenderNode::Layer(Box::new(turned_layer)),
        ]);
        let turned = LineGeometry::new(quarter_turned(start), quarter_turned(end), stroke);
        check(&mut renderer, "turned", graph, case, turned);
    }
}
