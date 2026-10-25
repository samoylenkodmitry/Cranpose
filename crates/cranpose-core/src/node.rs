use std::any::{Any, TypeId};

use smallvec::SmallVec;

use crate::{
    Applier, Command, Composer, DirtyBubble, NodeError, NodeId, composer_context,
    with_current_composer,
};

/// Runs `f` on the node `id` as an `N`, using the current composer. Returns an error if the node
/// does not exist or is not an `N`. Must be called during composition.
pub fn with_node_mut<N: Node + 'static, R>(
    id: NodeId,
    f: impl FnOnce(&mut N) -> R,
) -> Result<R, NodeError> {
    with_current_composer(|composer| composer.with_node_mut(id, f))
}

/// Makes the node `id` the parent of the nodes emitted next, using the current composer. Needs a
/// matching `pop_parent`. Must be called during composition.
pub fn push_parent(id: NodeId) {
    with_current_composer(|composer| composer.push_parent(id));
}

/// Closes the parent that the matching `push_parent` opened, using the current composer. Must be
/// called during composition.
pub fn pop_parent() {
    with_current_composer(Composer::pop_parent);
}

/// A node of the tree that a composition builds. An applier stores the nodes by id. Every method
/// has a default, so a simple node implements none of them.
pub trait Node: Any {
    /// Called once the node has its id, when the composition applies the node's creation.
    fn mount(&mut self) {}
    /// Called just before the node is removed from the applier.
    fn unmount(&mut self) {}
    /// Adds `child` to this node's child list, returning whether the list
    /// actually changed. A node that already holds the child returns `false`:
    /// callers record a structural change from this answer, and a structural
    /// change re-lowers the whole subtree, so claiming one for a no-op costs a
    /// full rebuild per call.
    fn insert_child(&mut self, _child: NodeId) -> bool {
        false
    }
    /// Removes `child` from this node's child list, returning whether the list
    /// actually changed. Only this node can answer that: a caller inspecting
    /// the child's parent pointer gets it wrong when the child was reparented
    /// while still listed here.
    fn remove_child(&mut self, _child: NodeId) -> bool {
        false
    }
    /// Moves the child at the first index to the second index in the child list.
    fn move_child(&mut self, _from: usize, _to: usize) {}
    /// Replaces the child list with `children`, in order.
    fn update_children(&mut self, _children: &[NodeId]) {}
    /// Whether the node adds and drops its children itself, as a
    /// subcomposition host does with its slot hosts: the child sync a
    /// composition pass ends with then leaves its list alone. The default
    /// keeps the list the passes compose.
    fn keeps_own_children(&self) -> bool {
        false
    }
    /// Copies child IDs into the provided scratch buffer without allocating a
    /// fresh container for every traversal. A node without children leaves
    /// the buffer empty.
    fn collect_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        out.clear();
    }
    /// Runs `f` on each child id in the order [`Node::collect_children_into`]
    /// lists them. A node that holds its list in place overrides this to
    /// visit it without a copy.
    fn for_each_child(&self, f: &mut dyn FnMut(NodeId)) {
        let mut children = SmallVec::<[NodeId; 8]>::new();
        self.collect_children_into(&mut children);
        for child in children {
            f(child);
        }
    }
    /// Copies the ids of the children this node owns, in order, into `out`. A node whose
    /// `collect_children_into` leaves some children out overrides this to list them all. The
    /// default calls `collect_children_into`.
    fn collect_owned_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        self.collect_children_into(out);
    }
    /// Finds a child in the order returned by [`Node::collect_owned_children_into`].
    /// Nodes with slice-backed child storage should override this to avoid
    /// materializing the whole list for a single lookup.
    ///
    /// ```
    /// use cranpose_core::{Node, NodeId};
    ///
    /// fn child_is_first(parent: &dyn Node, child: NodeId) -> bool {
    ///     parent.owned_child_index(child) == Some(0)
    /// }
    /// ```
    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        let mut children = SmallVec::<[NodeId; 8]>::new();
        self.collect_owned_children_into(&mut children);
        children.iter().position(|&id| id == child)
    }
    /// Called after the node is created to record its own ID.
    /// Useful for nodes that need to store their ID for later operations.
    fn set_node_id(&mut self, _id: NodeId) {}
    /// Called when this node is attached to a parent.
    /// Nodes with parent tracking should set their parent reference here.
    fn on_attached_to_parent(&mut self, _parent: NodeId) {}
    /// Called when this node is removed from its parent.
    /// Nodes with parent tracking should clear their parent reference here.
    fn on_removed_from_parent(&mut self) {}
    /// Get this node's parent ID (for nodes that track parents).
    /// Returns None if node has no parent or doesn't track parents.
    fn parent(&self) -> Option<NodeId> {
        None
    }
    /// Mark this node as needing layout (for nodes with dirty flags).
    /// Called during bubbling to propagate dirtiness up the tree.
    fn mark_needs_layout(&self) {}
    /// Check if this node needs layout (for nodes with dirty flags).
    fn needs_layout(&self) -> bool {
        false
    }
    /// Mark this node as needing measure (size may have changed).
    /// Called during bubbling when children are added/removed.
    fn mark_needs_measure(&self) {}
    /// Check if this node needs measure (for nodes with dirty flags).
    fn needs_measure(&self) -> bool {
        false
    }
    /// Mark that a node below this one needs layout, or measure when
    /// `measure` is set: a layout pass has to visit this node to reach it,
    /// while this node's own measurement may still hold. A node that does not
    /// track the difference marks itself dirty.
    fn mark_descendant_needs_layout(&self, measure: bool) {
        if measure {
            self.mark_needs_measure();
        } else {
            self.mark_needs_layout();
        }
    }
    /// Check if a node below this one needs layout or measure.
    fn descendant_needs_layout(&self) -> bool {
        false
    }
    /// Check if a node below this one needs measure.
    fn descendant_needs_measure(&self) -> bool {
        false
    }
    /// Check if a layout pass has to visit this node: the node or a node
    /// below it needs layout or measure.
    fn layout_dirty(&self) -> bool {
        self.needs_measure() || self.needs_layout() || self.descendant_needs_layout()
    }
    /// Whether the node lays out nothing itself and its parent lays out its
    /// children, so a change of its own is a change of its parent's.
    fn is_virtual(&self) -> bool {
        false
    }
    /// Mark this node as needing semantics recomputation.
    fn mark_needs_semantics(&self) {}
    /// Mark that the semantics tree has to read this node again while its
    /// own semantics are unchanged: a node below it changed its semantics,
    /// or its own placement or children changed.
    fn mark_descendant_needs_semantics(&self) {
        self.mark_needs_semantics();
    }
    /// Check if this node or a node below it needs semantics recomputation.
    fn needs_semantics(&self) -> bool {
        false
    }
    /// Set parent reference for dirty flag bubbling ONLY.
    /// This is a minimal version of on_attached_to_parent that doesn't trigger
    /// registry updates or other side effects. Used during measurement when we
    /// need to establish parent connections for bubble_measure_dirty without
    /// causing the full attachment lifecycle.
    ///
    /// An existing parent is preserved until the structural attach operation
    /// removes the node from its previous container.
    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        if self.parent().is_none() {
            self.on_attached_to_parent(parent);
        }
    }

    /// Returns a recycle pool key when this node supports shell reuse.
    fn recycle_key(&self) -> Option<TypeId> {
        None
    }

    /// Bounds how many recyclable shells of this node type should be retained.
    fn recycle_pool_limit(&self) -> Option<usize> {
        None
    }

    /// Clears live attachments before the node shell enters a recycle pool.
    fn prepare_for_recycle(&mut self) {}

    /// Optionally provides a compact replacement box for this recycled shell.
    ///
    /// Returning `Some` lets nodes move pooled survivors onto fresh compact
    /// storage so the recycle pool does not pin large spike-era allocations.
    fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
        None
    }

    /// Optionally moves a live node onto a fresh box during applier compaction.
    ///
    /// This is used after large-majority teardowns where a small surviving live
    /// tree can otherwise pin allocator pages from a much larger spike-era node
    /// population. Implementations must preserve the node's live state.
    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn Node>> {
        None
    }

    /// Returns the node-owned heap retained beyond the node's own box allocation.
    fn debug_heap_bytes(&self) -> usize {
        0
    }
}

