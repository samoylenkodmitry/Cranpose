#![expect(private_interfaces)]

use std::{any::type_name_of_val, cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
#[expect(unused_imports)]
use cranpose_foundation::InvalidationKind;
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierInvalidation, ModifierNodeChain, ModifierNodeContext,
    NodeCapabilities,
};

use super::{
    DimensionConstraint, EdgeInsets, LayoutProperties, Modifier, ModifierInspectorRecord,
    ModifierLocalAncestorResolver, ModifierLocalToken, Point, ResolvedModifiers,
    local::ModifierLocalManager, modifier_debug_enabled,
};
use crate::modifier_nodes::{
    AlignmentNode, FillDirection, FillNode, IntrinsicAxis, IntrinsicSizeNode, OffsetNode,
    PaddingNode, SizeNode, WeightNode,
};

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

pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    context: RefCell<BasicModifierNodeContext>,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
    aggregate_child_capabilities: NodeCapabilities,
    modifier_locals: ModifierLocalsHandle,
    inspector_snapshot: Vec<ModifierChainInspectorNode>,
    inspector_entry_scratch: Vec<Option<ModifierInspectorRecord>>,
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
            inspector_entry_scratch: Vec::new(),
            debug_logging: false,
        }
    }
}

impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements stored in `modifier`.
    pub fn update(&mut self, modifier: &Modifier) -> Vec<ModifierInvalidation> {
        let mut resolver = |_: &ModifierLocalToken| None;
        self.update_with_resolver(modifier, &mut resolver)
    }

    pub fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
    ) -> Vec<ModifierInvalidation> {
        self.chain
            .update_from_ref_iter(modifier.iter_elements(), &mut *self.context.borrow_mut());
        self.capabilities = self.chain.capabilities();
        self.aggregate_child_capabilities = self.chain.head().aggregate_child_capabilities();
        let modifier_local_invalidations = self
            .modifier_locals
            .borrow_mut()
            .sync(&self.chain, resolver);

        let needs_resolved_update = {
            let ctx = self.context.borrow();
            !ctx.invalidations().is_empty()
        };
        if needs_resolved_update {
            self.resolved = self.compute_resolved();
        }

        let should_log = self.debug_logging || modifier_debug_enabled();
        if should_log {
            self.collect_inspector_snapshot(modifier);
            crate::debug::log_modifier_chain(self.chain(), self.inspector_snapshot());
            crate::debug::emit_modifier_chain_trace(self.inspector_snapshot());
        }
        modifier_local_invalidations
    }

    /// Enables or disables per-handle modifier debug logging.
    pub fn set_debug_logging(&mut self, enabled: bool) {
        self.debug_logging = enabled;
    }

    pub fn set_node_id(&mut self, id: Option<NodeId>) {
        let old_id = self.context.borrow().node_id();
        if old_id == id {
            return;
        }

        self.context.borrow_mut().set_node_id(id);

        if id.is_some() {
            self.chain.detach_nodes();
            self.chain.repair_chain();
            self.chain.attach_nodes(&mut *self.context.borrow_mut());
        }
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Returns mutable access to the modifier node chain.
    pub fn chain_mut(&mut self) -> &mut ModifierNodeChain {
        &mut self.chain
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

    /// Drains invalidations requested during the last update cycle.
    pub fn take_invalidations(&self) -> Vec<ModifierInvalidation> {
        self.context.borrow_mut().take_invalidations()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved
    }

    pub fn modifier_locals_handle(&self) -> ModifierLocalsHandle {
        Rc::clone(&self.modifier_locals)
    }

    pub fn inspector_snapshot(&self) -> &[ModifierChainInspectorNode] {
        &self.inspector_snapshot
    }

    #[cfg(test)]
    pub fn refresh_inspector_snapshot(&mut self, modifier: &Modifier) {
        self.collect_inspector_snapshot(modifier);
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
            });
        });

        resolved.set_padding(padding);
        resolved.set_layout_properties(layout);
        resolved.set_offset(offset);
        resolved
    }

    fn collect_inspector_snapshot(&mut self, modifier: &Modifier) {
        if self.chain.is_empty() {
            self.inspector_snapshot.clear();
            return;
        }

        let chain_len = self.chain.len();
        self.inspector_entry_scratch.clear();
        self.inspector_entry_scratch.resize_with(chain_len, || None);
        for (index, metadata) in modifier.iter_inspector_metadata().enumerate() {
            if index >= chain_len {
                break;
            }
            self.inspector_entry_scratch[index] = Some(metadata.to_record());
        }

        self.inspector_snapshot.clear();
        self.inspector_snapshot.reserve(chain_len);

        let chain = &self.chain;
        let inspector_entry_scratch = &mut self.inspector_entry_scratch;
        let inspector_snapshot = &mut self.inspector_snapshot;

        chain.for_each_forward(|node_ref| {
            node_ref.with_node(|node| {
                let depth = node_ref.delegate_depth();
                let entry_index = node_ref.entry_index();
                let inspector = if depth == 0 {
                    entry_index
                        .and_then(|idx| inspector_entry_scratch.get_mut(idx))
                        .and_then(Option::take)
                } else {
                    None
                };
                inspector_snapshot.push(ModifierChainInspectorNode {
                    depth,
                    entry_index,
                    type_name: type_name_of_val(node),
                    capabilities: node_ref.kind_set(),
                    aggregate_child_capabilities: node_ref.aggregate_child_capabilities(),
                    inspector,
                });
            });
        });
    }

    /// Access a text field modifier node in the chain with a mutable callback.
    ///
    /// Searches for `TextFieldModifierNode` and calls the callback if found.
    /// Returns `None` if no text field modifier is in the chain.
    pub fn with_text_field_modifier_mut<R>(
        &mut self,
        mut f: impl FnMut(&mut crate::TextFieldModifierNode) -> R,
    ) -> Option<R> {
        let mut result = None;
        self.chain.visit_nodes_mut(|node, capabilities| {
            if capabilities.contains(NodeCapabilities::LAYOUT) {
                let any = node.as_any_mut();
                if let Some(text_field) = any.downcast_mut::<crate::TextFieldModifierNode>()
                    && result.is_none()
                {
                    result = Some(f(text_field));
                }
            }
        });
        result
    }
}
fn apply_size_node(layout: &mut LayoutProperties, node: &SizeNode) {
    apply_size_axis(
        node.min_width(),
        node.max_width(),
        node.enforce_incoming(),
        &mut layout.width,
        &mut layout.min_width,
        &mut layout.max_width,
    );
    apply_size_axis(
        node.min_height(),
        node.max_height(),
        node.enforce_incoming(),
        &mut layout.height,
        &mut layout.min_height,
        &mut layout.max_height,
    );
}

/// One axis of a size node: a range keeps its two bounds, a fixed side sets
/// the dimension, and a required size sets both.
fn apply_size_axis(
    min: Option<f32>,
    max: Option<f32>,
    enforce_incoming: bool,
    dimension: &mut DimensionConstraint,
    min_slot: &mut Option<f32>,
    max_slot: &mut Option<f32>,
) {
    let fixed = min == max;
    if let (true, Some(size)) = (fixed, max.or(min)) {
        *dimension = DimensionConstraint::Points(size);
    }
    if fixed && enforce_incoming {
        return;
    }
    if let Some(min) = min {
        *min_slot = Some(min);
    }
    if let Some(max) = max {
        *max_slot = Some(max);
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

#[cfg(test)]
#[path = "tests/chain_tests.rs"]
mod tests;
