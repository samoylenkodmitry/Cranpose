// Paths an app draws in a draw scope, drawn from the recording the scene
// keeps. A stroke whose first lines join a banded ring's run of records
// until that run cuts them off covers the pixels its lines say, and its
// lines pin no more of the ring's quads than the run's bound. The walk
// that slices a band between two series that cross fills it as the sweep
// over all its edges does.

use cranpose_ui::Size;
use cranpose_ui_graphics::{
    Brush, DrawScope, DrawStyle, LineGeometry, Path, Point, Stroke, for_each_stroke_line,
};
use support::page::*;

use crate::support;

const FRAME: u32 = 96;

// Levels either side of the expected value a pixel may land on: the
// target's rounding and the GPU's float arithmetic.
const TOLERANCE: u8 = 2;

// A zigzag below the ring: `segments` lines, each `step` to the right of
// the one before.
fn zigzag(segments: usize, step: f32) -> Vec<Point> {
    (0..=segments)
        .map(|index| {
            Point::new(
                6.0 + index as f32 * step,
                if index % 2 == 0 { 72.0 } else { 88.0 },
            )
        })
        .collect()
}

// 60 lines across the frame.
fn short_zigzag() -> Vec<Point> {
    zigzag(60, 1.4)
}

// 400 lines, enough to pin past the run's bound if every one joined the
// ring's segment.
fn long_zigzag() -> Vec<Point> {
    zigzag(400, 0.2)
}

const ZIGZAG_STROKE: Stroke = Stroke::new(2.0);

// The rows of the frame the ring leaves alone.
const BELOW_RING: u32 = 66;

// A black frame with one box that draws `draw` behind itself.
#[composable]
fn DrawPage(draw: fn(&mut dyn DrawScope)) {
    FramePage(FRAME, FRAME, Color::BLACK, move || {
        Box(
            Modifier::empty()
                .size(Size::new(FRAME as f32, FRAME as f32))
                .draw_behind(move |scope| draw(scope)),
            BoxSpec::default(),
            || {},
        );
    });
}

// A full ring wide and thin enough to draw as a banded strip.
fn draw_ring(scope: &mut dyn DrawScope) {
    scope.draw_arc(
        Brush::solid(Color::WHITE),
        Point::new(48.0, 34.0),
        28.0,
        0.0,
        std::f32::consts::TAU,
        Stroke::new(2.0),
    );
}

fn stroke(scope: &mut dyn DrawScope, points: &[Point]) {
    let mut path = Path::new();
    path.move_to(points[0]);
    for &point in &points[1..] {
        path.line_to(point);
    }
    scope.draw_path(
        &path,
        Brush::solid(Color::WHITE),
        DrawStyle::Stroke(ZIGZAG_STROKE),
    );
}

// The ring, then the zigzag stroked in the same draw: the stroke's first
// lines join the run the ring opened, which cuts them off once their pinned
// quads pass its bound.
#[composable]
fn RingThenZigzag() {
    DrawPage(|scope| {
        draw_ring(scope);
        stroke(scope, &short_zigzag());
    });
}

// The same with the long zigzag, whose lines would pin far past the bound
// if the run never cut them off.
#[composable]
fn RingThenLongZigzag() {
    DrawPage(|scope| {
        draw_ring(scope);
        stroke(scope, &long_zigzag());
    });
}

#[composable]
fn RingAlone() {
    DrawPage(draw_ring);
}

#[composable]
fn LongZigzagAlone() {
    DrawPage(|scope| stroke(scope, &long_zigzag()));
}

fn red(pixels: &[u8], x: u32, y: u32) -> u8 {
    pixels[((y * FRAME + x) * 4) as usize]
}

// The quads that a segment may pin on records drawn with more quads than
// they need, the bound past which a run cuts a new segment.
const PINNED_QUADS_BOUND: u64 = 512;

// The vertices that the shape draws of `page`'s frame run through the
// vertex stage, six a quad, which the renderer counts with its GPU stats on.
fn shape_vertices(page: fn()) -> Option<u64> {
    support::warm_app_frame_toggled(page, FRAME, FRAME, Some("CRANPOSE_GPU_STATS"))
        .map(|(_, stats)| stats.shape_vertices)
}

