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
/// use cranpose_core::{MemoryApplier, NodeMarks};
///
/// let applier = MemoryApplier::new();
/// let mut marks = NodeMarks::default();
/// marks.reset(&applier);
/// assert_eq!(marks.get(&applier, 7), 0);
/// marks.set(&applier, 7, 2);
/// assert_eq!(marks.get(&applier, 7), 2);
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
        match applier.resolve_node_index(id) {
            Some(slot) => self.dense.get(slot).copied().unwrap_or(0),
            None => self.elsewhere.get(&id).copied().unwrap_or(0),
        }
    }

    /// Adds the bits of `mark` to `id`'s mark, finding its storage once,
    /// and returns the mark it had.
    ///
    /// ```
    /// use cranpose_core::{MemoryApplier, NodeMarks};
    ///
    /// let applier = MemoryApplier::new();
    /// let mut marks = NodeMarks::default();
    /// marks.reset(&applier);
    /// assert_eq!(marks.add(&applier, 7, 1), 0);
    /// assert_eq!(marks.add(&applier, 7, 2), 1);
    /// assert_eq!(marks.get(&applier, 7), 3);
    /// ```
    #[inline]
    pub fn add(&mut self, applier: &MemoryApplier, id: NodeId, mark: u8) -> u8 {
        let Some(slot) = applier.resolve_node_index(id) else {
            let entry = self.elsewhere.entry(id).or_insert(0);
            let had = *entry;
            *entry |= mark;
            return had;
        };
        if slot >= self.dense.len() {
            self.dense.resize(slot + 1, 0);
        }
        let Some(entry) = self.dense.get_mut(slot) else {
            return 0;
        };
        let had = *entry;
        *entry |= mark;
        had
    }

    /// Marks `id` with `mark`.
    #[inline]
    pub fn set(&mut self, applier: &MemoryApplier, id: NodeId, mark: u8) {
        let Some(slot) = applier.resolve_node_index(id) else {
            self.elsewhere.insert(id, mark);
            return;
        };
        if slot >= self.dense.len() {
            self.dense.resize(slot + 1, 0);
        }
        if let Some(entry) = self.dense.get_mut(slot) {
            *entry = mark;
        }
    }
}
