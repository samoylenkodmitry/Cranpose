use cranpose_ui_graphics::{
    BAND_ANGULAR_PAD, BAND_MARGIN, FRAGMENT_KIND_ARC, Point, QUAD_VERTICES, RecordTables,
    ShapeRecord, TAU,
};

/// The strip a banded arc rasterizes as: `segments` quads between the
/// padded inner circle and a polygon circumscribing the padded outer
/// circle, over the padded sweep. Mirrors `vs_band` in `shape.wgsl`: the
/// fill estimate and the coverage proof read the same geometry the GPU
/// draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandStrip {
    pub(crate) center: [f32; 2],
    pub(crate) inner: f32,
    pub(crate) outer_vertex: f32,
    pub(crate) range_start: f32,
    pub(crate) step: f32,
    pub(crate) segments: u32,
}

impl BandStrip {
    pub(crate) fn of(record: &ShapeRecord, offset: Point, scale: f32, segments: u32) -> Self {
        let center = [
            (record.arc[0] + offset.x) * scale,
            (record.arc[1] + offset.y) * scale,
        ];
        let inner = record.arc_band[2] * scale;
        let outer = record.arc_band[3] * scale;
        let start = record.arc_normalized[0];
        let sweep = record.arc_normalized[1];
        let mid = (outer + inner) * 0.5;
        let ring_half = ((outer - inner) * 0.5).max(0.0) + BAND_MARGIN;
        let outer_padded = mid + ring_half;
        let inner_padded = (mid - ring_half).max(0.0);
        let mut range_start = 0.0;
        let mut range = TAU;
        if sweep < TAU {
            let pad = if ring_half < mid {
                (ring_half / mid).asin() + BAND_ANGULAR_PAD
            } else {
                std::f32::consts::PI
            };
            let padded = sweep + pad + pad;
            if padded < TAU {
                range_start = start - pad;
                range = padded;
            }
        }
        let step = range / segments as f32;
        Self {
            center,
            inner: inner_padded,
            outer_vertex: outer_padded / (step * 0.5).cos(),
            range_start,
            step,
            segments,
        }
    }

    /// The `segments` quads' triangles the GPU draws, six vertices each.
    fn vertex_count(&self) -> u32 {
        self.segments * QUAD_VERTICES
    }

    /// The device position of strip vertex `index`, as `vs_band` places it.
    #[cfg(test)]
    fn vertex(&self, index: u32) -> [f32; 2] {
        let segment = index / 6;
        let (boundary, radius) = match index % 6 {
            1 => (segment, self.outer_vertex),
            2 | 4 => (segment + 1, self.outer_vertex),
            5 => (segment + 1, self.inner),
            _ => (segment, self.inner),
        };
        let angle = self.range_start + self.step * boundary as f32;
        let (sin, cos) = angle.sin_cos();
        [self.center[0] + cos * radius, self.center[1] + sin * radius]
    }

    /// The device pixels the strip rasterizes: every quad is the trapezoid
    /// between the two radii over one step, so the sum needs one sine, not
    /// a walk over the vertices (the arena's 17,600 arcs are re-estimated
    /// whenever their tables change).
    pub(crate) fn area(&self) -> f64 {
        let quad = 0.5
            * (f64::from(self.outer_vertex) * f64::from(self.outer_vertex)
                - f64::from(self.inner) * f64::from(self.inner))
            * f64::from(self.step).sin().abs();
        quad * f64::from(self.segments)
    }

    /// The area summed over the strip's triangles, the slow form the
    /// analytic one must equal.
    #[cfg(test)]
    fn triangle_area_sum(&self) -> f64 {
        (0..self.vertex_count())
            .step_by(3)
            .map(|first| {
                triangle_area(
                    self.vertex(first),
                    self.vertex(first + 1),
                    self.vertex(first + 2),
                )
            })
            .sum()
    }
}

#[cfg(test)]
fn triangle_area(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f64 {
    let (ax, ay) = (f64::from(a[0]), f64::from(a[1]));
    let (bx, by) = (f64::from(b[0]), f64::from(b[1]));
    let (cx, cy) = (f64::from(c[0]), f64::from(c[1]));
    ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() * 0.5
}

/// The device pixels a record's quad rasterizes: the stored rect (the
/// disc for a scope-recorded arc), grown by the stroke's outer half for a
/// stroked rect, scaled. Canonicalization moves edges by at most a
/// sixteenth of a pixel, which the estimate ignores.
pub(crate) fn quad_area(record: &ShapeRecord, scale: f32) -> f64 {
    let rect = record.stored_rect();
    let half_stroke = if record.fragment_kind() == FRAGMENT_KIND_ARC {
        0.0
    } else {
        record.stroke().map_or(0.0, |stroke| stroke.width * 0.5)
    };
    let width = (rect.width + half_stroke + half_stroke) * scale;
    let height = (rect.height + half_stroke + half_stroke) * scale;
    f64::from(width) * f64::from(height)
}

/// The pixels a run's records rasterize, split by what the fragment stage
/// does for them: the shape kind and whether the brush is a gradient. The
/// split is what attributes a fill-bound frame to the records that cost
/// it, so the count per kind must be exact per record, not a sum.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShapeFill {
    pub(crate) pixels: [f64; ShapeFill::CLASSES],
    pub(crate) vertices: u64,
}

