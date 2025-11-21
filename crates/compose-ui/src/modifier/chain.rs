#![allow(private_interfaces)]

#[allow(unused_imports)] // Used in tests
use compose_foundation::InvalidationKind;

use compose_foundation::{
    BasicModifierNodeContext, ModifierInvalidation, ModifierNodeChain, NodeCapabilities,
};

use super::{
    local::ModifierLocalManager, DimensionConstraint, EdgeInsets, LayoutProperties, Modifier,
    ModifierInspectorRecord, ModifierLocalAncestorResolver, ModifierLocalToken, Point,
    ResolvedModifierLocal, ResolvedModifiers,
};
use crate::modifier_nodes::{
    AlignmentNode, FillDirection, FillNode, IntrinsicAxis, IntrinsicSizeNode, OffsetNode,
    PaddingNode, SizeNode, WeightNode,
};
use std::any::type_name_of_val;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

/// Snapshot of a modifier node inside a reconciled chain for debugging & inspector tooling.
#[derive(Clone, Debug, PartialEq)]
pub struct ModifierChainInspectorNode {
    pub depth: usize,
    pub entry_index: Option<usize>,
    pub type_name: &'static str,
    pub capabilities: NodeCapabilities,
    pub aggregate_child_capabilities: NodeCapabilities,
    pub inspector: Option<ModifierInspectorRecord>,
}

/// Runtime helper that keeps a [`ModifierNodeChain`] in sync with a [`Modifier`].
///
/// This is the first step toward Jetpack Compose parity: callers can keep a handle
/// per layout node, feed it the latest `Modifier`, and then drive layout/draw/input
/// phases through the reconciled chain.
pub type ModifierLocalsHandle = Rc<RefCell<ModifierLocalManager>>;

#[allow(dead_code)]
pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    context: RefCell<BasicModifierNodeContext>,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
    aggregate_child_capabilities: NodeCapabilities,
    modifier_locals: ModifierLocalsHandle,
    inspector_snapshot: Vec<ModifierChainInspectorNode>,
    debug_logging: bool,
}

impl Default for ModifierChainHandle {
    fn default() -> Self {
        Self {
            chain: ModifierNodeChain::new(),
            context: RefCell::new(BasicModifierNodeContext::new()),
            resolved: ResolvedModifiers::default(),
            capabilities: NodeCapabilities::default(),
            aggregate_child_capabilities: NodeCapabilities::default(),
            modifier_locals: Rc::new(RefCell::new(ModifierLocalManager::new())),
            inspector_snapshot: Vec::new(),
            debug_logging: false,
        }
    }
}

