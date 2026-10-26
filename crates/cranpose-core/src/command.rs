use crate::{
    Applier, ChildList, Node, NodeError, NodeId, apply_remove_child, attach_child_at,
    cleanup_detached_child, detach_child_from_parent, insert_child_with_reparenting,
    note_structural_move, sync_or_settle_children,
};
pub(crate) type CommandCallback =
    Box<dyn FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirtyBubble {
    pub(crate) layout: bool,
    pub(crate) measure: bool,
    pub(crate) semantics: bool,
}

impl DirtyBubble {
    pub(crate) const LAYOUT: Self = Self {
        layout: true,
        measure: false,
        semantics: false,
    };

    pub(crate) const MEASURE: Self = Self {
        layout: false,
        measure: true,
        semantics: false,
    };

    pub(crate) const LAYOUT_AND_MEASURE: Self = Self {
        layout: true,
        measure: true,
        semantics: false,
    };

    pub(crate) const SEMANTICS: Self = Self {
        layout: false,
        measure: false,
        semantics: true,
    };

    /// Marks `node_id` with the bubble's flags and every ancestor up to the
    /// root as having a dirty descendant.
    pub(crate) fn apply(self, applier: &mut dyn Applier, node_id: NodeId) {
        self.walk(applier, Some(node_id), true);
    }

    /// Marks every ancestor of `node_id` up to the root as having a dirty
    /// descendant, leaving `node_id`'s own flags as they are.
    pub(crate) fn apply_to_ancestors(self, applier: &mut dyn Applier, node_id: NodeId) {
        if self.layout || self.measure {
            applier.note_layout_dirt(node_id);
        }
        let parent = applier.get_mut(node_id).ok().and_then(|node| node.parent());
        self.walk(applier, parent, false);
    }

    /// Visits each node once for all the flags. The walk goes on past an
    /// ancestor that is already dirty: a scoped layout repass can leave a
    /// dirty node under a clean ancestor. It stops at one an earlier walk of
    /// the same batch of commands passed with its flags
    /// (`bubble_passes.rs`). A virtual start node passes its own flags to
    /// its parent. A walk that carries measure marks all that one carrying
    /// layout marks, so it passes with the layout flag too, and a layout
    /// walk after it stops where it passed.
    fn walk(self, applier: &mut dyn Applier, first: Option<NodeId>, mut start: bool) {
        let flags = u8::from(self.layout || self.measure)
            | u8::from(self.measure) << 1
            | u8::from(self.semantics) << 2;
        let mut next = first;
        while let Some(id) = next {
            let Some(node) = applier.bubble_node(id, flags, !start) else {
                break;
            };
            self.mark(node, start);
            next = node.parent();
            let started = start;
            start = start && node.is_virtual();
            if started && !start && (self.layout || self.measure) {
                applier.note_layout_dirt(id);
            }
        }
    }

    /// Marks the bubble's start node with its flags, and an ancestor as
    /// having a dirty descendant where it is not marked yet.
    fn mark(self, node: &mut dyn Node, start: bool) {
        if start {
            if self.layout {
                node.mark_needs_layout();
            }
            if self.measure {
                node.mark_needs_measure();
            }
            if self.semantics {
                node.mark_needs_semantics();
            }
            return;
        }
        let marked = if self.measure {
            node.descendant_needs_measure()
        } else {
            node.descendant_needs_layout()
        };
        if (self.layout || self.measure) && !marked {
            node.mark_descendant_needs_layout(self.measure);
        }
        if self.semantics && !node.needs_semantics() {
            node.mark_descendant_needs_semantics();
        }
    }
}

