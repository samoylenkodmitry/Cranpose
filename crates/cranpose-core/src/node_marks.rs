//! A small value for each applier node, kept in the applier's dense node
//! order: one walk over the tree reads and sets a node's mark with the two
//! array reads that find its storage, where a hash set hashed the id and
//! probed its groups.

use crate::{MemoryApplier, NodeId, collections::map::HashMap};

/// Marks on the nodes of one [`MemoryApplier`], for one walk over its tree.
///
/// A mark is a byte; an unmarked node reads `0`. Marks stay until
/// [`NodeMarks::reset`], and only a walk that leaves the applier's tree as it
/// is may keep them: a node stored again in another place loses its mark.
///
/// ```
/// use cranpose_core::{MemoryApplier, NodeId, NodeMarks};
///
/// let applier = MemoryApplier::new();
/// let mut marks = NodeMarks::default();
/// marks.reset(&applier);
/// assert_eq!(marks.get(&applier, NodeId(7)), 0);
/// marks.set(&applier, NodeId(7), 2);
/// assert_eq!(marks.get(&applier, NodeId(7)), 2);
/// ```
#[derive(Default)]
pub struct NodeMarks {
    dense: Vec<u8>,
    elsewhere: HashMap<NodeId, u8>,
}

impl NodeMarks {
    /// Unmarks every node, with room for every node `applier` holds now.
    pub fn reset(&mut self, applier: &MemoryApplier) {
        self.dense.clear();
        self.dense.resize(applier.state.nodes.len(), 0);
        self.elsewhere.clear();
    }

    /// The mark of `id`: `0` while it has none.
    #[inline]
    pub fn get(&self, applier: &MemoryApplier, id: NodeId) -> u8 {
        self.mark_at(MarkPlace::of(applier, id))
    }

    /// Adds the bits of `mark` to `id`'s mark, finding its storage once,
    /// and returns the mark it had.
    ///
    /// ```
    /// use cranpose_core::{MemoryApplier, NodeId, NodeMarks};
    ///
    /// let applier = MemoryApplier::new();
    /// let mut marks = NodeMarks::default();
    /// marks.reset(&applier);
    /// assert_eq!(marks.add(&applier, NodeId(7), 1), 0);
    /// assert_eq!(marks.add(&applier, NodeId(7), 2), 1);
    /// assert_eq!(marks.get(&applier, NodeId(7)), 3);
    /// ```
    #[inline]
    pub fn add(&mut self, applier: &MemoryApplier, id: NodeId, mark: u8) -> u8 {
        let entry = self.entry_at(MarkPlace::of(applier, id));
        let had = *entry;
        *entry |= mark;
        had
    }

    /// Adds the bits of `mark` to `id`'s mark and those of `ancestor_mark`
    /// to the mark of each node above it, from its parent up to the first
    /// that had them. Each node's storage is found once.
    pub fn add_with_ancestors(
        &mut self,
        applier: &MemoryApplier,
        id: NodeId,
        mark: u8,
        ancestor_mark: u8,
    ) {
        let mut place = MarkPlace::of(applier, id);
        *self.entry_at(place) |= mark;
        while let Some(parent) = applier.parent_at(place) {
            place = MarkPlace::of(applier, parent);
            let entry = self.entry_at(place);
            let had = *entry;
            *entry |= ancestor_mark;
            if had & ancestor_mark == ancestor_mark {
                break;
            }
        }
    }

    /// Marks `id` with `mark`.
    #[inline]
    pub fn set(&mut self, applier: &MemoryApplier, id: NodeId, mark: u8) {
        *self.entry_at(MarkPlace::of(applier, id)) = mark;
    }

    #[inline]
    pub(crate) fn mark_at(&self, place: MarkPlace) -> u8 {
        match place {
            MarkPlace::Dense(slot) => self.dense.get(slot).copied().unwrap_or(0),
            MarkPlace::Elsewhere(id) => self.elsewhere.get(&id).copied().unwrap_or(0),
        }
    }

    #[inline]
    pub(crate) fn entry_at(&mut self, place: MarkPlace) -> &mut u8 {
        match place {
            MarkPlace::Dense(slot) if slot < self.dense.len() => &mut self.dense[slot],
            _ => self.grown_entry_at(place),
        }
    }

    // The entry of a node stored past the room `reset` made, or kept apart.
    #[cold]
    #[inline(never)]
    fn grown_entry_at(&mut self, place: MarkPlace) -> &mut u8 {
        match place {
            MarkPlace::Dense(slot) => {
                self.dense.resize(slot + 1, 0);
                &mut self.dense[slot]
            }
            MarkPlace::Elsewhere(id) => self.elsewhere.entry(id).or_insert(0),
        }
    }
}

// Where a node's mark is kept, and its node in the applier: the node's
// dense storage slot, or its id for a node kept apart.
#[derive(Clone, Copy)]
pub(crate) enum MarkPlace {
    Dense(usize),
    Elsewhere(NodeId),
}

impl MarkPlace {
    #[inline]
    pub(crate) fn of(applier: &MemoryApplier, id: NodeId) -> Self {
        applier
            .resolve_node_index(id)
            .map_or(Self::Elsewhere(id), Self::Dense)
    }
}
