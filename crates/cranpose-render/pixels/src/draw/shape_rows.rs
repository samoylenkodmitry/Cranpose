//! How a placed shape paints its rows: the loops each kind of coverage
//! runs, and the coverage of a pixel.

use super::{
    BlendMode, BrushSampler, COLUMN_BAND, ClipBounds, DitherAxis, DitheredLine, GradientColors,
    Pixel, Point, Rect, RenderDiagnostics, RowRuns, Target, color_to_rgba, opaque_bytes, paint_row,
    sample_brush_rgba, shape_sdf,
};

// A placed shape's rows to paint. Each kind of coverage paints them through
// its own copy of the loop: the work a kind does once per shape, such as an
// arc's sines or a line's direction, then stays out of the other kinds'
// rows, where the compiler would otherwise hoist it for every shape.
pub(super) struct ShapeRows<'a, 'p, P> {
    pub(super) target: &'a mut Target<'p, P>,
    pub(super) draw: &'a crate::scene::DrawShape,
    pub(super) rect: Rect,
    pub(super) clip_bounds: ClipBounds,
    pub(super) dither_origin: Point,
    // The pixel the brush leaves on a pixel it covers whole, when it is an
    // opaque solid colour.
    pub(super) opaque: Option<P>,
    pub(super) diagnostics: &'a RenderDiagnostics,
}

impl<P: Pixel> ShapeRows<'_, '_, P> {
    // Paints a fill, which covers every pixel of its runs whole.
    pub(super) fn fill(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        match self.opaque {
            Some(pixel) => {
                let bounds = self.clip_bounds;
                for py in bounds.min_y..bounds.max_y {
                    for (start, end) in runs(py as f32 + 0.5, bounds.min_x, bounds.max_x) {
                        P::fill(self.target.row(py, start, end), pixel);
                    }
                }
            }
            None => {
                let brush = BrushSampler::new(&self.draw.brush, self.rect);
                if brush.has_uniform_rows() {
                    self.fill_uniform_rows(runs);
                } else if brush.has_uniform_columns()
                    && brush.is_opaque()
                    && self.draw.blend_mode == BlendMode::SrcOver
                {
                    self.fill_uniform_columns(runs);
                } else {
                    self.paint(runs, |_| Coverage::of(1.0));
                }
            }
        }
    }

    // Paints a fill whose brush gives every pixel of a row one color, such
    // as a gradient that runs straight down: the pixels [`Self::paint`]
    // paints, with each row's colors worked out once for the four columns
    // of the dither pattern instead of once a pixel.
    #[inline(never)]
    fn fill_uniform_rows(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            diagnostics,
            ..
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        let columns = DitherAxis::new(dither_origin.x);
        for py in clip_bounds.min_y..clip_bounds.max_y {
            let center_y = py as f32 + 0.5;
            let row_runs = runs(center_y, clip_bounds.min_x, clip_bounds.max_x);
            let Some(&(first, _)) = row_runs.iter().find(|(start, end)| start < end) else {
                continue;
            };
            let x = first as f32 + 0.5;
            if P::GRADIENT == GradientColors::Plain {
                let color = color_to_rgba(brush.color_at(Point::new(x, center_y)));
                paint_row(target, py, row_runs, color, draw.blend_mode, diagnostics);
                continue;
            }
            let colors = *DitheredLine::row(&brush, x, center_y, dither_origin).colors();
            let cells = colors.map(|color| {
                (draw.blend_mode == BlendMode::SrcOver && color[3] >= 1.0)
                    .then(|| P::opaque(opaque_bytes(color)))
            });
            let opaque = match cells {
                [Some(a), Some(b), Some(c), Some(d)] => Some([a, b, c, d]),
                _ => None,
            };
            let paints = colors.map(|color| P::paint(color, draw.blend_mode));
            for (start, end) in row_runs {
                let pixels = target.row(py, start, end);
                if let Some(pattern) = opaque
                    && let Some(first) = columns.stepping_cell(start, end)
                {
                    P::fill_pattern(pixels, [0, 1, 2, 3].map(|step| pattern[(first + step) & 3]));
                    continue;
                }
                for (px, dst) in (start..).zip(pixels) {
                    let cell = columns.cell(px);
                    match cells[cell] {
                        Some(pixel) => *dst = pixel,
                        None if colors[cell][3] > 0.0 => {
                            dst.blend_paint(paints[cell], 1.0, diagnostics);
                        }
                        None => {}
                    }
                }
            }
        }
    }

