use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;
use web_time::Instant;

use cranpose_core::{
    Composer, NodeError, NodeId, Phase, SlotId, SlotTable, SlotsHost, SubcomposeState,
};

use crate::layout::MeasuredNode;
use crate::modifier::{
    collect_modifier_slices_into, Modifier, ModifierChainHandle, ModifierNodeSlices, Point,
    ResolvedModifiers, Size,
};
use crate::widgets::nodes::{
    allocate_virtual_node_id, is_virtual_node, register_layout_node, LayoutNode,
    LayoutNodeCacheHandles, LayoutState,
};

use cranpose_foundation::{InvalidationKind, ModifierInvalidation, NodeCapabilities};

pub use cranpose_ui_layout::{Constraints, MeasureResult, Placement};

fn subcompose_telemetry_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_SUBCOMPOSE_TELEMETRY")
}

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
///
/// This is a type alias for the concrete `Placeable` struct from `cranpose_ui_layout`.
/// In subcompose layouts, placement is handled by returning a list of `Placement`s,
/// so the `place()` callback is not used.
pub type SubcomposePlaceable = cranpose_ui_layout::Placeable;

type CachedMeasureBatchRegistrar<'a> =
    Box<dyn FnMut(&[NodeId], Constraints, &mut Vec<Option<Size>>) + 'a>;
type RetainedMeasureLookup<'a> = Box<dyn FnMut(NodeId) -> Option<Rc<MeasuredNode>> + 'a>;
type RetainedMeasureRegistrar<'a> = Box<dyn FnMut(&[Rc<MeasuredNode>]) + 'a>;

pub(crate) struct CachedBatchMeasureInputs<'a> {
    pub(crate) measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
    pub(crate) cached_measure_batch_registrar: CachedMeasureBatchRegistrar<'a>,
    pub(crate) retained_measure_lookup: RetainedMeasureLookup<'a>,
    pub(crate) retained_measure_registrar: RetainedMeasureRegistrar<'a>,
    pub(crate) error: &'a RefCell<Option<NodeError>>,
}

/// Base trait for measurement scopes.
pub trait SubcomposeLayoutScope: cranpose_ui_layout::MeasureScope {
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
        Content: FnMut() + 'static;

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
    cached_measure_batch_registrar: CachedMeasureBatchRegistrar<'a>,
    retained_measure_lookup: RetainedMeasureLookup<'a>,
    retained_measure_registrar: RetainedMeasureRegistrar<'a>,
    error: &'a RefCell<Option<NodeError>>,
    parent_handle: SubcomposeLayoutNodeHandle,
    root_id: NodeId,
    placement_scratch: Vec<Placement>,
    cached_measure_node_scratch: Vec<NodeId>,
    cached_measure_size_scratch: Vec<Option<Size>>,
    cached_measure_missing_scratch: Vec<NodeId>,
    registered_measurement_node_ids: Vec<NodeId>,
    pending_commands_applied: bool,
}

struct SubcomposeMeasureScopeInit<'a> {
    composer: Composer,
    state: &'a mut SubcomposeState,
    constraints: Constraints,
    measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
    cached_measure_batch_registrar: CachedMeasureBatchRegistrar<'a>,
    retained_measure_lookup: RetainedMeasureLookup<'a>,
    retained_measure_registrar: RetainedMeasureRegistrar<'a>,
    error: &'a RefCell<Option<NodeError>>,
    parent_handle: SubcomposeLayoutNodeHandle,
    root_id: NodeId,
    placement_scratch: Vec<Placement>,
}

impl<'a> SubcomposeMeasureScopeImpl<'a> {
    fn new(init: SubcomposeMeasureScopeInit<'a>) -> Self {
        Self {
            composer: init.composer,
            state: init.state,
            constraints: init.constraints,
            measurer: init.measurer,
            cached_measure_batch_registrar: init.cached_measure_batch_registrar,
            retained_measure_lookup: init.retained_measure_lookup,
            retained_measure_registrar: init.retained_measure_registrar,
            error: init.error,
            parent_handle: init.parent_handle,
            root_id: init.root_id,
            placement_scratch: init.placement_scratch,
            cached_measure_node_scratch: Vec::new(),
            cached_measure_size_scratch: Vec::new(),
            cached_measure_missing_scratch: Vec::new(),
            registered_measurement_node_ids: Vec::new(),
            pending_commands_applied: false,
        }
    }

