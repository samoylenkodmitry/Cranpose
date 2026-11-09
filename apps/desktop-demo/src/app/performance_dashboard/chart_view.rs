// What a launch chart shows of its curves: which it draws, on what scale, and
// what the pointer reads from them.

use cranpose_ui::{Point, Size};

use super::{launch_chart::LaunchCurve, plot::Axis, Curve, Metric, CHART_STEP_MS};

// A chart always draws its best LEADERS curves. Past them it draws only the
// curves that reach no more than LEADER_REACH times as far as the leaders: a
// curve far above them stretches the scale until the leaders collapse into
// one line.
const LEADERS: usize = 5;
const LEADER_REACH: f64 = 1.5;
// How near, in points, the pointer comes to a curve to pick it up, or to
// another curve to let go of the one it holds.
const PICK_DISTANCE: f32 = 12.0;
// The frame rates between a chart's grid lines.
const FPS_STEP: f64 = 30.0;

// The curves of a launch chart with the run's length and the metric they read.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plotted {
    pub(super) curves: Vec<LaunchCurve>,
    pub(super) run_ms: f64,
    pub(super) metric: Metric,
}

impl Plotted {
    // Where a curve's `value` at `step` sits on a chart of `size` on `axis`.
    pub(super) fn point(&self, size: Size, step: usize, value: f64, axis: Axis) -> Point {
        Point {
            x: size.width * (step as f64 * CHART_STEP_MS / self.run_ms) as f32,
            y: axis.y(size.height, value),
        }
    }

    // The curve of `subject`, if it has one.
    pub(super) fn curve_of(&self, subject: usize) -> Option<&LaunchCurve> {
        self.curves
            .iter()
            .find(|curve| curve.line.subject == subject)
    }
}

// What sets a chart's scale: the highest launch of a sampled metric, the
// highest median of the frame rate.
pub(super) fn reach(curve: &LaunchCurve, kind: Curve) -> f64 {
    let values = if matches!(kind, Curve::Samples(_)) {
        &curve.highest
    } else {
        &curve.median
    };
    values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0, f64::max)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    // Drawn, and counted in the scale.
    Within,
    // Drawn but not counted: it rides the chart's top edge.
    Beyond,
    LeftOut,
}

// The place of each curve. While bars are `picked`, the picked curves are
// within and the rest are left out. Otherwise a frame rate chart keeps every
// curve, as its scale is the display's; a chart of a sampled metric keeps the
// leaders and the curves near them, and Cranpose's always, beyond the scale
// if it reaches far.
fn places(curves: &[LaunchCurve], kind: Curve, picked: &[usize]) -> Vec<Place> {
    if !picked.is_empty() {
        return curves
            .iter()
            .map(|curve| {
                if picked.contains(&curve.line.subject) {
                    Place::Within
                } else {
                    Place::LeftOut
                }
            })
            .collect();
    }
    if kind == Curve::Frames || curves.len() <= LEADERS {
        return vec![Place::Within; curves.len()];
    }
    let reaches: Vec<f64> = curves.iter().map(|curve| reach(curve, kind)).collect();
    let mut ranked: Vec<usize> = (0..curves.len()).collect();
    ranked.sort_by(|&a, &b| reaches[a].total_cmp(&reaches[b]));
    let leaders = reaches[ranked[LEADERS - 1]];
    let mut places = vec![Place::LeftOut; curves.len()];
    for (rank, &index) in ranked.iter().enumerate() {
        let near = rank < LEADERS || reaches[index] <= LEADER_REACH * leaders;
        places[index] = if near {
            Place::Within
        } else if curves[index].line.emphasized {
            Place::Beyond
        } else {
            Place::LeftOut
        };
    }
    places
}

// The subjects a chart draws and the scale it draws them on.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ChartView {
    pub(super) shown: Vec<usize>,
    // The subjects the chart leaves out for reaching far above its leaders;
    // none while bars are picked.
    pub(super) left_out: Vec<usize>,
    pub(super) picking: bool,
    pub(super) axis: Axis,
}

