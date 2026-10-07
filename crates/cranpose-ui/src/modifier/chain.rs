#![expect(private_interfaces)]

use std::{any::type_name_of_val, cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
#[expect(unused_imports)]
use cranpose_foundation::InvalidationKind;
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierInvalidations, ModifierNodeChain, ModifierNodeContext,
    NodeCapabilities,
};

use super::{
    DimensionConstraint, EdgeInsets, LayoutProperties, Modifier, ModifierInspectorRecord,
    ModifierLocalAncestorResolver, ModifierLocalToken, Point, ResolvedModifiers,
    local::ModifierLocalManager, modifier_debug_enabled,
};
use crate::modifier_nodes::{
    AlignmentNode, FillDirection, FillNode, OffsetNode, PaddingNode, SizeNode, WeightNode,
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
    layout_direction: crate::LayoutDirection,
    context: RefCell<BasicModifierNodeContext>,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
    aggregate_child_capabilities: NodeCapabilities,
    /// `None` until the chain first provides or reads a modifier local, as
    /// most chains never do.
    modifier_locals: Option<ModifierLocalsHandle>,
    debug_logging: bool,
    /// Counts the changes to the chain's nodes, their layout direction and
    /// their attachment, so a reader that keeps what it derived from the
    /// chain knows when to derive it again.
    revision: u64,
}

impl Default for ModifierChainHandle {
    fn default() -> Self {
        Self {
            chain: ModifierNodeChain::new(),
            layout_direction: crate::LayoutDirection::Ltr,
            context: RefCell::new(BasicModifierNodeContext::new()),
            resolved: ResolvedModifiers::default(),
            capabilities: NodeCapabilities::default(),
            aggregate_child_capabilities: NodeCapabilities::default(),
            modifier_locals: None,
            debug_logging: false,
            revision: 0,
        }
    }
}

impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn for_layout_direction(layout_direction: crate::LayoutDirection) -> Self {
        Self {
            layout_direction,
            ..Self::default()
        }
    }

    pub(crate) fn layout_direction(&self) -> crate::LayoutDirection {
        self.layout_direction
    }

    /// The count of changes to the chain; see the field.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn set_layout_direction(&mut self, direction: crate::LayoutDirection) -> bool {
        if self.layout_direction == direction {
            return false;
        }
        self.layout_direction = direction;
        self.revision += 1;
        self.update_offset_direction();
        self.resolved = self.compute_resolved();
        true
    }

    fn update_offset_direction(&mut self) {
        let direction = self.layout_direction;
        self.chain.visit_nodes_mut(|node, capabilities| {
            if capabilities.contains(NodeCapabilities::LAYOUT)
                && let Some(offset) = node.as_any_mut().downcast_mut::<OffsetNode>()
            {
                offset.set_layout_direction(direction);
            }
        });
    }

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements stored in `modifier`.
    pub fn update(&mut self, modifier: &Modifier) -> ModifierInvalidations {
        let mut resolver = |_: &ModifierLocalToken| None;
        self.update_with_resolver(modifier, &mut resolver)
    }

    /// Updates in place the chain's elements of type `E`, for a modifier
    /// that differs from the last one only in those, as a text's string
    /// does: the elements must not change the chain's capabilities, links,
    /// offsets or resolved layout properties. `false` when the chain cannot
    /// take it so; the caller then updates the whole chain.
    pub(crate) fn update_elements_in_place<E: 'static>(&mut self, modifier: &Modifier) -> bool {
        // A chain that reads modifier locals syncs them on every update.
        if self
            .capabilities
            .contains(NodeCapabilities::MODIFIER_LOCALS)
        {
            return false;
        }
        self.chain.update_entries_of_type(
            modifier.iter_elements(),
            modifier.element_count(),
            std::any::TypeId::of::<E>(),
            &mut *self.context.borrow_mut(),
        )
    }

    pub fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
    ) -> ModifierInvalidations {
        self.revision += 1;
        self.chain
            .update_from_ref_iter(modifier.iter_elements(), &mut *self.context.borrow_mut());
        if self.layout_direction.is_rtl() {
            self.update_offset_direction();
        }
        self.capabilities = self.chain.capabilities();
        self.aggregate_child_capabilities = self.chain.head().aggregate_child_capabilities();
        let modifier_local_invalidations = self.sync_modifier_locals(resolver);

        let needs_resolved_update = {
            let ctx = self.context.borrow();
            !ctx.invalidations().is_empty()
        };
        if needs_resolved_update {
            self.resolved = self.compute_resolved();
        }

        let should_log = self.debug_logging || modifier_debug_enabled();
        if should_log {
            let snapshot = self.inspector_snapshot(modifier);
            crate::debug::log_modifier_chain(self.chain(), &snapshot);
            crate::debug::emit_modifier_chain_trace(&snapshot);
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
        self.revision += 1;

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
        self.revision += 1;
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
        self.revision += 1;
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
    pub fn take_invalidations(&self) -> ModifierInvalidations {
        self.context.borrow_mut().take_invalidations()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved
    }

    /// The chain's modifier locals, or `None` while it has never provided or
    /// read one.
    pub fn modifier_locals_handle(&self) -> Option<ModifierLocalsHandle> {
        self.modifier_locals.clone()
    }

    /// Brings the chain's modifier locals up to date, making their manager
    /// the first time the chain provides or reads one.
    fn sync_modifier_locals(
        &mut self,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
    ) -> ModifierInvalidations {
        if self.modifier_locals.is_none()
            && !self.chain.has_capability(NodeCapabilities::MODIFIER_LOCALS)
        {
            return ModifierInvalidations::new();
        }
        self.modifier_locals
            .get_or_insert_with(ModifierLocalsHandle::default)
            .borrow_mut()
            .sync(&self.chain, resolver)
    }

    /// The chain's nodes in order, with the inspector record of the element
    /// in `modifier` each top-level node came from. Built on request for
    /// debug output, so chains keep no inspector state.
    pub fn inspector_snapshot(&self, modifier: &Modifier) -> Vec<ModifierChainInspectorNode> {
        let chain_len = self.chain.len();
        let mut records: Vec<Option<ModifierInspectorRecord>> = modifier
            .iter_inspector_metadata()
            .take(chain_len)
            .map(|metadata| Some(metadata.to_record()))
            .collect();
        let mut snapshot = Vec::with_capacity(chain_len);
        self.chain.for_each_forward(|node_ref| {
            node_ref.with_node(|node| {
                let depth = node_ref.delegate_depth();
                let entry_index = node_ref.entry_index();
                let inspector = if depth == 0 {
                    entry_index
                        .and_then(|idx| records.get_mut(idx))
                        .and_then(Option::take)
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
                        layout.row_baseline = false;
                    }
                    if alignment_node.row_baseline() {
                        layout.row_alignment = None;
                        layout.row_baseline = true;
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

#[cfg(test)]
#[path = "tests/chain_tests.rs"]
mod tests;
