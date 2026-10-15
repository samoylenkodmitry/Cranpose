use cranpose_ui_graphics::{Brush, Color, Point, Rect};

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

/// The brush's color at `(x, y)` in `rect`, a gradient's dithered from
/// `dither_origin` as Skia dithers it.
#[doc(hidden)]
pub fn sample_brush_rgba(
    brush: &Brush,
    rect: Rect,
    x: f32,
    y: f32,
    dither_origin: Point,
) -> [f32; 4] {
    let rgba = color_to_rgba(brush.color_at(rect, Point::new(x, y)));
    match brush {
        Brush::Solid(_) => rgba,
        _ => dither_gradient(rgba, x - dither_origin.x, y - dither_origin.y),
    }
}

#[cfg(test)]
#[path = "tests/brush_sampling_tests.rs"]
mod tests;
