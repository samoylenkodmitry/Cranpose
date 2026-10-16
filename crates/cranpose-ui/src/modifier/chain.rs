use std::{
    any::{TypeId, type_name_of_val},
    cell::RefCell,
    rc::Rc,
};

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
use crate::{
    modifier_nodes::{
        AlignmentNode, FillDirection, FillElement, FillNode, OffsetNode, PaddingNode, SizeElement,
        SizeNode, WeightNode,
    },
    text_modifier_node::TextModifierElement,
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
    /// What the chain's nodes asked for while they attached on a remount, or
    /// on a new id whose caller reports it later: the node's next chain
    /// operation reports it. A context lives for one operation only; a kept
    /// one took a hundred bytes of every node.
    pending_invalidations: Option<Box<ModifierInvalidations>>,
    resolved: ResolvedModifiers,
    capabilities: NodeCapabilities,
    aggregate_child_capabilities: NodeCapabilities,
    /// `None` until the chain first provides or reads a modifier local, as
    /// most chains never do.
    modifier_locals: Option<ModifierLocalsHandle>,
    debug_logging: bool,
    /// A number no other chain on this thread had, taken again on each
    /// change to the chain's nodes, their layout direction or their
    /// attachment: a reader that keeps what it derived from a chain knows
    /// when to derive it again, also after its node takes a new chain.
    revision: u64,
}

/// What an update in place of a modifier element changes beyond its own
/// node. A chain takes a change in place only in elements whose values the
/// node's slices, coordinators, semantics and registry entry do not read:
/// those stay as they were.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InPlaceReach {
    /// Nothing else: a text's slices read its layout through a handle that
    /// the update changes in place.
    Node,
    /// The chain's resolved layout properties and the parent data they give.
    Resolved,
}

impl InPlaceReach {
    /// The reach of an update in place of an element of `element_type`;
    /// `None` when something else reads its values and a change to it
    /// updates the whole chain.
    pub(crate) fn of(element_type: TypeId) -> Option<Self> {
        if element_type == TypeId::of::<TextModifierElement>() {
            Some(Self::Node)
        } else if element_type == TypeId::of::<SizeElement>()
            || element_type == TypeId::of::<FillElement>()
        {
            Some(Self::Resolved)
        } else {
            None
        }
    }
}

impl Default for ModifierChainHandle {
    fn default() -> Self {
        Self::with_chain(ModifierNodeChain::new())
    }
}

fn next_revision() -> u64 {
    crate::thread_state::cells(|cells| {
        let revision = cells.last_modifier_revision.get() + 1;
        cells.last_modifier_revision.set(revision);
        revision
    })
}

