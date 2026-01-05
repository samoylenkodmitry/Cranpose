use std::cell::{Cell, Ref, RefCell, RefMut};
use std::rc::Rc;

use compose_core::{
    Composer, NodeError, NodeId, Phase, SlotBackend, SlotId, SlotsHost, SubcomposeState,
};
use indexmap::IndexSet;

use crate::modifier::{Modifier, ModifierChainHandle, Point, ResolvedModifiers, Size};
use compose_foundation::{InvalidationKind, ModifierInvalidation, NodeCapabilities};

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
        self.measured_size.unwrap_or(Size {
            width: 0.0,
            height: 0.0,
        })
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

    /// Checks if a node has no parent (is a root node).
    /// Used to filter subcompose results to only include true root nodes.
    fn node_has_no_parent(&self, node_id: NodeId) -> bool;
}

/// Concrete implementation of [`SubcomposeMeasureScope`].
pub struct SubcomposeMeasureScopeImpl<'a> {
    composer: Composer,
    state: &'a mut SubcomposeState,
    constraints: Constraints,
    measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
    error: Rc<RefCell<Option<NodeError>>>,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    pub fn new(
        composer: Composer,
        state: &'a mut SubcomposeState,
        constraints: Constraints,
        measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
        error: Rc<RefCell<Option<NodeError>>>,
    ) -> Self {
        Self {
            composer,
            state,
            constraints,
            measurer,
            error,
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
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
        if self.error.borrow().is_some() {
            return SubcomposePlaceable::new(child.node_id, Size::default());
        }

        if let Err(err) = self.composer.apply_pending_commands() {
            self.record_error(err);
            return SubcomposePlaceable::new(child.node_id, Size::default());
        }

        let size = (self.measurer)(child.node_id, constraints);
        SubcomposePlaceable::new(child.node_id, size)
    }

    fn node_has_no_parent(&self, node_id: NodeId) -> bool {
        self.composer.node_has_no_parent(node_id)
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

    /// Returns the number of active slots in the subcompose state.
    ///
    /// Used by lazy layouts to report statistics about slot usage.
    pub fn active_slots_count(&self) -> usize {
        self.state.active_slots_count()
    }

    /// Returns the number of reusable slots in the pool.
    ///
    /// Used by lazy layouts to report statistics about cached slots.
    pub fn reusable_slots_count(&self) -> usize {
        self.state.reusable_slots_count()
    }

    /// Registers the content type for a slot.
    ///
    /// Call this before `subcompose()` to enable content-type-aware slot reuse.
    /// If the policy supports content types (like `ContentTypeReusePolicy`),
    /// slots with matching content types can reuse each other's nodes.
    pub fn register_content_type(&mut self, slot_id: SlotId, content_type: u64) {
        self.state.register_content_type(slot_id, content_type);
    }

    /// Updates the content type for a slot, handling Some→None transitions.
    ///
    /// If `content_type` is `Some(type)`, registers the type for the slot.
    /// If `content_type` is `None`, removes any previously registered type.
    /// This ensures stale types don't drive incorrect reuse after transitions.
    pub fn update_content_type(&mut self, slot_id: SlotId, content_type: Option<u64>) {
        self.state.update_content_type(slot_id, content_type);
    }

    /// Returns whether the last subcomposed slot was reused.
    ///
    /// Returns `Some(true)` if the slot already existed (was reused from pool or
    /// was recomposed), `Some(false)` if it was newly created, or `None` if no
    /// slot has been subcomposed yet this pass.
    ///
    /// This is useful for tracking composition statistics in lazy layouts.
    pub fn was_last_slot_reused(&self) -> Option<bool> {
        self.state.was_last_slot_reused()
    }
}

/// Trait object representing a reusable measure policy.
pub type MeasurePolicy =
    dyn for<'scope> Fn(&mut SubcomposeMeasureScopeImpl<'scope>, Constraints) -> MeasureResult;

/// Node responsible for orchestrating measure-time subcomposition.
pub struct SubcomposeLayoutNode {
    inner: Rc<RefCell<SubcomposeLayoutNodeInner>>,
    /// Parent tracking for dirty flag bubbling (P0.2 fix)
    parent: Cell<Option<NodeId>>,
    /// Node's own ID
    id: Cell<Option<NodeId>>,
    // Dirty flags for selective measure/layout/render
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    needs_semantics: Cell<bool>,
    needs_redraw: Cell<bool>,
    needs_pointer_pass: Cell<bool>,
    needs_focus_sync: Cell<bool>,
}