/// Unified API for bubbling layout dirty flags from a node to the root (Applier context).
///
/// This is the canonical function for dirty bubbling during the apply phase (structural changes).
/// Call this after mutations like insert/remove/move that happen during apply.
///
/// # Behavior
/// 1. Marks the starting node as needing layout
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height): a scoped layout repass can leave a dirty node
/// under a clean ancestor, so the walk does not stop at a dirty ancestor.
///
/// # Usage
/// - Call from composer mutations (insert/remove/move) during apply phase
/// - Call from applier-level operations that modify the tree structure
pub fn bubble_layout_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::LAYOUT.apply(applier, node_id);
}

/// Unified API for bubbling measure dirty flags from a node to the root (Applier context).
///
/// Call this when a node's size may have changed (children added/removed, modifier changed).
/// This ensures that measure_layout will increment the cache epoch and re-measure the subtree.
///
/// # Behavior
/// 1. Marks the starting node as needing measure
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops at the root (node with no parent)
pub fn bubble_measure_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::MEASURE.apply(applier, node_id);
}

/// Unified API for bubbling semantics dirty flags from a node to the root (Applier context).
///
/// This mirrors [`bubble_layout_dirty`] but toggles semantics-specific dirty
/// flags instead of layout ones, allowing semantics updates to propagate during
/// the apply phase without forcing layout work.
pub fn bubble_semantics_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::SEMANTICS.apply(applier, node_id);
}

