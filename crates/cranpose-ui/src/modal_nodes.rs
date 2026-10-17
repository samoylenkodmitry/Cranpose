//! The nodes that can take a window over, counted while an accessibility
//! client needs the modal walk, so the shell skips the walk while there are
//! none.
//!
//! Modality comes from the generic `.semantics` closure, which backs almost
//! every interactive node, so no node type can be ruled out without running
//! its closure. The count runs closures only for nodes whose semantics may
//! have changed, and only outside composition, where a closure's state reads
//! subscribe nothing.

use std::cell::{Cell, RefCell};

use cranpose_core::{Applier, MemoryApplier, Node, NodeId};
use cranpose_foundation::SemanticsReach;
use smallvec::SmallVec;

use crate::{subcompose_layout::SubcomposeLayoutNode, widgets::nodes::LayoutNode};

/// One app context's modal nodes.
#[derive(Default)]
pub(crate) struct ModalNodes {
    counting: Cell<bool>,
    /// Nodes whose reach was modal when last read.
    modal: RefCell<SmallVec<[NodeId; 2]>>,
    /// Nodes whose reach may have changed since the count was settled.
    changed: RefCell<Vec<NodeId>>,
}

impl ModalNodes {
    /// [`crate::render_state::AppContext::modal_node_may_be_live`] of the
    /// context that holds these nodes.
    pub(crate) fn may_be_live(&self, applier: &mut MemoryApplier, count: bool) -> bool {
        if !count {
            self.set_counting(false);
            return true;
        }
        if self.counting.get() {
            self.settle(applier);
        } else {
            self.set_counting(true);
            applier.for_each_node_mut(|node| {
                if let Some((id, reach)) = node_reach(node) {
                    self.mark(id, reach.is_modal);
                }
            });
        }
        !self.modal.borrow().is_empty()
    }

    fn set_counting(&self, counting: bool) {
        if self.counting.replace(counting) == counting {
            return;
        }
        crate::thread_state::cells(|cells| {
            let contexts = &cells.modal_counting_contexts;
            contexts.set(if counting {
                contexts.get() + 1
            } else {
                contexts.get().saturating_sub(1)
            });
        });
        self.modal.borrow_mut().clear();
        self.changed.borrow_mut().clear();
    }

    fn mark(&self, id: NodeId, modal: bool) {
        let mut ids = self.modal.borrow_mut();
        match (modal, ids.iter().position(|&known| known == id)) {
            (true, None) => ids.push(id),
            (false, Some(index)) => {
                ids.swap_remove(index);
            }
            _ => {}
        }
    }

    /// Reads the reach of every node queued since the last settle, and drops
    /// the modal nodes that left the applier. No borrow is held while a
    /// closure runs.
    fn settle(&self, applier: &mut MemoryApplier) {
        loop {
            let Some(id) = self.changed.borrow_mut().pop() else {
                break;
            };
            self.mark(id, modal_at(applier, id));
        }
        let mut index = 0;
        loop {
            let Some(id) = self.modal.borrow().get(index).copied() else {
                break;
            };
            if modal_at(applier, id) {
                index += 1;
            } else {
                self.modal.borrow_mut().swap_remove(index);
            }
        }
    }
}

/// Whether the node `applier` holds under `id` is a modal layout node.
fn modal_at(applier: &mut MemoryApplier, id: NodeId) -> bool {
    applier
        .get_mut(id)
        .ok()
        .and_then(node_reach)
        .is_some_and(|(node_id, reach)| node_id == id && reach.is_modal)
}

impl Drop for ModalNodes {
    fn drop(&mut self) {
        self.set_counting(false);
    }
}

/// A layout node's id and reach, or `None` for any other node.
fn node_reach(node: &mut dyn Node) -> Option<(NodeId, SemanticsReach)> {
    let node = node.as_any_mut();
    if let Some(layout) = node.downcast_ref::<LayoutNode>() {
        return Some((layout.node_id()?, layout.semantics_reach()));
    }
    let subcompose = node.downcast_ref::<SubcomposeLayoutNode>()?;
    Some((subcompose.id()?, subcompose.semantics_reach()))
}

/// Queues `node` for the modal count after its semantics may have changed:
/// its modifiers synced, it was marked for semantics, or it was mounted.
pub(crate) fn reach_changed(node: Option<NodeId>) {
    let Some(node) = node else {
        return;
    };
    if crate::thread_state::cells(|cells| cells.modal_counting_contexts.get()) == 0 {
        return;
    }
    let _ = crate::render_state::with_current_modal_nodes(|nodes| {
        if nodes.counting.get() {
            nodes.changed.borrow_mut().push(node);
        }
    });
}

#[cfg(test)]
#[path = "tests/modal_nodes_tests.rs"]
mod tests;
