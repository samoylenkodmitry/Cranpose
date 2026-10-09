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
    /// Corner radii for rectangles; zero for bands, whose trig the GPU
    /// derives from `arc_normalized` once per uploaded band and writes here.
    /// No other record's row equals the row of a band that draws: such a
    /// band's sweep, `arc_normalized[1]`, is positive and its radii and
    /// `arc_normalized[2..]` zero, while a rect's and an unbanded round
    /// rect's `arc_normalized` is zero, a banded ring's radii are positive,
    /// a line of positive length carries its half length in
    /// `arc_normalized[2]` and one of no length points along +X. A record
    /// that becomes or stops being a band therefore uploads a row of its
    /// own, and a band whose row is unchanged keeps its trig.
    pub radii: [f32; 4],
    /// A band's normalised start and sweep, the rest zero; a line's unit
    /// direction and half length.
    pub arc_normalized: [f32; 4],
}

/// A record's original line or arc arguments, kept on the CPU only for the
/// records that have any.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct RecordSource {
    index: u32,
    arguments: [f32; 4],
}

/// Recorded shapes stored in parallel columns, ready for GPU upload.
///
/// Angle changes leave the body column intact. Original line and arc
/// arguments remain on the CPU so materialisation preserves exactly what
/// the caller supplied; the rects, round rects and path slices that make up
/// most recordings have none, and keep no column for them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeRecords {
    bodies: Vec<ShapeRecordBody>,
    curves: Vec<ShapeRecordCurve>,
    /// By record index.
    sources: Vec<RecordSource>,
}

impl ShapeRecords {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            bodies: Vec::with_capacity(capacity),
            curves: Vec::with_capacity(capacity),
            sources: Vec::new(),
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

    /// Whether the record at `index` is an opaque fill whose interior is worth
    /// laying down ahead of later paint: the test that marks a segment's
    /// [`occluders`](crate::RecordSegment::occluders). `false` past the end.
    pub fn occludes(&self, index: usize) -> bool {
        self.bodies
            .get(index)
            .zip(self.curves.get(index))
            .is_some_and(|(body, curve)| crate::record::interior_occludes(body, curve))
    }

    /// Reconstructs one complete record, including the original arc arguments.
    pub fn get(&self, index: usize) -> Option<ShapeRecord> {
        self.bodies
            .get(index)
            .map(|body| reconstruct(body, &self.curves[index], self.source(index)))
    }

    /// Iterates over complete records in draw order without allocating.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = ShapeRecord> + DoubleEndedIterator + '_ {
        self.bodies
            .iter()
            .zip(&self.curves)
            .enumerate()
            .map(|(index, (body, curve))| reconstruct(body, curve, self.source(index)))
    }

    /// The original arguments of record `index`: zeros for a record kept
    /// without any.
    fn source(&self, index: usize) -> [f32; 4] {
        let Ok(index) = u32::try_from(index) else {
            return [0.0; 4];
        };
        self.sources
            .binary_search_by_key(&index, |source| source.index)
            .map_or([0.0; 4], |found| self.sources[found].arguments)
    }

    pub(crate) fn capacity(&self) -> usize {
        self.bodies.capacity().min(self.curves.capacity())
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.bodies.capacity() * std::mem::size_of::<ShapeRecordBody>()
            + self.curves.capacity() * std::mem::size_of::<ShapeRecordCurve>()
            + self.sources.capacity() * std::mem::size_of::<RecordSource>()
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.sources)
    }

    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
        self.curves.clear();
        self.sources.clear();
    }

    pub(crate) fn trim_growth(&mut self) {
        trim_growth(&mut self.bodies);
        trim_growth(&mut self.curves);
        trim_growth(&mut self.sources);
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.bodies.reserve(additional);
        self.curves.reserve(additional);
    }

    pub(crate) fn push(
        &mut self,
        body: ShapeRecordBody,
        curve: ShapeRecordCurve,
        source: [f32; 4],
    ) {
        if source.iter().any(|argument| argument.to_bits() != 0)
            && let Ok(index) = u32::try_from(self.bodies.len())
        {
            self.sources.push(RecordSource {
                index,
                arguments: source,
            });
        }
        push_sized(&mut self.bodies, body);
        push_sized(&mut self.curves, curve);
    }
}

/// Gives back the room past an eighth more than `items` holds once growth
/// left over a quarter more: a recording drawn again keeps its capacity,
/// so room its first growth left would stay for as long as its node.
pub(crate) fn trim_growth<T>(items: &mut Vec<T>) {
    let len = items.len();
    if items.capacity() > len + len / 4 {
        items.shrink_to(len + len / 8);
    }
}

/// Pushes `item`, giving a vector that has no room yet exactly one slot:
/// most recordings hold one shape and one segment, where the first growth
/// would make room for four. A second item grows the vector as usual.
pub(crate) fn push_sized<T>(items: &mut Vec<T>, item: T) {
    if items.capacity() == 0 {
        items.reserve_exact(1);
    }
    items.push(item);
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
