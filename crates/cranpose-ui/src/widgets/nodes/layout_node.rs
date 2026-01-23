use crate::{
    layout::MeasuredNode,
    modifier::{
        Modifier, ModifierChainHandle, ModifierLocalSource, ModifierLocalToken,
        ModifierLocalsHandle, ModifierNodeSlices, Point, ResolvedModifierLocal, ResolvedModifiers,
        Size,
    },
};
use cranpose_core::{Node, NodeId};
use cranpose_foundation::{
    InvalidationKind, ModifierInvalidation, NodeCapabilities, SemanticsConfiguration,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy};
use indexmap::IndexSet;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Retained layout state for a LayoutNode.
/// This mirrors Jetpack Compose's approach where each node stores its own
/// measured size and placed position, eliminating the need for per-frame
/// LayoutTree reconstruction.
#[derive(Clone, Debug)]
pub struct LayoutState {
    /// The measured size of this node (width, height).
    pub size: Size,
    /// Position relative to parent's content origin.
    pub position: Point,
    /// True if this node has been placed in the current layout pass.
    pub is_placed: bool,
    /// The constraints used for the last measurement.
    pub measurement_constraints: Constraints,
    /// Offset of the content box relative to the node origin (e.g. due to padding).
    pub content_offset: Point,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            size: Size::default(),
            position: Point::default(),
            is_placed: false,
            measurement_constraints: Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            content_offset: Point::default(),
        }
    }
}

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

    pub(crate) fn epoch(&self) -> u64 {
        self.state.borrow().epoch
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
    modifier_chain: ModifierChainHandle,
    resolved_modifiers: ResolvedModifiers,
    modifier_capabilities: NodeCapabilities,
    modifier_child_capabilities: NodeCapabilities,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    /// The actual children of this node (folded view - includes virtual nodes as-is)
    pub children: IndexSet<NodeId>,
    cache: LayoutNodeCacheHandles,
    // Dirty flags for selective measure/layout/render
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    needs_semantics: Cell<bool>,
    needs_redraw: Cell<bool>,
    needs_pointer_pass: Cell<bool>,
    needs_focus_sync: Cell<bool>,
    /// Parent for dirty flag bubbling (skips virtual nodes)
    parent: Cell<Option<NodeId>>,
    /// Direct parent in the tree (may be virtual)
    folded_parent: Cell<Option<NodeId>>,
    // Node's own ID (set by applier after creation)
    id: Cell<Option<NodeId>>,
    debug_modifiers: Cell<bool>,
    /// Virtual node flag - virtual nodes are transparent containers for subcomposition
    /// Their children are flattened into the parent's children list for measurement
    is_virtual: bool,
    /// Count of virtual children (for lazy unfolded children computation)
    virtual_children_count: Cell<usize>,

    // Caching for modifier slices to avoid repeated allocation
    modifier_slices_buffer: RefCell<ModifierNodeSlices>,
    modifier_slices_snapshot: RefCell<Rc<ModifierNodeSlices>>,

    /// Retained layout state (size, position) for this node.
    /// Updated by measure/place and read by renderer.
    /// Wrapped in Rc to ensure state is shared across clones (e.g. SubcomposeLayout usage).
    layout_state: Rc<RefCell<LayoutState>>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self::new_with_virtual(modifier, measure_policy, false)
    }

    /// Create a virtual LayoutNode for subcomposition slot containers.
    /// Virtual nodes are transparent - their children are flattened into parent's children list.
    pub fn new_virtual() -> Self {
        Self::new_with_virtual(
            Modifier::empty(),
            Rc::new(crate::layout::policies::EmptyMeasurePolicy),
            true,
        )
    }

    fn new_with_virtual(
        modifier: Modifier,
        measure_policy: Rc<dyn MeasurePolicy>,
        is_virtual: bool,
    ) -> Self {
        let mut node = Self {
            modifier,
            modifier_chain: ModifierChainHandle::new(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_capabilities: NodeCapabilities::default(),
            modifier_child_capabilities: NodeCapabilities::default(),
            measure_policy,
            children: IndexSet::new(),
            cache: LayoutNodeCacheHandles::default(),
            needs_measure: Cell::new(true), // New nodes need initial measure
            needs_layout: Cell::new(true),  // New nodes need initial layout
            needs_semantics: Cell::new(true), // Semantics snapshot needs initial build
            needs_redraw: Cell::new(true),  // First render should draw the node
            needs_pointer_pass: Cell::new(false),
            needs_focus_sync: Cell::new(false),
            parent: Cell::new(None),        // Non-virtual parent for bubbling
            folded_parent: Cell::new(None), // Direct parent (may be virtual)
            id: Cell::new(None),            // ID set by applier after creation
            debug_modifiers: Cell::new(false),
            is_virtual,
            virtual_children_count: Cell::new(0),
            modifier_slices_buffer: RefCell::new(ModifierNodeSlices::default()),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            layout_state: Rc::new(RefCell::new(LayoutState::default())),
        };
        node.sync_modifier_chain();
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        // Always sync the modifier chain because element equality is used for node
        // matching/reuse, not for skipping updates. Closures may capture updated
        // state values that need to be passed to nodes even when the Modifier
        // compares as equal. This matches Jetpack Compose where update() is always
        // called on matched nodes.
        let modifier_changed = !self.modifier.structural_eq(&modifier);
        self.modifier = modifier;
        self.sync_modifier_chain();
        if modifier_changed {
            self.cache.clear();
            self.mark_needs_measure();
            self.request_semantics_update();
        }
    }

    fn sync_modifier_chain(&mut self) {
        let start_parent = self.parent();
        let mut resolver = move |token: ModifierLocalToken| {
            resolve_modifier_local_from_parent_chain(start_parent, token)
        };
        self.modifier_chain
            .set_debug_logging(self.debug_modifiers.get());
        self.modifier_chain.set_node_id(self.id.get());
        let modifier_local_invalidations = self
            .modifier_chain
            .update_with_resolver(&self.modifier, &mut resolver);
        self.resolved_modifiers = self.modifier_chain.resolved_modifiers();
        self.modifier_capabilities = self.modifier_chain.capabilities();
        self.modifier_child_capabilities = self.modifier_chain.aggregate_child_capabilities();

        // Update modifier slices cache
        // We reuse the buffer to avoid allocation, then create a shared snapshot
        use crate::modifier::collect_modifier_slices_into;
        let mut buffer = self.modifier_slices_buffer.borrow_mut();
        collect_modifier_slices_into(self.modifier_chain.chain(), &mut buffer);
        *self.modifier_slices_snapshot.borrow_mut() = Rc::new(buffer.clone());

        let mut invalidations = self.modifier_chain.take_invalidations();
        invalidations.extend(modifier_local_invalidations);
        self.dispatch_modifier_invalidations(&invalidations);
        self.refresh_registry_state();
    }

    fn dispatch_modifier_invalidations(&self, invalidations: &[ModifierInvalidation]) {
        for invalidation in invalidations {
            match invalidation.kind() {
                InvalidationKind::Layout => {
                    if self.has_layout_modifier_nodes() {
                        self.mark_needs_measure();
                        if let Some(id) = self.id.get() {
                            crate::schedule_layout_repass(id);
                        }
                    }
                }
                InvalidationKind::Draw => {
                    if self.has_draw_modifier_nodes() {
                        self.mark_needs_redraw();
                    }
                }
                InvalidationKind::PointerInput => {
                    if self.has_pointer_input_modifier_nodes() {
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
                    if self.has_focus_modifier_nodes() {
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

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        // Only mark dirty if policy actually changed (pointer comparison)
        if !Rc::ptr_eq(&self.measure_policy, &policy) {
            self.measure_policy = policy;
            self.cache.clear();
            self.mark_needs_measure();
        }
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

    /// Check if this node needs layout.
    pub fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    /// Mark this node as needing semantics recomputation.
    pub fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    /// Clear the semantics dirty flag after rebuilding semantics.
    pub(crate) fn clear_needs_semantics(&self) {
        self.needs_semantics.set(false);
    }

    /// Returns true when semantics need to be recomputed.
    pub fn needs_semantics(&self) -> bool {
        self.needs_semantics.get()
    }

    /// Returns true when this node requested a redraw since the last render pass.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    pub fn clear_needs_redraw(&self) {
        self.needs_redraw.set(false);
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

    /// Clear the measure dirty flag after measuring.
    pub(crate) fn clear_needs_measure(&self) {
        self.needs_measure.set(false);
    }

    /// Clear the layout dirty flag after laying out.
    pub(crate) fn clear_needs_layout(&self) {
        self.needs_layout.set(false);
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

    /// Set this node's ID (called by applier after creation).
    pub fn set_node_id(&mut self, id: NodeId) {
        if let Some(existing) = self.id.replace(Some(id)) {
            unregister_layout_node(existing);
        }
        register_layout_node(id, self);
        self.refresh_registry_state();

        // Propagate the ID to the modifier chain. This triggers a lifecycle update
        // for nodes that depend on the node ID for invalidation (e.g., ScrollNode).
        self.modifier_chain.set_node_id(Some(id));
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> Option<NodeId> {
        self.id.get()
    }

    /// Set this node's parent (called when node is added as child).
    /// Sets both folded_parent (direct) and parent (first non-virtual ancestor for bubbling).
    pub fn set_parent(&self, parent: NodeId) {
        self.folded_parent.set(Some(parent));
        // For now, parent = folded_parent. Virtual parent skipping requires applier access.
        // The actual virtual-skipping happens in bubble_measure_dirty via applier traversal.
        self.parent.set(Some(parent));
        self.refresh_registry_state();
    }

    /// Clear this node's parent (called when node is removed from parent).
    pub fn clear_parent(&self) {
        self.folded_parent.set(None);
        self.parent.set(None);
        self.refresh_registry_state();
    }

    /// Get this node's parent for dirty flag bubbling (may skip virtual nodes).
    pub fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    /// Get this node's direct parent (may be a virtual node).
    pub fn folded_parent(&self) -> Option<NodeId> {
        self.folded_parent.get()
    }

    /// Returns true if this is a virtual node (transparent container for subcomposition).
    pub fn is_virtual(&self) -> bool {
        self.is_virtual
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache.clone()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved_modifiers
    }

    pub fn modifier_capabilities(&self) -> NodeCapabilities {
        self.modifier_capabilities
    }

    pub fn modifier_child_capabilities(&self) -> NodeCapabilities {
        self.modifier_child_capabilities
    }

    pub fn set_debug_modifiers(&mut self, enabled: bool) {
        self.debug_modifiers.set(enabled);
        self.modifier_chain.set_debug_logging(enabled);
    }

    pub fn debug_modifiers_enabled(&self) -> bool {
        self.debug_modifiers.get()
    }

    pub fn modifier_locals_handle(&self) -> ModifierLocalsHandle {
        self.modifier_chain.modifier_locals_handle()
    }

    pub fn has_layout_modifier_nodes(&self) -> bool {
        self.modifier_capabilities
            .contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_modifier_nodes(&self) -> bool {
        self.modifier_capabilities.contains(NodeCapabilities::DRAW)
    }

    pub fn has_pointer_input_modifier_nodes(&self) -> bool {
        self.modifier_capabilities
            .contains(NodeCapabilities::POINTER_INPUT)
    }

    pub fn has_semantics_modifier_nodes(&self) -> bool {
        self.modifier_capabilities
            .contains(NodeCapabilities::SEMANTICS)
    }

    pub fn has_focus_modifier_nodes(&self) -> bool {
        self.modifier_capabilities.contains(NodeCapabilities::FOCUS)
    }

    fn refresh_registry_state(&self) {
        if let Some(id) = self.id.get() {
            let parent = self.parent();
            let capabilities = self.modifier_child_capabilities();
            let modifier_locals = self.modifier_locals_handle();
            LAYOUT_NODE_REGISTRY.with(|registry| {
                if let Some(entry) = registry.borrow_mut().get_mut(&id) {
                    entry.parent = parent;
                    entry.modifier_child_capabilities = capabilities;
                    entry.modifier_locals = modifier_locals;
                }
            });
        }
    }

    pub fn modifier_slices_snapshot(&self) -> Rc<ModifierNodeSlices> {
        self.modifier_slices_snapshot.borrow().clone()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Retained Layout State API
    // ═══════════════════════════════════════════════════════════════════════

    /// Returns a clone of the current layout state.
    pub fn layout_state(&self) -> LayoutState {
        self.layout_state.borrow().clone()
    }

    /// Returns the measured size of this node.
    pub fn measured_size(&self) -> Size {
        self.layout_state.borrow().size
    }

    /// Returns the position of this node relative to its parent.
    pub fn position(&self) -> Point {
        self.layout_state.borrow().position
    }

    /// Returns true if this node has been placed in the current layout pass.
    pub fn is_placed(&self) -> bool {
        self.layout_state.borrow().is_placed
    }

    /// Updates the measured size of this node. Called during measurement.
    pub fn set_measured_size(&self, size: Size) {
        let mut state = self.layout_state.borrow_mut();
        state.size = size;
    }

    /// Updates the position of this node. Called during placement.
    pub fn set_position(&self, position: Point) {
        let mut state = self.layout_state.borrow_mut();
        state.position = position;
        state.is_placed = true;
    }

    /// Records the constraints used for measurement. Used for relayout optimization.
    pub fn set_measurement_constraints(&self, constraints: Constraints) {
        self.layout_state.borrow_mut().measurement_constraints = constraints;
    }

    /// Records the content offset (e.g. from padding).
    pub fn set_content_offset(&self, offset: Point) {
        self.layout_state.borrow_mut().content_offset = offset;
    }

    /// Clears the is_placed flag. Called at the start of a layout pass.
    pub fn clear_placed(&self) {
        self.layout_state.borrow_mut().is_placed = false;
    }

    pub fn semantics_configuration(&self) -> Option<SemanticsConfiguration> {
        crate::modifier::collect_semantics_from_chain(self.modifier_chain.chain())
    }

    /// Returns a reference to the modifier chain for layout/draw pipeline integration.
    pub(crate) fn modifier_chain(&self) -> &ModifierChainHandle {
        &self.modifier_chain
    }

    /// Access the text field modifier node (if present) with a mutable callback.
    ///
    /// This is used for keyboard event dispatch to text fields.
    /// Returns `None` if no text field modifier is found in the chain.
    pub fn with_text_field_modifier_mut<R>(
        &mut self,
        f: impl FnMut(&mut crate::TextFieldModifierNode) -> R,
    ) -> Option<R> {
        self.modifier_chain.with_text_field_modifier_mut(f)
    }

    /// Returns a handle to the shared layout state.
    /// Used by layout system to update state without borrowing the Applier.
    pub fn layout_state_handle(&self) -> Rc<RefCell<LayoutState>> {
        self.layout_state.clone()
    }
}
impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        let mut node = Self {
            modifier: self.modifier.clone(),
            modifier_chain: ModifierChainHandle::new(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_capabilities: self.modifier_capabilities,
            modifier_child_capabilities: self.modifier_child_capabilities,
            measure_policy: self.measure_policy.clone(),
            children: self.children.clone(),
            cache: self.cache.clone(),
            needs_measure: Cell::new(self.needs_measure.get()),
            needs_layout: Cell::new(self.needs_layout.get()),
            needs_semantics: Cell::new(self.needs_semantics.get()),
            needs_redraw: Cell::new(self.needs_redraw.get()),
            needs_pointer_pass: Cell::new(self.needs_pointer_pass.get()),
            needs_focus_sync: Cell::new(self.needs_focus_sync.get()),
            parent: Cell::new(self.parent.get()),
            folded_parent: Cell::new(self.folded_parent.get()),
            id: Cell::new(None),
            debug_modifiers: Cell::new(self.debug_modifiers.get()),
            is_virtual: self.is_virtual,
            virtual_children_count: Cell::new(self.virtual_children_count.get()),
            modifier_slices_buffer: RefCell::new(ModifierNodeSlices::default()),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            // Share the same layout state across clones
            layout_state: self.layout_state.clone(),
        };
        node.sync_modifier_chain();
        node
    }
}

impl Node for LayoutNode {
    fn mount(&mut self) {
        let (chain, mut context) = self.modifier_chain.chain_and_context_mut();
        chain.repair_chain();
        chain.attach_nodes(&mut *context);
    }

    fn unmount(&mut self) {
        self.modifier_chain.chain_mut().detach_nodes();
    }

    fn set_node_id(&mut self, id: NodeId) {
        // Delegate to inherent method to ensure proper registration and chain updates
        LayoutNode::set_node_id(self, id);
    }

    fn insert_child(&mut self, child: NodeId) {
        if is_virtual_node(child) {
            let count = self.virtual_children_count.get();
            self.virtual_children_count.set(count + 1);
        }
        self.children.insert(child);
        self.cache.clear();
        self.mark_needs_measure();
    }

    fn remove_child(&mut self, child: NodeId) {
        if self.children.shift_remove(&child) {
            if is_virtual_node(child) {
                let count = self.virtual_children_count.get();
                if count > 0 {
                    self.virtual_children_count.set(count - 1);
                }
            }
            self.cache.clear();
            self.mark_needs_measure();
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
        self.needs_layout.set(true);
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
    /// Only sets the parent Cell without triggering registry updates.
    /// This is used during SubcomposeLayout measurement where we need parent
    /// pointers for bubble_measure_dirty but don't want full attachment side effects.
    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        self.parent.set(Some(parent));
    }
}

impl Drop for LayoutNode {
    fn drop(&mut self) {
        if let Some(id) = self.id.get() {
            unregister_layout_node(id);
        }
    }
}

thread_local! {
    static LAYOUT_NODE_REGISTRY: RefCell<HashMap<NodeId, LayoutNodeRegistryEntry>> =
        RefCell::new(HashMap::new());
    // Start at a high value to avoid conflicts with SlotTable IDs (which start low).
    // We use a value compatible with 32-bit (WASM) usize to prevent truncation issues.
    // 0xC0000000 is ~3.2 billion, leaving ~1 billion IDs before overflow.
    static VIRTUAL_NODE_ID_COUNTER: std::sync::atomic::AtomicUsize = const { std::sync::atomic::AtomicUsize::new(0xC0000000) };
}

struct LayoutNodeRegistryEntry {
    parent: Option<NodeId>,
    modifier_child_capabilities: NodeCapabilities,
    modifier_locals: ModifierLocalsHandle,
    is_virtual: bool,
}

pub(crate) fn register_layout_node(id: NodeId, node: &LayoutNode) {
    LAYOUT_NODE_REGISTRY.with(|registry| {
        registry.borrow_mut().insert(
            id,
            LayoutNodeRegistryEntry {
                parent: node.parent(),
                modifier_child_capabilities: node.modifier_child_capabilities(),
                modifier_locals: node.modifier_locals_handle(),
                is_virtual: node.is_virtual(),
            },
        );
    });
}

pub(crate) fn unregister_layout_node(id: NodeId) {
    LAYOUT_NODE_REGISTRY.with(|registry| {
        registry.borrow_mut().remove(&id);
    });
}

pub(crate) fn is_virtual_node(id: NodeId) -> bool {
    LAYOUT_NODE_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|entry| entry.is_virtual)
            .unwrap_or(false)
    })
}

pub(crate) fn allocate_virtual_node_id() -> NodeId {
    use std::sync::atomic::Ordering;
    // Allocate IDs from a high range to avoid conflict with SlotTable IDs.
    // Thread-local counter avoids cross-thread contention (WASM is single-threaded anyway).
    VIRTUAL_NODE_ID_COUNTER.with(|counter| counter.fetch_add(1, Ordering::Relaxed))
}

fn resolve_modifier_local_from_parent_chain(
    start: Option<NodeId>,
    token: ModifierLocalToken,
) -> Option<ResolvedModifierLocal> {
    let mut current = start;
    while let Some(parent_id) = current {
        let (next_parent, resolved) = LAYOUT_NODE_REGISTRY.with(|registry| {
            let registry = registry.borrow();
            if let Some(entry) = registry.get(&parent_id) {
                let resolved = if entry
                    .modifier_child_capabilities
                    .contains(NodeCapabilities::MODIFIER_LOCALS)
                {
                    entry
                        .modifier_locals
                        .borrow()
                        .resolve(token)
                        .map(|value| value.with_source(ModifierLocalSource::Ancestor))
                } else {
                    None
                };
                (entry.parent, resolved)
            } else {
                (None, None)
            }
        });
        if let Some(value) = resolved {
            return Some(value);
        }
        current = next_parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::Size as GeometrySize;
    use cranpose_ui_layout::{Measurable, MeasureResult};
    use std::rc::Rc;

    #[derive(Default)]
    struct TestMeasurePolicy;

    impl MeasurePolicy for TestMeasurePolicy {
        fn measure(
            &self,
            _measurables: &[Box<dyn Measurable>],
            _constraints: Constraints,
        ) -> MeasureResult {
            MeasureResult::new(
                GeometrySize {
                    width: 0.0,
                    height: 0.0,
                },
                Vec::new(),
            )
        }

        fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
            0.0
        }

        fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
            0.0
        }

        fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
            0.0
        }

        fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
            0.0
        }
    }

    fn fresh_node() -> LayoutNode {
        LayoutNode::new(Modifier::empty(), Rc::new(TestMeasurePolicy))
    }

    fn invalidation(kind: InvalidationKind) -> ModifierInvalidation {
        ModifierInvalidation::new(kind, NodeCapabilities::for_invalidation(kind))
    }

    #[test]
    fn layout_invalidation_requires_layout_capability() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Layout)]);

        assert!(!node.needs_measure());
        assert!(!node.needs_layout());
    }

    #[test]
    fn semantics_configuration_reflects_modifier_state() {
        let mut node = fresh_node();
        node.set_modifier(Modifier::empty().semantics(|config| {
            config.content_description = Some("greeting".into());
            config.is_clickable = true;
        }));

        let config = node
            .semantics_configuration()
            .expect("expected semantics configuration");
        assert_eq!(config.content_description.as_deref(), Some("greeting"));
        assert!(config.is_clickable);
    }

    #[test]
    fn layout_invalidation_marks_flags_when_capability_present() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::LAYOUT;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Layout)]);

        assert!(node.needs_measure());
        assert!(node.needs_layout());
    }

    #[test]
    fn draw_invalidation_marks_redraw_flag_when_capable() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Draw)]);

        assert!(node.needs_redraw());
        assert!(!node.needs_layout());
    }

    #[test]
    fn semantics_invalidation_sets_semantics_flag_only() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.clear_needs_semantics();
        node.modifier_capabilities = NodeCapabilities::SEMANTICS;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Semantics)]);

        assert!(node.needs_semantics());
        assert!(!node.needs_measure());
        assert!(!node.needs_layout());
    }

    #[test]
    fn pointer_invalidation_requires_pointer_capability() {
        let mut node = fresh_node();
        node.clear_needs_pointer_pass();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;
        // Note: We don't assert on global take_pointer_invalidation() because
        // it's shared across tests running in parallel and causes flakiness.
        // The node's local state is sufficient to verify correct dispatch behavior.

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::PointerInput)]);

        assert!(!node.needs_pointer_pass());
    }

    #[test]
    fn pointer_invalidation_marks_flag_and_requests_queue() {
        let mut node = fresh_node();
        node.clear_needs_pointer_pass();
        node.modifier_capabilities = NodeCapabilities::POINTER_INPUT;
        node.modifier_child_capabilities = node.modifier_capabilities;
        // Note: We don't assert on global take_pointer_invalidation() because
        // it's shared across tests running in parallel and causes flakiness.
        // The node's local state is sufficient to verify correct dispatch behavior.

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::PointerInput)]);

        assert!(node.needs_pointer_pass());
    }

    #[test]
    fn focus_invalidation_requires_focus_capability() {
        let mut node = fresh_node();
        node.clear_needs_focus_sync();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;
        crate::take_focus_invalidation();

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Focus)]);

        assert!(!node.needs_focus_sync());
        assert!(!crate::take_focus_invalidation());
    }

    #[test]
    fn focus_invalidation_marks_flag_and_requests_queue() {
        let mut node = fresh_node();
        node.clear_needs_focus_sync();
        node.modifier_capabilities = NodeCapabilities::FOCUS;
        node.modifier_child_capabilities = node.modifier_capabilities;
        crate::take_focus_invalidation();

        node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Focus)]);

        assert!(node.needs_focus_sync());
        assert!(crate::take_focus_invalidation());
    }

    #[test]
    fn set_modifier_marks_semantics_dirty() {
        let mut node = fresh_node();
        node.clear_needs_semantics();
        node.set_modifier(Modifier::empty().semantics(|config| {
            config.is_clickable = true;
        }));

        assert!(node.needs_semantics());
    }

    #[test]
    fn modifier_child_capabilities_reflect_chain_head() {
        let mut node = fresh_node();
        node.set_modifier(Modifier::empty().padding(4.0));
        assert!(
            node.modifier_child_capabilities()
                .contains(NodeCapabilities::LAYOUT),
            "padding should introduce layout capability"
        );
    }
}
