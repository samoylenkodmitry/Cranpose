use crate::NodeId;

/// A set of node ids, one bit a node, indexed by the id: a list that names
/// each node once asks it in place of a stamp in every node. It grows to
/// the highest id it holds, as ids are reused lowest first.
#[derive(Clone, Debug, Default)]
pub struct NodeBits(Vec<u64>);

impl NodeBits {
    /// Adds `node`; `false` when the set held it already.
    #[inline]
    pub fn insert(&mut self, node: NodeId) -> bool {
        let (word, bit) = (node.0 / 64, 1 << (node.0 % 64));
        if word >= self.0.len() {
            self.0.resize(word + 1, 0);
        }
        let held = self.0[word] & bit != 0;
        self.0[word] |= bit;
        !held
    }

    /// Takes `node` out of the set.
    #[inline]
    pub fn remove(&mut self, node: NodeId) {
        if let Some(word) = self.0.get_mut(node.0 / 64) {
            *word &= !(1 << (node.0 % 64));
        }
    }
}
