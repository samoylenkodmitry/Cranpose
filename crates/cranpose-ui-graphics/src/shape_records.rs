use bytemuck::{Pod, Zeroable};

use crate::ShapeRecord;

/// Shape properties independent of an arc's angles, in instance-buffer layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ShapeRecordBody {
    /// The stored local rectangle.
    pub rect: [f32; 4],
    /// The solid colour or first gradient stop.
    pub color: [f32; 4],
    /// The stroke width.
    pub stroke_width: f32,
    /// Packed shape, stroke, blend and arc facts from [`ShapeRecord::flags`].
    pub flags: u32,
    /// Zero for a solid brush, otherwise one plus its table index.
    pub brush: u32,
    /// The placement index when a renderer combines recordings.
    pub placement: u32,
    /// Arc centre x and y, followed by normalised inner and outer radii.
    pub arc_geometry: [f32; 4],
}

/// Corner radii and arc-angle properties, in instance-buffer layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ShapeRecordCurve {
    /// Corner radii for rectangles, arc trigonometry for bands.
    pub radii: [f32; 4],
    /// Normalised arc start and sweep, then strip start and padded sweep.
    pub arc_normalized: [f32; 4],
}

/// Recorded shapes stored in parallel columns, ready for GPU upload.
///
/// Angle changes leave the body column intact. Original arc arguments remain
/// on the CPU so materialisation preserves exactly what the caller supplied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeRecords {
    bodies: Vec<ShapeRecordBody>,
    curves: Vec<ShapeRecordCurve>,
    sources: Vec<[f32; 4]>,
}

impl ShapeRecords {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            bodies: Vec::with_capacity(capacity),
            curves: Vec::with_capacity(capacity),
            sources: Vec::with_capacity(capacity),
        }
    }

    /// The number of recorded shapes.
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether the recording has no shapes.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// The angle-independent column, directly usable as GPU instance data.
    pub fn bodies(&self) -> &[ShapeRecordBody] {
        &self.bodies
    }

    /// The radius and angle column, directly usable as GPU instance data.
    pub fn curves(&self) -> &[ShapeRecordCurve] {
        &self.curves
    }

    /// Reconstructs one complete record, including the original arc arguments.
    pub fn get(&self, index: usize) -> Option<ShapeRecord> {
        self.bodies
            .get(index)
            .map(|body| reconstruct(body, &self.curves[index], self.sources[index]))
    }

    /// Iterates over complete records in draw order without allocating.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = ShapeRecord> + DoubleEndedIterator + '_ {
        self.bodies
            .iter()
            .zip(&self.curves)
            .zip(&self.sources)
            .map(|((body, curve), &source)| reconstruct(body, curve, source))
    }

    pub(crate) fn capacity(&self) -> usize {
        self.bodies
            .capacity()
            .min(self.curves.capacity())
            .min(self.sources.capacity())
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.bodies.capacity() * std::mem::size_of::<ShapeRecordBody>()
            + self.curves.capacity() * std::mem::size_of::<ShapeRecordCurve>()
            + self.sources.capacity() * std::mem::size_of::<[f32; 4]>()
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.sources)
    }

    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
        self.curves.clear();
        self.sources.clear();
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.bodies.reserve(additional);
        self.curves.reserve(additional);
        self.sources.reserve(additional);
    }

    pub(crate) fn push(
        &mut self,
        body: ShapeRecordBody,
        curve: ShapeRecordCurve,
        source: [f32; 4],
    ) {
        self.bodies.push(body);
        self.curves.push(curve);
        self.sources.push(source);
    }
}