#[test]
fn a_stroke_after_a_banded_ring_pins_no_more_quads_than_the_bound() {
    let (Some(both), Some(ring), Some(zigzag)) = (
        shape_vertices(RingThenLongZigzag),
        shape_vertices(RingAlone),
        shape_vertices(LongZigzagAlone),
    ) else {
        return;
    };
    // A line in the ring's segment draws with the ring's quads.
    let pinned = both.saturating_sub(ring + zigzag) / 6;
    assert!(
        pinned <= PINNED_QUADS_BOUND,
        "the stroke pins {pinned} quads in the ring's segment ({both} vertices against {ring} \
         for the ring and {zigzag} for the stroke)"
    );
}

#[test]
fn a_stroke_that_follows_a_banded_ring_covers_the_pixels_of_its_lines() {
    let Some((frame, _)) = support::warm_app_frame(RingThenZigzag, FRAME, FRAME) else {
        return;
    };
    let mut lines = Vec::new();
    for_each_stroke_line(
        &short_zigzag(),
        false,
        ZIGZAG_STROKE,
        |line: LineGeometry| {
            lines.push(line);
        },
    );
    assert!(lines.len() >= 60, "the zigzag strokes as its segments");
    let mut drawn = 0;
    for y in BELOW_RING..FRAME {
        for x in 0..FRAME {
            let centre = Point::new(x as f32 + 0.5, y as f32 + 0.5);
            // Each line blends over what the ones before it left.
            let value = lines.iter().fold(0.0f32, |value, line| {
                value + line.coverage(centre) * (1.0 - value)
            });
            let expected = (value * 255.0).round();
            let actual = red(&frame.pixels, x, y);
            drawn += i32::from(actual > 0);
            assert!(
                (f32::from(actual) - expected).abs() <= f32::from(TOLERANCE),
                "pixel ({x}, {y}) is {actual}, the stroke's lines say {expected}"
            );
        }
    }
    assert!(drawn > 0, "the stroke must draw something");
}

// The band between two series that cross three times: an upper series
// from left to right, then a lower one back.
fn crossing_band() -> Path {
    let upper = |x: f32| 48.0 + 30.0 * (x * 0.11).sin();
    let lower = |x: f32| 48.0 + 26.0 * (x * 0.11 + 2.2).sin();
    let xs: Vec<f32> = (0..=44).map(|index| 4.0 + index as f32 * 2.0).collect();
    let mut path = Path::new();
    for (index, &x) in xs.iter().enumerate() {
        let point = Point::new(x, upper(x));
        if index == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    for &x in xs.iter().rev() {
        path.line_to(Point::new(x, lower(x)));
    }
    path.close();
    path
}

// The band alone, which the walk along its two chains slices.
#[composable]
fn CrossingBand() {
    DrawPage(|scope| {
        scope.draw_path(
            &crossing_band(),
            Brush::solid(Color::WHITE),
            DrawStyle::Fill,
        );
    });
}

// The band with a second contour past the frame's right edge, which sends
// the fill to the sweep over all its edges.
#[composable]
fn CrossingBandBesideAnother() {
    DrawPage(|scope| {
        let mut path = crossing_band();
        path.move_to(Point::new(200.0, 10.0));
        path.line_to(Point::new(210.0, 10.0));
        path.line_to(Point::new(205.0, 20.0));
        path.close();
        scope.draw_path(&path, Brush::solid(Color::WHITE), DrawStyle::Fill);
    });
}

#[test]
fn a_band_between_crossing_series_fills_as_the_sweep_fills_it() {
    let Some((walked, _)) = support::warm_app_frame(CrossingBand, FRAME, FRAME) else {
        return;
    };
    let Some((swept, _)) = support::warm_app_frame(CrossingBandBesideAnother, FRAME, FRAME) else {
        return;
    };
    let mut inked = 0;
    for y in 0..FRAME {
        for x in 0..FRAME {
            let (walk, sweep) = (red(&walked.pixels, x, y), red(&swept.pixels, x, y));
            inked += i32::from(walk > 0);
            assert!(
                walk.abs_diff(sweep) <= TOLERANCE,
                "pixel ({x}, {y}) is {walk} where the sweep fills {sweep}"
            );
        }
    }
    assert!(inked > 0, "the band must draw something");
}