pub(crate) enum Command {
    BubbleDirty {
        node_id: NodeId,
        bubble: DirtyBubble,
    },
    RemoveNode {
        id: NodeId,
    },
    MountNode {
        id: NodeId,
    },
    AttachChild {
        parent_id: NodeId,
        child_id: NodeId,
        insert_index: Option<usize>,
        bubble: DirtyBubble,
    },
    InsertChild {
        parent_id: NodeId,
        child_id: NodeId,
        appended_index: usize,
        insert_index: usize,
        bubble: DirtyBubble,
    },
    MoveChild {
        parent_id: NodeId,
        from_index: usize,
        to_index: usize,
        bubble: DirtyBubble,
    },
    RemoveChild {
        parent_id: NodeId,
        child_id: NodeId,
    },
    DetachChild {
        parent_id: NodeId,
        child_id: NodeId,
    },
    /// Makes `parent_id`'s children `expected_children`; `None` when the pass
    /// composed the children the parent held before it, in the same order.
    SyncChildren {
        parent_id: NodeId,
        expected_children: Option<ChildList>,
    },
    Callback(CommandCallback),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeferredChildCleanup {
    pub(crate) child_id: NodeId,
    pub(crate) generation: u32,
    pub(crate) removed_from_parent: bool,
}

#[derive(Default)]
pub(crate) struct DeferredChildCleanupQueue {
    pending: Vec<DeferredChildCleanup>,
    preserved: Vec<(NodeId, u32)>,
}

impl DeferredChildCleanupQueue {
    pub(crate) fn push(&mut self, cleanup: DeferredChildCleanup) {
        if self
            .preserved
            .iter()
            .any(|&(preserved_id, preserved_generation)| {
                preserved_id == cleanup.child_id && preserved_generation == cleanup.generation
            })
        {
            return;
        }
        self.pending.push(cleanup);
    }

    fn preserve(&mut self, child_id: NodeId, generation: u32) {
        if !self
            .preserved
            .iter()
            .any(|&(preserved_id, preserved_generation)| {
                preserved_id == child_id && preserved_generation == generation
            })
        {
            self.preserved.push((child_id, generation));
        }
        self.pending
            .retain(|cleanup| cleanup.child_id != child_id || cleanup.generation != generation);
    }

    pub(crate) fn flush(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        for cleanup in self.pending.drain(..) {
            cleanup_detached_child(applier, cleanup)?;
        }
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.preserved.clear();
    }
}

impl Command {
    pub(crate) fn callback(
        callback: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
    ) -> Self {
        Self::Callback(Box::new(callback))
    }

    pub(crate) fn apply(self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        let mut deferred_cleanup = DeferredChildCleanupQueue::default();
        self.apply_with_cleanup(applier, &mut deferred_cleanup)?;
        deferred_cleanup.flush(applier)
    }

    pub(crate) fn apply_with_cleanup(
        self,
        applier: &mut dyn Applier,
        deferred_cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match self {
            Self::BubbleDirty { node_id, bubble } => {
                bubble.apply(applier, node_id);
                Ok(())
            }
            Self::RemoveNode { id } => {
                if let Ok(node) = applier.get_mut(id) {
                    node.unmount();
                }
                match applier.remove(id) {
                    Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
                    Err(err) => Err(err),
                }
            }
            Self::MountNode { id } => {
                let node = match applier.get_mut(id) {
                    Ok(node) => node,
                    Err(NodeError::Missing { .. }) => return Ok(()),
                    Err(err) => return Err(err),
                };
                node.set_node_id(id);
                node.mount();
                Ok(())
            }
            Self::AttachChild {
                parent_id,
                child_id,
                insert_index,
                bubble,
            } => {
                attach_child_at(applier, parent_id, child_id, insert_index, bubble);
                Ok(())
            }
            Self::InsertChild {
                parent_id,
                child_id,
                appended_index,
                insert_index,
                bubble,
            } => {
                insert_child_with_reparenting(applier, parent_id, child_id);
                bubble.apply(applier, parent_id);
                if insert_index != appended_index
                    && let Ok(parent_node) = applier.get_mut(parent_id)
                {
                    parent_node.move_child(appended_index, insert_index);
                }
                Ok(())
            }
            Self::MoveChild {
                parent_id,
                from_index,
                to_index,
                bubble,
            } => {
                if let Ok(parent_node) = applier.get_mut(parent_id) {
                    parent_node.move_child(from_index, to_index);
                }
                bubble.apply(applier, parent_id);
                note_structural_move(parent_id, from_index, to_index);
                applier.record_structural_change(parent_id);
                Ok(())
            }
            Self::RemoveChild {
                parent_id,
                child_id,
            } => apply_remove_child(applier, parent_id, child_id, deferred_cleanup),
            Self::DetachChild {
                parent_id,
                child_id,
            } => {
                let generation = applier.node_generation(child_id);
                detach_child_from_parent(applier, parent_id, child_id)?;
                deferred_cleanup.preserve(child_id, generation);
                Ok(())
            }
            Self::SyncChildren {
                parent_id,
                expected_children,
            } => sync_or_settle_children(
                applier,
                parent_id,
                expected_children.as_deref(),
                deferred_cleanup,
            ),
            Self::Callback(callback) => callback(applier),
        }
    }
}
