use super::{
    ARC_BUCKET_SEGMENTS, ARC_BUCKETS, ARC_RING_RADII, ARC_RING_SEGMENTS_PER_RADIAN, BAND_MARGIN,
    BAND_MIN_SEGMENTS, BAND_STRIP_OVERSHOOT, BAND_VERTEX_PIXELS, band_padded_range, strip_vertices,
};
use crate::{ArcGeometry, float::at_least};

/// The ring a band's strip is built around, in the command's units: what
/// `band_position` derives from a record, derived here from the geometry the
/// record is made of or from the record itself, with the padded sweep
/// the strip covers computed once.
pub(super) struct BandRing {
    pub(super) mid: f32,
    pub(super) ring_half: f32,
    pub(super) range: f32,
    pub(super) segments_per_radian: f32,
}

impl BandRing {
    pub(super) fn of_geometry(geometry: &ArcGeometry) -> Self {
        Self::new(
            geometry.inner_radius,
            geometry.outer_radius,
            geometry.start_angle,
            geometry.sweep_angle,
        )
    }

    pub(super) fn new(inner: f32, outer: f32, start: f32, sweep: f32) -> Self {
        let mid = (outer + inner) * 0.5;
        let ring_half = at_least((outer - inner) * 0.5, 0.0) + BAND_MARGIN;
        let (_, range) = band_padded_range(mid, ring_half, start, sweep);
        Self {
            mid,
            ring_half,
            range,
            segments_per_radian: Self::segments_per_radian(outer),
        }
    }

    /// The segments per radian a full ring takes at `outer_radius`: the
    /// angular step every band at that radius keeps.
    #[inline]
    fn segments_per_radian(outer_radius: f32) -> f32 {
        let bucket = usize::from(outer_radius > ARC_RING_RADII[0])
            + usize::from(outer_radius > ARC_RING_RADII[1])
            + usize::from(outer_radius > ARC_RING_RADII[2]);
        ARC_RING_SEGMENTS_PER_RADIAN[bucket]
    }

    /// The fewest of [`ARC_BUCKET_SEGMENTS`] whose step over the padded
    /// sweep stays within the ring step at this radius.
    #[inline]
    pub(super) fn segments(&self) -> u32 {
        let exact = self.range * self.segments_per_radian;
        if exact <= BAND_MIN_SEGMENTS as f32 {
            return BAND_MIN_SEGMENTS;
        }
        let floor = exact as u32;
        let needed = if (floor as f32) < exact {
            floor + 1
        } else {
            floor
        };
        needed
            .max(BAND_MIN_SEGMENTS)
            .next_power_of_two()
            .min(ARC_BUCKET_SEGMENTS[ARC_BUCKETS - 1])
    }

    /// The strip's cost in pixels: the padded ring's area over the padded
    /// sweep, plus what its `segments` quads' vertices cost.
    pub(super) fn strip_pixels(&self, segments: u32) -> f32 {
        self.range * self.mid * (self.ring_half + self.ring_half) * BAND_STRIP_OVERSHOOT
            + vertex_pixels(strip_vertices(segments))
    }
}

pub(super) fn vertex_pixels(vertices: u32) -> f32 {
    vertices as f32 * BAND_VERTEX_PIXELS
}