impl ModifierChainHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_chain(chain: ModifierNodeChain) -> Self {
        Self {
            chain,
            layout_direction: crate::LayoutDirection::Ltr,
            pending_invalidations: None,
            resolved: ResolvedModifiers::default(),
            capabilities: NodeCapabilities::default(),
            aggregate_child_capabilities: NodeCapabilities::default(),
            modifier_locals: None,
            debug_logging: false,
            revision: next_revision(),
        }
    }

    /// The handle of a layout node's chain: the chain's nodes attach once
    /// the node has its id, at [`Self::move_to_node`].
    pub(crate) fn detached() -> Self {
        Self::with_chain(ModifierNodeChain::detached())
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

    /// The chain's current revision; see the field.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn set_layout_direction(&mut self, direction: crate::LayoutDirection) -> bool {
        if self.layout_direction == direction {
            return false;
        }
        self.layout_direction = direction;
        self.revision = next_revision();
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

    /// Reconciles the underlying [`ModifierNodeChain`] with the elements
    /// stored in `modifier`, for no node. Returns the invalidations its nodes
    /// requested, then those of its modifier locals.
    pub fn update(&mut self, modifier: &Modifier) -> ModifierInvalidations {
        self.update_on_node(None, modifier)
    }

    /// [`Self::update`] for the chain of node `node_id`.
    pub(crate) fn update_on_node(
        &mut self,
        node_id: Option<NodeId>,
        modifier: &Modifier,
    ) -> ModifierInvalidations {
        let mut resolver = |_: &ModifierLocalToken| None;
        let mut context = self.context(node_id);
        let locals = self.update_with_resolver(modifier, &mut resolver, &mut context);
        let mut invalidations = context.take_invalidations();
        invalidations.extend(locals);
        invalidations
    }

    /// A context for an operation on the chain of node `node_id`, holding
    /// what the chain's nodes asked for since the last one.
    pub(crate) fn context(&mut self, node_id: Option<NodeId>) -> BasicModifierNodeContext {
        let pending = self
            .pending_invalidations
            .take()
            .map_or_else(ModifierInvalidations::new, |pending| *pending);
        BasicModifierNodeContext::for_node(node_id, pending)
    }

    /// Keeps what `context` holds for the chain's next operation.
    fn keep_for_next(&mut self, mut context: BasicModifierNodeContext) {
        let invalidations = context.take_invalidations();
        if !invalidations.is_empty() {
            self.pending_invalidations = Some(Box::new(invalidations));
        }
    }

    /// Updates in place the chain's elements that [`InPlaceReach::of`]
    /// accepts, for a modifier that differs from the last one only in those
    /// with `reach`, as a text's string or an animated fill fraction does.
    /// `false` when the chain cannot take it so; the caller then updates the
    /// whole chain.
    pub(crate) fn update_elements_in_place(
        &mut self,
        modifier: &Modifier,
        reach: InPlaceReach,
        context: &mut BasicModifierNodeContext,
    ) -> bool {
        // A chain that reads modifier locals syncs them on every update.
        if self
            .capabilities
            .contains(NodeCapabilities::MODIFIER_LOCALS)
        {
            return false;
        }
        let updated = self.chain.update_entries_in_place(
            modifier.iter_elements(),
            modifier.element_count(),
            |element_type| InPlaceReach::of(element_type).is_some(),
            context,
        );
        if updated && reach == InPlaceReach::Resolved {
            self.resolved = self.compute_resolved();
        }
        updated
    }

    /// Reconciles the chain with `modifier`, its nodes asking `context` for
    /// what they invalidate. Returns the invalidations of its modifier locals.
    pub(crate) fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
        context: &mut BasicModifierNodeContext,
    ) -> ModifierInvalidations {
        self.revision = next_revision();
        self.chain
            .update_from_ref_iter(modifier.iter_elements(), context);
        if self.layout_direction.is_rtl() {
            self.update_offset_direction();
        }
        self.capabilities = self.chain.capabilities();
        self.aggregate_child_capabilities = self.chain.head().aggregate_child_capabilities();
        let modifier_local_invalidations = self.sync_modifier_locals(resolver);

        if !context.invalidations().is_empty() {
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

    /// Moves the chain from node `previous` to the node `context` is for,
    /// attaching its nodes again for the new id.
    pub(crate) fn move_to_node(
        &mut self,
        previous: Option<NodeId>,
        context: &mut BasicModifierNodeContext,
    ) {
        let id = context.node_id();
        if previous == id {
            return;
        }
        self.revision = next_revision();
        if id.is_some() {
            self.chain.detach_nodes();
            self.chain.repair_chain();
            self.chain.attach_nodes(context);
        }
    }

    /// [`Self::move_to_node`] for a caller that reports what the nodes ask
    /// for at its next operation.
    pub(crate) fn move_to_node_later(&mut self, previous: Option<NodeId>, id: Option<NodeId>) {
        let mut context = self.context(id);
        self.move_to_node(previous, &mut context);
        self.keep_for_next(context);
    }

    /// Attaches the chain's nodes again on a remount of node `node_id`; what
    /// they ask for, its next chain operation reports.
    pub(crate) fn remount(&mut self, node_id: Option<NodeId>) {
        self.revision = next_revision();
        let mut context = self.context(node_id);
        self.chain.repair_chain();
        self.chain.attach_nodes(&mut context);
        self.keep_for_next(context);
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Returns mutable access to the modifier node chain.
    pub fn chain_mut(&mut self) -> &mut ModifierNodeChain {
        self.revision = next_revision();
        &mut self.chain
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

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved
    }

    /// What the chain tells the parent's measure policy about its node.
    /// The outermost node of each kind wins, as Compose folds
    /// `modifyParentData` from the innermost node out.
    pub(crate) fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        let props = self.resolved.layout_properties();
        let weight = props.weight().unwrap_or_default();
        let mut data = cranpose_ui_layout::ParentData {
            weight: weight.weight,
            fill: weight.fill,
            box_alignment: props.box_alignment(),
            row_alignment: props.row_alignment(),
            row_baseline: props.row_baseline(),
            column_alignment: props.column_alignment(),
            ..Default::default()
        };
        let (mut fill_width, mut fill_height) = (None, None);
        self.chain
            .for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
                node_ref.with_node(|node| {
                    let any = node.as_any();
                    if let Some(fill) = any.downcast_ref::<FillNode>() {
                        let (horizontal, vertical) = match fill.direction() {
                            FillDirection::Horizontal => (true, false),
                            FillDirection::Vertical => (false, true),
                            FillDirection::Both => (true, true),
                        };
                        if horizontal {
                            fill_width.get_or_insert_with(|| fill.fraction());
                        }
                        if vertical {
                            fill_height.get_or_insert_with(|| fill.fraction());
                        }
                    } else if data.layout_data.is_none()
                        && let Some(node) = any.downcast_ref::<super::layout_data::LayoutDataNode>()
                    {
                        data.layout_data = Some(Rc::clone(node.data()));
                    }
                });
            });
        data.fill_width_fraction = fill_width.unwrap_or_default();
        data.fill_height_fraction = fill_height.unwrap_or_default();
        data
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
                    // Parent data goes to the outermost node, as Compose
                    // folds `modifyParentData` from the innermost node out.
                    layout
                        .weight
                        .get_or_insert_with(|| weight_node.layout_weight());
                } else if let Some(alignment_node) = any.downcast_ref::<AlignmentNode>() {
                    apply_alignment_node(&mut layout, alignment_node);
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
}
/// Sets the alignments a parent reads from `node` that no outer node set.
fn apply_alignment_node(layout: &mut LayoutProperties, node: &AlignmentNode) {
    if layout.box_alignment.is_none() {
        layout.box_alignment = node.box_alignment();
    }
    if layout.column_alignment.is_none() {
        layout.column_alignment = node.column_alignment();
    }
    // A row alignment and a baseline alignment both set where a Row child
    // sits across it: the outer one of either kind wins.
    if layout.row_alignment.is_none() && !layout.row_baseline {
        layout.row_alignment = node.row_alignment();
        layout.row_baseline = node.row_baseline();
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
