use std::{
    hash::{Hash, Hasher},
    sync::OnceLock,
};

use super::{BrushRecord, GradientStopRecord, RecordSegment};
use crate::{FxHasher, ShapeRecords};

/// The POD half of a recording: what the GPU reads, shared with the
/// renderer behind an `Arc` so a packet carries it without a copy.
#[derive(Clone, Debug, Default)]
pub struct RecordTables {
    /// The shape records, stored in columns.
    pub shapes: ShapeRecords,
    /// Runs of consecutive entries that one draw can take, in recorded order.
    pub segments: Vec<RecordSegment>,
    // The brush, stop and explicit stop tables, `None` until a shape paints
    // with a gradient: most recordings paint none.
    gradients: Option<Box<GradientTables>>,
    fingerprint: OnceLock<u64>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct GradientTables {
    pub(super) brushes: Vec<BrushRecord>,
    pub(super) stops: Vec<GradientStopRecord>,
    pub(super) explicit_stops: Vec<f32>,
}

impl GradientTables {
    fn with_capacity_of(&self) -> Self {
        Self {
            brushes: Vec::with_capacity(self.brushes.capacity()),
            stops: Vec::with_capacity(self.stops.capacity()),
            explicit_stops: Vec::with_capacity(self.explicit_stops.capacity()),
        }
    }
}

impl PartialEq for RecordTables {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes
            && self.brushes() == other.brushes()
            && self.stops() == other.stops()
            && self.explicit_stops() == other.explicit_stops()
            && self.segments == other.segments
    }
}

impl RecordTables {
    /// Gradient brushes, which shape records refer to by index.
    pub fn brushes(&self) -> &[BrushRecord] {
        self.gradients
            .as_deref()
            .map_or(&[], |tables| &tables.brushes)
    }

    /// Gradient stops of all brushes, which brush records refer to by index.
    pub fn stops(&self) -> &[GradientStopRecord] {
        self.gradients
            .as_deref()
            .map_or(&[], |tables| &tables.stops)
    }

    /// Stop positions the app gave explicitly, which brush records refer to
    /// by index.
    pub fn explicit_stops(&self) -> &[f32] {
        self.gradients
            .as_deref()
            .map_or(&[], |tables| &tables.explicit_stops)
    }

    // The gradient tables, made on the first gradient a shape paints with.
    pub(super) fn gradients_mut(&mut self) -> &mut GradientTables {
        self.gradients.get_or_insert_default()
    }

    // Forgets the fingerprint before the tables change.
    pub(super) fn forget_fingerprint(&mut self) {
        self.fingerprint.take();
    }

    pub(super) fn clear(&mut self) {
        self.shapes.clear();
        if let Some(tables) = self.gradients.as_deref_mut() {
            tables.brushes.clear();
            tables.stops.clear();
            tables.explicit_stops.clear();
        }
        self.segments.clear();
        self.fingerprint.take();
    }

    /// Gives back the room growth left in every table; see
    /// [`crate::shape_records::trim_growth`]. The content, and so the
    /// fingerprint, stay.
    pub(super) fn trim_growth(&mut self) {
        use crate::shape_records::trim_growth;
        self.shapes.trim_growth();
        if let Some(tables) = self.gradients.as_deref_mut() {
            trim_growth(&mut tables.brushes);
            trim_growth(&mut tables.stops);
            trim_growth(&mut tables.explicit_stops);
        }
        trim_growth(&mut self.segments);
    }

    /// Empty tables with this one's capacities, so a recording that starts
    /// while a scene still holds the last one grows nothing.
    pub(super) fn with_capacity_of(&self) -> Self {
        Self {
            shapes: ShapeRecords::with_capacity(self.shapes.capacity()),
            segments: Vec::with_capacity(self.segments.capacity()),
            gradients: self
                .gradients
                .as_deref()
                .map(|tables| Box::new(tables.with_capacity_of())),
            fingerprint: OnceLock::new(),
        }
    }

    /// A hash of everything the GPU reads and of the segments, computed
    /// the first time a cache asks and kept while the tables stand.
    pub fn fingerprint(&self) -> u64 {
        *self.fingerprint.get_or_init(|| {
            let mut hasher = FxHasher::default();
            hasher.write(bytemuck::cast_slice(self.shapes.bodies()));
            hasher.write(bytemuck::cast_slice(self.shapes.curves()));
            hasher.write(self.shapes.source_bytes());
            hasher.write(bytemuck::cast_slice(self.brushes()));
            hasher.write(bytemuck::cast_slice(self.stops()));
            hasher.write(bytemuck::cast_slice(self.explicit_stops()));
            self.segments.hash(&mut hasher);
            hasher.finish()
        })
    }

    /// The heap the tables hold, capacity included.
    pub fn heap_bytes(&self) -> usize {
        let gradients = self.gradients.as_deref().map_or(0, |tables| {
            size_of::<GradientTables>()
                + tables.brushes.capacity() * size_of::<BrushRecord>()
                + tables.stops.capacity() * size_of::<GradientStopRecord>()
                + tables.explicit_stops.capacity() * size_of::<f32>()
        });
        self.shapes.heap_bytes() + gradients + self.segments.capacity() * size_of::<RecordSegment>()
    }
}