impl SubcomposeLayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<MeasurePolicy>) -> Self {
        let inner = Rc::new(RefCell::new(SubcomposeLayoutNodeInner::new(measure_policy)));
        let node = Self {
            inner,
            parent: Cell::new(None),
            id: Cell::new(None),
            needs_measure: Cell::new(true),
            needs_layout: Cell::new(true),
            needs_semantics: Cell::new(true),
            needs_redraw: Cell::new(true),
            needs_pointer_pass: Cell::new(false),
            needs_focus_sync: Cell::new(false),
        };
        // Set modifier and dispatch invalidations after borrow is released
        // Pass empty prev_caps since this is initial construction
        let (invalidations, _) = node.inner.borrow_mut().set_modifier_collect(modifier);
        node.dispatch_modifier_invalidations(&invalidations, NodeCapabilities::empty());
        node
    }

    /// Creates a SubcomposeLayoutNode with ContentTypeReusePolicy.
    ///
    /// Use this for lazy lists to enable content-type-aware slot reuse.
    /// Slots with matching content types can reuse each other's nodes,
    /// improving efficiency when scrolling through items with different types.
    pub fn with_content_type_policy(modifier: Modifier, measure_policy: Rc<MeasurePolicy>) -> Self {
        let mut inner_data = SubcomposeLayoutNodeInner::new(measure_policy);
        inner_data
            .state
            .set_policy(Box::new(compose_core::ContentTypeReusePolicy::new()));
        let inner = Rc::new(RefCell::new(inner_data));
        let node = Self {
            inner,
            parent: Cell::new(None),
            id: Cell::new(None),
            needs_measure: Cell::new(true),
            needs_layout: Cell::new(true),
            needs_semantics: Cell::new(true),
            needs_redraw: Cell::new(true),
            needs_pointer_pass: Cell::new(false),
            needs_focus_sync: Cell::new(false),
        };
        // Set modifier and dispatch invalidations after borrow is released
        // Pass empty prev_caps since this is initial construction
        let (invalidations, _) = node.inner.borrow_mut().set_modifier_collect(modifier);
        node.dispatch_modifier_invalidations(&invalidations, NodeCapabilities::empty());
        node
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
        // Capture capabilities BEFORE updating to detect removed modifiers
        let prev_caps = self.modifier_capabilities();
        // Collect invalidations while inner is borrowed, then dispatch after release
        let (invalidations, modifier_changed) = {
            let mut inner = self.inner.borrow_mut();
            inner.set_modifier_collect(modifier)
        };
        // Now dispatch invalidations after the borrow is released
        // Pass both prev and curr caps so removed modifiers still trigger invalidation
        self.dispatch_modifier_invalidations(&invalidations, prev_caps);
        if modifier_changed {
            self.mark_needs_measure();
            self.request_semantics_update();
        }
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

    /// Mark this node as needing measure. Also marks it as needing layout.
    pub fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
        self.needs_layout.set(true);
    }

    /// Mark this node as needing layout (but not necessarily measure).
    pub fn mark_needs_layout_flag(&self) {
        self.needs_layout.set(true);
    }

    /// Mark this node as needing redraw without forcing measure/layout.
    pub fn mark_needs_redraw(&self) {
        let already_dirty = self.needs_redraw.replace(true);
        if !already_dirty {
            crate::request_render_invalidation();
        }
    }

    /// Check if this node needs measure.
    pub fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    /// Mark this node as needing semantics recomputation.
    pub fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    /// Returns true when semantics need to be recomputed.
    pub fn needs_semantics_flag(&self) -> bool {
        self.needs_semantics.get()
    }

    /// Returns true when this node requested a redraw since the last render pass.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    /// Marks this node as needing a fresh pointer-input pass.
    pub fn mark_needs_pointer_pass(&self) {
        self.needs_pointer_pass.set(true);
    }

    /// Returns true when pointer-input state needs to be recomputed.
    pub fn needs_pointer_pass(&self) -> bool {
        self.needs_pointer_pass.get()
    }

    /// Clears the pointer-input dirty flag after hosts service it.
    pub fn clear_needs_pointer_pass(&self) {
        self.needs_pointer_pass.set(false);
    }

    /// Marks this node as needing a focus synchronization.
    pub fn mark_needs_focus_sync(&self) {
        self.needs_focus_sync.set(true);
    }

    /// Returns true when focus state needs to be synchronized.
    pub fn needs_focus_sync(&self) -> bool {
        self.needs_focus_sync.get()
    }

    /// Clears the focus dirty flag after the focus manager processes it.
    pub fn clear_needs_focus_sync(&self) {
        self.needs_focus_sync.set(false);
    }

    fn request_semantics_update(&self) {
        let already_dirty = self.needs_semantics.replace(true);
        if already_dirty {
            return;
        }

        if let Some(id) = self.id.get() {
            compose_core::queue_semantics_invalidation(id);
        }
    }

    /// Returns the modifier capabilities for this node.
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

    pub fn has_focus_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::FOCUS)
    }

    /// Dispatches modifier invalidations to the appropriate subsystems.
    ///
    /// `prev_caps` contains the capabilities BEFORE the modifier update.
    /// Invalidations are dispatched if EITHER the previous OR current capabilities
    /// include the relevant type. This ensures that removing the last modifier
    /// of a type still triggers proper invalidation.
    fn dispatch_modifier_invalidations(
        &self,
        invalidations: &[ModifierInvalidation],
        prev_caps: NodeCapabilities,
    ) {
        let curr_caps = self.modifier_capabilities();
        for invalidation in invalidations {
            match invalidation.kind() {
                InvalidationKind::Layout => {
                    if curr_caps.contains(NodeCapabilities::LAYOUT)
                        || prev_caps.contains(NodeCapabilities::LAYOUT)
                    {
                        self.mark_needs_measure();
                    }
                }
                InvalidationKind::Draw => {
                    if curr_caps.contains(NodeCapabilities::DRAW)
                        || prev_caps.contains(NodeCapabilities::DRAW)
                    {
                        self.mark_needs_redraw();
                    }
                }
                InvalidationKind::PointerInput => {
                    if curr_caps.contains(NodeCapabilities::POINTER_INPUT)
                        || prev_caps.contains(NodeCapabilities::POINTER_INPUT)
                    {
                        self.mark_needs_pointer_pass();
                        crate::request_pointer_invalidation();
                        // Schedule pointer repass for this node
                        if let Some(id) = self.id.get() {
                            crate::schedule_pointer_repass(id);
                        }
                    }
                }
                InvalidationKind::Semantics => {
                    self.request_semantics_update();
                }
                InvalidationKind::Focus => {
                    if curr_caps.contains(NodeCapabilities::FOCUS)
                        || prev_caps.contains(NodeCapabilities::FOCUS)
                    {
                        self.mark_needs_focus_sync();
                        crate::request_focus_invalidation();
                        // Schedule focus invalidation for this node
                        if let Some(id) = self.id.get() {
                            crate::schedule_focus_invalidation(id);
                        }
                    }
                }
            }
        }
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

    fn set_node_id(&mut self, id: NodeId) {
        self.id.set(Some(id));
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent.set(Some(parent));
    }

    fn on_removed_from_parent(&mut self) {
        self.parent.set(None);
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        self.needs_semantics.get()
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

    pub fn has_focus_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::FOCUS)
    }

    pub fn set_debug_modifiers(&self, enabled: bool) {
        self.inner.borrow_mut().set_debug_modifiers(enabled);
    }

    pub fn measure<'a>(
        &self,
        composer: &Composer,
        _node_id: NodeId,
        constraints: Constraints,
        measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
        error: Rc<RefCell<Option<NodeError>>>,
    ) -> Result<MeasureResult, NodeError> {
        let (policy, mut state, slots) = {
            let mut inner = self.inner.borrow_mut();
            let policy = Rc::clone(&inner.measure_policy);
            let state = std::mem::take(&mut inner.state);
            let slots = std::mem::take(&mut inner.slots);
            (policy, state, slots)
        };
        state.begin_pass();

        let previous = composer.phase();
        if !matches!(previous, Phase::Measure | Phase::Layout) {
            composer.enter_phase(Phase::Measure);
        }

        let slots_host = Rc::new(SlotsHost::new(slots));
        let constraints_copy = constraints;
        // Use subcompose_slot instead of subcompose_in to preserve slot table across
        // measurement passes. This prevents lazy list item groups from being wiped and
        // recreated on every scroll, which caused thrashing.
        // TODO: validate this architecture with JC kotlin codebase
        let result = composer.subcompose_slot(&slots_host, |inner_composer| {
            let mut scope = SubcomposeMeasureScopeImpl::new(
                inner_composer.clone(),
                &mut state,
                constraints_copy,
                measurer,
                Rc::clone(&error),
            );
            (policy)(&mut scope, constraints_copy)
        })?;

        state.finish_pass();

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

    /// Updates the modifier and collects invalidations without dispatching them.
    /// Returns the invalidations and whether the modifier changed.
    fn set_modifier_collect(&mut self, modifier: Modifier) -> (Vec<ModifierInvalidation>, bool) {
        let modifier_changed = self.modifier != modifier;
        self.modifier = modifier;
        self.modifier_chain.set_debug_logging(self.debug_modifiers);
        let modifier_local_invalidations = self.modifier_chain.update(&self.modifier);
        self.resolved_modifiers = self.modifier_chain.resolved_modifiers();
        self.modifier_capabilities = self.modifier_chain.capabilities();

        // Collect invalidations from modifier chain updates
        let mut invalidations = self.modifier_chain.take_invalidations();
        invalidations.extend(modifier_local_invalidations);

        (invalidations, modifier_changed)
    }

    fn set_debug_modifiers(&mut self, enabled: bool) {
        self.debug_modifiers = enabled;
        self.modifier_chain.set_debug_logging(enabled);
    }
}

#[cfg(test)]
#[path = "tests/subcompose_layout_tests.rs"]
mod tests;
