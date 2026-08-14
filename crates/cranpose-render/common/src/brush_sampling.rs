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

/// One output level, the step an 8-bit render target quantises to.
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

/// `rgba` with the gradient dither for `(x, y)` folded in.
///
/// Alpha is left alone — Skia perturbs only the colour channels — and the
/// result is clamped, so a stop already at black or white cannot be pushed
/// outside the gamut by half a level. A pixel the gradient did not paint at
/// all is returned untouched: half a level of colour under zero alpha is
/// invisible on screen but not in a buffer, and an empty gradient has to stay
/// exactly transparent.
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
                Some(sample_t) => dither_gradient(
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t)),
                    x,
                    y,
                ),
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
                Some(sample_t) => dither_gradient(
                    color_to_rgba(interpolate_colors(colors, stops.as_deref(), sample_t)),
                    x,
                    y,
                ),
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
            dither_gradient(
                color_to_rgba(interpolate_colors(colors, stops.as_deref(), t)),
                x,
                y,
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
        // Past the end the stop is pure blue. The dither moves every channel
        // by less than half a level, so the colour that reaches an 8-bit
        // target is still pure blue whatever cell the pixel falls in.
        for y in 0..4 {
            for x in 0..4 {
                let sampled = sample_brush_rgba(&brush, sample_rect(), 120.0 + x as f32, y as f32);
                let bytes: Vec<u8> = sampled.iter().map(|c| (c * 255.0).round() as u8).collect();
                assert_eq!(bytes, vec![0, 0, 255, 255], "cell ({x}, {y})");
            }
        }
    }

    #[test]
    fn mirror_tile_mode_normalizes_across_repeated_segments() {
        assert_eq!(normalize_gradient_t(1.25, TileMode::Mirror), Some(0.75));
        assert_eq!(normalize_gradient_t(1.75, TileMode::Mirror), Some(0.25));
    }

    /// The matrix, spelled out. Written down rather than recomputed from the
    /// same bit-twiddle it is checking, so a "simplification" of the striping
    /// has something to fail against.
    const BAYER_4X4: [[u32; 4]; 4] = [[0, 4, 1, 5], [8, 12, 9, 13], [2, 6, 3, 7], [10, 14, 11, 15]];

    #[test]
    fn the_dither_lays_out_skias_bayer_matrix() {
        for y in 0..4u32 {
            for x in 0..4u32 {
                let expected = BAYER_4X4[y as usize][x as usize] as f32 / 16.0 - 15.0 / 32.0;
                // The one-pixel phase is undone here, so this test is about the
                // matrix and `the_dither_is_a_pixel_ahead_of_the_fragment` is
                // about where it sits.
                assert_eq!(
                    gradient_dither_offset(x as f32 - 1.0 + 4.0, y as f32 - 1.0 + 4.0),
                    expected,
                    "cell ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn the_dither_is_a_pixel_ahead_of_the_fragment() {
        assert_eq!(
            gradient_dither_offset(4.0, 4.0),
            gradient_dither_offset(5.0 - 1.0, 5.0 - 1.0),
        );
        assert_eq!(
            gradient_dither_offset(3.0, 3.0),
            BAYER_4X4[0][0] as f32 / 16.0 - 15.0 / 32.0,
        );
    }

    #[test]
    fn the_dither_repeats_every_four_pixels_and_never_moves_a_whole_level() {
        for y in 0..16u32 {
            for x in 0..16u32 {
                assert_eq!(
                    gradient_dither_offset(x as f32, y as f32),
                    gradient_dither_offset((x % 4) as f32, (y % 4) as f32),
                );
            }
        }
        let offsets: Vec<f32> = (0..4)
            .flat_map(|y| (0..4).map(move |x| gradient_dither_offset(x as f32, y as f32)))
            .collect();
        assert!(offsets.iter().all(|offset| offset.abs() < 0.5));
        // Half a level either way, and centred: the dither must not shift a
        // gradient's average brightness, only break up where it steps.
        let mean = offsets.iter().sum::<f32>() / offsets.len() as f32;
        assert!(mean.abs() < 1e-6, "mean offset {mean}");
    }

    #[test]
    fn a_solid_brush_is_left_alone() {
        let brush = Brush::Solid(Color(0.25, 0.5, 0.75, 1.0));
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    sample_brush_rgba(&brush, sample_rect(), x as f32, y as f32),
                    [0.25, 0.5, 0.75, 1.0],
                );
            }
        }
    }

    #[test]
    fn the_dither_moves_a_flat_gradient_off_one_value_onto_two() {
        // A ramp so slow that a whole 4x4 block samples the same colour is
        // exactly where undithered output bands. Rounded to bytes, the block
        // has to come out as two neighbouring levels, not one flat one.
        let grey = 100.4 / 255.0;
        let brush = Brush::linear_gradient_range(
            vec![Color(grey, grey, grey, 1.0), Color(grey, grey, grey, 1.0)],
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
        );
        let mut levels = std::collections::BTreeSet::new();
        for y in 0..4 {
            for x in 0..4 {
                let sampled = sample_brush_rgba(&brush, sample_rect(), x as f32, y as f32);
                levels.insert((sampled[0] * 255.0).round() as u8);
            }
        }
        assert_eq!(levels.into_iter().collect::<Vec<_>>(), vec![100, 101]);
    }
}
