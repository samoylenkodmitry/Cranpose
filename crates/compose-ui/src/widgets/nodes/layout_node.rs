use crate::{
    layout::{mark_measure_dirty, MeasuredNode},
    modifier::Modifier,
};
use compose_core::{Node, NodeId};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};
use compose_ui_layout::{Constraints, MeasurePolicy};
use indexmap::IndexSet;
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
    id: Option<NodeId>,
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
            id: None,
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_node_id(&mut self, id: NodeId) {
        self.id = Some(id);
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        if self.modifier == modifier {
            return;
        }
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
        self.cache.clear();
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        if Rc::ptr_eq(&self.measure_policy, &policy) {
            return;
        }
        self.measure_policy = policy;
        self.cache.clear();
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache.clone()
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
            id: self.id,
        }
    }
}

impl Node for LayoutNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
        self.cache.clear();
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
        self.cache.clear();
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
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
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
        self.cache.clear();
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}
