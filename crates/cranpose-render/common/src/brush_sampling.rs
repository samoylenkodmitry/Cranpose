use cranpose_ui_graphics::{BrushSampler, Color, Point, whole};

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
    dither_offset(dither_cell(x), dither_cell(y))
}

// The column or row of the pattern a coordinate falls in, of which only
// the two low bits count.
fn dither_cell(coordinate: f32) -> u32 {
    whole::floor(coordinate).max(0.0) as u32 + 1
}

fn dither_offset(column: u32, row: u32) -> f32 {
    let m = ((row & 1) << 3) | ((column & 1) << 2) | (row & 2) | ((column & 2) >> 1);
    m as f32 * (1.0 / 16.0) - (15.0 / 32.0)
}

fn dither_by(rgba: [f32; 4], offset: f32) -> [f32; 4] {
    if rgba[3] <= 0.0 {
        return rgba;
    }
    let offset = offset * LEVEL;
    [
        (rgba[0] + offset).clamp(0.0, 1.0),
        (rgba[1] + offset).clamp(0.0, 1.0),
        (rgba[2] + offset).clamp(0.0, 1.0),
        rgba[3],
    ]
}

/// The brush's color at `(x, y)`, a gradient's dithered from
/// `dither_origin` as Skia dithers it.
#[doc(hidden)]
pub fn sample_brush_rgba(
    brush: &BrushSampler<'_>,
    x: f32,
    y: f32,
    dither_origin: Point,
) -> [f32; 4] {
    let rgba = color_to_rgba(brush.color_at(Point::new(x, y)));
    if brush.is_solid() {
        return rgba;
    }
    dither_by(
        rgba,
        gradient_dither_offset(x - dither_origin.x, y - dither_origin.y),
    )
}

/// The colors a brush gives the pixels of one row or one column along which
/// it does not change: what [`sample_brush_rgba`] gives each, worked out
/// once for each of the four cells of the dither pattern along the line.
#[doc(hidden)]
pub struct DitheredLine {
    by_cell: [[f32; 4]; 4],
}

impl DitheredLine {
    /// The row at `y` of `brush`, which [`BrushSampler::has_uniform_rows`],
    /// sampled at its pixel centre `x`: its colors by the column of the
    /// pattern a pixel falls in.
    pub fn row(brush: &BrushSampler<'_>, x: f32, y: f32, dither_origin: Point) -> Self {
        let row = dither_cell(y - dither_origin.y);
        Self::dithered(brush, Point::new(x, y), |column| dither_offset(column, row))
    }

    /// The column at `x` of `brush`, which
    /// [`BrushSampler::has_uniform_columns`], sampled at its pixel centre
    /// `y`: its colors by the row of the pattern a pixel falls in.
    pub fn column(brush: &BrushSampler<'_>, x: f32, y: f32, dither_origin: Point) -> Self {
        let column = dither_cell(x - dither_origin.x);
        Self::dithered(brush, Point::new(x, y), |row| dither_offset(column, row))
    }

    fn dithered(brush: &BrushSampler<'_>, at: Point, offset: impl Fn(u32) -> f32) -> Self {
        let rgba = color_to_rgba(brush.color_at(at));
        if brush.is_solid() {
            return Self { by_cell: [rgba; 4] };
        }
        Self {
            by_cell: [0, 1, 2, 3].map(|cell| dither_by(rgba, offset(cell))),
        }
    }

    /// The four colors, by the cell of the pattern a pixel falls in.
    pub fn colors(&self) -> &[[f32; 4]; 4] {
        &self.by_cell
    }
}

/// An axis of the dither pattern measured from `origin`: which of its four
/// cells along the axis each pixel falls in.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DitherAxis {
    origin: f32,
    // The origin as a whole number, when it is one that a pixel's centre
    // subtracts from exactly.
    whole_origin: Option<i32>,
}

impl DitherAxis {
    pub fn new(origin: f32) -> Self {
        Self {
            origin,
            whole_origin: (whole::fract(origin) == 0.0 && origin.abs() < WHOLE_COORDINATES)
                .then_some(origin as i32),
        }
    }

    /// The cell of the pixel at `pixel`, which [`DitheredLine::colors`]
    /// are indexed by.
    pub fn cell(self, pixel: i32) -> usize {
        // A whole origin puts the centre of the pixel at
        // `pixel - origin + 0.5`, exactly, so its cell is
        // `max(pixel - origin, 0) + 1` without the float steps.
        let cell = match self.whole_origin {
            Some(origin) if pixel.abs() < WHOLE_PIXELS => (pixel - origin).max(0) as u32 + 1,
            _ => dither_cell(pixel as f32 + 0.5 - self.origin),
        };
        (cell & 3) as usize
    }

    /// The cell of `start`, when the cells of the pixels `start..end` step
    /// by one from it and start over after the fourth: from a whole origin
    /// on, they do.
    pub fn stepping_cell(self, start: i32, end: i32) -> Option<usize> {
        let origin = self.whole_origin?;
        (start >= origin && start > -WHOLE_PIXELS && end <= WHOLE_PIXELS).then(|| self.cell(start))
    }
}

// Below this magnitude a pixel's centre and a whole origin subtract exactly.
const WHOLE_COORDINATES: f32 = 4_194_304.0;
const WHOLE_PIXELS: i32 = 4_194_304;

#[cfg(test)]
#[path = "tests/brush_sampling_tests.rs"]
mod tests;
