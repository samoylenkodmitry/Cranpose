use cranpose_ui_graphics::{Brush, Color, Point, Rect, TileMode};

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

const LEVEL: f32 = 1.0 / 255.0;

/// The ordered-dither offset a gradient gets at device pixel `(x, y)`, in
/// output levels — the same value Skia adds, so a Cranpose gradient lands on
/// the same bytes as the Jetpack Compose gradient it is standing in for.
///
/// Skia dithers a gradient it draws to an 8-bit target. The pattern is not
/// noise: it is a 4x4 Bayer matrix built by striping the low two bits of the
/// device coordinate — `(X:a1a2, Y:b1b2)` becomes `b1 a1 b2 a2` — and mapped
/// onto `[-15/32, +15/32]`, half a level either way. Undithered, a slow ramp
/// quantises into visible bands; dithered, the band edges break into the
/// checkerboard every Android gradient has.
///
/// "A gradient", not "every gradient ever": this is the behaviour of the
/// platforms Cranpose targets, and it has a floor. Captured on one emulator
/// host from one APK, a Compose `radialGradient` over black comes back as
/// `round(255*v + m/16 − 15/32)` of the analytic ramp on an android-34 Wear
/// image at both 454x454 and 384x384, and as bare `round(255*v)` — no spatial
/// structure, residual variance exactly the 1/12 of a plain rounding — on an
/// android-30 one. Only the system image moves between those. So an
/// android-30 capture is not a reference for this function and never was; a
/// build compared against one reads as half a level wrong over most of every
/// gradient it draws, which is exactly what it should read as.
///
/// ```text
///  x→   0   1   2   3
/// y 0   0   4   1   5
///   1   8  12   9  13
///   2   2   6   3   7
///   3  10  14  11  15
/// ```
///
/// Recovered from device captures rather than from memory: binning
/// `compose − cranpose` over a radial gradient by `(x % 4, y % 4)` reproduces
/// this matrix, and the per-cell means track `m / 16 − 15 / 32` to within the
/// estimator's own bias.
///
/// Confirmed since without cranpose in the loop at all, which is the reading
/// that matters — the difference of two builds cannot say which one carries
/// the pattern. Against the gradient's own analytic ramp the Compose frame's
/// residual, binned the same way, IS this table: rms 0.005 of a level per
/// cell over 109k pixels, against 0.285 for the flat table an undithered
/// build gives.
///
/// The pattern is anchored one pixel on from the coordinate handed in, and
/// that is measured too. Evaluated at the fragment's own coordinate the
/// dither came out as the mirror of Skia's — two dithers disagreeing is worse
/// than one, and a captured frame went from 39.5% identical pixels against
/// the Compose build to 23.6%. A probe shader that painted `floor(position)`
/// straight into the frame said why: the fragment that lands on captured
/// column N reports column N-1. The pattern is a phase as much as a matrix,
/// so the phase is part of what has to match.
pub fn gradient_dither_offset(x: f32, y: f32) -> f32 {
    let px = x.floor().max(0.0) as u32 + 1;
    let py = y.floor().max(0.0) as u32 + 1;
    let m = ((py & 1) << 3) | ((px & 1) << 2) | (py & 2) | ((px & 2) >> 1);
    m as f32 * (1.0 / 16.0) - (15.0 / 32.0)
}

fn dither_gradient(rgba: [f32; 4], x: f32, y: f32) -> [f32; 4] {
    if rgba[3] <= 0.0 {
        return rgba;
    }
    let offset = gradient_dither_offset(x, y) * LEVEL;
    [
        (rgba[0] + offset).clamp(0.0, 1.0),
        (rgba[1] + offset).clamp(0.0, 1.0),
        (rgba[2] + offset).clamp(0.0, 1.0),
        rgba[3],
    ]
}

fn sample_tiled_gradient_rgba(
    t: f32,
    tile_mode: TileMode,
    colors: &[Color],
    stops: Option<&[f32]>,
    x: f32,
    y: f32,
) -> [f32; 4] {
    match normalize_gradient_t(t, tile_mode) {
        Some(sample_t) => dither_gradient(
            color_to_rgba(interpolate_colors(colors, stops, sample_t)),
            x,
            y,
        ),
        None => color_to_rgba(TRANSPARENT),
    }
}

#[doc(hidden)]
pub fn sample_brush_rgba(
    brush: &Brush,
    rect: Rect,
    x: f32,
    y: f32,
    dither_origin: Point,
) -> [f32; 4] {
    let dither = Point::new(x - dither_origin.x, y - dither_origin.y);
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
            sample_tiled_gradient_rgba(t, *tile_mode, colors, stops.as_deref(), dither.x, dither.y)
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
            sample_tiled_gradient_rgba(t, *tile_mode, colors, stops.as_deref(), dither.x, dither.y)
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
            dither_gradient(
                color_to_rgba(interpolate_colors(colors, stops.as_deref(), t)),
                dither.x,
                dither.y,
            )
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

    if let Some(stops) = stops
        && stops.len() == colors.len()
    {
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
#[path = "tests/brush_sampling_tests.rs"]
mod tests;
