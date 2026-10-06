//! A filled path the GPU draws covers each pixel as its slices say, at any
//! scale, matches the mask the CPU rasterizes for it, and resolves its brush
//! against its draw scope, as Compose's `drawPath` does.

use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, DrawScope, DrawScopeDefault, DrawStyle, GraphicsLayer, Path,
    PathFillRule, Point, Rect, Size, Trapezoid,
};

use crate::{shared_test_support, support};

const FRAME: u32 = 64;

/// Levels either side of the expected value a pixel may land on: the
/// target's rounding and the GPU's float arithmetic.
const TOLERANCE: i32 = 2;

fn polygon(points: impl IntoIterator<Item = (f32, f32)>) -> Path {
    let mut path = Path::new();
    for (index, (x, y)) in points.into_iter().enumerate() {
        if index == 0 {
            path.move_to(Point::new(x, y));
        } else {
            path.line_to(Point::new(x, y));
        }
    }
    path.close();
    path
}

/// Paths in a 64-unit frame: a sparkline whose steep peaks stand above the
/// columns beside them, a star that crosses itself, a frame around a hole
/// wound the other way, a sliver narrower than a pixel and a many-sided
/// disc.
fn paths() -> Vec<(&'static str, Path)> {
    let heights = [44.0, 6.0, 40.0, 30.5, 52.0, 8.25, 20.0, 47.0];
    let sparkline = polygon(
        std::iter::once((2.0, 60.0))
            .chain(
                heights
                    .iter()
                    .enumerate()
                    .map(|(index, &y)| (2.0 + index as f32 * 8.5, y)),
            )
            .chain(std::iter::once((61.5, 60.0))),
    );
    let star = polygon((0..5).map(|index| {
        let angle = std::f32::consts::FRAC_PI_2 + index as f32 * 4.0 * std::f32::consts::PI / 5.0;
        (32.0 + 28.0 * angle.cos(), 33.0 - 28.0 * angle.sin())
    }));
    let mut frame = polygon([(4.0, 4.0), (60.0, 4.0), (60.0, 60.0), (4.0, 60.0)]);
    frame.move_to(Point::new(32.0, 12.5));
    frame.line_to(Point::new(12.5, 32.0));
    frame.line_to(Point::new(32.0, 51.5));
    frame.line_to(Point::new(51.5, 32.0));
    frame.close();
    let sliver = polygon([(10.0, 6.0), (10.4, 58.0), (54.0, 30.0)]);
    let disc = polygon((0..48).map(|index| {
        let angle = index as f32 * std::f32::consts::TAU / 48.0;
        (31.7 + 26.3 * angle.cos(), 32.2 + 26.3 * angle.sin())
    }));
    vec![
        ("sparkline", sparkline),
        ("star", star),
        ("frame around a hole", frame),
        ("sliver", sliver),
        ("disc", disc),
    ]
}

fn frame_of(primitives: Vec<DrawPrimitive>, size: f32) -> RenderGraph {
    RenderGraph::new(shared_test_support::layer_node(
        Rect::from_size(Size::new(size, size)),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))],
    ))
}

/// The path filled white over black by a draw scope `size` units square.
fn filled(path: &Path, brush: Brush, size: f32) -> Vec<DrawPrimitive> {
    let mut scope = DrawScopeDefault::new(Size::new(size, size));
    scope.draw_rect(Brush::solid(Color::BLACK));
    scope.draw_path(path, brush, DrawStyle::Fill);
    scope.into_primitives()
}

fn slices(primitives: &[DrawPrimitive]) -> Vec<Trapezoid> {
    primitives
        .iter()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Trapezoid { trapezoid, .. } => Some(*trapezoid),
            _ => None,
        })
        .collect()
}

fn scaled(slice: Trapezoid, scale: f32) -> Trapezoid {
    Trapezoid {
        left: slice.left * scale,
        right: slice.right * scale,
        top: slice.top.map(|y| y * scale),
        bottom: slice.bottom.map(|y| y * scale),
        ..slice
    }
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph, scale: f32) -> Vec<u8> {
    renderer.scene_mut().graph = Some(graph);
    renderer
        .capture_frame_with_scale(FRAME, FRAME, scale)
        .unwrap_or_else(|err| panic!("capture failed: {err:?}"))
        .pixels
}

fn red(pixels: &[u8], x: u32, y: u32) -> i32 {
    i32::from(pixels[((y * FRAME + x) * 4) as usize])
}

#[test]
fn a_gpu_path_fill_covers_each_pixel_as_its_slices_say() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping path fill coverage: headless WGPU init failed: {err}");
            return;
        }
    };
    for scale in [1.0, 2.625] {
        let size = FRAME as f32 / scale;
        for (name, path) in paths() {
            let path = scaled_path(&path, 1.0 / scale);
            let primitives = filled(&path, Brush::solid(Color::WHITE), size);
            let slices: Vec<Trapezoid> = slices(&primitives)
                .into_iter()
                .map(|slice| scaled(slice, scale))
                .collect();
            assert!(!slices.is_empty(), "{name} at {scale}: the fill slices");
            let pixels = capture(&mut renderer, frame_of(primitives, size), scale);
            let mut drawn = 0;
            for y in 0..FRAME {
                for x in 0..FRAME {
                    let centre = Point::new(x as f32 + 0.5, y as f32 + 0.5);
                    // Each slice blends over what the ones before it left.
                    let value = slices.iter().fold(0.0f32, |value, slice| {
                        value + slice.coverage(centre) * (1.0 - value)
                    });
                    let expected = (value * 255.0).round() as i32;
                    let actual = red(&pixels, x, y);
                    drawn += i32::from(actual > 0);
                    assert!(
                        (actual - expected).abs() <= TOLERANCE,
                        "{name} at scale {scale}: pixel ({x}, {y}) is {actual}, its slices say {expected}"
                    );
                }
            }
            assert!(drawn > 0, "{name} at {scale}: the fill must draw something");
        }
    }
}

