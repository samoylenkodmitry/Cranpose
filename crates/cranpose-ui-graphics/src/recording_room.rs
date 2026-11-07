//! The room a recording's buffers had, for a recording of the same command
//! to start with, so that it grows none of them while it records as much
//! again.

use std::sync::Arc;

use crate::record::{CommandRecording, RecordTables, ShapeRecorder};

/// The room a recording's buffers had: a recording that starts with it
/// allocates each buffer once while it records as much again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordingRoom {
    shapes: [usize; 3],
    brushes: usize,
    stops: usize,
    explicit_stops: usize,
    segments: usize,
    gradient_lists: usize,
    others: usize,
    groups: usize,
}

impl RecordingRoom {
    /// The room the buffers of `recording` have.
    pub fn of(recording: &CommandRecording) -> Self {
        let tables = &recording.shapes.tables;
        Self {
            shapes: tables.shapes.capacities(),
            brushes: tables.brushes.capacity(),
            stops: tables.stops.capacity(),
            explicit_stops: tables.explicit_stops.capacity(),
            segments: tables.segments.capacity(),
            gradient_lists: recording.shapes.gradient_lists.capacity(),
            others: recording.content.others.capacity(),
            groups: recording.content.groups.capacity(),
        }
    }

    /// An empty recording whose buffers have this room. One with no room
    /// for shapes shares the empty shape columns, as a new recording does.
    pub fn empty_recording(self) -> CommandRecording {
        let room = self;
        let mut recording = CommandRecording::default();
        recording.content.others.reserve_exact(room.others);
        recording.content.groups.reserve_exact(room.groups);
        if room.shapes == [0; 3] {
            return recording;
        }
        let mut tables = RecordTables::default();
        tables.shapes = crate::shape_records::ShapeRecords::with_capacities(room.shapes);
        tables.brushes.reserve_exact(room.brushes);
        tables.stops.reserve_exact(room.stops);
        tables.explicit_stops.reserve_exact(room.explicit_stops);
        tables.segments.reserve_exact(room.segments);
        let mut shapes = ShapeRecorder::default();
        shapes.tables = tables;
        shapes.gradient_lists.reserve_exact(room.gradient_lists);
        recording.shapes = Arc::new(shapes);
        recording
    }
}