/// Schedules semantics bubbling for a node using the active composer if present.
///
/// This defers the work to the apply phase where we can safely mutate the
/// applier tree without re-entrantly borrowing the composer during composition.
pub fn queue_semantics_invalidation(node_id: NodeId) {
    let _ = composer_context::try_with_composer(|composer| {
        composer.enqueue_semantics_invalidation(node_id);
    });
}

/// Unified API for bubbling layout dirty flags from a node to the root (Composer context).
///
/// This is the canonical function for dirty bubbling during composition (property changes).
/// Call this after property changes that happen during composition via with_node_mut.
///
/// # Behavior
/// 1. Marks the starting node as needing layout
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height): a scoped layout repass can leave a dirty node
/// under a clean ancestor, so the walk does not stop at a dirty ancestor.
///
/// # Type Requirements
/// The node type N must implement Node (which includes mark_needs_layout, parent, etc.).
/// Typically this will be LayoutNode or similar layout-aware node types.
///
/// # Usage
/// - Call from property setters during composition (e.g., set_modifier, set_measure_policy)
/// - Call from widget composition when layout-affecting state changes
pub fn bubble_layout_dirty_in_composer<N: Node + 'static>(node_id: NodeId) {
    bubble_layout_dirty_composer::<N>(node_id);
}

/// Unified API for bubbling measure dirty flags from a node to the root during composition.
///
/// This queues a dirty-bubble command on the active composer so measure invalidation
/// runs during the apply phase, avoiding re-entrant applier borrows while widgets are
/// mutating nodes via `with_node_mut`.
pub fn bubble_measure_dirty_in_composer(node_id: NodeId) {
    with_current_composer(|composer| {
        composer.commands_mut().push(Command::BubbleDirty {
            node_id,
            bubble: DirtyBubble {
                layout: false,
                measure: true,
                semantics: false,
            },
        });
    });
}

/// Unified API for bubbling semantics dirty flags from a node to the root (Composer context).
///
/// This mirrors [`bubble_layout_dirty_in_composer`] but routes through the semantics
/// dirty flag instead of the layout one. Modifier nodes can request semantics
/// invalidations without triggering measure/layout work, and the runtime can
/// query the root to determine whether the semantics tree needs rebuilding.
pub fn bubble_semantics_dirty_in_composer<N: Node + 'static>(node_id: NodeId) {
    bubble_semantics_dirty_composer::<N>(node_id);
}

fn bubble_layout_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_layout();
    });
    with_current_composer(|composer| composer.borrow_applier().note_layout_dirt(node_id));

    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        let advanced = with_node_mut(parent_id, |node: &mut N| {
            node.mark_descendant_needs_layout(false);
            true
        })
        .unwrap_or(false);

        if advanced {
            node_id = parent_id;
        } else {
            break;
        }
    }
}

fn bubble_semantics_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_semantics();
    });

    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        let advanced = with_node_mut(parent_id, |node: &mut N| {
            if !node.needs_semantics() {
                node.mark_descendant_needs_semantics();
            }
            true
        })
        .unwrap_or(false);

        if advanced {
            node_id = parent_id;
        } else {
            break;
        }
    }
}

impl dyn Node {
    /// Returns the node as `Any`, for downcasting to its concrete type.
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