fn scaled_path(path: &Path, scale: f32) -> Path {
    let mut out = Path::new();
    for contour in path.contours() {
        let mut points = contour.points.iter();
        if let Some(first) = points.next() {
            out.move_to(Point::new(first.x * scale, first.y * scale));
        }
        for point in points {
            out.line_to(Point::new(point.x * scale, point.y * scale));
        }
        out.close();
    }
    out
}

#[test]
fn a_gpu_path_fill_matches_the_mask_the_cpu_rasterizes() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping path fill parity: headless WGPU init failed: {err}");
            return;
        }
    };
    for (name, path) in paths() {
        let size = FRAME as f32;
        let pixels = capture(
            &mut renderer,
            frame_of(filled(&path, Brush::solid(Color::WHITE), size), size),
            1.0,
        );
        let mask = path.to_vector_path(PathFillRule::NonZero).coverage_mask(
            FRAME as usize,
            FRAME as usize,
            Point::new(0.0, 0.0),
            1.0,
            1.0,
        );
        let mut differences = 0i64;
        let mut inked = 0i64;
        for y in 0..FRAME {
            for x in 0..FRAME {
                let actual = red(&pixels, x, y);
                let expected = i32::from(mask[(y * FRAME + x) as usize]);
                // Pixels the edge crosses differ by how each takes its
                // share; no pixel may flip between inside and outside.
                assert!(
                    (actual - expected).abs() <= 128,
                    "{name}: pixel ({x}, {y}) is {actual}, the mask says {expected}"
                );
                differences += i64::from((actual - expected).abs());
                inked += i64::from(expected.max(actual) > 0);
            }
        }
        let mean = differences as f64 / inked as f64;
        assert!(
            mean < 0.02 * 255.0,
            "{name}: the fill differs from the mask by {mean:.2} levels on average"
        );
    }
}

#[test]
fn a_path_fill_resolves_its_brush_against_its_scope() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping path fill brush: headless WGPU init failed: {err}");
            return;
        }
    };
    let size = FRAME as f32;
    let gradient = || {
        Brush::vertical_gradient(
            vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)],
            0.0,
            size,
        )
    };
    let mut scope = DrawScopeDefault::new(Size::new(size, size));
    scope.draw_rect(gradient());
    let reference = capture(&mut renderer, frame_of(scope.into_primitives(), size), 1.0);
    let lower_half = polygon([(0.0, 32.0), (size, 32.0), (size, size), (0.0, size)]);
    let drawn = capture(
        &mut renderer,
        frame_of(filled(&lower_half, gradient(), size), size),
        1.0,
    );
    for y in 33..FRAME {
        for x in 1..FRAME - 1 {
            for channel in 0..3 {
                let index = ((y * FRAME + x) * 4 + channel) as usize;
                let (actual, expected) = (i32::from(drawn[index]), i32::from(reference[index]));
                assert!(
                    (actual - expected).abs() <= TOLERANCE,
                    "pixel ({x}, {y}) channel {channel} is {actual}; the gradient over the \
                     scope is {expected} there"
                );
            }
        }
    }
}

#[test]
fn a_slice_holds_its_gradient_s_last_colour_past_the_last_stop() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping path fill gradient clamp: headless WGPU init failed: {err}");
            return;
        }
    };
    let size = FRAME as f32;
    let (first, last) = (Color(0.8, 0.2, 0.4, 1.0), Color(0.3, 0.6, 0.5, 1.0));
    let gradient = Brush::vertical_gradient(vec![first, last], 16.0, 40.0);
    // The fill runs eight rows past the gradient's end, where its colour
    // holds at the last stop's.
    let band = polygon([(8.0, 16.0), (56.0, 16.0), (56.0, 47.8), (8.0, 47.8)]);
    let pixels = capture(
        &mut renderer,
        frame_of(filled(&band, gradient, size), size),
        1.0,
    );
    for y in 17..47 {
        let t = ((y as f32 + 0.5 - 16.0) / 24.0).clamp(0.0, 1.0);
        for x in [12, 32, 52] {
            for (channel, (from, to)) in [
                (first.r(), last.r()),
                (first.g(), last.g()),
                (first.b(), last.b()),
            ]
            .into_iter()
            .enumerate()
            {
                let expected = ((from + (to - from) * t) * 255.0).round() as i32;
                let actual = i32::from(pixels[((y * FRAME + x) * 4) as usize + channel]);
                assert!(
                    (actual - expected).abs() <= TOLERANCE,
                    "pixel ({x}, {y}) channel {channel} is {actual}; the gradient there is {expected}"
                );
            }
        }
    }
}