impl ChartView {
    pub(super) fn new(plotted: &Plotted, picked: &[usize]) -> Self {
        let kind = plotted.metric.curve();
        let places = places(&plotted.curves, kind, picked);
        let placed = || plotted.curves.iter().zip(&places);
        let within: Vec<&LaunchCurve> = placed()
            .filter(|(_, place)| **place == Place::Within)
            .map(|(curve, _)| curve)
            .collect();
        let peak = within
            .iter()
            .map(|curve| reach(curve, kind))
            .fold(0.0, f64::max);
        let low = within
            .iter()
            .flat_map(|curve| curve.lowest.iter().copied())
            .filter(|value| value.is_finite())
            .fold(f64::INFINITY, f64::min);
        let subjects = |wanted: fn(Place) -> bool| -> Vec<usize> {
            placed()
                .filter(|(_, place)| wanted(**place))
                .map(|(curve, _)| curve.line.subject)
                .collect()
        };
        let axis = if kind == Curve::Frames {
            Axis {
                floor: 0.0,
                top: chart_top(peak),
                step: FPS_STEP,
            }
        } else {
            Axis::fitting(low, peak)
        };
        Self {
            shown: subjects(|place| place != Place::LeftOut),
            left_out: if picked.is_empty() {
                subjects(|place| place == Place::LeftOut)
            } else {
                Vec::new()
            },
            picking: !picked.is_empty(),
            axis,
        }
    }

    // The step whose time is the nearest to the pointer at `x`.
    fn step_at(&self, plotted: &Plotted, x: f32, size: Size) -> usize {
        let last = plotted
            .curves
            .first()
            .map_or(0, |curve| curve.median.len().saturating_sub(1));
        let at = |step: usize| plotted.point(size, step, 0.0, self.axis).x;
        let after = (0..=last).take_while(|step| at(*step) < x).count();
        match after {
            0 => 0,
            after if after > last => last,
            after if x - at(after - 1) <= at(after) - x => after - 1,
            after => after,
        }
    }

    // What the pointer at `pointer` on a chart of `size` reads. Without
    // picked bars, it holds the curve it reads until another curve comes
    // within PICK_DISTANCE of it, and reads none while no curve does. With
    // picked bars, it reads them all at the time under it.
    pub(super) fn read(
        &self,
        plotted: &Plotted,
        pointer: Point,
        size: Size,
        held: Option<usize>,
    ) -> Option<ChartHover> {
        if size.width <= 0.0 {
            return None;
        }
        let step = self.step_at(plotted, pointer.x, size);
        let reading = |curve: &LaunchCurve| -> Option<Point> {
            let value = curve
                .median
                .get(step)
                .copied()
                .filter(|value| value.is_finite())?;
            self.shown
                .contains(&curve.line.subject)
                .then(|| plotted.point(size, step, value, self.axis))
        };
        let hover = |subject, anchor| ChartHover {
            subject,
            step,
            anchor,
            width: size.width,
        };
        if self.picking {
            let x = plotted.point(size, step, 0.0, self.axis).x;
            let reads = plotted.curves.iter().any(|curve| reading(curve).is_some());
            return reads.then(|| hover(None, Point { x, y: pointer.y }));
        }
        let nearest = plotted
            .curves
            .iter()
            .filter_map(|curve| Some((curve.line.subject, reading(curve)?)))
            .min_by(|(_, a), (_, b)| (a.y - pointer.y).abs().total_cmp(&(b.y - pointer.y).abs()))
            .filter(|(_, point)| (point.y - pointer.y).abs() <= PICK_DISTANCE);
        let kept = held.and_then(|subject| Some((subject, reading(plotted.curve_of(subject)?)?)));
        nearest
            .or(kept)
            .map(|(subject, point)| hover(Some(subject), point))
    }
}

// Where the pointer is over a chart: the time it reads, and where a tooltip
// hangs. A curve the pointer holds is named; with picked bars none is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ChartHover {
    pub(super) subject: Option<usize>,
    pub(super) step: usize,
    pub(super) anchor: Point,
    // The side of the anchor a tooltip takes is the one with more room.
    pub(super) width: f32,
}

// The frame rate a launch chart's top stands for: the next `FPS_STEP` above
// its highest median, `peak`, two steps at least. A median a frame or two over
// a grid line, as a window can count at 60 Hz, stays under it.
fn chart_top(peak: f64) -> f64 {
    (FPS_STEP * (peak / FPS_STEP - 0.1).ceil()).max(2.0 * FPS_STEP)
}
