use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use compose_core::{
    Composer, NodeError, NodeId, Phase, SlotBackend, SlotId, SlotsHost, SubcomposeState,
};
use indexmap::IndexSet;

use crate::modifier::{Modifier, ModifierChainHandle, Point, ResolvedModifiers, Size};
use compose_foundation::NodeCapabilities;

pub use compose_ui_layout::{Constraints, MeasureResult, Placement};

/// Representation of a subcomposed child that can later be measured by the policy.
///
/// In lazy layouts, this represents an item that has been composed but may or
/// may not have been measured yet. Call `measure()` to get the actual size.
#[derive(Clone, Copy, Debug)]
pub struct SubcomposeChild {
    node_id: NodeId,
    /// Measured size of the child (set after measurement).
    /// Width in x, height in y.
    measured_size: Option<Size>,
}

impl SubcomposeChild {
    pub fn new(node_id: NodeId) -> Self {
        Self { 
            node_id,
            measured_size: None,
        }
    }

    /// Creates a SubcomposeChild with a known size.
    pub fn with_size(node_id: NodeId, size: Size) -> Self {
        Self {
            node_id,
            measured_size: Some(size),
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns the measured size of this child.
    /// 
    /// Returns a default size if the child hasn't been measured yet.
    /// For lazy layouts using placeholder sizes, this returns the estimated size.
    pub fn size(&self) -> Size {
        self.measured_size.unwrap_or(Size { width: 0.0, height: 0.0 })
    }

    /// Returns the measured width.
    pub fn width(&self) -> f32 {
        self.size().width
    }

    /// Returns the measured height.
    pub fn height(&self) -> f32 {
        self.size().height
    }

    /// Sets the measured size for this child.
    pub fn set_size(&mut self, size: Size) {
        self.measured_size = Some(size);
    }
}

impl PartialEq for SubcomposeChild {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}

/// A measured child that is ready to be placed.
#[derive(Clone, Copy, Debug)]
pub struct SubcomposePlaceable {
    node_id: NodeId,
    size: Size,
}

impl SubcomposePlaceable {
    pub fn new(node_id: NodeId, size: Size) -> Self {
        Self { node_id, size }
    }
}

impl compose_ui_layout::Placeable for SubcomposePlaceable {
    fn place(&self, _x: f32, _y: f32) {
        // No-op: in SubcomposeLayout, placement is handled by returning a list of Placements
    }

    fn width(&self) -> f32 {
        self.size.width
    }

    fn height(&self) -> f32 {
        self.size.height
    }

    fn node_id(&self) -> NodeId {
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

    /// Measures a subcomposed child with the given constraints.
    fn measure(&mut self, child: SubcomposeChild, constraints: Constraints) -> SubcomposePlaceable;
}

/// Concrete implementation of [`SubcomposeMeasureScope`].
pub struct SubcomposeMeasureScopeImpl<'a> {
    composer: Composer,
    state: &'a mut SubcomposeState,
    constraints: Constraints,
    measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    pub fn new(
        composer: Composer,
        state: &'a mut SubcomposeState,
        constraints: Constraints,
        measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
    ) -> Self {
        Self {
            composer,
            state,
            constraints,
            measurer,
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

    fn measure(&mut self, child: SubcomposeChild, constraints: Constraints) -> SubcomposePlaceable {
        let size = (self.measurer)(child.node_id, constraints);
        SubcomposePlaceable::new(child.node_id, size)
    }
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    /// Subcomposes content and assigns estimated sizes to children.
    ///
    /// This is used by lazy layouts where true measurement happens later.
    /// The `estimate_size` function provides size estimates based on index.
    pub fn subcompose_with_size<Content, F>(
        &mut self, 
        slot_id: SlotId, 
        content: Content,
        estimate_size: F,
    ) -> Vec<SubcomposeChild>
    where
        Content: FnOnce(),
        F: Fn(usize) -> Size,
    {
        let (_, nodes) = self
            .composer
            .subcompose_measurement(self.state, slot_id, |_| content());
        nodes
            .into_iter()
            .enumerate()
            .map(|(i, node_id)| SubcomposeChild::with_size(node_id, estimate_size(i)))
            .collect()
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

    pub fn set_debug_modifiers(&mut self, enabled: bool) {
        self.inner.borrow_mut().set_debug_modifiers(enabled);
    }

    pub fn modifier(&self) -> Modifier {
        self.handle().modifier()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.inner.borrow().resolved_modifiers
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
        self.resolved_modifiers().layout_properties()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.inner.borrow().resolved_modifiers
    }

    pub fn total_offset(&self) -> Point {
        self.resolved_modifiers().offset()
    }

    pub fn modifier_capabilities(&self) -> NodeCapabilities {
        self.inner.borrow().modifier_capabilities
    }

    pub fn has_layout_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::DRAW)
    }

    pub fn has_pointer_input_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::POINTER_INPUT)
    }

    pub fn has_semantics_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::SEMANTICS)
    }

    pub fn set_debug_modifiers(&self, enabled: bool) {
        self.inner.borrow_mut().set_debug_modifiers(enabled);
    }

    pub fn measure<'a>(
        &self,
        composer: &Composer,
        node_id: NodeId,
        constraints: Constraints,
        measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
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
                measurer,
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
    modifier_chain: ModifierChainHandle,
    resolved_modifiers: ResolvedModifiers,
    modifier_capabilities: NodeCapabilities,
    state: SubcomposeState,
    measure_policy: Rc<MeasurePolicy>,
    children: IndexSet<NodeId>,
    slots: SlotBackend,
    debug_modifiers: bool,
}

impl SubcomposeLayoutNodeInner {
    fn new(measure_policy: Rc<MeasurePolicy>) -> Self {
        Self {
            modifier: Modifier::empty(),
            modifier_chain: ModifierChainHandle::new(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_capabilities: NodeCapabilities::default(),
            state: SubcomposeState::default(),
            measure_policy,
            children: IndexSet::new(),
            slots: SlotBackend::default(),
            debug_modifiers: false,
        }
    }

    fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.measure_policy = policy;
    }

    fn set_modifier(&mut self, modifier: Modifier) {
        self.modifier = modifier;
        self.modifier_chain.set_debug_logging(self.debug_modifiers);
        let modifier_local_invalidations = self.modifier_chain.update(&self.modifier);
        self.resolved_modifiers = self.modifier_chain.resolved_modifiers();
        self.modifier_capabilities = self.modifier_chain.capabilities();
        // Drain invalidations for now; subcompose nodes will route them when subsystems are ready.
        let _ = self.modifier_chain.take_invalidations();
        debug_assert!(
            modifier_local_invalidations.is_empty(),
            "subcompose layout nodes currently ignore modifier local invalidations"
        );
    }

    fn set_debug_modifiers(&mut self, enabled: bool) {
        self.debug_modifiers = enabled;
        self.modifier_chain.set_debug_logging(enabled);
    }
}

#[cfg(test)]
#[path = "tests/subcompose_layout_tests.rs"]
mod tests;
