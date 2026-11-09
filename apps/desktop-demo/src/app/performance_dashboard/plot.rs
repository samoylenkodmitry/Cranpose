// The chart area of the Performance tab: its grid, its scale and its labels.

use std::rc::Rc;

use cranpose_ui::{
    composable, Box, BoxSpec, Brush, Canvas, DrawScope, Modifier, Point, Stroke, Text,
};

use super::{text_style, Palette};

/// How tall a launch chart is, in points.
pub(super) const CHART_HEIGHT: f32 = 190.0;
/// The room over the top grid line, as a share of its value: a line at the
/// top stays off the chart's edge.
const CHART_HEADROOM: f64 = 1.08;

// A chart's scale: the values its bottom and top grid lines stand for, and
// the step between its grid lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Axis {
    pub(super) floor: f64,
    pub(super) top: f64,
    pub(super) step: f64,
}

// Data this far above zero, as a share of its highest value, gets a floor of
// its own: zero would leave it in a thin band at the top of the chart.
const FLOOR_SHARE: f64 = 0.6;

impl Axis {
    // The axis from zero for values up to `peak`: the step 1, 2 or 5 times a
    // power of ten that splits the top in three at most.
    pub(super) fn from_zero(peak: f64) -> Self {
        let peak = if peak.is_finite() { peak.max(1.0) } else { 1.0 };
        let step = nice_step(peak);
        Self {
            floor: 0.0,
            top: step * (peak / step).ceil(),
            step,
        }
    }

    // The axis for values from `low` to `peak`: from zero, unless they all
    // stay above FLOOR_SHARE of `peak`, then from the grid line under them, so
    // that their differences fill the chart.
    pub(super) fn fitting(low: f64, peak: f64) -> Self {
        if !(low.is_finite() && peak.is_finite()) || low < FLOOR_SHARE * peak || peak <= low {
            return Self::from_zero(peak);
        }
        let span = peak - low;
        let step = nice_step(span);
        Self {
            floor: step * ((low - 0.05 * span) / step).floor(),
            top: step * (peak / step).ceil(),
            step,
        }
    }

    // Where `value` sits on a chart `height` tall: the floor at its bottom
    // edge and the top grid line under a little headroom; a value beyond
    // them sits on them.
    pub(super) fn y(&self, height: f32, value: f64) -> f32 {
        let span = self.top - self.floor;
        let share = (value.clamp(self.floor, self.top) - self.floor) / (span * CHART_HEADROOM);
        height * (1.0 - share as f32)
    }

    // The grid lines, every `step` from the floor to the top.
    fn grid(&self) -> impl Iterator<Item = f64> + use<> {
        let (step, top) = (self.step, self.top);
        let first = (self.floor / step).round() * step;
        std::iter::successors(Some(first), move |value| Some(value + step))
            .take_while(move |value| *value <= top + step * 1e-9)
    }
}

// The step, 1, 2 or 5 times a power of ten, that splits `span` in three at
// most.
fn nice_step(span: f64) -> f64 {
    let rough = span / 3.0;
    let power = 10f64.powf(rough.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * power)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * power)
}

/// What a chart paints over its grid.
type ChartDraw = Rc<dyn Fn(&mut dyn DrawScope)>;

/// A chart's area: a labelled grid line on its `axis`, under what `draw`
/// paints.
#[composable]
pub(super) fn Plot(palette: Palette, axis: Axis, draw: impl Fn(&mut dyn DrawScope) + 'static) {
    let draw: ChartDraw = Rc::new(draw);
    Box(
        Modifier::empty().fill_max_width().height(CHART_HEIGHT),
        BoxSpec::default(),
        move || {
            let draw = Rc::clone(&draw);
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                let size = scope.size();
                for value in axis.grid() {
                    let y = axis.y(size.height, value).min(size.height - 0.5);
                    scope.draw_line(
                        Brush::solid(palette.baseline.with_alpha(0.3)),
                        Point { x: 0.0, y },
                        Point { x: size.width, y },
                        Stroke::new(1.0),
                    );
                }
                draw(scope);
            });
            let decimals = match axis.step {
                step if step >= 1.0 => 0,
                step if step >= 0.1 => 1,
                step if step >= 0.01 => 2,
                _ => 3,
            };
            for value in axis.grid() {
                let y = axis.y(CHART_HEIGHT, value) - 14.0;
                Text(
                    format!("{value:.decimals$}"),
                    Modifier::empty().offset(2.0, y.max(0.0)),
                    text_style(10.0, palette.muted, None),
                );
            }
        },
    );
}