#[allow(dead_code)]
impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements stored in `modifier`.
    pub fn update(&mut self, modifier: &Modifier) -> Vec<ModifierInvalidation> {
        let mut resolver = |_: ModifierLocalToken| None;
        self.update_with_resolver(modifier, &mut resolver)
    }

    pub fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
    ) -> Vec<ModifierInvalidation> {
        let elements = modifier.elements();
        self.chain
            .update_from_slice(&elements, &mut *self.context.borrow_mut());
        self.capabilities = self.chain.capabilities();
        self.aggregate_child_capabilities = self.chain.head().aggregate_child_capabilities();
        let modifier_local_invalidations = self
            .modifier_locals
            .borrow_mut()
            .sync(&self.chain, resolver);
        self.resolved = self.compute_resolved();
        self.inspector_snapshot = self.collect_inspector_snapshot(modifier);
        let should_log = self.debug_logging || global_modifier_debug_flag();
        if should_log {
            crate::debug::log_modifier_chain(self.chain(), self.inspector_snapshot());
            crate::debug::emit_modifier_chain_trace(self.inspector_snapshot());
        }
        modifier_local_invalidations
    }

    /// Enables or disables per-handle modifier debug logging.
    pub fn set_debug_logging(&mut self, enabled: bool) {
        self.debug_logging = enabled;
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Returns mutable access to the modifier node chain.
    pub fn chain_mut(&mut self) -> &mut ModifierNodeChain {
        &mut self.chain
    }

    /// Returns mutable access to the modifier node context.
    pub fn context_mut(&self) -> std::cell::RefMut<'_, BasicModifierNodeContext> {
        self.context.borrow_mut()
    }

    /// Returns mutable references to both the chain and context.
    /// This is a convenience method for measurement that avoids borrow conflicts.
    pub fn chain_and_context_mut(
        &mut self,
    ) -> (
        &mut ModifierNodeChain,
        std::cell::RefMut<'_, BasicModifierNodeContext>,
    ) {
        (&mut self.chain, self.context.borrow_mut())
    }

    /// Returns the aggregated capability mask for the reconciled chain.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.capabilities
    }

    /// Returns the aggregate child capability mask rooted at the sentinel head.
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.aggregate_child_capabilities
    }

    pub fn has_layout_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::DRAW)
    }

    pub fn has_pointer_input_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::POINTER_INPUT)
    }

    pub fn has_semantics_nodes(&self) -> bool {
        self.capabilities.contains(NodeCapabilities::SEMANTICS)
    }

    /// Drains invalidations requested during the last update cycle.
    pub fn take_invalidations(&self) -> Vec<ModifierInvalidation> {
        self.context.borrow_mut().take_invalidations()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved
    }

    pub fn resolve_modifier_local(
        &self,
        token: ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        self.modifier_locals.borrow().resolve(token)
    }

    pub fn modifier_locals_handle(&self) -> ModifierLocalsHandle {
        Rc::clone(&self.modifier_locals)
    }

    pub fn inspector_snapshot(&self) -> &[ModifierChainInspectorNode] {
        &self.inspector_snapshot
    }

    /// Visits all LayoutModifierNodes in the chain with mutable access.
    pub(crate) fn visit_layout_nodes_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut dyn compose_foundation::ModifierNode),
    {
        self.chain.visit_nodes_mut(|node, capabilities| {
            if capabilities.contains(NodeCapabilities::LAYOUT) {
                f(node);
            }
        });
    }

    fn compute_resolved(&self) -> ResolvedModifiers {
        let mut resolved = ResolvedModifiers::default();
        let mut layout = LayoutProperties::default();
        let mut padding = EdgeInsets::default();
        let mut offset = Point::default();

        self.chain.for_each_forward(|node_ref| {
            node_ref.with_node(|node| {
                let any = node.as_any();
                if let Some(padding_node) = any.downcast_ref::<PaddingNode>() {
                    padding += padding_node.padding();
                } else if let Some(size_node) = any.downcast_ref::<SizeNode>() {
                    apply_size_node(&mut layout, size_node);
                } else if let Some(fill_node) = any.downcast_ref::<FillNode>() {
                    apply_fill_node(&mut layout, fill_node);
                } else if let Some(intrinsic_node) = any.downcast_ref::<IntrinsicSizeNode>() {
                    apply_intrinsic_size_node(&mut layout, intrinsic_node);
                } else if let Some(weight_node) = any.downcast_ref::<WeightNode>() {
                    layout.weight = Some(weight_node.layout_weight());
                } else if let Some(alignment_node) = any.downcast_ref::<AlignmentNode>() {
                    if let Some(alignment) = alignment_node.box_alignment() {
                        layout.box_alignment = Some(alignment);
                    }
                    if let Some(alignment) = alignment_node.column_alignment() {
                        layout.column_alignment = Some(alignment);
                    }
                    if let Some(alignment) = alignment_node.row_alignment() {
                        layout.row_alignment = Some(alignment);
                    }
                } else if let Some(offset_node) = any.downcast_ref::<OffsetNode>() {
                    let delta = offset_node.offset();
                    offset.x += delta.x;
                    offset.y += delta.y;
                }
                // Note: BackgroundNode, CornerShapeNode, and GraphicsLayerNode are no longer
                // tracked in ResolvedModifiers. Visual rendering now flows through modifier slices
                // collected from the node chain at draw time.
            });
        });

        resolved.set_padding(padding);
        resolved.set_layout_properties(layout);
        resolved.set_offset(offset);
        resolved
    }

    fn collect_inspector_snapshot(&self, modifier: &Modifier) -> Vec<ModifierChainInspectorNode> {
        if self.chain.is_empty() {
            return Vec::new();
        }

        let mut per_entry: Vec<Option<ModifierInspectorRecord>> = vec![None; self.chain.len()];
        for (index, metadata) in modifier.inspector_metadata().iter().enumerate() {
            if index >= per_entry.len() {
                break;
            }
            per_entry[index] = Some(metadata.to_record());
        }

        let mut snapshot = Vec::new();
        self.chain.for_each_forward(|node_ref| {
            node_ref.with_node(|node| {
                let depth = node_ref.delegate_depth();
                let entry_index = node_ref.entry_index();
                let inspector = if depth == 0 {
                    entry_index
                        .and_then(|idx| per_entry.get_mut(idx))
                        .and_then(|slot| slot.take())
                } else {
                    None
                };
                snapshot.push(ModifierChainInspectorNode {
                    depth,
                    entry_index,
                    type_name: type_name_of_val(node),
                    capabilities: node_ref.kind_set(),
                    aggregate_child_capabilities: node_ref.aggregate_child_capabilities(),
                    inspector,
                });
            });
        });
        snapshot
    }
}

fn apply_size_node(layout: &mut LayoutProperties, node: &SizeNode) {
    if let Some(width) = node.max_width().or(node.min_width()) {
        layout.width = DimensionConstraint::Points(width);
    }
    if let Some(height) = node.max_height().or(node.min_height()) {
        layout.height = DimensionConstraint::Points(height);
    }
    if !node.enforce_incoming() {
        if let Some(min_width) = node.min_width() {
            layout.min_width = Some(min_width);
        }
        if let Some(max_width) = node.max_width() {
            layout.max_width = Some(max_width);
        }
        if let Some(min_height) = node.min_height() {
            layout.min_height = Some(min_height);
        }
        if let Some(max_height) = node.max_height() {
            layout.max_height = Some(max_height);
        }
    }
}