fn reconstruct(body: &ShapeRecordBody, curve: &ShapeRecordCurve, source: [f32; 4]) -> ShapeRecord {
    ShapeRecord {
        rect: body.rect,
        radii: curve.radii,
        color: body.color,
        stroke_width: body.stroke_width,
        flags: body.flags,
        brush: body.brush,
        reserved: body.placement,
        arc: [
            body.arc_geometry[0],
            body.arc_geometry[1],
            source[0],
            source[1],
        ],
        arc_band: [
            source[2],
            source[3],
            body.arc_geometry[2],
            body.arc_geometry[3],
        ],
        arc_normalized: curve.arc_normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ShapeRecord {
        ShapeRecord {
            rect: [1.0, 2.0, 3.0, 4.0],
            radii: [5.0, 6.0, 7.0, 8.0],
            color: [0.2, 0.4, 0.6, 0.8],
            stroke_width: 9.0,
            flags: 10,
            brush: 11,
            reserved: 12,
            arc: [13.0, 14.0, 15.0, 16.0],
            arc_band: [17.0, 18.0, 19.0, 20.0],
            arc_normalized: [21.0, 22.0, 23.0, 24.0],
        }
    }

    fn append_sample(records: &mut ShapeRecords) {
        records.push(
            ShapeRecordBody {
                rect: [1.0, 2.0, 3.0, 4.0],
                color: [0.2, 0.4, 0.6, 0.8],
                stroke_width: 9.0,
                flags: 10,
                brush: 11,
                placement: 12,
                arc_geometry: [13.0, 14.0, 19.0, 20.0],
            },
            ShapeRecordCurve {
                radii: [5.0, 6.0, 7.0, 8.0],
                arc_normalized: [21.0, 22.0, 23.0, 24.0],
            },
            [15.0, 16.0, 17.0, 18.0],
        );
    }

    #[test]
    fn columns_preserve_every_record_bit_and_gpu_field() {
        let record = sample();
        let mut special = record;
        special.arc = [
            f32::NEG_INFINITY,
            -0.0,
            f32::from_bits(0x7fc0_0021),
            f32::INFINITY,
        ];
        special.arc_band = [-0.0, f32::from_bits(0xffc0_0001), 1.0, 2.0];
        let mut records = ShapeRecords::default();
        assert!(records.is_empty());
        assert_eq!(records.get(0), None);
        append_sample(&mut records);
        let mut body = records.bodies()[0];
        body.arc_geometry = [f32::NEG_INFINITY, -0.0, 1.0, 2.0];
        records.push(
            body,
            records.curves()[0],
            [
                special.arc[2],
                special.arc[3],
                special.arc_band[0],
                special.arc_band[1],
            ],
        );
        assert_eq!(records.len(), 2);
        assert_eq!(records.iter().len(), 2);
        for (actual, expected) in records.iter().zip([record, special]) {
            assert_eq!(bytemuck::bytes_of(&actual), bytemuck::bytes_of(&expected));
        }
        assert_eq!(records.get(0), Some(record));
        assert_eq!(records.get(2), None);
        assert_eq!(records.iter().rev().nth(1), Some(record));
        assert_eq!(records.bodies()[0].arc_geometry, [13.0, 14.0, 19.0, 20.0]);
        assert_eq!(records.curves()[0].radii, record.radii);
        assert_eq!(records.curves()[0].arc_normalized, record.arc_normalized);
        assert_eq!(std::mem::size_of::<ShapeRecordBody>(), 64);
        assert_eq!(std::mem::size_of::<ShapeRecordCurve>(), 32);
        assert_eq!(
            &records.source_bytes()[..16],
            bytemuck::bytes_of(&[15.0f32, 16.0, 17.0, 18.0])
        );
    }

    #[test]
    fn clearing_and_reserving_keep_columns_aligned_without_reallocating() {
        let mut records = ShapeRecords::with_capacity(4);
        append_sample(&mut records);
        let capacity = records.capacity();
        let bytes = records.heap_bytes();
        let bodies = records.bodies().as_ptr();
        let curves = records.curves().as_ptr();
        records.clear();
        records.reserve(2);
        append_sample(&mut records);
        assert_eq!(records.capacity(), capacity);
        assert_eq!(records.heap_bytes(), bytes);
        assert_eq!(records.bodies().as_ptr(), bodies);
        assert_eq!(records.curves().as_ptr(), curves);
        assert_eq!(records.len(), records.curves().len());
        assert_eq!(records.get(0), Some(sample()));
        assert_eq!(records.clone(), records);
    }
}
