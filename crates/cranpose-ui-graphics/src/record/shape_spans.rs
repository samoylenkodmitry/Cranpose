//! The shapes of a range of a recorder's segments: how many there are,
//! where they draw, what they hash to and the primitives they came from, for
//! a reader of one run of a recorder that many runs share.

use std::{hash::Hasher, ops::Range, slice};

use super::{
    NO_EXPLICIT_STOPS, NO_SEGMENT_KEY, RecordLane, RecordSegment, ShapeRecorder,
    materialize::materialize_shape, record_coverage,
};
use crate::{DrawPrimitive, Rect};

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl ShapeRecorder {
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

    /// The rect the pixels of the shapes in `segments` lie within; `None`
    /// when the segments hold no shape.
    pub fn coverage_in(&self, segments: &Range<u32>) -> Option<Rect> {
        let bodies = self.tables.shapes.bodies();
        self.shapes_in(segments)
            .map(|index| record_coverage(&bodies[index]))
            .reduce(|covered, coverage| covered.union(coverage))
    }

    /// Writes what the shapes in `segments` draw into `state`: their
    /// records and the gradients they paint with.
    pub fn hash_in(&self, segments: &Range<u32>, state: &mut impl Hasher) {
        let tables = &self.tables;
        for index in self.shapes_in(segments) {
            let body = &tables.shapes.bodies()[index];
            state.write(bytemuck::bytes_of(body));
            state.write(bytemuck::bytes_of(&tables.shapes.curves()[index]));
            let Some(brush) = (body.brush as usize)
                .checked_sub(1)
                .map(|brush| &tables.brushes()[brush])
            else {
                continue;
            };
            state.write(bytemuck::bytes_of(brush));
            let stops = brush.stop_start as usize..(brush.stop_start + brush.stop_count) as usize;
            state.write(bytemuck::cast_slice(&tables.stops()[stops]));
            if brush.explicit_len != NO_EXPLICIT_STOPS {
                let explicit = brush.explicit_start as usize
                    ..(brush.explicit_start + brush.explicit_len) as usize;
                state.write(bytemuck::cast_slice(&tables.explicit_stops()[explicit]));
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
