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
#[path = "tests/shape_records_tests.rs"]
mod tests;
