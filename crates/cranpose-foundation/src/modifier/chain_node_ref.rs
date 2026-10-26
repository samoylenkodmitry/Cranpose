//! Read-only handles on the positions of a [`ModifierNodeChain`], and the
//! iterator over them.

use super::{ModifierNode, ModifierNodeChain, NodeCapabilities, NodePath};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraversalDirection {
    Forward,
    Backward,
}

/// Iterator walking a modifier chain by indexing into `ordered_nodes`.
pub struct ModifierChainIter<'a> {
    chain: &'a ModifierNodeChain,
    cursor: usize,
    remaining: usize,
    direction: TraversalDirection,
}

impl<'a> ModifierChainIter<'a> {
    pub(super) fn forward(chain: &'a ModifierNodeChain) -> Self {
        Self {
            chain,
            cursor: 0,
            remaining: chain.ordered_nodes.len(),
            direction: TraversalDirection::Forward,
        }
    }

    pub(super) fn backward(chain: &'a ModifierNodeChain) -> Self {
        let len = chain.ordered_nodes.len();
        Self {
            chain,
            cursor: len.wrapping_sub(1),
            remaining: len,
            direction: TraversalDirection::Backward,
        }
    }
}

impl<'a> Iterator for ModifierChainIter<'a> {
    type Item = ModifierChainNodeRef<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let node_ref = self.chain.node_ref(ChainPosition::Node(self.cursor));
        self.remaining -= 1;
        match self.direction {
            TraversalDirection::Forward => self.cursor += 1,
            TraversalDirection::Backward => self.cursor = self.cursor.wrapping_sub(1),
        }
        Some(node_ref)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for ModifierChainIter<'_> {}
impl std::iter::FusedIterator for ModifierChainIter<'_> {}

/// A cheap read-only handle on one position of a [`ModifierNodeChain`]: a node or the head or tail
/// marker. Obtain it from `head`, `tail` or the chain iterators and walk the chain with `parent`
/// and `child`.
#[derive(Clone)]
pub struct ModifierChainNodeRef<'a> {
    pub(super) chain: &'a ModifierNodeChain,
    pub(super) position: ChainPosition,
}

// Where a node ref points: the head marker, the node at an index of the
// chain's order, or the tail marker.
#[derive(Clone, Copy)]
pub(super) enum ChainPosition {
    Head,
    Node(usize),
    Tail,
}

impl ModifierChainNodeRef<'_> {
    fn path(&self) -> Option<NodePath> {
        match self.position {
            ChainPosition::Node(index) => Some(self.chain.ordered_nodes[index].0),
            ChainPosition::Head | ChainPosition::Tail => None,
        }
    }

    /// Provides access to the node via a closure, properly handling RefCell borrows.
    /// Returns None for sentinel nodes.
    pub fn with_node<R>(&self, f: impl FnOnce(&dyn ModifierNode) -> R) -> Option<R> {
        self.chain.with_path(self.path()?, f)
    }

    /// Returns the parent reference, including sentinel head when applicable.
    #[inline]
    pub fn parent(&self) -> Option<Self> {
        let position = match self.position {
            ChainPosition::Head => return None,
            ChainPosition::Node(index) => index.checked_sub(1),
            ChainPosition::Tail => self.chain.ordered_nodes.len().checked_sub(1),
        }
        .map_or(ChainPosition::Head, ChainPosition::Node);
        Some(self.chain.node_ref(position))
    }

    /// Returns the child reference, including sentinel tail for the last entry.
    #[inline]
    pub fn child(&self) -> Option<Self> {
        let next = match self.position {
            ChainPosition::Head => 0,
            ChainPosition::Node(index) => index + 1,
            ChainPosition::Tail => return None,
        };
        let position = if next < self.chain.ordered_nodes.len() {
            ChainPosition::Node(next)
        } else {
            ChainPosition::Tail
        };
        Some(self.chain.node_ref(position))
    }

    /// Returns the capability mask for this specific node.
    #[inline]
    pub fn kind_set(&self) -> NodeCapabilities {
        match self.position {
            ChainPosition::Node(index) => self.chain.ordered_nodes[index].1,
            ChainPosition::Head | ChainPosition::Tail => NodeCapabilities::empty(),
        }
    }

    /// Returns the entry index backing this node when it is part of the chain.
    pub fn entry_index(&self) -> Option<usize> {
        self.path().map(|path| path.entry())
    }

    /// Returns how many delegate hops separate this node from its root element.
    pub fn delegate_depth(&self) -> usize {
        self.path().map_or(0, |path| path.delegates().len())
    }

    /// Returns the aggregated capability mask for the subtree rooted at this node.
    #[inline]
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        match self.position {
            ChainPosition::Head => self.chain.aggregated_capabilities,
            ChainPosition::Node(index) => self.chain.ordered_nodes[index].2,
            ChainPosition::Tail => NodeCapabilities::empty(),
        }
    }

    /// Returns true if this reference targets the sentinel head.
    pub fn is_head(&self) -> bool {
        matches!(self.position, ChainPosition::Head)
    }

    /// Returns true if this reference targets the sentinel tail.
    pub fn is_tail(&self) -> bool {
        matches!(self.position, ChainPosition::Tail)
    }

    /// Returns true if this reference targets either sentinel.
    pub fn is_sentinel(&self) -> bool {
        matches!(self.position, ChainPosition::Head | ChainPosition::Tail)
    }

    /// Returns true if this node has any capability bits present in `mask`.
    pub fn has_capability(&self, mask: NodeCapabilities) -> bool {
        !mask.is_empty() && self.kind_set().intersects(mask)
    }

    /// Visits descendant nodes, optionally including `self`, in insertion order.
    pub fn visit_descendants<F>(self, include_self: bool, mut f: F)
    where
        F: FnMut(Self),
    {
        let mut current = if include_self {
            Some(self)
        } else {
            self.child()
        };
        while let Some(node) = current {
            if node.is_tail() {
                break;
            }
            if !node.is_sentinel() {
                f(node.clone());
            }
            current = node.child();
        }
    }

    /// Visits descendant nodes that match `mask`, short-circuiting when possible.
    pub fn visit_descendants_matching<F>(self, include_self: bool, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(Self),
    {
        if mask.is_empty() {
            self.visit_descendants(include_self, f);
            return;
        }

        if !self.aggregate_child_capabilities().intersects(mask) {
            return;
        }

        self.visit_descendants(include_self, |node| {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        });
    }

    /// Visits ancestor nodes up to (but excluding) the sentinel head.
    pub fn visit_ancestors<F>(self, include_self: bool, mut f: F)
    where
        F: FnMut(Self),
    {
        let mut current = if include_self {
            Some(self)
        } else {
            self.parent()
        };
        while let Some(node) = current {
            if node.is_head() {
                break;
            }
            f(node.clone());
            current = node.parent();
        }
    }

    /// Visits ancestor nodes that match `mask`.
    pub fn visit_ancestors_matching<F>(self, include_self: bool, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(Self),
    {
        if mask.is_empty() {
            self.visit_ancestors(include_self, f);
            return;
        }

        self.visit_ancestors(include_self, |node| {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        });
    }
}
