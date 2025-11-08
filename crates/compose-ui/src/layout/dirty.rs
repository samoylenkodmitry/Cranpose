use compose_core::NodeId;
use std::cell::RefCell;
use std::collections::HashMap;

use super::MeasuredNode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirtyPhase(u8);

impl DirtyPhase {
    pub(crate) const MEASURE: Self = Self(1 << 0);

    fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Default)]
struct LayoutDirtyState {
    parents: HashMap<NodeId, Option<NodeId>>,
    dirty: HashMap<NodeId, DirtyPhase>,
}

impl LayoutDirtyState {
    fn mark_dirty(&mut self, node_id: NodeId, phases: DirtyPhase) {
        let mut current = Some(node_id);
        while let Some(id) = current {
            self.dirty
                .entry(id)
                .and_modify(|flags| flags.insert(phases))
                .or_insert(phases);
            current = self.parents.get(&id).and_then(|parent| *parent);
        }
    }

    fn mark_clean(&mut self, node_id: NodeId, phases: DirtyPhase) {
        if let Some(flags) = self.dirty.get_mut(&node_id) {
            flags.remove(phases);
            if flags.is_empty() {
                self.dirty.remove(&node_id);
            }
        }
    }

    fn is_dirty(&self, node_id: NodeId, phases: DirtyPhase) -> bool {
        self.dirty
            .get(&node_id)
            .map(|flags| flags.contains(phases))
            .unwrap_or(false)
    }

    fn has_dirty(&self, phases: DirtyPhase) -> bool {
        self.dirty.values().any(|flags| flags.contains(phases))
    }

    fn rebuild_parents(&mut self, root: &MeasuredNode) {
        fn walk(
            node: &MeasuredNode,
            parent: Option<NodeId>,
            map: &mut HashMap<NodeId, Option<NodeId>>,
        ) {
            map.insert(node.node_id, parent);
            for child in node.children.iter() {
                walk(&child.node, Some(node.node_id), map);
            }
        }

        let mut new_map = HashMap::new();
        walk(root, None, &mut new_map);
        self.parents = new_map;
        self.dirty
            .retain(|node_id, _| self.parents.contains_key(node_id));
    }
}

thread_local! {
    static LAYOUT_DIRTY_STATE: RefCell<LayoutDirtyState> = RefCell::new(LayoutDirtyState::default());
}

fn with_state<R>(f: impl FnOnce(&mut LayoutDirtyState) -> R) -> R {
    LAYOUT_DIRTY_STATE.with(|state| {
        let mut state = state.borrow_mut();
        f(&mut state)
    })
}

pub(crate) fn mark_dirty(node_id: NodeId, phases: DirtyPhase) {
    with_state(|state| state.mark_dirty(node_id, phases));
}

pub(crate) fn mark_clean(node_id: NodeId, phases: DirtyPhase) {
    with_state(|state| state.mark_clean(node_id, phases));
}

pub(crate) fn is_dirty(node_id: NodeId, phases: DirtyPhase) -> bool {
    LAYOUT_DIRTY_STATE.with(|state| state.borrow().is_dirty(node_id, phases))
}

pub(crate) fn has_dirty(phases: DirtyPhase) -> bool {
    LAYOUT_DIRTY_STATE.with(|state| state.borrow().has_dirty(phases))
}

pub(crate) fn rebuild_parent_links(root: &MeasuredNode) {
    with_state(|state| state.rebuild_parents(root));
}