fn apply_fill_node(layout: &mut LayoutProperties, node: &FillNode) {
    let fraction = node.fraction();
    match node.direction() {
        FillDirection::Horizontal => {
            layout.width = DimensionConstraint::Fraction(fraction);
        }
        FillDirection::Vertical => {
            layout.height = DimensionConstraint::Fraction(fraction);
        }
        FillDirection::Both => {
            layout.width = DimensionConstraint::Fraction(fraction);
            layout.height = DimensionConstraint::Fraction(fraction);
        }
    }
}

fn apply_intrinsic_size_node(layout: &mut LayoutProperties, node: &IntrinsicSizeNode) {
    let constraint = DimensionConstraint::Intrinsic(node.intrinsic_size());
    match node.axis() {
        IntrinsicAxis::Width => {
            layout.width = constraint;
        }
        IntrinsicAxis::Height => {
            layout.height = constraint;
        }
    }
}

fn global_modifier_debug_flag() -> bool {
    static ENV_DEBUG: OnceLock<bool> = OnceLock::new();
    *ENV_DEBUG.get_or_init(|| std::env::var_os("COMPOSE_DEBUG_MODIFIERS").is_some())
}

#[cfg(test)]
mod tests {
    use compose_foundation::{ModifierInvalidation, ModifierNode, NodeCapabilities};

    use super::*;
    use crate::modifier::Color;
    use crate::modifier_nodes::PaddingNode;

    #[test]
    fn attaches_padding_node_and_invalidates_layout() {
        let mut handle = ModifierChainHandle::new();

        let _ = handle.update(&Modifier::empty().padding(8.0));

        assert_eq!(handle.chain().len(), 1);

        let invalidations = handle.take_invalidations();
        assert_eq!(
            invalidations,
            vec![ModifierInvalidation::new(
                InvalidationKind::Layout,
                NodeCapabilities::LAYOUT
            )]
        );
    }

    #[test]
    fn reuses_nodes_between_updates() {
        let mut handle = ModifierChainHandle::new();

        let _ = handle.update(&Modifier::empty().padding(12.0));
        let first_ptr = node_ptr::<PaddingNode>(&handle);
        handle.take_invalidations();

        let _ = handle.update(&Modifier::empty().padding(12.0));
        let second_ptr = node_ptr::<PaddingNode>(&handle);

        assert_eq!(first_ptr, second_ptr, "expected the node to be reused");
        assert!(
            handle.take_invalidations().is_empty(),
            "no additional invalidations should be issued for a pure update"
        );
    }

    #[test]
    fn modifier_slices_capture_background_and_shape() {
        use crate::modifier::slices::collect_modifier_slices;

        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(
            &Modifier::empty()
                .background(Color(0.2, 0.3, 0.4, 1.0))
                .then(Modifier::empty().rounded_corners(8.0)),
        );

        // Background and shape are now captured in modifier slices as draw commands
        let slices = collect_modifier_slices(handle.chain());
        assert!(
            !slices.draw_commands().is_empty(),
            "Expected draw commands for background"
        );

        let _ = handle.update(
            &Modifier::empty()
                .rounded_corners(4.0)
                .then(Modifier::empty().background(Color(0.9, 0.1, 0.1, 1.0))),
        );
        let slices = collect_modifier_slices(handle.chain());
        assert!(
            !slices.draw_commands().is_empty(),
            "Expected draw commands after update"
        );

        let _ = handle.update(&Modifier::empty());
        let slices = collect_modifier_slices(handle.chain());
        assert!(
            slices.draw_commands().is_empty(),
            "Expected no draw commands with empty modifier"
        );
    }

    #[test]
    fn capability_mask_updates_with_chain() {
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(&Modifier::empty().padding(4.0));
        assert_eq!(handle.capabilities(), NodeCapabilities::LAYOUT);
        assert!(handle.has_layout_nodes());
        assert!(!handle.has_draw_nodes());
        handle.take_invalidations();

        let color = Color(0.5, 0.6, 0.7, 1.0);
        let _ = handle.update(&Modifier::empty().background(color));
        assert_eq!(handle.capabilities(), NodeCapabilities::DRAW);
        assert!(handle.has_draw_nodes());
        assert!(!handle.has_layout_nodes());
    }

    fn node_ptr<N: ModifierNode + 'static>(handle: &ModifierChainHandle) -> *const N {
        handle
            .chain()
            .node::<N>(0)
            .map(|node| &*node as *const N)
            .expect("expected node to exist")
    }
}
