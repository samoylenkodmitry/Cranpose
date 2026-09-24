use cranpose_ui_graphics::{
    BAND_MARGIN, BAND_QUAD_MARGIN, FRAGMENT_KIND_ARC, Point, QUAD_VERTICES, RecordTables,
    ShapeRecord, StrokeCap, strip_vertices,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandStrip {
    pub(crate) center: [f32; 2],
    pub(crate) inner: f32,
    pub(crate) outer_vertex: f32,
    pub(crate) range_start: f32,
    pub(crate) step: f32,
    pub(crate) segments: u32,
    quad: Option<[[f32; 2]; 4]>,
}

impl BandStrip {
    pub(crate) fn of(record: &ShapeRecord, offset: Point, scale: f32, segments: u32) -> Self {
        let center = [
            (record.arc[0] + offset.x) * scale,
            (record.arc[1] + offset.y) * scale,
        ];
        let inner = record.arc_band[2] * scale;
        let outer = record.arc_band[3] * scale;
        let mid = (outer + inner) * 0.5;
        let margin = if segments == 1 {
            BAND_QUAD_MARGIN
        } else {
            BAND_MARGIN
        };
        let ring_half = ((outer - inner) * 0.5).max(0.0) + margin;
        let outer_padded = mid + ring_half;
        let inner_padded = (mid - ring_half).max(0.0);
        let range_start = record.arc_normalized[2];
        let range = record.arc_normalized[3];
        let step = range / segments as f32;
        let quad = (segments == 1).then(|| {
            let [sin_mid, cos_mid, sin_half, cos_half] = record.radii;
            let half_width = mid * sin_half + ring_half;
            let half_width = if record.band_cap() == StrokeCap::Butt {
                half_width.min((mid + ring_half) * sin_half + margin * cos_half)
            } else {
                half_width
            };
            std::array::from_fn(|index| {
                let x = if index / 2 == 1 {
                    half_width
                } else {
                    -half_width
                };
                let y = if index % 2 == 1 {
                    mid + ring_half
                } else {
                    mid * cos_half - ring_half
                };
                [
                    center[0] + (-sin_mid * x + cos_mid * y),
                    center[1] + (cos_mid * x + sin_mid * y),
                ]
            })
        });
        Self {
            center,
            inner: inner_padded,
            outer_vertex: if quad.is_some() {
                0.0
            } else {
                outer_padded / (step * 0.5).cos()
            },
            range_start,
            step,
            segments,
            quad,
        }
    }

    /// The device position of strip vertex `index`, as `band_position`
    /// places it: boundary `index / 2`, outer when odd.
    #[cfg(test)]
    fn vertex(&self, index: u32) -> [f32; 2] {
        if let Some(quad) = self.quad {
            return quad[index as usize];
        }
        let radius = if index % 2 == 1 {
            self.outer_vertex
        } else {
            self.inner
        };
        let angle = self.range_start + self.step * (index / 2) as f32;
        let (sin, cos) = angle.sin_cos();
        [self.center[0] + cos * radius, self.center[1] + sin * radius]
    }

    pub(crate) fn area(&self) -> f64 {
        if let Some([a, b, c, d]) = self.quad {
            return triangle_area(a, b, c) + triangle_area(c, b, d);
        }
        let quad = 0.5
            * (f64::from(self.outer_vertex) * f64::from(self.outer_vertex)
                - f64::from(self.inner) * f64::from(self.inner))
            * f64::from(self.step).sin().abs();
        quad * f64::from(self.segments)
    }

    /// The triangles the strip's index pattern draws, in device space.
    #[cfg(test)]
    fn triangles(&self) -> Vec<[[f32; 2]; 3]> {
        let indices: Vec<u32> = cranpose_ui_graphics::strip_index_pattern(self.segments).collect();
        indices
            .chunks(3)
            .map(|triangle| {
                [
                    self.vertex(triangle[0]),
                    self.vertex(triangle[1]),
                    self.vertex(triangle[2]),
                ]
            })
            .collect()
    }

    /// The area summed over the triangles the strip's index pattern
    /// draws, the slow form the analytic one must equal.
    #[cfg(test)]
    fn triangle_area_sum(&self) -> f64 {
        self.triangles()
            .into_iter()
            .map(|[a, b, c]| triangle_area(a, b, c))
            .sum()
    }
}

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

    /// Counts one record drawn in a segment of `class_segments` strip
    /// segments, or as a quad on the uniform floor: the pixels of its
    /// strip when it is banded and bands draw, of its quad otherwise, and
    /// the vertices the draw invokes for it, the segment's stride, which
    /// its own strip may leave pinned. A degenerate arc draws nothing.
    pub(crate) fn add_record(
        &mut self,
        record: &ShapeRecord,
        offset: Point,
        scale: f32,
        class_segments: Option<u32>,
    ) {
        if record.is_degenerate_arc() {
            return;
        }
        let pixels = if class_segments.is_some() && record.is_banded() {
            BandStrip::of(record, offset, scale, record.band_segments()).area()
        } else {
            quad_area(record, scale)
        };
        self.pixels[Self::class(record)] += pixels;
        self.vertices += u64::from(class_segments.map_or(QUAD_VERTICES, strip_vertices));
    }

    pub(crate) fn of_draws(
        tables: &RecordTables,
        offset: Point,
        scale: f32,
        draws: impl Iterator<Item = (std::ops::Range<u32>, Option<u32>)>,
    ) -> Self {
        let mut fill = Self::default();
        for (records, class_segments) in draws {
            for record in tables
                .shapes
                .iter()
                .skip(records.start as usize)
                .take(records.len())
            {
                fill.add_record(&record, offset, scale, class_segments);
            }
        }
        fill
    }

    pub(crate) fn total(&self) -> f64 {
        self.pixels.iter().sum()
    }
}

#[cfg(test)]
#[path = "tests/run_geometry_tests.rs"]
mod tests;