impl ShapeFill {
    pub(crate) const CLASSES: usize = 6;
    pub(crate) const LABELS: [&'static str; ShapeFill::CLASSES] = [
        "fill",
        "fill_grad",
        "stroke",
        "stroke_grad",
        "arc",
        "arc_grad",
    ];

    fn class(record: &ShapeRecord) -> usize {
        record.fragment_kind() as usize * 2 + usize::from(record.is_gradient())
    }

    /// Counts one record: its strip when `bands` draw it and it is banded,
    /// its quad otherwise; a degenerate arc draws nothing.
    pub(crate) fn add_record(
        &mut self,
        record: &ShapeRecord,
        offset: Point,
        scale: f32,
        bands: bool,
    ) {
        if record.is_degenerate_arc() {
            return;
        }
        let (pixels, vertices) = if bands && record.is_banded() {
            let strip = BandStrip::of(record, offset, scale, record.band_segments());
            (strip.area(), strip.vertex_count())
        } else {
            (quad_area(record, scale), QUAD_VERTICES)
        };
        self.pixels[Self::class(record)] += pixels;
        self.vertices += vertices as u64;
    }

    /// The fill of `tables` under `offset` at `scale`.
    pub(crate) fn of_tables(tables: &RecordTables, offset: Point, scale: f32, bands: bool) -> Self {
        let mut fill = Self::default();
        for record in &tables.shapes {
            fill.add_record(record, offset, scale, bands);
        }
        fill
    }

