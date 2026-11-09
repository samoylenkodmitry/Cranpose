//! Where a layout node's parent placed it and what size it measured, and
//! the placement bookkeeping of a layout pass.

use std::cell::Cell;

use cranpose_core::{NodeId, PackedU64};

use crate::{
    modifier::{Point, Size},
    render_state::AppContext,
};

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
    fn clear(&self) -> u64 {
        self.unplaced.set(self.unplaced.get() + 1);
        self.id.get()
    }

    /// Whether a node cleared in pass `cleared_in` was cleared by the running
    /// pass, which then counts it placed again.
    fn restore(&self, cleared_in: u64) -> bool {
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
    size: Size,
    position: Point,
    is_placed: bool,
    /// The layout pass that cleared the placed flag while the node was placed.
    cleared_in_pass: PackedU64,
    content_offset: Point,
    z_index: f32,
    // Whether the node's last placement gave a child a z index other than 0.
    places_children_with_z_index: bool,
}

// Where a parent placed a child: its position in the parent, and the z
// index the placement gave it among its siblings.
#[derive(Clone, Copy, Default)]
pub(crate) struct ChildPlacement {
    position: Point,
    z_index: f32,
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

    // Writes the content offset of node `node_id`, reporting an actual
    // change to the semantics tree.
    pub(crate) fn set_content_offset(&mut self, node_id: Option<NodeId>, offset: Point) {
        if self.content_offset != offset {
            self.content_offset = offset;
            note_semantics_change(node_id);
        }
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

    // Writes the measured size of node `node_id`, self-reporting an actual
    // change to the scene phase of `context`, the node's app context.
    pub(crate) fn set_size(&mut self, node_id: Option<NodeId>, size: Size, context: &AppContext) {
        if self.size != size {
            if let Some(id) = node_id {
                context.note_geometry_scene_node(id);
            }
            self.size = size;
            note_semantics_change(node_id);
        }
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

    pub(crate) fn set_places_children_with_z_index(&mut self, places: bool) {
        if self.places_children_with_z_index != places {
            self.places_children_with_z_index = places;
            count_z_index_placer(places);
        }
    }

    // Places node `node_id` where `at` says, with its z index.
    pub(crate) fn place_child(
        &mut self,
        node_id: Option<NodeId>,
        at: ChildPlacement,
        placer: Option<NodeId>,
        context: &AppContext,
    ) {
        self.place(node_id, at.position, placer, context);
        self.set_z_index(node_id, at.z_index);
    }

    // Sets the z index the parent placed node `node_id` with. A change
    // reorders the node among its siblings, which its parent's layer holds.
    pub(crate) fn set_z_index(&mut self, node_id: Option<NodeId>, z_index: f32) {
        if self.z_index.to_bits() == z_index.to_bits() {
            return;
        }
        self.z_index = z_index;
        if let Some(id) = node_id {
            crate::schedule_z_order_change(id);
        }
    }

    // Writes the placed position of node `node_id` and marks it placed,
    // self-reporting to the scene phase of `context`, the node's app
    // context, an actual move, and a placement of a node the scene does not
    // draw: one the running pass did not find placed. A move is reported
    // apart, with `placer`, the node whose placement put the node there, so
    // the scene can keep what it drew for the node.
    pub(crate) fn place(
        &mut self,
        node_id: Option<NodeId>,
        position: Point,
        placer: Option<NodeId>,
        context: &AppContext,
    ) {
        // A node the running pass cleared while it was placed changes
        // nothing the scene draws by being placed again.
        let newly_placed =
            !self.is_placed && !context.placement_pass().restore(self.cleared_in_pass.get());
        let moved = self.position != position;
        if let Some(id) = node_id {
            if newly_placed {
                context.note_geometry_scene_node(id);
            } else if moved {
                context.record_moved_scene_node(id, placer);
            }
        }
        self.position = position;
        if moved || !self.is_placed {
            self.is_placed = true;
            note_semantics_change(node_id);
        }
    }

    // Clears the placed flag of node `node_id` at the start of a layout pass
    // of `context`. Until its parent places it again the pass counts the
    // node unplaced, and the pass names it to the scene phase if it ends
    // that way.
    pub(crate) fn clear_placed(&mut self, node_id: Option<NodeId>, context: &AppContext) {
        if !self.is_placed {
            return;
        }
        self.is_placed = false;
        self.cleared_in_pass = PackedU64::new(context.placement_pass().clear());
        note_semantics_change(node_id);
    }

    /// Whether `pass` found this node placed and left it unplaced.
    pub(crate) fn unplaced_in(&self, pass: u64) -> bool {
        !self.is_placed && self.cleared_in_pass.get() == pass
    }
}

// Notes to the semantics tree that layout changed node `node_id`.
#[inline]
pub(crate) fn note_semantics_change(node_id: Option<NodeId>) {
    if let Some(id) = node_id {
        crate::semantics_layout_log::record_layout_change(id);
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
