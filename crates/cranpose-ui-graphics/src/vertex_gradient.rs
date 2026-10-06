use crate::{BRUSH_KIND_LINEAR, BrushRecord, GradientStopRecord, TileMode};

const INFINITE_GRADIENT_POINT: f32 = 1.0e30;
const MIN_STOP_SPAN: f32 = 0.00001;
const MIN_SQUARED_LENGTH: f32 = 1.0;

/// A linear gradient between two stops resolved against a `width` x
/// `height` rect: its start, its direction and squared length, and the
/// stops' positions. `None` for any other brush or a degenerate one.
struct TwoStopLine {
    start: (f32, f32),
    direction: (f32, f32),
    length: f32,
    stops: (f32, f32),
}

fn two_stop_line(
    brush: &BrushRecord,
    stops: &[GradientStopRecord],
    width: f32,
    height: f32,
) -> Option<TwoStopLine> {
    let start = brush.stop_start as usize;
    let (first, last) = (stops.get(start)?, stops.get(start + 1)?);
    if brush.kind != BRUSH_KIND_LINEAR
        || brush.stop_count != 2
        || brush.params.iter().any(|value| value.is_nan())
    {
        return None;
    }
    let [start_x, start_y, end_x, end_y] = [
        (brush.params[0], width),
        (brush.params[1], height),
        (brush.params[2], width),
        (brush.params[3], height),
    ]
    .map(|(value, extent)| resolved_point(value, extent));
    let (dx, dy) = (end_x - start_x, end_y - start_y);
    let line = TwoStopLine {
        start: (start_x, start_y),
        direction: (dx, dy),
        length: dx * dx + dy * dy,
        stops: (first.position[0], last.position[0]),
    };
    (line.stops.1 - line.stops.0 >= MIN_STOP_SPAN && line.length >= MIN_SQUARED_LENGTH)
        .then_some(line)
}

pub(crate) fn spans_quad(
    brush: &BrushRecord,
    stops: &[GradientStopRecord],
    rect: [f32; 4],
) -> bool {
    let [_, _, width, height] = rect;
    let Some(line) =
        two_stop_line(brush, stops, width, height).filter(|_| continuous(brush.tile_mode))
    else {
        return false;
    };
    let (band_start, band_end) = (line.stops.0.max(0.0), line.stops.1.min(1.0));
    [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
        .into_iter()
        .all(|(x, y)| {
            let t = ((x - line.start.0) * line.direction.0 + (y - line.start.1) * line.direction.1)
                / line.length;
            t >= band_start && t <= band_end
        })
}

/// Whether a slice of a path fill may take `brush`, resolved against
/// `brush_rect`, from its vertices: a clamped linear gradient between two
/// stops within `0..=1`. Clamping where the slice lies between the stops
/// then gives exactly the colour the gradient sampler gives.
pub(crate) fn clamps_between_two_stops(
    brush: &BrushRecord,
    stops: &[GradientStopRecord],
    brush_rect: [f32; 4],
) -> bool {
    let [_, _, width, height] = brush_rect;
    brush.tile_mode == TileMode::Clamp as u32
        && two_stop_line(brush, stops, width, height)
            .is_some_and(|line| line.stops.0 >= 0.0 && line.stops.1 <= 1.0)
}

fn continuous(tile_mode: u32) -> bool {
    tile_mode == TileMode::Clamp as u32 || tile_mode == TileMode::Mirror as u32
}

fn resolved_point(value: f32, extent: f32) -> f32 {
    if value.abs() < INFINITE_GRADIENT_POINT {
        value
    } else if value > 0.0 {
        extent
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "tests/vertex_gradient_tests.rs"]
mod tests;
