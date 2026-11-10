//! Where a layout node's parent placed it and what size it measured, and
//! the placement bookkeeping of a layout pass.

use std::cell::Cell;

use cranpose_core::PackedU64;

use crate::modifier::{Point, Size};

/// The running layout pass and how many placed nodes it has cleared and not
/// placed again yet: the nodes it unplaces, unless their parents place them
/// before it ends. Ids start past 0, the pass a node no pass cleared names.
pub(crate) struct PlacementPass {
    id: Cell<u64>,
    unplaced: Cell<usize>,
}

impl PlacementPass {
    pub(crate) const fn new() -> Self {
        Self {
            id: Cell::new(1),
            unplaced: Cell::new(0),
        }
    }

    /// Starts the placement bookkeeping of a layout pass.
    pub(crate) fn begin(&self) {
        self.id.set(self.id.get() + 1);
        self.unplaced.set(0);
    }

    /// The running pass, when it has left a node unplaced that it found placed.
    pub(crate) fn with_unplaced_nodes(&self) -> Option<u64> {
        (self.unplaced.get() > 0).then(|| self.id.get())
    }

    /// Counts a placed node the running pass cleared, and returns the pass.
    pub(super) fn clear(&self) -> u64 {
        self.unplaced.set(self.unplaced.get() + 1);
        self.id.get()
    }

    /// Whether a node cleared in pass `cleared_in` was cleared by the running
    /// pass, which then counts it placed again.
    pub(super) fn restore(&self, cleared_in: u64) -> bool {
        let restored = cleared_in == self.id.get();
        if restored {
            self.unplaced.set(self.unplaced.get().saturating_sub(1));
        }
        restored
    }
}

/// The measured size and placed position that a layout node kept from its latest layout pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutState {
    pub(super) size: Size,
    pub(super) position: Point,
    pub(super) is_placed: bool,
    // The layout pass that cleared the placed flag while the node was placed.
    pub(super) cleared_in_pass: PackedU64,
    pub(super) content_offset: Point,
    pub(super) z_index: f32,
    // Whether the node's last placement gave a child a z index other than 0.
    pub(super) places_children_with_z_index: bool,
}

// Where a parent placed a child: its position in the parent, and the z
// index the placement gave it among its siblings.
#[derive(Clone, Copy, Default)]
pub(crate) struct ChildPlacement {
    pub(super) position: Point,
    pub(super) z_index: f32,
}

impl ChildPlacement {
    pub(crate) fn new(position: Point, z_index: f32) -> Self {
        Self { position, z_index }
    }
}

impl LayoutState {
    /// Returns the measured size, in logical pixels.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the placed position relative to the parent, in logical pixels.
    pub fn position(&self) -> Point {
        self.position
    }

    /// Offset of the content box relative to the node origin (e.g. due to
    /// padding).
    pub fn content_offset(&self) -> Point {
        self.content_offset
    }

    /// The same state placed at the origin: what a window root's own scene
    /// starts from, since the position its parent gave it belongs to the
    /// parent's window.
    pub fn at_origin(mut self) -> Self {
        self.position = Point::default();
        self
    }

    /// Returns whether layout has placed the node.
    pub fn is_placed(&self) -> bool {
        self.is_placed
    }

    /// The z index the node's parent last placed it with. It adds to the
    /// node's `Modifier::z_index` to order the node among its siblings.
    pub fn z_index(&self) -> f32 {
        self.z_index
    }

    /// Whether the node's last placement gave one of its children a z index
    /// other than 0: the scene then orders the children by z index.
    pub fn places_children_with_z_index(&self) -> bool {
        self.places_children_with_z_index
    }

    // Whether `pass` found this node placed and left it unplaced.
    pub(crate) fn unplaced_in(&self, pass: u64) -> bool {
        !self.is_placed && self.cleared_in_pass.get() == pass
    }
}

// Counts a layout node whose last placement gave a child a z index among the
// thread's reasons to order siblings, when `places`, or takes it out.
pub(super) fn count_z_index_placer(places: bool) {
    crate::thread_state::cells(|cells| {
        let reasons = &cells.z_order_reasons;
        reasons.set(if places {
            reasons.get() + 1
        } else {
            reasons.get().saturating_sub(1)
        });
    });
}
