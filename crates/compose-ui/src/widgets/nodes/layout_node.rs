use crate::{
    layout::MeasuredNode,
    modifier::{
        collect_modifier_slices, Modifier, ModifierChainHandle, ModifierLocalSource,
        ModifierLocalToken, ModifierLocalsHandle, ModifierNodeSlices, ResolvedModifierLocal,
        ResolvedModifiers,
    },
};
use compose_core::{Node, NodeId};
use compose_foundation::{InvalidationKind, NodeCapabilities, SemanticsConfiguration};
use compose_ui_layout::{Constraints, MeasurePolicy};
use indexmap::IndexSet;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

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
    pub children: IndexSet<NodeId>,
    cache: LayoutNodeCacheHandles,
    // Dirty flags for selective measure/layout/render
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    needs_semantics: Cell<bool>,
    // Parent tracking for dirty flag bubbling (Jetpack Compose style)
    parent: Cell<Option<NodeId>>,
    // Node's own ID (set by applier after creation)
    id: Cell<Option<NodeId>>,
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        let mut node = Self {
            modifier: Modifier::empty(),
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
            parent: Cell::new(None),        // No parent initially
            id: Cell::new(None),            // ID set by applier after creation
        };
        node.set_modifier(modifier);
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        // Only mark dirty if modifier actually changed
        if self.modifier != modifier {
            self.modifier = modifier;
            self.sync_modifier_chain();
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
        let modifier_local_invalidations = self
            .modifier_chain
            .update_with_resolver(&self.modifier, &mut resolver);
        self.resolved_modifiers = self.modifier_chain.resolved_modifiers();
        self.modifier_capabilities = self.modifier_chain.capabilities();
        self.modifier_child_capabilities = self.modifier_chain.aggregate_child_capabilities();
        let mut invalidations = self.modifier_chain.take_invalidations();
        invalidations.extend(modifier_local_invalidations);
        self.dispatch_modifier_invalidations(&invalidations);
        self.refresh_registry_state();
    }

    fn dispatch_modifier_invalidations(&self, invalidations: &[InvalidationKind]) {
        for invalidation in invalidations {
            match invalidation {
                InvalidationKind::Layout => {
                    if self.has_layout_modifier_nodes() {
                        self.mark_needs_measure();
                    }
                }
                InvalidationKind::Draw => {
                    if self.has_draw_modifier_nodes() {
                        self.mark_needs_layout();
                    }
                }
                InvalidationKind::PointerInput => {}
                InvalidationKind::Semantics => {
                    self.request_semantics_update();
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

    fn request_semantics_update(&self) {
        let already_dirty = self.needs_semantics.replace(true);
        if already_dirty {
            return;
        }

        if let Some(id) = self.id.get() {
            compose_core::queue_semantics_invalidation(id);
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

    /// Set this node's ID (called by applier after creation).
    pub fn set_node_id(&self, id: NodeId) {
        if let Some(existing) = self.id.replace(Some(id)) {
            unregister_layout_node(existing);
        }
        register_layout_node(id, self);
        self.refresh_registry_state();
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> Option<NodeId> {
        self.id.get()
    }

    /// Set this node's parent (called when node is added as child).
    pub fn set_parent(&self, parent: NodeId) {
        self.parent.set(Some(parent));
        self.refresh_registry_state();
    }

    /// Clear this node's parent (called when node is removed from parent).
    pub fn clear_parent(&self) {
        self.parent.set(None);
        self.refresh_registry_state();
    }

    /// Get this node's parent.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    fn resolve_modifier_local_from_ancestors(
        &self,
        token: ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        resolve_modifier_local_from_parent_chain(self.parent(), token)
    }

    pub(crate) fn cache_handles(&self) -> LayoutNodeCacheHandles {
        self.cache.clone()
    }

    pub(crate) fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved_modifiers
    }

    pub fn modifier_capabilities(&self) -> NodeCapabilities {
        self.modifier_capabilities
    }

    pub fn modifier_child_capabilities(&self) -> NodeCapabilities {
        self.modifier_child_capabilities
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

    pub fn modifier_slices_snapshot(&self) -> ModifierNodeSlices {
        collect_modifier_slices(self.modifier_chain.chain())
    }

    pub fn semantics_configuration(&self) -> Option<SemanticsConfiguration> {
        crate::modifier::collect_semantics_from_chain(self.modifier_chain.chain())
    }
}

/// Legacy bubbling function kept for test compatibility only.
/// DO NOT USE in production code - all bubbling now happens automatically
/// via composer reconciliation and pop_parent().
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn bubble_dirty_flags(node_id: compose_core::NodeId) {
    compose_core::bubble_layout_dirty_in_composer::<LayoutNode>(node_id);
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
            parent: Cell::new(self.parent.get()),
            id: Cell::new(None),
        };
        node.sync_modifier_chain();
        node
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

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        self.needs_semantics.get()
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
}

struct LayoutNodeRegistryEntry {
    parent: Option<NodeId>,
    modifier_child_capabilities: NodeCapabilities,
    modifier_locals: ModifierLocalsHandle,
}

fn register_layout_node(id: NodeId, node: &LayoutNode) {
    LAYOUT_NODE_REGISTRY.with(|registry| {
        registry.borrow_mut().insert(
            id,
            LayoutNodeRegistryEntry {
                parent: node.parent(),
                modifier_child_capabilities: node.modifier_child_capabilities(),
                modifier_locals: node.modifier_locals_handle(),
            },
        );
    });
}

fn unregister_layout_node(id: NodeId) {
    LAYOUT_NODE_REGISTRY.with(|registry| {
        registry.borrow_mut().remove(&id);
    });
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
    use compose_ui_graphics::Size as GeometrySize;
    use compose_ui_layout::{Measurable, MeasureResult};
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

    #[test]
    fn layout_invalidation_requires_layout_capability() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[InvalidationKind::Layout]);

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

        node.dispatch_modifier_invalidations(&[InvalidationKind::Layout]);

        assert!(node.needs_measure());
        assert!(node.needs_layout());
    }

    #[test]
    fn draw_invalidation_requires_draw_capability() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::LAYOUT;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[InvalidationKind::Draw]);

        assert!(!node.needs_layout());
    }

    #[test]
    fn draw_invalidation_marks_layout_flag_when_capable() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::DRAW;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[InvalidationKind::Draw]);

        assert!(node.needs_layout());
    }

    #[test]
    fn semantics_invalidation_sets_semantics_flag_only() {
        let mut node = fresh_node();
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.clear_needs_semantics();
        node.modifier_capabilities = NodeCapabilities::SEMANTICS;
        node.modifier_child_capabilities = node.modifier_capabilities;

        node.dispatch_modifier_invalidations(&[InvalidationKind::Semantics]);

        assert!(node.needs_semantics());
        assert!(!node.needs_measure());
        assert!(!node.needs_layout());
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
        node.set_modifier(Modifier::padding(4.0));
        assert!(
            node.modifier_child_capabilities()
                .contains(NodeCapabilities::LAYOUT),
            "padding should introduce layout capability"
        );
    }
}