    // Paints a fill whose opaque brush gives every pixel of a column one
    // color, such as a gradient that runs straight across: the pixels
    // [`Self::paint`] paints, with each column's bytes worked out once for
    // the four rows of the dither pattern instead of once a pixel.
    #[inline(never)]
    fn fill_uniform_columns(self, runs: impl Fn(f32, i32, i32) -> RowRuns) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            ..
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        let rows = DitherAxis::new(dither_origin.y);
        let top = clip_bounds.min_y as f32 + 0.5;
        let mut band = [[P::opaque([0, 0, 0, 255]); 4]; COLUMN_BAND];
        let mut left = clip_bounds.min_x;
        while left < clip_bounds.max_x {
            let right = left
                .saturating_add(COLUMN_BAND as i32)
                .min(clip_bounds.max_x);
            for (cells, px) in band.iter_mut().zip(left..right) {
                let x = px as f32 + 0.5;
                *cells = match P::GRADIENT {
                    GradientColors::Dithered => DitheredLine::column(&brush, x, top, dither_origin)
                        .colors()
                        .map(|color| P::opaque(opaque_bytes(color))),
                    GradientColors::Plain => {
                        let color = color_to_rgba(brush.color_at(Point::new(x, top)));
                        [P::opaque(opaque_bytes(color)); 4]
                    }
                };
            }
            for py in clip_bounds.min_y..clip_bounds.max_y {
                let cell = rows.cell(py);
                for (start, end) in runs(py as f32 + 0.5, left, right) {
                    if start >= end {
                        continue;
                    }
                    let first = (start - left) as usize;
                    for (dst, cells) in target.row(py, start, end).iter_mut().zip(&band[first..]) {
                        *dst = cells[cell];
                    }
                }
            }
            left = right;
        }
    }

    // Paints the pixels of each row's `runs` by their `coverage`, which
    // is not asked again for the pixels a coverage settles. Out of line,
    // as [`draw_shadow`] is.
    #[inline(never)]
    pub(super) fn paint(
        self,
        runs: impl Fn(f32, i32, i32) -> RowRuns,
        coverage: impl Fn(Point) -> Coverage,
    ) {
        let Self {
            target,
            draw,
            rect,
            clip_bounds,
            dither_origin,
            opaque,
            diagnostics,
        } = self;
        let brush = BrushSampler::new(&draw.brush, rect);
        // A solid brush blends the same color over every pixel.
        let solid = brush.is_solid().then(|| {
            P::paint(
                color_to_rgba(brush.color_at(Point::default())),
                draw.blend_mode,
            )
        });
        let settles = clip_bounds.within(SETTLED_COORDINATES);
        for py in clip_bounds.min_y..clip_bounds.max_y {
            let center_y = py as f32 + 0.5;
            for (start, end) in runs(center_y, clip_bounds.min_x, clip_bounds.max_x) {
                let mut settled = Coverage::of(0.0);
                for (px, dst) in (start..).zip(target.row(py, start, end)) {
                    let center_x = px as f32 + 0.5;
                    let coverage = if settled.next > 0 {
                        settled.next -= 1;
                        settled.value
                    } else {
                        settled = coverage(Point::new(center_x, center_y));
                        if !settles {
                            settled.next = 0;
                        }
                        settled.value
                    };
                    if coverage <= 0.0 {
                        continue;
                    }
                    if let Some(pixel) = opaque
                        && coverage >= 1.0
                    {
                        *dst = pixel;
                        continue;
                    }

                    let paint = solid.unwrap_or_else(|| {
                        let sample = match P::GRADIENT {
                            GradientColors::Dithered => {
                                sample_brush_rgba(&brush, center_x, center_y, dither_origin)
                            }
                            GradientColors::Plain => {
                                color_to_rgba(brush.color_at(Point::new(center_x, center_y)))
                            }
                        };
                        P::paint(sample, draw.blend_mode)
                    });
                    dst.blend_paint(paint, coverage, diagnostics);
                }
            }
        }
    }
}

// A pixel's coverage, and how many pixels after it along the row take the
// same, which `ShapeRows::paint` then does not work out.
#[derive(Clone, Copy)]
pub(super) struct Coverage {
    value: f32,
    next: u32,
}

impl Coverage {
    pub(super) fn of(value: f32) -> Self {
        Self { value, next: 0 }
    }

    // The coverage of a pixel at `distance` from a shape's edge, where the
    // distance changes by at most one from pixel to pixel, as an exact
    // distance does. Inside by more than half a pixel the coverage is 1,
    // outside by as much 0, and a distance that far in or out stays there for
    // as many whole pixels as it reaches past that half pixel. The margin
    // keeps the float error of the distance, far smaller, from settling a
    // pixel the distance itself would not.
    pub(super) fn of_distance(distance: f32) -> Self {
        let value = shape_sdf::coverage_for_distance(distance);
        let reach = if distance <= -0.5 - SETTLE_MARGIN {
            -0.5 - SETTLE_MARGIN - distance
        } else if distance >= 0.5 + SETTLE_MARGIN {
            distance - 0.5 - SETTLE_MARGIN
        } else {
            0.0
        };
        Self {
            value,
            next: if distance.is_finite() {
                reach as u32
            } else {
                0
            },
        }
    }
}

// Far more than the rounding of a distance between points of a window of up
// to `SETTLED_COORDINATES` pixels a side.
const SETTLE_MARGIN: f32 = 1.0 / 32.0;
const SETTLED_COORDINATES: i32 = 16_384;
