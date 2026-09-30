use crate::{BRUSH_KIND_LINEAR, BrushRecord, GradientStopRecord, TileMode};

const INFINITE_GRADIENT_POINT: f32 = 1.0e30;
const MIN_STOP_SPAN: f32 = 0.00001;
const MIN_SQUARED_LENGTH: f32 = 1.0;

pub(crate) fn spans_quad(
    brush: &BrushRecord,
    stops: &[GradientStopRecord],
    rect: [f32; 4],
) -> bool {
    let start = brush.stop_start as usize;
    let (Some(first), Some(last)) = (stops.get(start), stops.get(start + 1)) else {
        return false;
    };
    if brush.kind != BRUSH_KIND_LINEAR
        || brush.stop_count != 2
        || !continuous(brush.tile_mode)
        || brush.params.iter().any(|value| value.is_nan())
    {
        return false;
    }
    let [_, _, width, height] = rect;
    let [start_x, start_y, end_x, end_y] = [
        (brush.params[0], width),
        (brush.params[1], height),
        (brush.params[2], width),
        (brush.params[3], height),
    ]
    .map(|(value, extent)| resolved_point(value, extent));
    let (dx, dy) = (end_x - start_x, end_y - start_y);
    let length = dx * dx + dy * dy;
    let (low, high) = (first.position[0], last.position[0]);
    let (band_start, band_end) = (low.max(0.0), high.min(1.0));
    high - low >= MIN_STOP_SPAN
        && length >= MIN_SQUARED_LENGTH
        && [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
            .into_iter()
            .all(|(x, y)| {
                let t = ((x - start_x) * dx + (y - start_y) * dy) / length;
                t >= band_start && t <= band_end
            })
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