    pub(crate) fn total(&self) -> f64 {
        self.pixels.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{
        ARC_BAND_MIN_RADIUS, Brush, Color, DrawScope, DrawScopeDefault, Size, Stroke, StrokeCap,
    };

    use super::*;

    fn inside_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
        let sign = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            (p[0] - r[0]) * (q[1] - r[1]) - (q[0] - r[0]) * (p[1] - r[1])
        };
        let d1 = sign(p, a, b);
        let d2 = sign(p, b, c);
        let d3 = sign(p, c, a);
        let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(negative && positive)
    }

    fn strip_covers(strip: &BandStrip, point: [f32; 2]) -> bool {
        (0..strip.vertex_count()).step_by(3).any(|first| {
            inside_triangle(
                point,
                strip.vertex(first),
                strip.vertex(first + 1),
                strip.vertex(first + 2),
            )
        })
    }

    fn sdf_arc_band(p: [f32; 2], record: &ShapeRecord, scale: f32) -> f32 {
        let center = [record.arc[0] * scale, record.arc[1] * scale];
        let inner = record.arc_band[2] * scale;
        let outer = record.arc_band[3] * scale;
        let start = record.arc_normalized[0];
        let sweep = record.arc_normalized[1];
        let ra = (outer + inner) * 0.5;
        let rb = ((outer - inner) * 0.5).max(0.0);
        let (mid_sin, mid_cos, half_sin, half_cos) = if sweep >= TAU && start == 0.0 {
            (0.0, -1.0, 0.0, -1.0)
        } else {
            let half = sweep.clamp(0.0, TAU) * 0.5;
            let (ms, mc) = (start + half).sin_cos();
            let (hs, hc) = half.sin_cos();
            (ms, mc, hs.max(0.0), hc)
        };
        let d = [p[0] - center[0], p[1] - center[1]];
        let mut q = [
            -mid_sin * d[0] + mid_cos * d[1],
            mid_cos * d[0] + mid_sin * d[1],
        ];
        q[0] = q[0].abs();
        let mut dist = if half_cos * q[0] > half_sin * q[1] {
            let dx = q[0] - half_sin * ra;
            let dy = q[1] - half_cos * ra;
            (dx * dx + dy * dy).sqrt() - rb
        } else {
            ((q[0] * q[0] + q[1] * q[1]).sqrt() - ra).abs() - rb
        };
        let plane = half_cos * q[0] - half_sin * q[1];
        match record.band_cap() {
            StrokeCap::Butt => dist = dist.max(plane),
            StrokeCap::Square => dist = dist.max(plane - rb),
            StrokeCap::Round => {}
        }
        dist
    }

    fn shader_shades(record: &ShapeRecord, scale: f32, point: [f32; 2]) -> bool {
        let dist = sdf_arc_band(point, record, scale);
        let t = ((dist + 0.5).clamp(0.0, 1.0)).powi(2) * (3.0 - 2.0 * (dist + 0.5).clamp(0.0, 1.0));
        1.0 - t >= 0.001
    }

    fn recorded_arcs(record: impl FnOnce(&mut DrawScopeDefault)) -> Vec<ShapeRecord> {
        let mut scope = DrawScopeDefault::new(Size::new(600.0, 600.0));
        record(&mut scope);
        scope.finish().shapes().to_vec()
    }

    fn assert_strip_covers_shader(record: &ShapeRecord, scale: f32) {
        let strip = BandStrip::of(record, Point::default(), scale, record.band_segments());
        let rect = record.coverage_rect();
        let left = ((rect.x * scale).floor() as i32) - 2;
        let top = ((rect.y * scale).floor() as i32) - 2;
        let right = (((rect.x + rect.width) * scale).ceil() as i32) + 2;
        let bottom = (((rect.y + rect.height) * scale).ceil() as i32) + 2;
        let mut shaded = 0usize;
        for y in top..bottom {
            for x in left..right {
                let point = [x as f32 + 0.5, y as f32 + 0.5];
                if shader_shades(record, scale, point) {
                    shaded += 1;
                    assert!(
                        strip_covers(&strip, point),
                        "pixel {point:?} is shaded by the arc SDF but outside the strip of {record:?}"
                    );
                }
            }
        }
        assert!(shaded > 0, "the arc must shade something: {record:?}");
        let disc = quad_area(record, scale);
        assert!(
            strip.area() < disc,
            "the strip must cost less than the disc: {} vs {disc}",
            strip.area()
        );
    }

    #[test]
    fn every_pixel_the_arc_shader_shades_lies_inside_its_strip() {
        let brush = Brush::solid(Color::WHITE);
        let records = recorded_arcs(|scope| {
            let center = Point::new(300.0, 300.0);
            scope.draw_arc(brush.clone(), center, 20.0, 0.0, TAU, Stroke::new(4.0));
            scope.draw_arc(brush.clone(), center, 90.0, 0.3, 1.2, Stroke::new(6.0));
            scope.draw_arc(brush.clone(), center, 200.0, 4.0, 2.5, Stroke::new(12.0));
            scope.draw_arc(brush.clone(), center, 250.0, 5.5, 2.0, Stroke::new(3.0));
            scope.draw_annular_sector(brush.clone(), center, 100.0, 140.0, 1.0, 0.4);
            scope.draw_annular_sector(brush.clone(), center, 12.0, 30.0, 2.0, 3.0);
            scope.draw_arc(
                brush,
                Point::new(100.0, 100.0),
                ARC_BAND_MIN_RADIUS,
                0.0,
                TAU,
                Stroke::new(2.0),
            );
        });
        let banded: Vec<bool> = records.iter().map(ShapeRecord::is_banded).collect();
        assert_eq!(
            banded,
            [true, true, true, true, true, true, false],
            "the ring at the smallest band radius costs more as a strip than as \
             its quad once its vertices are charged"
        );
        for record in records.iter().filter(|record| record.is_banded()) {
            for scale in [1.0, 2.75] {
                assert_strip_covers_shader(record, scale);
            }
        }
    }

    #[test]
    fn the_analytic_strip_area_equals_its_triangles() {
        let records = recorded_arcs(|scope| {
            let center = Point::new(300.0, 300.0);
            let brush = Brush::solid(Color::WHITE);
            scope.draw_arc(brush.clone(), center, 20.0, 0.0, TAU, Stroke::new(4.0));
            scope.draw_arc(brush.clone(), center, 90.0, 0.3, 1.2, Stroke::new(6.0));
            scope.draw_annular_sector(brush, center, 100.0, 140.0, 1.0, 0.4);
        });
        for record in &records {
            let strip = BandStrip::of(record, Point::new(3.0, 7.0), 1.5, record.band_segments());
            let analytic = strip.area();
            let summed = strip.triangle_area_sum();
            assert!(
                (analytic - summed).abs() <= summed * 1e-4,
                "{analytic} vs {summed} for {record:?}"
            );
        }
    }

    #[test]
    fn the_fill_estimate_counts_strips_for_bands_and_quads_for_the_rest() {
        let records = recorded_arcs(|scope| {
            scope.draw_arc(
                Brush::solid(Color::WHITE),
                Point::new(300.0, 300.0),
                200.0,
                0.0,
                TAU,
                Stroke::new(4.0),
            );
            scope.draw_rect_at(
                cranpose_ui_graphics::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 20.0,
                },
                Brush::solid(Color::WHITE),
            );
        });
        let mut tables = RecordTables::default();
        tables.shapes = records.clone();
        let banded = ShapeFill::of_tables(&tables, Point::default(), 1.0, true);
        let quads = ShapeFill::of_tables(&tables, Point::default(), 1.0, false);
        assert_eq!(banded.pixels[0], 200.0);
        assert_eq!(quads.pixels[0], 200.0);
        assert!(banded.pixels[4] < 2.0 * 6284.0 * 8.0);
        assert!(quads.pixels[4] > 150_000.0);
        assert_eq!(banded.total(), banded.pixels[0] + banded.pixels[4]);
    }
}
