use crate::{layout::MeasuredNode, modifier::Modifier};
use compose_core::{Node, NodeId};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use compose_ui_layout::{Constraints, MeasurePolicy};
use indexmap::IndexSet;
use std::cell::Cell;
use std::hash::Hash;
use std::{cell::RefCell, hash::Hasher, rc::Rc};

#[derive(Clone)]
struct MeasurementCacheEntry {
    constraints: Constraints,
    measured: Rc<MeasuredNode>,
}

#[derive(Clone, Copy, Debug)]
pub enum IntrinsicKind {
    MinWidth(f32),
    MaxWidth(f32),
    MinHeight(f32),
    MaxHeight(f32),
}

impl IntrinsicKind {
    fn discriminant(&self) -> u8 {
        match self {
            IntrinsicKind::MinWidth(_) => 0,
            IntrinsicKind::MaxWidth(_) => 1,
            IntrinsicKind::MinHeight(_) => 2,
            IntrinsicKind::MaxHeight(_) => 3,
        }
    }

    fn value_bits(&self) -> u32 {
        match self {
            IntrinsicKind::MinWidth(value)
            | IntrinsicKind::MaxWidth(value)
            | IntrinsicKind::MinHeight(value)
            | IntrinsicKind::MaxHeight(value) => value.to_bits(),
        }
    }
}

impl PartialEq for IntrinsicKind {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant() == other.discriminant() && self.value_bits() == other.value_bits()
    }
}

impl Eq for IntrinsicKind {}

impl Hash for IntrinsicKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.discriminant().hash(state);
        self.value_bits().hash(state);
    }
}

#[derive(Default)]
struct NodeCacheState {
    epoch: u64,
    measurements: Vec<MeasurementCacheEntry>,
    intrinsics: Vec<(IntrinsicKind, f32)>,
}

#[derive(Clone, Default)]
pub(crate) struct LayoutNodeCacheHandles {
    state: Rc<RefCell<NodeCacheState>>,
}

impl LayoutNodeCacheHandles {
    pub(crate) fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.measurements.clear();
        state.intrinsics.clear();
        state.epoch = 0;
    }

    pub(crate) fn activate(&self, epoch: u64) {
        let mut state = self.state.borrow_mut();
        if state.epoch != epoch {
            state.measurements.clear();
            state.intrinsics.clear();
            state.epoch = epoch;
        }
    }

    pub(crate) fn get_measurement(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let state = self.state.borrow();
        state
            .measurements
            .iter()
            .find(|entry| entry.constraints == constraints)
            .map(|entry| Rc::clone(&entry.measured))
    }

    pub(crate) fn store_measurement(&self, constraints: Constraints, measured: Rc<MeasuredNode>) {
        let mut state = self.state.borrow_mut();
        if let Some(entry) = state
            .measurements
            .iter_mut()
            .find(|entry| entry.constraints == constraints)
        {
            entry.measured = measured;
        } else {
            state.measurements.push(MeasurementCacheEntry {
                constraints,
                measured,
            });
        }
    }

    pub(crate) fn get_intrinsic(&self, kind: &IntrinsicKind) -> Option<f32> {
        let state = self.state.borrow();
        state
            .intrinsics
            .iter()
            .find(|(stored_kind, _)| stored_kind == kind)
            .map(|(_, value)| *value)
    }

    pub(crate) fn store_intrinsic(&self, kind: IntrinsicKind, value: f32) {
        let mut state = self.state.borrow_mut();
        if let Some((_, existing)) = state
            .intrinsics
            .iter_mut()
            .find(|(stored_kind, _)| stored_kind == &kind)
        {
            *existing = value;
        } else {
            state.intrinsics.push((kind, value));
        }
    }
}

