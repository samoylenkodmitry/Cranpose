// The chart area of the Performance tab: its grid, its scale and its labels.

use std::rc::Rc;

use cranpose_ui::{
    composable, Box, BoxSpec, Brush, Canvas, DrawScope, Modifier, Point, Stroke, Text,
};

use super::{text_style, Palette};

/// How tall a launch chart is, in points.
const CHART_HEIGHT: f32 = 190.0;
/// The room over the top grid line, as a share of its value: a line at the
/// top stays off the chart's edge.
const CHART_HEADROOM: f64 = 1.08;

/// A chart's top and the step between its grid lines for values up to
/// `peak`: the step 1, 2 or 5 times a power of ten that splits the top in
/// three at most.
pub(super) fn scale(peak: f64) -> (f64, f64) {
    let peak = if peak.is_finite() { peak.max(1.0) } else { 1.0 };
    let rough = peak / 3.0;
    let power = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * power)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * power);
    (step * (peak / step).ceil(), step)
}

/// The grid lines from 0 to `top`, every `step`.
fn grid(top: f64, step: f64) -> impl Iterator<Item = f64> {
    (0..)
        .map(move |line| f64::from(line) * step)
        .take_while(move |value| *value <= top)
}

/// Where `value` sits on a chart `height` tall whose top grid line stands
/// for `top`; a value over the top sits on it.
pub(super) fn chart_y(height: f32, value: f64, top: f64) -> f32 {
    height * (1.0 - (value.min(top) / (top * CHART_HEADROOM)) as f32)
}

/// What a chart paints over its grid.
type ChartDraw = Rc<dyn Fn(&mut dyn DrawScope)>;

/// A chart's area: a labelled grid line every `step` from 0 to `top`, under
/// what `draw` paints.
#[composable]
pub(super) fn Plot(
    palette: Palette,
    top: f64,
    step: f64,
    draw: impl Fn(&mut dyn DrawScope) + 'static,
) {
    let draw: ChartDraw = Rc::new(draw);
    Box(
        Modifier::empty().fill_max_width().height(CHART_HEIGHT),
        BoxSpec::default(),
        move || {
            let draw = Rc::clone(&draw);
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                let size = scope.size();
                for value in grid(top, step) {
                    let y = chart_y(size.height, value, top).min(size.height - 0.5);
                    scope.draw_line(
                        Brush::solid(palette.baseline.with_alpha(0.3)),
                        Point { x: 0.0, y },
                        Point { x: size.width, y },
                        Stroke::new(1.0),
                    );
                }
                draw(scope);
            });
            for value in grid(top, step) {
                let y = chart_y(CHART_HEIGHT, value, top) - 14.0;
                Text(
                    if step < 1.0 {
                        format!("{value:.1}")
                    } else {
                        format!("{value:.0}")
                    },
                    Modifier::empty().offset(2.0, y.max(0.0)),
                    text_style(10.0, palette.muted, None),
                );
            }
        },
    );
}
