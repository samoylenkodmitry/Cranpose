//! The charts: a grid of charts whose paths move every frame, drawn from a
//! frame index `k` as the gauntlet's sparklines are. `Screens.kt` draws the
//! same screen in Jetpack Compose; the benchmark's README describes it.
//!
//! Each chart fills the band between its two series, which cross, draws a
//! gauge (a full track ring, then the arc of its value over it) and strokes
//! both series and a dashed target line. The rings draw as banded strips,
//! and the upper series' stroke follows them in the same draw, so its first
//! lines join the rings' run of records until that run cuts them off.

use std::f32::consts::{FRAC_PI_2, TAU};

use cranpose::prelude::*;

use crate::data::{self, PALETTE};

/// The charts in each row of the grid.
const COLUMNS: usize = 4;
/// The gauge: its radius, its stroke width and the distance of its centre
/// from the chart's top and right edges, in dp.
const GAUGE_RADIUS: f32 = 28.0;
const GAUGE_WIDTH: f32 = 3.0;
const GAUGE_INSET: f32 = 40.0;
const TRACK: Color = Color::from_rgb_u8(0xE5, 0xE7, 0xEB);
/// The series' stroke width and the target line's dashes, in dp.
const SERIES_WIDTH: f32 = 1.5;
const TARGET_DASHES: [f32; 2] = [6.0, 4.0];
const TARGET: Color = Color::from_rgb_u8(0x6B, 0x72, 0x80);
const CHART_BACKGROUND: Color = Color::from_rgb_u8(0xFF, 0xFF, 0xFF);

/// How heavy the charts screen is: `charts` charts of `points` points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChartsLoad {
    pub charts: usize,
    pub points: usize,
}

#[composable]
pub fn ChartsScreen(load: ChartsLoad) {
    let frame = rememberMutableStateOf(|| 0u32);
    LaunchedEffect((), async move |scope| {
        let clock = scope.runtime().frame_clock();
        let mut index = 0u32;
        loop {
            clock.next_frame().await;
            index += 1;
            frame.set(index);
        }
    });
    let rows = load.charts.div_ceil(COLUMNS);
    Column(
        Modifier::empty().fill_max_width().weight(1.0).padding(8.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            for row in 0..rows {
                Row(
                    Modifier::empty().fill_max_width().weight(1.0),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                    move || {
                        for chart in row * COLUMNS..load.charts.min((row + 1) * COLUMNS) {
                            Chart(chart, load.points, frame);
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn Chart(chart: usize, points: usize, frame: MutableState<u32>) {
    let line = PALETTE[chart % PALETTE.len()];
    let lower_line = PALETTE[(chart + 3) % PALETTE.len()];
    let fill = remember(move || {
        Brush::vertical_gradient_default(vec![line.with_alpha(0.45), line.with_alpha(0.05)])
    })
    .with(|fill| fill.clone());
    Canvas(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .background(CHART_BACKGROUND),
        move |scope| {
            // Read in the draw phase only: frames redraw, nothing recomposes.
            let k = frame.get();
            let size = scope.size();
            let step = size.width / (points - 1) as f32;
            let at = |series: usize, point: usize| Point {
                x: point as f32 * step,
                y: size.height * data::chart_value(chart, series, point, k),
            };
            // The band runs back along the lower series, then on along the
            // upper one; each series' stroke runs the same way.
            let mut band = Path::new();
            let mut lower = Path::new();
            for point in (0..points).rev() {
                let place = at(1, point);
                if point == points - 1 {
                    band.move_to(place);
                    lower.move_to(place);
                } else {
                    band.line_to(place);
                    lower.line_to(place);
                }
            }
            let mut upper = Path::new();
            for point in 0..points {
                let place = at(0, point);
                band.line_to(place);
                if point == 0 {
                    upper.move_to(place);
                } else {
                    upper.line_to(place);
                }
            }
            band.close();
            scope.draw_path(&band, fill.clone(), DrawStyle::Fill);
            let gauge = Point {
                x: size.width - GAUGE_INSET,
                y: GAUGE_INSET,
            };
            scope.draw_arc(
                Brush::solid(TRACK),
                gauge,
                GAUGE_RADIUS,
                -FRAC_PI_2,
                TAU,
                Stroke::new(GAUGE_WIDTH),
            );
            scope.draw_arc(
                Brush::solid(line),
                gauge,
                GAUGE_RADIUS,
                -FRAC_PI_2,
                TAU * data::chart_gauge(chart, k),
                Stroke::new(GAUGE_WIDTH),
            );
            scope.draw_path(
                &upper,
                Brush::solid(line),
                DrawStyle::Stroke(Stroke::new(SERIES_WIDTH)),
            );
            scope.draw_path(
                &lower,
                Brush::solid(lower_line),
                DrawStyle::Stroke(Stroke::new(SERIES_WIDTH).with_join(StrokeJoin::Round)),
            );
            let mut target = Path::new();
            target.move_to(Point {
                x: 0.0,
                y: size.height * 0.5,
            });
            target.line_to(Point {
                x: size.width,
                y: size.height * 0.5,
            });
            scope.draw_path(
                &target,
                Brush::solid(TARGET),
                DrawStyle::DashedStroke(Stroke::new(1.0), DashPathEffect::new(&TARGET_DASHES, 0.0)),
            );
        },
    );
}