pub struct LayoutNode {
    pub modifier: Modifier,
    pub mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    pub children: IndexSet<NodeId>,
    cache: LayoutNodeCacheHandles,
    // Dirty flags for selective measure/layout/render
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    // Parent tracking for dirty flag bubbling (Jetpack Compose style)
    parent: Cell<Option<NodeId>>,
    // Node's own ID (set by applier after creation)
    id: Cell<Option<NodeId>>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy,
            children: IndexSet::new(),
            cache: LayoutNodeCacheHandles::default(),
            needs_measure: Cell::new(true), // New nodes need initial measure
            needs_layout: Cell::new(true),  // New nodes need initial layout
            parent: Cell::new(None),        // No parent initially
            id: Cell::new(None),            // ID set by applier after creation
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
        self.cache.clear();
        self.mark_needs_measure();
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        self.measure_policy = policy;
        self.cache.clear();
        self.mark_needs_measure();
    }

    /// Mark this node as needing measure. Also marks it as needing layout.
    pub fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
        self.needs_layout.set(true);
    }

    /// Mark this node as needing layout (but not necessarily measure).
    pub fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
    }

    /// Check if this node needs measure.
    pub fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    /// Check if this node needs layout.
    pub fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    /// Clear the measure dirty flag after measuring.
    pub(crate) fn clear_needs_measure(&self) {
        self.needs_measure.set(false);
    }

    /// Clear the layout dirty flag after laying out.
    pub(crate) fn clear_needs_layout(&self) {
        self.needs_layout.set(false);
    }

    /// Set this node's ID (called by applier after creation).
    pub fn set_node_id(&self, id: NodeId) {
        self.id.set(Some(id));
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> Option<NodeId> {
        self.id.get()
    }

    /// Set this node's parent (called when node is added as child).
    pub fn set_parent(&self, parent: NodeId) {
        self.parent.set(Some(parent));
    }

    /// Clear this node's parent (called when node is removed from parent).
    pub fn clear_parent(&self) {
        self.parent.set(None);
    }

    /// Get this node's parent.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache.clone()
    }
}

/// Bubble dirty flags up the parent chain from a LayoutNode.
/// This should be called after marking a node dirty during recomposition.
/// Uses the current composer context to access nodes via with_node_mut.
pub fn bubble_dirty_flags(node_id: compose_core::NodeId) {
    let mut current_id = node_id;
    loop {
        // Get parent of current node
        let parent_id = match compose_core::with_node_mut(current_id, |node: &mut LayoutNode| {
            node.parent()
        }) {
            Ok(Some(pid)) => pid,
            _ => break, // No parent or error - stop bubbling
        };

        // Mark parent as needing layout
        let should_continue = compose_core::with_node_mut(parent_id, |node: &mut LayoutNode| {
            if !node.needs_layout() {
                node.mark_needs_layout();
                true // Continue bubbling
            } else {
                false // Already dirty, stop
            }
        }).unwrap_or(false);

        if should_continue {
            current_id = parent_id;
        } else {
            break;
        }
    }
}

impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        Self {
            modifier: self.modifier.clone(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            measure_policy: self.measure_policy.clone(),
            children: self.children.clone(),
            cache: self.cache.clone(),
            needs_measure: Cell::new(self.needs_measure.get()),
            needs_layout: Cell::new(self.needs_layout.get()),
            parent: Cell::new(self.parent.get()),
            id: Cell::new(self.id.get()),
        }
    }
}

impl Node for LayoutNode {
    fn set_node_id(&mut self, id: NodeId) {
        self.id.set(Some(id));
    }

    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
        self.cache.clear();
        self.mark_needs_measure();
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
        self.cache.clear();
        self.mark_needs_measure();
        // TODO: Clear parent via applier.with_node(child, |n| n.clear_parent())
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let mut ordered: Vec<NodeId> = self.children.iter().copied().collect();
        let child = ordered.remove(from);
        let target = to.min(ordered.len());
        ordered.insert(target, child);
        self.children.clear();
        for id in ordered {
            self.children.insert(id);
        }
        self.cache.clear();
        self.mark_needs_measure();
        // Parent doesn't change when moving within same parent
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
        self.cache.clear();
        self.mark_needs_measure();
        // TODO: Update parent links via applier for all new children
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.set_parent(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.clear_parent();
    }
}
