//! A layout node's layout state as layout writes it: one cell a field, so a
//! write takes no borrow of the whole state.

use std::cell::Cell;

use cranpose_core::{NodeId, PackedU64};

use super::layout_state::{ChildPlacement, LayoutState, count_z_index_placer};
use crate::{
    modifier::{Point, Size},
    render_state::AppContext,
};

// The size a node measured, where its parent placed it, and the stamps that
// note the node once in each list of changed nodes a take empties.
#[derive(Default)]
pub(crate) struct LayoutCells {
    size: Cell<Size>,
    position: Cell<Point>,
    is_placed: Cell<bool>,
    // The layout pass that cleared the placed flag while the node was placed.
    cleared_in_pass: Cell<PackedU64>,
    content_offset: Cell<Point>,
    semantics_stamp: Cell<PackedU64>,
    // The take of the moved-node list this node was last noted in.
    moved_noted_in: Cell<PackedU64>,
    // The take of the reshaped-node list this node was last noted in.
    reshaped_noted_in: Cell<PackedU64>,
    z_index: Cell<f32>,
    // Whether the node's last placement gave a child a z index other than 0.
    places_children_with_z_index: Cell<bool>,
}

impl LayoutCells {
    // The state as a reader outside layout sees it.
    pub(crate) fn snapshot(&self) -> LayoutState {
        LayoutState {
            size: self.size.get(),
            position: self.position.get(),
            is_placed: self.is_placed.get(),
            cleared_in_pass: self.cleared_in_pass.get(),
            content_offset: self.content_offset.get(),
            z_index: self.z_index.get(),
            places_children_with_z_index: self.places_children_with_z_index.get(),
        }
    }

    pub(crate) fn size(&self) -> Size {
        self.size.get()
    }

    pub(crate) fn position(&self) -> Point {
        self.position.get()
    }

    pub(crate) fn is_placed(&self) -> bool {
        self.is_placed.get()
    }

    pub(crate) fn z_index(&self) -> f32 {
        self.z_index.get()
    }

    pub(crate) fn places_children_with_z_index(&self) -> bool {
        self.places_children_with_z_index.get()
    }

    // Writes the content offset of node `node_id`, reporting an actual
    // change to the semantics tree.
    pub(crate) fn set_content_offset(&self, node_id: Option<NodeId>, offset: Point) {
        if self.content_offset.get() != offset {
            self.content_offset.set(offset);
            self.note_semantics_change(node_id);
        }
    }

    // Notes to the scene phase of `context` that the layout pass changed the
    // geometry of this node, `node_id`.
    #[inline]
    pub(crate) fn note_geometry_change(&self, node_id: NodeId, context: &AppContext) {
        stamp(&self.reshaped_noted_in, |noted| {
            context.note_geometry_scene_node(node_id, noted);
        });
    }

    // Writes the measured size of node `node_id`, self-reporting an actual
    // change to the scene phase of `context`, the node's app context.
    #[inline]
    pub(crate) fn set_size(&self, node_id: Option<NodeId>, size: Size, context: &AppContext) {
        if self.size.get() != size {
            if let Some(id) = node_id {
                self.note_geometry_change(id, context);
            }
            self.size.set(size);
            self.note_semantics_change(node_id);
        }
    }

    pub(crate) fn set_places_children_with_z_index(&self, places: bool) {
        if self.places_children_with_z_index.replace(places) != places {
            count_z_index_placer(places);
        }
    }

    // Places node `node_id` where `at` says, with its z index.
    pub(crate) fn place_child(
        &self,
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
    pub(crate) fn set_z_index(&self, node_id: Option<NodeId>, z_index: f32) {
        if self.z_index.replace(z_index).to_bits() == z_index.to_bits() {
            return;
        }
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
        &self,
        node_id: Option<NodeId>,
        position: Point,
        placer: Option<NodeId>,
        context: &AppContext,
    ) {
        let was_placed = self.is_placed.get();
        // A node the running pass cleared while it was placed changes
        // nothing the scene draws by being placed again.
        let newly_placed = !was_placed
            && !context
                .placement_pass()
                .restore(self.cleared_in_pass.get().get());
        let moved = self.position.replace(position) != position;
        if let Some(id) = node_id {
            if newly_placed {
                self.note_geometry_change(id, context);
            } else if moved {
                stamp(&self.moved_noted_in, |noted| {
                    context.record_moved_scene_node(id, placer, noted);
                });
            }
        }
        if moved || !was_placed {
            self.is_placed.set(true);
            self.note_semantics_change(node_id);
        }
    }

    // Clears the placed flag of node `node_id` at the start of a layout pass
    // of `context`. Until its parent places it again the pass counts the
    // node unplaced, and the pass names it to the scene phase if it ends
    // that way.
    pub(crate) fn clear_placed(&self, node_id: Option<NodeId>, context: &AppContext) {
        if !self.is_placed.replace(false) {
            return;
        }
        self.cleared_in_pass
            .set(PackedU64::new(context.placement_pass().clear()));
        self.note_semantics_change(node_id);
    }

    #[inline]
    pub(crate) fn note_semantics_change(&self, node_id: Option<NodeId>) {
        if let Some(id) = node_id {
            stamp(&self.semantics_stamp, |noted| {
                crate::semantics_layout_log::record_layout_change(id, noted);
            });
        }
    }
}

// Runs `note` on the stamp `cell` holds, which it may write.
#[inline]
fn stamp(cell: &Cell<PackedU64>, note: impl FnOnce(&mut PackedU64)) {
    let mut noted = cell.get();
    note(&mut noted);
    cell.set(noted);
}
