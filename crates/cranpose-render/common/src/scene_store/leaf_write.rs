// A leaf written in one step: a layout node drawn in its parent's layer,
// placed by a translation alone. One pass over its rows takes the place of
// the setters a leaf would go through one by one.

use cranpose_core::{NodeId, RecordId};
use cranpose_ui::{Point, Size};

use super::{Item, RecordFlags, RecordKind, SceneStore, records::same_pair};

/// What a leaf holds once a scene update wrote it.
#[derive(Clone, Copy, Debug)]
pub struct Leaf<'a> {
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
    /// The leaf's draws, in draw order.
    pub items: &'a [Item],
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Writes `id` as `leaf`: a leaf of `leaf.node` without properties,
    /// placed by a translation alone, whose draws changed in the update in
    /// progress. The draws and children `id` held before stay in the store:
    /// the caller removes them.
    pub fn write_leaf(&mut self, id: RecordId, leaf: Leaf<'_>) {
        let index = id.index();
        self.set_kind(id, RecordKind::Leaf, Some(leaf.node));
        let unpropped = self.warm[index].props.is_some();
        if unpropped {
            self.drop_props(index);
            super::facts::forget_children(self, index);
        }
        let size = [leaf.size.width, leaf.size.height];
        let resized = !same_pair(self.geo[index].size, size);
        self.geo[index].size = size;
        let offset = [leaf.content_offset.x, leaf.content_offset.y];
        let shifted = !same_pair(self.warm[index].content_offset, offset);
        self.warm[index].content_offset = offset;
        let reflagged = self.write_leaf_flags(index, leaf.flags);
        let moved = self.place_leaf_row(index, leaf.origin_in_parent) || shifted;
        // The content change noted below leaves the leaf's containment and
        // reach to be worked out again.
        self.replace_items(id, leaf.items);
        if moved {
            self.moved(index);
        }
        if let Some(parent) = self.tree[index].parent
            && (unpropped || resized || reflagged || moved)
        {
            self.stale(parent.index());
        }
        self.note_content_change(id);
        if moved {
            self.note_subtree_change(self.tree[index].parent);
        }
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
            if moved {
                self.note_subtree_change(Some(parent));
            }
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