    fn register_measurement_node_id(&mut self, node_id: NodeId) {
        if !self.registered_measurement_node_ids.contains(&node_id) {
            self.registered_measurement_node_ids.push(node_id);
        }
    }

    fn into_placement_scratch(self) -> Vec<Placement> {
        self.placement_scratch
    }

    pub(crate) fn layout_with_placement_builder(
        &mut self,
        width: f32,
        height: f32,
        build: impl FnOnce(&mut Vec<Placement>),
    ) -> MeasureResult {
        self.placement_scratch.clear();
        build(&mut self.placement_scratch);
        MeasureResult::new(
            Size { width, height },
            std::mem::take(&mut self.placement_scratch),
        )
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            eprintln!("[SubcomposeLayout] Error suppressed: {:?}", err);
            *slot = Some(err);
        }
    }

    fn ensure_pending_commands_applied(&mut self) -> bool {
        if self.pending_commands_applied {
            return true;
        }

        let telemetry_start = subcompose_telemetry_enabled().then(Instant::now);
        if let Err(err) = self.composer.apply_pending_commands() {
            self.record_error(err);
            return false;
        }
        if let Some(start) = telemetry_start {
            log::warn!(
                "[subcompose-telemetry] apply_pending_commands_ms={:.2}",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }

        self.pending_commands_applied = true;
        true
    }

    fn perform_subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<NodeId>
    where
        Content: FnMut() + 'static,
    {
        let telemetry_start = subcompose_telemetry_enabled().then(Instant::now);
        let mut inner = self.parent_handle.inner.borrow_mut();

        // Reuse or create virtual node
        let (virtual_node_id, is_reused) =
            if let Some(node_id) = self.state.take_node_from_reusables(slot_id) {
                (node_id, true)
            } else {
                let id = allocate_virtual_node_id();
                let node = LayoutNode::new_virtual();
                // CRITICAL FIX: Register virtual node in Applier so that insert_child commands
                // can find it. Previously, virtual nodes were only stored in inner.virtual_nodes
                // which caused applier.get_mut(virtual_node_id) to fail, breaking child attachment.
                if let Err(e) = self
                    .composer
                    .register_virtual_node(id, Box::new(node.clone()))
                {
                    eprintln!(
                        "[Subcompose] Failed to register virtual node {}: {:?}",
                        id, e
                    );
                }
                register_layout_node(id, &node);

                inner.virtual_nodes.insert(id, Rc::new(node));
                inner.children.push(id);
                (id, false)
            };

        // Keep the slot root in the outer parent frame so SyncChildren preserves
        // the virtual subtree instead of deleting it after subcomposition.
        self.composer.record_subcompose_child(virtual_node_id);

        // Keep both the retained virtual-node clone and the applier copy wired to the
        // live subcompose root so bubbling can cross the virtual slot boundary.
        if let Some(v_node) = inner.virtual_nodes.get(&virtual_node_id) {
            v_node.set_parent(self.root_id);
        }

        drop(inner);

        let content_holder = self.state.callback_holder(slot_id);
        content_holder.update(content);

        let _ = self
            .composer
            .with_node_mut::<LayoutNode, _>(virtual_node_id, |node| {
                node.set_parent(self.root_id);
            });

        let slot_host = self.state.get_or_create_slots(slot_id);
        let holder_for_slot = content_holder.clone();
        let scopes = self
            .composer
            .subcompose_slot(&slot_host, Some(virtual_node_id), move |_| {
                compose_subcompose_slot_content(holder_for_slot.clone());
            })
            .map(|(_, scopes)| scopes)
            .unwrap_or_default();
        self.pending_commands_applied = false;

        self.state
            .register_active(slot_id, &[virtual_node_id], &scopes);

        // CRITICAL FIX: Read children from the Applier's copy of the virtual node,
        // NOT from inner.virtual_nodes. The Applier's copy received insert_child calls
        // during subcomposition, while inner.virtual_nodes is an out-of-sync clone.
        let children = self.composer.get_node_children(virtual_node_id).to_vec();
        if let Some(start) = telemetry_start {
            log::warn!(
                "[subcompose-telemetry] slot={} reused={} children={} subcompose_ms={:.2}",
                slot_id.raw(),
                is_reused,
                children.len(),
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        children
    }

    pub(crate) fn activate_exact_retained_slot_with_known_children(
        &mut self,
        slot_id: SlotId,
        known_children: &[u64],
    ) -> Option<(Vec<SubcomposeChild>, bool)> {
        let mut expected_children = Vec::with_capacity(known_children.len());
        for &node_id in known_children {
            expected_children.push(NodeId::try_from(node_id).ok()?);
        }

        let virtual_node_ids = match self.activate_current_active_slot_roots(slot_id) {
            Some(virtual_node_ids) => {
                for virtual_node_id in &virtual_node_ids {
                    self.composer.record_subcompose_child(*virtual_node_id);
                }
                virtual_node_ids
            }
            None => self.activate_recycled_exact_retained_slot_roots(slot_id)?,
        };

        // Always read the LIVE roots out of the applier and compare them against
        // the caller's cached list. Reporting `children_match = true` from the
        // cached ids alone is a lie whenever the slot recomposed and emitted a
        // different set of root children: the caller would reuse the cached
        // measurement, and any newly composed root would never be measured or
        // placed.
        //
        // Flush queued composer commands first so the read sees the tree the
        // last recomposition actually produced. Every caller runs this on the
        // very next statement (`children_need_relayout`), and it is a no-op once
        // applied, so this only moves the flush one statement earlier.
        if !self.ensure_pending_commands_applied() {
            return None;
        }

        let mut activated_children = Vec::with_capacity(expected_children.len());
        for virtual_node_id in virtual_node_ids {
            activated_children.extend(
                self.composer
                    .get_node_children(virtual_node_id)
                    .iter()
                    .copied(),
            );
        }
        let children_match = activated_children == expected_children;
        Some((
            activated_children
                .into_iter()
                .map(SubcomposeChild::new)
                .collect(),
            children_match,
        ))
    }

    fn activate_current_active_slot_roots(&mut self, slot_id: SlotId) -> Option<Vec<NodeId>> {
        self.state.activate_current_active_slot(slot_id)
    }

    fn activate_recycled_exact_retained_slot_roots(
        &mut self,
        slot_id: SlotId,
    ) -> Option<Vec<NodeId>> {
        let activation = self.state.take_exact_slot_activation(slot_id)?;
        let virtual_node_ids = activation.nodes;
        let scopes = activation.scopes;
        let reactivate_scopes = activation.reactivate_scopes;

        if reactivate_scopes {
            let inner = self.parent_handle.inner.borrow();
            for virtual_node_id in &virtual_node_ids {
                self.composer.record_subcompose_child(*virtual_node_id);
                if let Some(v_node) = inner.virtual_nodes.get(virtual_node_id) {
                    v_node.set_parent(self.root_id);
                }
            }
            for virtual_node_id in &virtual_node_ids {
                let _ = self
                    .composer
                    .with_node_mut::<LayoutNode, _>(*virtual_node_id, |node| {
                        node.set_parent(self.root_id);
                    });
            }
        } else {
            for virtual_node_id in &virtual_node_ids {
                self.composer.record_subcompose_child(*virtual_node_id);
            }
        }

        self.state.register_active_with_scope_reactivation(
            slot_id,
            &virtual_node_ids,
            &scopes,
            reactivate_scopes,
        );
        Some(virtual_node_ids)
    }
}

impl<'a> SubcomposeLayoutScope for SubcomposeMeasureScopeImpl<'a> {
    fn constraints(&self) -> Constraints {
        self.constraints
    }

    fn layout<I>(&mut self, width: f32, height: f32, placements: I) -> MeasureResult
    where
        I: IntoIterator<Item = Placement>,
    {
        self.layout_with_placement_builder(width, height, |scratch| {
            scratch.extend(placements);
        })
    }
}

impl cranpose_ui_layout::MeasureScope for SubcomposeMeasureScopeImpl<'_> {
    fn density(&self) -> f32 {
        crate::current_density()
    }

    fn font_scale(&self) -> f32 {
        crate::current_font_scale()
    }
}

impl<'a> SubcomposeMeasureScope for SubcomposeMeasureScopeImpl<'a> {
    fn subcompose<Content>(&mut self, slot_id: SlotId, content: Content) -> Vec<SubcomposeChild>
    where
        Content: FnMut() + 'static,
    {
        let nodes = self.perform_subcompose(slot_id, content);
        nodes.into_iter().map(SubcomposeChild::new).collect()
    }

    fn measure(&mut self, child: SubcomposeChild, constraints: Constraints) -> SubcomposePlaceable {
        if self.error.borrow().is_some() {
            // Already in error state - return zero-size placeable
            return SubcomposePlaceable::value(0.0, 0.0, child.node_id);
        }

        let telemetry_start = subcompose_telemetry_enabled().then(Instant::now);
        if !self.ensure_pending_commands_applied() {
            return SubcomposePlaceable::value(0.0, 0.0, child.node_id);
        }

        let size = (self.measurer)(child.node_id, constraints);
        self.register_measurement_node_id(child.node_id);
        if let Some(start) = telemetry_start {
            log::warn!(
                "[subcompose-telemetry] child={} measure_ms={:.2} size=({:.2},{:.2})",
                child.node_id,
                start.elapsed().as_secs_f64() * 1000.0,
                size.width,
                size.height
            );
        }
        SubcomposePlaceable::value(size.width, size.height, child.node_id)
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
        Content: FnMut() + 'static,
        F: Fn(usize) -> Size,
    {
        let nodes = self.perform_subcompose(slot_id, content);
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

    pub(crate) fn set_reusable_pool_limits(&mut self, per_type: usize, untyped: usize) {
        self.state.set_reusable_pool_limits(per_type, untyped);
    }

    pub(crate) fn recycle_active_slots_where(&mut self, predicate: impl FnMut(SlotId) -> bool) {
        let disposed = self.state.recycle_active_slots_where(predicate);
        debug_assert!(
            disposed.is_empty(),
            "lazy subcompose reusable pool limits must retain recycled active slots"
        );
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

    pub(crate) fn measure_retained(
        &mut self,
        child: SubcomposeChild,
        constraints: Constraints,
    ) -> (SubcomposePlaceable, Option<Rc<MeasuredNode>>) {
        let placeable = self.measure(child, constraints);
        let retained = (self.retained_measure_lookup)(child.node_id);
        (placeable, retained)
    }

    pub(crate) fn register_retained_measurements(&mut self, measurements: &[Rc<MeasuredNode>]) {
        if measurements.is_empty() {
            return;
        }

        for measured in measurements {
            self.register_measurement_node_id(measured.node_id());
        }
        (self.retained_measure_registrar)(measurements);
    }

    /// Whether any slot root child still owes layout work — either a re-measure
    /// (`needs_measure`) or a placement-only repass (`needs_layout`).
    ///
    /// Both must be checked. `schedule_layout_repass` (scroll offsets, text
    /// field panning) bubbles `needs_layout` *without* `needs_measure` on
    /// purpose, so that reaching the root does not bump the global layout cache
    /// epoch and wipe every cached measurement in the app. A caller that only
    /// asked "do you need measuring?" would call such a subtree clean and reuse
    /// the cached measurement, freezing the scroll on screen.
    ///
    /// Stays O(number of slot roots): dirty bubbling already lifted any
    /// descendant's dirtiness onto these roots, so no subtree walk is needed.
    pub(crate) fn children_need_relayout(&mut self, children: &[SubcomposeChild]) -> bool {
        if !self.ensure_pending_commands_applied() {
            return true;
        }

        let mut root_ids = smallvec::SmallVec::<[NodeId; 8]>::new();
        root_ids.extend(children.iter().map(SubcomposeChild::node_id));
        self.composer.nodes_need_measure(&root_ids) || self.composer.nodes_need_layout(&root_ids)
    }

    pub(crate) fn ensure_cached_measurement_node_ids<I>(
        &mut self,
        node_ids: I,
        constraints: Constraints,
    ) -> usize
    where
        I: IntoIterator<Item = NodeId>,
    {
        if self.error.borrow().is_some() || !self.ensure_pending_commands_applied() {
            return 0;
        }

        self.cached_measure_node_scratch.clear();
        self.cached_measure_node_scratch.extend(
            node_ids
                .into_iter()
                .filter(|node_id| !self.registered_measurement_node_ids.contains(node_id)),
        );
        if self.cached_measure_node_scratch.is_empty() {
            return 0;
        }

        self.cached_measure_size_scratch.clear();
        (self.cached_measure_batch_registrar)(
            &self.cached_measure_node_scratch,
            constraints,
            &mut self.cached_measure_size_scratch,
        );
        self.cached_measure_size_scratch
            .resize(self.cached_measure_node_scratch.len(), None);

        let mut cached_count = 0;
        self.cached_measure_missing_scratch.clear();
        for index in 0..self.cached_measure_node_scratch.len() {
            let node_id = self.cached_measure_node_scratch[index];
            if self.cached_measure_size_scratch[index].is_some() {
                cached_count += 1;
                self.register_measurement_node_id(node_id);
            } else {
                self.cached_measure_missing_scratch.push(node_id);
            }
        }

        let mut missing = std::mem::take(&mut self.cached_measure_missing_scratch);
        for node_id in missing.drain(..) {
            let _ = self.measure(SubcomposeChild::new(node_id), constraints);
        }
        self.cached_measure_missing_scratch = missing;

        cached_count
    }
}

fn compose_subcompose_slot_content(holder: cranpose_core::CallbackHolder) {
    cranpose_core::with_current_composer(|composer| {
        let holder_for_recompose = holder.clone();
        composer.set_recranpose_callback(move |_composer| {
            compose_subcompose_slot_content(holder_for_recompose.clone());
        });
    });

    let invoke = holder.clone_rc();
    invoke();
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
    virtual_children_count: Cell<usize>,
    /// Retained layout state (size, position) for rendering.
    layout_state: RefCell<LayoutState>,
    cache_handles: LayoutNodeCacheHandles,
    modifier_slices_snapshot: RefCell<Rc<ModifierNodeSlices>>,
    modifier_slices_dirty: Cell<bool>,
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
            virtual_children_count: Cell::new(0),
            layout_state: RefCell::new(LayoutState::default()),
            cache_handles: LayoutNodeCacheHandles::default(),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            modifier_slices_dirty: Cell::new(true),
        };
        // Set modifier and dispatch invalidations after borrow is released
        // Pass empty prev_caps since this is initial construction
        let (invalidations, _) = node.inner.borrow_mut().set_modifier_collect(modifier);
        node.dispatch_modifier_invalidations(&invalidations, NodeCapabilities::empty());
        node.update_modifier_slices_cache();
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
            .set_policy(Box::new(cranpose_core::ContentTypeReusePolicy::new()));
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
            virtual_children_count: Cell::new(0),
            layout_state: RefCell::new(LayoutState::default()),
            cache_handles: LayoutNodeCacheHandles::default(),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            modifier_slices_dirty: Cell::new(true),
        };
        // Set modifier and dispatch invalidations after borrow is released
        // Pass empty prev_caps since this is initial construction
        let (invalidations, _) = node.inner.borrow_mut().set_modifier_collect(modifier);
        node.dispatch_modifier_invalidations(&invalidations, NodeCapabilities::empty());
        node.update_modifier_slices_cache();
        node
    }

    pub fn handle(&self) -> SubcomposeLayoutNodeHandle {
        SubcomposeLayoutNodeHandle {
            inner: Rc::clone(&self.inner),
        }
    }

    #[doc(hidden)]
    pub fn debug_scope_ids_by_slot(&self) -> Vec<(u64, Vec<usize>)> {
        self.inner.borrow().state.debug_scope_ids_by_slot()
    }

    #[doc(hidden)]
    pub fn debug_slot_table_for_slot(
        &self,
        slot_id: cranpose_core::SlotId,
    ) -> Option<Vec<cranpose_core::SlotDebugEntry>> {
        self.inner.borrow().state.debug_slot_table_for_slot(slot_id)
    }

    #[doc(hidden)]
    pub fn debug_slot_table_groups_for_slot(
        &self,
        slot_id: cranpose_core::SlotId,
    ) -> Option<Vec<cranpose_core::subcompose::DebugSlotGroup>> {
        self.inner
            .borrow()
            .state
            .debug_slot_table_groups_for_slot(slot_id)
    }

    pub fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        let mut inner = self.inner.borrow_mut();
        if Rc::ptr_eq(&inner.measure_policy, &policy) {
            return;
        }
        inner.set_measure_policy(policy);
        drop(inner);
        self.invalidate_subcomposition();
    }

    /// Records the source composition context for measure-time subcomposition.
    pub fn set_captured_context(&mut self, context: cranpose_core::CapturedCompositionContext) {
        self.inner.borrow_mut().captured_context = Some(context);
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
        self.update_modifier_slices_cache();
        if modifier_changed {
            self.request_semantics_update();
        }
    }

    /// Updates the cached modifier slices from the modifier chain.
    fn update_modifier_slices_cache(&self) {
        let inner = self.inner.borrow();
        let mut snapshot = self.modifier_slices_snapshot.borrow_mut();
        collect_modifier_slices_into(inner.modifier_chain.chain(), Rc::make_mut(&mut snapshot));
        self.modifier_slices_dirty.set(false);
    }

    pub(crate) fn mark_modifier_slices_dirty(&self) {
        self.modifier_slices_dirty.set(true);
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

    /// Returns a clone of the current layout state.
    pub fn layout_state(&self) -> LayoutState {
        self.layout_state.borrow().clone()
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache_handles.clone()
    }

    /// Updates the position of this node. Called during placement.
    pub fn set_position(&self, position: Point) {
        let mut state = self.layout_state.borrow_mut();
        state.position = position;
        state.is_placed = true;
    }

    /// Updates the measured size of this node. Called during measurement.
    pub fn set_measured_size(&self, size: Size) {
        let mut state = self.layout_state.borrow_mut();
        state.size = size;
    }

    /// Clears the is_placed flag. Called at the start of a layout pass.
    pub fn clear_placed(&self) {
        self.layout_state.borrow_mut().is_placed = false;
    }

    /// Returns the modifier slices snapshot for rendering.
    pub fn modifier_slices_snapshot(&self) -> Rc<ModifierNodeSlices> {
        if self.modifier_slices_dirty.get() {
            self.update_modifier_slices_cache();
        }
        self.modifier_slices_snapshot.borrow().clone()
    }

    pub fn state(&self) -> Ref<'_, SubcomposeState> {
        Ref::map(self.inner.borrow(), |inner| &inner.state)
    }

    pub fn state_mut(&self) -> RefMut<'_, SubcomposeState> {
        RefMut::map(self.inner.borrow_mut(), |inner| &mut inner.state)
    }

    pub fn invalidate_subcomposition(&self) {
        self.inner.borrow().state.invalidate_scopes();
        self.mark_needs_measure();
        if let Some(id) = self.id.get() {
            cranpose_core::bubble_measure_dirty_in_composer(id);
        }
    }

    pub fn request_measure_recompose(&self) {
        self.mark_needs_measure();
        if let Some(id) = self.id.get() {
            cranpose_core::bubble_measure_dirty_in_composer(id);
        }
    }

    pub fn active_children(&self) -> Vec<NodeId> {
        current_subcompose_children(&self.inner.borrow())
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
        self.needs_redraw.set(true);
        if let Some(id) = self.id.get() {
            crate::schedule_draw_repass(id);
        }
        crate::request_render_invalidation();
    }

    /// Check if this node needs measure.
    pub fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    pub(crate) fn clear_needs_measure(&self) {
        self.needs_measure.set(false);
    }

    pub(crate) fn clear_needs_layout(&self) {
        self.needs_layout.set(false);
    }

    /// Mark this node as needing semantics recomputation.
    pub fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    /// Returns true when semantics need to be recomputed.
    pub fn needs_semantics_flag(&self) -> bool {
        self.needs_semantics.get()
    }

    pub(crate) fn clear_needs_semantics(&self) {
        self.needs_semantics.set(false);
    }

    #[cfg(test)]
    pub(crate) fn clear_needs_semantics_for_tests(&self) {
        self.clear_needs_semantics();
    }

    /// Returns true when this node requested a redraw since the last render pass.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    pub fn clear_needs_redraw(&self) {
        self.needs_redraw.set(false);
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
            cranpose_core::queue_semantics_invalidation(id);
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
            self.modifier_slices_dirty.set(true);
            let invalidation_caps = invalidation.capabilities();
            let has_capability = |capability| {
                curr_caps.contains(capability)
                    || prev_caps.contains(capability)
                    || invalidation_caps.contains(capability)
            };
            match invalidation.kind() {
                InvalidationKind::Layout => {
                    if has_capability(NodeCapabilities::LAYOUT) {
                        self.mark_needs_measure();
                    }
                }
                InvalidationKind::Draw => {
                    if has_capability(NodeCapabilities::DRAW) {
                        self.mark_needs_redraw();
                    }
                }
                InvalidationKind::PointerInput => {
                    if has_capability(NodeCapabilities::POINTER_INPUT) {
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
                    if has_capability(NodeCapabilities::FOCUS) {
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

impl cranpose_core::Node for SubcomposeLayoutNode {
    fn mount(&mut self) {
        let mut inner = self.inner.borrow_mut();
        let (chain, mut context) = inner.modifier_chain.chain_and_context_mut();
        chain.repair_chain();
        chain.attach_nodes(&mut *context);
    }

    fn unmount(&mut self) {
        self.inner
            .borrow_mut()
            .modifier_chain
            .chain_mut()
            .detach_nodes();
    }

    fn insert_child(&mut self, child: NodeId) {
        let mut inner = self.inner.borrow_mut();
        if inner.children.contains(&child) {
            return;
        }
        if is_virtual_node(child) {
            let count = self.virtual_children_count.get();
            self.virtual_children_count.set(count + 1);
        }
        inner.children.push(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        let mut inner = self.inner.borrow_mut();
        let before = inner.children.len();
        inner.children.retain(|&id| id != child);
        if inner.children.len() < before && is_virtual_node(child) {
            let count = self.virtual_children_count.get();
            if count > 0 {
                self.virtual_children_count.set(count - 1);
            }
        }
    }

    fn move_child(&mut self, from: usize, to: usize) {
        let mut inner = self.inner.borrow_mut();
        if from == to || from >= inner.children.len() {
            return;
        }
        let child = inner.children.remove(from);
        let target = to.min(inner.children.len());
        inner.children.insert(target, child);
    }

    fn update_children(&mut self, children: &[NodeId]) {
        let mut inner = self.inner.borrow_mut();
        inner.children.clear();
        inner.children.extend_from_slice(children);
    }

    fn children(&self) -> Vec<NodeId> {
        current_subcompose_children(&self.inner.borrow())
    }

    fn set_node_id(&mut self, id: NodeId) {
        self.id.set(Some(id));
        self.inner.borrow_mut().modifier_chain.set_node_id(Some(id));
        self.update_modifier_slices_cache();
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

    fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
        self.needs_layout.set(true); // Measure implies layout
    }

    fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        self.needs_semantics.get()
    }

    /// Minimal parent setter for dirty flag bubbling.
    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        self.parent.set(Some(parent));
    }
}

#[derive(Clone)]
pub struct SubcomposeLayoutNodeHandle {
    inner: Rc<RefCell<SubcomposeLayoutNodeInner>>,
}

impl SubcomposeLayoutNodeHandle {
    pub(crate) fn measured_children_scratch(
        &self,
    ) -> Rc<RefCell<HashMap<NodeId, Rc<MeasuredNode>>>> {
        let scratch = {
            let inner = self.inner.borrow();
            Rc::clone(&inner.measured_children_scratch)
        };
        scratch.borrow_mut().clear();
        scratch
    }

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
        node_id: NodeId,
        constraints: Constraints,
        measurer: Box<dyn FnMut(NodeId, Constraints) -> Size + 'a>,
        mut cached_measure_registrar: Box<dyn FnMut(NodeId, Constraints) -> Option<Size> + 'a>,
        error: &'a RefCell<Option<NodeError>>,
    ) -> Result<MeasureResult, NodeError> {
        self.measure_with_cached_batch(
            composer,
            node_id,
            constraints,
            CachedBatchMeasureInputs {
                measurer,
                cached_measure_batch_registrar: Box::new(
                    move |node_ids, child_constraints, out| {
                        out.clear();
                        out.reserve(node_ids.len());
                        for &child_id in node_ids {
                            out.push(cached_measure_registrar(child_id, child_constraints));
                        }
                    },
                ),
                retained_measure_lookup: Box::new(|_| None),
                retained_measure_registrar: Box::new(|_| {}),
                error,
            },
        )
    }

    pub(crate) fn measure_with_cached_batch<'a>(
        &self,
        composer: &Composer,
        node_id: NodeId,
        constraints: Constraints,
        callbacks: CachedBatchMeasureInputs<'a>,
    ) -> Result<MeasureResult, NodeError> {
        let CachedBatchMeasureInputs {
            measurer,
            cached_measure_batch_registrar,
            retained_measure_lookup,
            retained_measure_registrar,
            error,
        } = callbacks;
        let (policy, mut state, slots_host, placement_scratch, captured_context) = {
            let mut inner = self.inner.borrow_mut();
            let policy = Rc::clone(&inner.measure_policy);
            let state = std::mem::take(&mut inner.state);
            let slots_host = Rc::clone(&inner.slots);
            let placement_scratch = std::mem::take(&mut inner.placement_scratch);
            let captured_context = inner.captured_context.clone();
            (
                policy,
                state,
                slots_host,
                placement_scratch,
                captured_context,
            )
        };
        state.begin_pass();

        let previous = composer.phase();
        if !matches!(previous, Phase::Measure | Phase::Layout) {
            composer.enter_phase(Phase::Measure);
        }

        let constraints_copy = constraints;
        // Architecture Note: Using subcompose_slot (not subcompose_in) to preserve the
        // SlotTable across measurement passes. This matches JC's SubcomposeLayout behavior
        // where `subcompose()` is called during measure and the slot table persists between
        // frames. Without this, lazy list item groups would be wiped and recreated on every
        // scroll frame, causing O(visible_items) recomposition overhead ("thrashing").
        //
        // Reference: LazyLayoutMeasureScope.subcompose() in JC reuses existing slots by key,
        // and SubcomposeLayoutState holds `slotIdToNode` map across measurements.
        let fallback_context;
        let context = if let Some(context) = captured_context.as_ref() {
            context
        } else {
            fallback_context = composer.capture_composition_context();
            &fallback_context
        };
        let ((result, placement_scratch), _) = composer.subcompose_slot_with_context(
            &slots_host,
            Some(node_id),
            context,
            |inner_composer| {
                let mut scope = SubcomposeMeasureScopeImpl::new(SubcomposeMeasureScopeInit {
                    composer: inner_composer.clone(),
                    state: &mut state,
                    constraints: constraints_copy,
                    measurer,
                    cached_measure_batch_registrar,
                    retained_measure_lookup,
                    retained_measure_registrar,
                    error,
                    parent_handle: self.clone(),
                    root_id: node_id,
                    placement_scratch,
                });
                let result = (policy)(&mut scope, constraints_copy);
                (result, scope.into_placement_scratch())
            },
        )?;

        state.finish_pass();

        if previous != composer.phase() {
            composer.enter_phase(previous);
        }

        {
            let mut inner = self.inner.borrow_mut();
            inner.state = state;
            inner.placement_scratch = placement_scratch;

            // Store placement children for children() traversal.
            // This avoids clearing/rebuilding the structural children set on every measure,
            // eliminating O(n) allocator churn. The structural children (virtual nodes) are
            // tracked via insert_child/remove_child, while last_placements tracks rendered nodes.
            inner.last_placements = result.placements.iter().map(|p| p.node_id).collect();
        }

        Ok(result)
    }

    pub(crate) fn recycle_placement_scratch(&self, mut placements: Vec<Placement>) {
        placements.clear();
        let mut inner = self.inner.borrow_mut();
        if placements.capacity() > inner.placement_scratch.capacity() {
            inner.placement_scratch = placements;
        }
    }

    pub fn set_active_children<I>(&self, children: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        let mut inner = self.inner.borrow_mut();
        inner.children.clear();
        inner.children.extend(children);
    }
}

fn current_subcompose_children(inner: &SubcomposeLayoutNodeInner) -> Vec<NodeId> {
    if !inner.last_placements.is_empty() {
        inner.last_placements.clone()
    } else {
        inner.children.clone()
    }
}

struct SubcomposeLayoutNodeInner {
    modifier: Modifier,
    modifier_chain: ModifierChainHandle,
    resolved_modifiers: ResolvedModifiers,
    modifier_capabilities: NodeCapabilities,
    state: SubcomposeState,
    measure_policy: Rc<MeasurePolicy>,
    children: Vec<NodeId>,
    slots: Rc<SlotsHost>,
    debug_modifiers: bool,
    // Owns virtual nodes created during subcomposition
    virtual_nodes: HashMap<NodeId, Rc<LayoutNode>>,
    // Cached placement children from the last measure pass.
    // Used by children() for semantic/render traversal without clearing structural children.
    last_placements: Vec<NodeId>,
    placement_scratch: Vec<Placement>,
    measured_children_scratch: Rc<RefCell<HashMap<NodeId, Rc<MeasuredNode>>>>,
    /// Locals and source ownership captured where this node was composed.
    captured_context: Option<cranpose_core::CapturedCompositionContext>,
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
            children: Vec::new(),
            slots: Rc::new(SlotsHost::new(SlotTable::default())),
            debug_modifiers: false,
            virtual_nodes: HashMap::new(),
            last_placements: Vec::new(),
            placement_scratch: Vec::new(),
            measured_children_scratch: Rc::new(RefCell::new(HashMap::default())),
            captured_context: None,
        }
    }

    fn set_measure_policy(&mut self, policy: Rc<MeasurePolicy>) {
        self.measure_policy = policy;
        // The root measurement subcomposition caches its slot table separately
        // from per-item slot scopes. When a widget updates the data captured by
        // the measure lambda through shared cells, the next layout pass must not
        // reuse the previous root measure group wholesale.
        if let Err(err) = self.slots.reset() {
            log::error!(
                "failed to reset root measurement slots after measure policy update: {err}"
            );
        }
    }

    /// Updates the modifier and collects invalidations without dispatching them.
    /// Returns the invalidations and whether the modifier changed.
    fn set_modifier_collect(&mut self, modifier: Modifier) -> (Vec<ModifierInvalidation>, bool) {
        let modifier_changed = !self.modifier.structural_eq(&modifier);
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
