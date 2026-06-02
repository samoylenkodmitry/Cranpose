use cranpose_ui_graphics::{Brush, Color, Rect, TileMode};

const TRANSPARENT: Color = Color(0.0, 0.0, 0.0, 0.0);

#[doc(hidden)]
pub fn color_to_rgba(color: Color) -> [f32; 4] {
    [
        color.0.clamp(0.0, 1.0),
        color.1.clamp(0.0, 1.0),
        color.2.clamp(0.0, 1.0),
        color.3.clamp(0.0, 1.0),
    ]
}

#[doc(hidden)]
pub fn sample_brush_rgba(brush: &Brush, rect: Rect, x: f32, y: f32) -> [f32; 4] {
    match brush {
        Brush::Solid(color) => color_to_rgba(*color),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            let sx = resolve_gradient_point(rect.x, rect.width, start.x);
            let sy = resolve_gradient_point(rect.y, rect.height, start.y);
            let ex = resolve_gradient_point(rect.x, rect.width, end.x);
            let ey = resolve_gradient_point(rect.y, rect.height, end.y);
            let dx = ex - sx;
            let dy = ey - sy;
            let denom = (dx * dx + dy * dy).max(f32::EPSILON);
            let t = ((x - sx) * dx + (y - sy) * dy) / denom;
            match normalize_gradient_t(t, *tile_mode) {
                Some(sample_t) => {
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t))
                }
                None => color_to_rgba(TRANSPARENT),
            }
        }
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let radius = (*radius).max(f32::EPSILON);
            let dx = x - cx;
            let dy = y - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let t = distance / radius;
            match normalize_gradient_t(t, *tile_mode) {
                Some(sample_t) => {
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t))
                }
                None => color_to_rgba(TRANSPARENT),
            }
        }
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            let cx = rect.x + center.x;
            let cy = rect.y + center.y;
            let dx = x - cx;
            let dy = y - cy;
            let angle = dy.atan2(dx);
            let t = (angle / std::f32::consts::TAU + 0.5).clamp(0.0, 1.0);
            color_to_rgba(interpolate_colors(colors, stops.as_deref(), t))
        }
    }
}

fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        origin + value
    } else if value.is_sign_positive() {
        origin + extent
    } else {
        origin
    }
}

#[doc(hidden)]
pub fn normalize_gradient_t(t: f32, tile_mode: TileMode) -> Option<f32> {
    match tile_mode {
        TileMode::Clamp => Some(t.clamp(0.0, 1.0)),
        TileMode::Decal => {
            if (0.0..=1.0).contains(&t) {
                Some(t)
            } else {
                None
            }
        }
        TileMode::Repeated => Some(t.rem_euclid(1.0)),
        TileMode::Mirror => {
            let wrapped = t.rem_euclid(2.0);
            if wrapped <= 1.0 {
                Some(wrapped)
            } else {
                Some(2.0 - wrapped)
            }
        }
    }
}

fn interpolate_colors(colors: &[Color], stops: Option<&[f32]>, t: f32) -> Color {
    if colors.is_empty() {
        return TRANSPARENT;
    }
    if colors.len() == 1 {
        return colors[0];
    }
    let clamped = t.clamp(0.0, 1.0);

    if let Some(stops) = stops {
        if stops.len() == colors.len() {
            if clamped <= stops[0] {
                return colors[0];
            }
            for index in 0..(stops.len() - 1) {
                let start = stops[index];
                let end = stops[index + 1];
                if clamped <= end {
                    let span = (end - start).max(f32::EPSILON);
                    let frac = ((clamped - start) / span).clamp(0.0, 1.0);
                    return lerp_color(colors[index], colors[index + 1], frac);
                }
            }
            return last_color(colors);
        }
    }

    let segments = (colors.len() - 1) as f32;
    let scaled = clamped * segments;
    let index = scaled.floor() as usize;
    if index >= colors.len() - 1 {
        return last_color(colors);
    }
    let frac = scaled - index as f32;
    lerp_color(colors[index], colors[index + 1], frac)
}

fn last_color(colors: &[Color]) -> Color {
    colors.last().copied().unwrap_or(TRANSPARENT)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let lerp = |start: f32, end: f32| start + (end - start) * t;
    Color(
        lerp(a.0, b.0),
        lerp(a.1, b.1),
        lerp(a.2, b.2),
        lerp(a.3, b.3),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::Point;

    fn sample_rect() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        }
    }

    #[test]
    fn empty_gradient_samples_transparent_instead_of_panicking() {
        let brush =
            Brush::linear_gradient_range(Vec::new(), Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        assert_eq!(
            sample_brush_rgba(&brush, sample_rect(), 50.0, 10.0),
            [0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn clamped_gradient_samples_last_color_at_end() {
        let brush = Brush::linear_gradient_range(
            vec![Color::RED, Color::BLUE],
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
        );
        assert_eq!(
            sample_brush_rgba(&brush, sample_rect(), 120.0, 10.0),
            color_to_rgba(Color::BLUE)
        );
    }

    #[test]
    fn mirror_tile_mode_normalizes_across_repeated_segments() {
        assert_eq!(normalize_gradient_t(1.25, TileMode::Mirror), Some(0.75));
        assert_eq!(normalize_gradient_t(1.75, TileMode::Mirror), Some(0.25));
    }
}
