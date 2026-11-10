// A leaf written in one step: a layout node drawn in its parent's layer,
// placed by a translation alone. One pass over its rows takes the place of
// the setters a leaf would go through one by one.

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{Point, Size};

use super::{Item, RecordFlags, RecordKind, SceneStore, records::same_pair};

/// What a leaf holds once a scene update wrote it, its draws aside.
#[derive(Clone, Copy, Debug)]
pub struct Leaf {
    /// The layout node the leaf draws for.
    pub node: NodeId,
    /// The node's size.
    pub size: Size,
    /// Offset of the node's content box from its origin.
    pub content_offset: Point,
    /// Where layout placed the node within its parent's content, before the
    /// parent's content offset.
    pub origin_in_parent: Point,
    /// The leaf's flags, such as [`RecordFlags::MOTION_ANIMATED`].
    pub flags: RecordFlags,
}

// What writing a leaf's rows changed.
#[derive(Clone, Copy)]
struct LeafRows {
    // The leaf moved in its parent, or its content within it.
    moved: bool,
    // The parent's containment and reach no longer hold.
    parent_stale: bool,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Writes `id` as `leaf`: a leaf of `leaf.node` without properties,
    /// placed by a translation alone, whose draws changed in the update in
    /// progress, to `items`. The draws and children `id` held before stay in
    /// the store: the caller removes them.
    pub fn write_leaf(&mut self, id: RecordId, leaf: Leaf, items: &[Item]) {
        let rows = self.write_leaf_rows(id, leaf);
        // The content change noted below leaves the leaf's containment and
        // reach to be worked out again.
        self.replace_items(id, items);
        self.end_leaf_write(id, rows);
    }

    // Writes `id` as `leaf` around the items it holds, which the update in
    // progress drew again in place.
    pub(crate) fn redraw_leaf(&mut self, id: RecordId, leaf: Leaf) {
        let rows = self.write_leaf_rows(id, leaf);
        self.end_leaf_write(id, rows);
    }

    #[inline]
    fn write_leaf_rows(&mut self, id: RecordId, leaf: Leaf) -> LeafRows {
        let index = id.index();
        self.set_kind(id, RecordKind::Leaf, Some(leaf.node));
        let unpropped = self.warm[index].props.is_some();
        if unpropped {
            self.unprop(index);
        }
        let size = [leaf.size.width, leaf.size.height];
        let resized = !same_pair(self.geo[index].size, size);
        self.geo[index].size = size;
        let offset = [leaf.content_offset.x, leaf.content_offset.y];
        let shifted = !same_pair(self.warm[index].content_offset, offset);
        self.warm[index].content_offset = offset;
        let reflagged = self.write_leaf_flags(index, leaf.flags);
        let moved = self.place_leaf_row(index, leaf.origin_in_parent) || shifted;
        LeafRows {
            moved,
            parent_stale: unpropped || resized || reflagged || moved,
        }
    }

    #[inline]
    fn end_leaf_write(&mut self, id: RecordId, rows: LeafRows) {
        let index = id.index();
        if rows.moved {
            self.moved(index);
        }
        if let Some(parent) = self.tree[index].parent
            && rows.parent_stale
        {
            self.stale(parent.index());
        }
        self.note_content_change(id);
    }

    // Drops the properties of the record at `index`, which becomes a leaf,
    // with the child facts they decided. A layer seldom turns into a leaf.
    #[cold]
    #[inline(never)]
    fn unprop(&mut self, index: usize) {
        self.drop_props(index);
        super::facts::forget_children(self, index);
    }

    /// Places `id`, a leaf, at `origin_in_parent` by a translation alone,
    /// with `flags`.
    pub fn place_leaf(&mut self, id: RecordId, origin_in_parent: Point, flags: RecordFlags) {
        let index = id.index();
        let reflagged = self.write_leaf_flags(index, flags);
        let moved = self.place_leaf_row(index, origin_in_parent);
        if moved {
            self.moved(index);
        }
        if reflagged {
            self.stale(index);
        }
        if let Some(parent) = self.tree[index].parent
            && (moved || reflagged)
        {
            self.stale(parent.index());
        }
    }

    /// Moves `id`, a leaf, to `origin_in_parent` by a translation alone,
    /// among the items of a parent whose content the update in progress
    /// changes: the caller notes that change with
    /// [`Self::note_content_change`] of the parent.
    pub fn move_leaf(&mut self, id: RecordId, origin_in_parent: Point) {
        let index = id.index();
        if self.place_leaf_row(index, origin_in_parent) {
            self.moved(index);
        }
    }

    // Writes the flags of the leaf at `index` the store does not keep
    // itself; returns whether they changed.
    #[inline]
    fn write_leaf_flags(&mut self, index: usize, flags: RecordFlags) -> bool {
        let current = self.tree[index].flags;
        let flags = flags
            .without(RecordFlags::STORE_KEPT)
            .with(current.only(RecordFlags::STORE_KEPT));
        if flags == current {
            return false;
        }
        self.tree[index].flags = flags;
        if flags.only(RecordFlags::ORIGIN_SINKS) != current.only(RecordFlags::ORIGIN_SINKS) {
            super::facts::forget_children(self, index);
        }
        true
    }

    // Puts the leaf at `index` at `origin_in_parent`, its transform the
    // translation there; returns whether it moved.
    #[inline]
    fn place_leaf_row(&mut self, index: usize, origin_in_parent: Point) -> bool {
        let placement = [origin_in_parent.x, origin_in_parent.y];
        let placed = !same_pair(self.geo[index].placement, placement);
        self.geo[index].placement = placement;
        let transformed = self.tree[index].transform.take();
        if let Some(entry) = transformed {
            self.transforms.remove(entry.index());
        }
        placed || transformed.is_some()
    }
}
