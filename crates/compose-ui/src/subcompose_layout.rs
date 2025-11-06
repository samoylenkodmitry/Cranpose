use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use compose_core::{
    Composer, NodeError, NodeId, Phase, SlotBackend, SlotId, SlotsHost, SubcomposeState,
};
use indexmap::IndexSet;

use crate::modifier::{Modifier, Point, Size};
use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

pub use compose_ui_layout::{Constraints, MeasureResult, Placement};

/// Representation of a subcomposed child that can later be measured by the policy.
#[derive(Clone, Debug, PartialEq)]
pub struct SubcomposeChild {
    node_id: NodeId,
}

impl SubcomposeChild {
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Base trait for measurement scopes.
pub trait SubcomposeLayoutScope {
    fn constraints(&self) -> Constraints;

    fn layout<I>(&mut self, width: f32, height: f32, placements: I) -> MeasureResult
    where
        I: IntoIterator<Item = Placement>,
    {
        MeasureResult::new(Size { width, height }, placements.into_iter().collect())
    }
}

/// Public trait exposed to measure policies for subcomposition.
pub trait SubcomposeMeasureScope: SubcomposeLayoutScope {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<SubcomposeChild>
    where
        Content: FnOnce();
}

/// Concrete implementation of [`SubcomposeMeasureScope`].
pub struct SubcomposeMeasureScopeImpl<'a> {
    composer: Composer,
    state: &'a mut SubcomposeState,
    constraints: Constraints,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    pub fn new(
        composer: Composer,
        state: &'a mut SubcomposeState,
        constraints: Constraints,
    ) -> Self {
        Self {
            composer,
            state,
            constraints,
        }
    }
}

impl<'a> SubcomposeLayoutScope for SubcomposeMeasureScopeImpl<'a> {
    fn constraints(&self) -> Constraints {
        self.constraints
    }
}

impl<'a> SubcomposeMeasureScope for SubcomposeMeasureScopeImpl<'a> {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<SubcomposeChild>
    where
        Content: FnOnce(),
    {
        let (_, nodes) = self
            .composer
            .subcompose_measurement(self.state, slot_id, |_| content());
        nodes.into_iter().map(SubcomposeChild::new).collect()
    }
}

/// Trait object representing a reusable measure policy.
pub type MeasurePolicy =
    dyn for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult;

/// Node responsible for orchestrating measure-time subcomposition.
pub struct SubcomposeLayoutNode {
    inner: Rc<RefCell<SubcomposeLayoutNodeInner>>,
}

impl SubcomposeLayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<MeasurePolicy>) -> Self {
        let mut inner = SubcomposeLayoutNodeInner::new(measure_policy);
        inner.set_modifier(modifier);
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    pub fn handle(&self) -> SubcomposeLayoutNodeHandle {
        SubcomposeLayoutNodeHandle {
            inner: Rc::clone(&self.inner),
        }
    }

    pub fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.inner.borrow_mut().set_measure_policy(policy);
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        self.inner.borrow_mut().set_modifier(modifier);
    }

    pub fn modifier(&self) -> Modifier {
        self.handle().modifier()
    }

    pub fn state(&self) -> Ref<'_, SubcomposeState> {
        Ref::map(self.inner.borrow(), |inner| &inner.state)
    }

    pub fn state_mut(&self) -> RefMut<'_, SubcomposeState> {
        RefMut::map(self.inner.borrow_mut(), |inner| &mut inner.state)
    }

    pub fn active_children(&self) -> Vec<NodeId> {
        self.inner.borrow().children.iter().copied().collect()
    }
}

impl compose_core::Node for SubcomposeLayoutNode {
    fn insert_child(&mut self, child: NodeId) {
        self.inner.borrow_mut().children.insert(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.inner.borrow_mut().children.shift_remove(&child);
    }

    fn move_child(&mut self, from: usize, to: usize) {
        let mut inner = self.inner.borrow_mut();
        if from == to || from >= inner.children.len() {
            return;
        }
        let mut ordered: Vec<NodeId> = inner.children.iter().copied().collect();
        let child = ordered.remove(from);
        let target = to.min(ordered.len());
        ordered.insert(target, child);
        inner.children.clear();
        for id in ordered {
            inner.children.insert(id);
        }
    }

    fn update_children(&mut self, children: &[NodeId]) {
        let mut inner = self.inner.borrow_mut();
        inner.children.clear();
        for &child in children {
            inner.children.insert(child);
        }
    }

    fn children(&self) -> Vec<NodeId> {
        self.inner.borrow().children.iter().copied().collect()
    }
}

#[derive(Clone)]
pub struct SubcomposeLayoutNodeHandle {
    inner: Rc<RefCell<SubcomposeLayoutNodeInner>>,
}

impl SubcomposeLayoutNodeHandle {
    pub fn modifier(&self) -> Modifier {
        self.inner.borrow().modifier.clone()
    }

    pub fn layout_properties(&self) -> crate::modifier::LayoutProperties {
        self.inner.borrow().modifier.layout_properties()
    }

    pub fn total_offset(&self) -> Point {
        self.inner.borrow().modifier.total_offset()
    }

    pub fn measure(
        &self,
        composer: &Composer,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<MeasureResult, NodeError> {
        let (policy, mut state, slots) = {
            let mut inner = self.inner.borrow_mut();
            let policy = Rc::clone(&inner.measure_policy);
            let state = std::mem::take(&mut inner.state);
            let slots = std::mem::take(&mut inner.slots);
            (policy, state, slots)
        };

        let previous = composer.phase();
        if !matches!(previous, Phase::Measure | Phase::Layout) {
            composer.enter_phase(Phase::Measure);
        }

        let slots_host = Rc::new(SlotsHost::new(slots));
        let constraints_copy = constraints;
        let result = composer.subcompose_in(&slots_host, Some(node_id), |inner_composer| {
            let mut scope = SubcomposeMeasureScopeImpl::new(
                inner_composer.clone(),
                &mut state,
                constraints_copy,
            );
            (policy)(&mut scope, constraints_copy)
        })?;

        state.dispose_or_reuse_starting_from_index(0);

        if previous != composer.phase() {
            composer.enter_phase(previous);
        }

        {
            let mut inner = self.inner.borrow_mut();
            inner.slots = slots_host.take();
            inner.state = state;
        }

        Ok(result)
    }

    pub fn set_active_children<I>(&self, children: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        let mut inner = self.inner.borrow_mut();
        inner.children.clear();
        for child in children {
            inner.children.insert(child);
        }
    }
}

struct SubcomposeLayoutNodeInner {
    modifier: Modifier,
    mods: ModifierNodeChain,
    modifier_context: BasicModifierNodeContext,
    state: SubcomposeState,
    measure_policy: Rc<MeasurePolicy>,
    children: IndexSet<NodeId>,
    slots: SlotBackend,
}

impl SubcomposeLayoutNodeInner {
    fn new(measure_policy: Rc<MeasurePolicy>) -> Self {
        Self {
            modifier: Modifier::empty(),
            mods: ModifierNodeChain::new(),
            modifier_context: BasicModifierNodeContext::new(),
            state: SubcomposeState::default(),
            measure_policy,
            children: IndexSet::new(),
            slots: SlotBackend::default(),
        }
    }

    fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.measure_policy = policy;
    }

    fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.mods
            .update_from_slice(self.modifier.elements(), &mut self.modifier_context);
    }
}

#[cfg(test)]
#[path = "tests/subcompose_layout_tests.rs"]
mod tests;
