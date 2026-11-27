//! The shapes of a range of a recorder's segments: how many there are,
//! where they draw and the primitives they came from, for a reader of one
//! run of a recorder that many runs share.

use std::{ops::Range, slice};

use super::{
    NO_SEGMENT_KEY, RecordLane, RecordSegment, RecordTables, ShapeRecorder,
    materialize::materialize_shape, record_coverage,
};
use crate::{DrawPrimitive, Rect};

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl ShapeRecorder {
    /// An empty recorder with room for `shapes` shapes in `segments`
    /// segments, so that shapes that fit grow none of its tables.
    pub fn with_capacity(shapes: usize, segments: usize) -> Self {
        Self::with_tables(RecordTables::with_capacity(shapes, segments))
    }

    // An empty recorder over `tables`.
    pub(super) fn with_tables(tables: RecordTables) -> Self {
        Self {
            tables,
            gradient_lists: crate::rare_list::RareList::default(),
            last_segment_key: NO_SEGMENT_KEY,
            segment_waste: 0,
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
        }
    }

    /// Starts a segment of its own for the next shape, so that a run can
    /// name the shapes recorded after this call apart from those before.
    pub fn end_segment(&mut self) {
        self.last_segment_key = NO_SEGMENT_KEY;
        self.segment_waste = 0;
    }

    /// How many shapes the recorder holds before its storage grows.
    pub fn shape_capacity(&self) -> usize {
        self.tables.shapes.capacity()
    }

    /// Drops the segments from `len` on with their shapes, the shapes
    /// recorded last. The gradient tables stay whole while a shape left
    /// paints with a brush.
    pub fn truncate_segments(&mut self, len: u32) {
        self.tables.truncate_segments(len);
        self.forget_unused_gradient_lists();
    }

    /// Gives back the room growth left in the recorder's tables. The shapes
    /// stay.
    pub fn trim_growth(&mut self) {
        self.tables.trim_growth();
    }

    /// Keeps the segments `keep` takes, by index, with their shapes, moved
    /// to the front in order, and drops every other segment and shape. The
    /// gradient tables stay whole while a kept shape paints with a brush.
    pub fn keep_segments(&mut self, keep: impl Fn(u32) -> bool) {
        self.tables.keep_segments(keep);
        self.forget_unused_gradient_lists();
    }

    // Lets go of the gradient lists once the tables dropped their brushes,
    // and starts a segment of its own for the next shape. The bounds stay:
    // they still hold every shape left.
    fn forget_unused_gradient_lists(&mut self) {
        if self.tables.brushes().is_empty() {
            self.gradient_lists.clear();
        }
        self.end_segment();
    }

    /// Iterates over the segments in the index range `segments`.
    pub fn segments_in(&self, segments: &Range<u32>) -> slice::Iter<'_, RecordSegment> {
        self.tables.segments[segments.start as usize..segments.end as usize].iter()
    }

    // The indexes of the shape records the segments `segments` hold.
    fn shapes_in(&self, segments: &Range<u32>) -> impl Iterator<Item = usize> + '_ {
        self.segments_in(segments)
            .filter(|segment| segment.lane == RecordLane::Shapes)
            .flat_map(RecordSegment::range)
    }

    /// How many shapes the segments `segments` hold.
    pub fn shape_count_in(&self, segments: &Range<u32>) -> u32 {
        self.segments_in(segments)
            .filter(|segment| segment.lane == RecordLane::Shapes)
            .map(|segment| segment.count)
            .sum()
    }

    /// The rect the pixels of the shapes from the shape at index `first` on
    /// lie within; `None` when the recorder holds no shape past `first`.
    pub fn coverage_from(&self, first: usize) -> Option<Rect> {
        self.tables.shapes.bodies()[first..]
            .iter()
            .map(record_coverage)
            .reduce(|covered, coverage| covered.union(coverage))
    }

    /// Records the shapes in `segments` of `source` again after this
    /// recorder's shapes, from the primitives they were recorded from.
    pub fn push_shapes_of(&mut self, source: &Self, segments: &Range<u32>) {
        for segment in source.segments_in(segments) {
            if segment.lane != RecordLane::Shapes {
                continue;
            }
            for index in segment.range() {
                self.push_primitive(materialize_shape(source, index));
            }
        }
    }

    /// The primitive each shape in `segments` was recorded from, in order.
    pub fn shape_primitives_in(
        &self,
        segments: &Range<u32>,
    ) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.shapes_in(segments)
            .map(|index| materialize_shape(self, index))
    }
}
