use std::{
    any::{Any, TypeId, type_name_of_val},
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{NodeId, PackedU64};
#[expect(unused_imports, reason = "only some cfgs read it")]
use cranpose_foundation::InvalidationKind;
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierInvalidations, ModifierNode, ModifierNodeChain,
    ModifierNodeContext, NodeCapabilities, PlainElement as _,
};

use super::{
    DimensionConstraint, EdgeInsets, Elements, LayoutProperties, Modifier, ModifierInspectorRecord,
    ModifierKind, ModifierLocalAncestorResolver, ModifierLocalToken, Point, ResolvedModifiers,
    element_match::{self, ElementMatch},
    layout_data::LayoutDataNode,
    local::ModifierLocalManager,
    modifier_debug_enabled,
    offset::OffsetPxNode,
};
mod shared_parent_data;
pub(crate) use shared_parent_data::SharedParentData;

use crate::{
    modifier_nodes::{
        AlignmentElement, FillDirection, FillElement, OffsetElement, PaddingElement, SizeElement,
        WeightElement,
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

pub(crate) type ModifierLocalsHandle = Rc<RefCell<ModifierLocalManager>>;

pub struct ModifierChainHandle {
    chain: ModifierNodeChain,
    layout_direction: crate::LayoutDirection,
    /// What the chain's nodes asked for while they attached on a remount, or
    /// on a new id whose caller reports it later: the node's next chain
    /// operation reports it. A context lives for one operation only; a kept
    /// one took a hundred bytes of every node.
    pending_invalidations: Option<Box<ModifierInvalidations>>,
    /// What the chain tells the parent's measure policy, which reads it on
    /// every pass. The resolved modifiers, which only a subcompose node's
    /// measure and on-request readers take, are worked out when read.
    parent_data: SharedParentData,
    /// `None` until the chain first provides or reads a modifier local, as
    /// most chains never do.
    modifier_locals: Option<ModifierLocalsHandle>,
    debug_logging: bool,
    /// A number no other chain on this thread had, taken again on each
    /// change to the chain's nodes, their layout direction or their
    /// attachment: a reader that keeps what it derived from a chain knows
    /// when to derive it again, also after its node takes a new chain.
    revision: PackedU64,
    // The entries whose values layout reads and an update in place changed
    // since the node's coordinators last took them, one bit an entry: such
    // an update leaves the revision as it is, and a coordinator keeps a
    // copy of a plain element's value.
    changed_in_place: Cell<u64>,
    // The inspector metadata of the modifier last set, which the layout
    // tree's inspection reads.
    #[cfg(feature = "inspection")]
    inspector: Option<Rc<Vec<super::InspectorMetadata>>>,
}

// The inspector metadata `modifier` carries.
#[cfg(feature = "inspection")]
fn inspector_of(modifier: &Modifier) -> Option<Rc<Vec<super::InspectorMetadata>>> {
    match &modifier.kind {
        ModifierKind::Empty => None,
        ModifierKind::Single { inspector, .. } => inspector.clone(),
    }
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
    // The node's own measure, which reads the element's new value: a size
    // enters nothing the node's parent reads.
    Measure,
    /// The node's measure and the parent data the chain gives its parent.
    Resolved,
}

/// An update in place of a modifier's chain: how far it reaches, and the
/// elements whose nodes take it, one bit an element index.
#[derive(Clone, Copy)]
pub(crate) struct InPlaceUpdate {
    pub(crate) reach: InPlaceReach,
    pub(crate) elements: u64,
}

impl InPlaceReach {
    /// The reach of an update in place of an element of `element_type`;
    /// `None` when something else reads its values and a change to it
    /// updates the whole chain.
    pub(crate) fn of(element_type: TypeId) -> Option<Self> {
        if element_type == TypeId::of::<TextModifierElement>() {
            Some(Self::Node)
        } else if element_type == TypeId::of::<SizeElement>() {
            Some(Self::Measure)
        } else if element_type == TypeId::of::<FillElement>() {
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
            parent_data: SharedParentData::default(),
            modifier_locals: None,
            debug_logging: false,
            revision: PackedU64::new(next_revision()),
            changed_in_place: Cell::new(0),
            #[cfg(feature = "inspection")]
            inspector: None,
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
        self.revision.get()
    }

    // The entries an update in place changed since the last call, one bit
    // an entry; see the field.
    pub(crate) fn take_changed_in_place(&self) -> u64 {
        self.changed_in_place.take()
    }

    pub(crate) fn set_layout_direction(&mut self, direction: crate::LayoutDirection) -> bool {
        if self.layout_direction == direction {
            return false;
        }
        self.layout_direction = direction;
        self.revision = PackedU64::new(next_revision());
        self.update_offset_direction();
        self.resolve();
        true
    }

    fn update_offset_direction(&mut self) {
        let direction = self.layout_direction;
        self.chain.visit_nodes_mut(|node, capabilities| {
            if capabilities.contains(NodeCapabilities::DRAW)
                && let Some(offset) = node.as_any_mut().downcast_mut::<OffsetPxNode>()
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

    /// Updates in place the chain's elements `update` names, for a modifier
    /// that differs from the last one only in those, as a text's string or
    /// an animated fill fraction does. `false` when the chain cannot take it
    /// so; the caller then updates the whole chain.
    pub(crate) fn update_elements_in_place(
        &mut self,
        modifier: &Modifier,
        update: InPlaceUpdate,
        context: &mut BasicModifierNodeContext,
    ) -> bool {
        // A chain that reads modifier locals syncs them on every update.
        if self
            .chain
            .capabilities()
            .contains(NodeCapabilities::MODIFIER_LOCALS)
        {
            return false;
        }
        let updated = self.chain.update_entries_in_place(
            modifier.iter_elements(),
            modifier.element_count(),
            update.elements,
            context,
        );
        #[cfg(feature = "inspection")]
        if updated {
            self.inspector = inspector_of(modifier);
        }
        if updated && update.reach != InPlaceReach::Node {
            self.changed_in_place
                .set(self.changed_in_place.get() | update.elements);
        }
        if updated && update.reach == InPlaceReach::Resolved {
            self.resolve_fills();
        }
        updated
    }

    // Asks `context` for what an update of the last node's values
    // invalidates, after its caller updated the node in place.
    pub(crate) fn invalidate_last(&self, context: &mut BasicModifierNodeContext) {
        self.chain.invalidate_last(context);
    }

    /// Reconciles the chain with `modifier`, its nodes asking `context` for
    /// what they invalidate. Returns the invalidations of its modifier locals.
    pub(crate) fn update_with_resolver(
        &mut self,
        modifier: &Modifier,
        resolver: &mut ModifierLocalAncestorResolver<'_>,
        context: &mut BasicModifierNodeContext,
    ) -> ModifierInvalidations {
        self.revision = PackedU64::new(next_revision());
        self.chain
            .update_from_ref_iter(modifier.iter_elements(), context);
        #[cfg(feature = "inspection")]
        {
            self.inspector = inspector_of(modifier);
        }
        if self.layout_direction.is_rtl() {
            self.update_offset_direction();
        }
        let modifier_local_invalidations = self.sync_modifier_locals(resolver);

        if !context.invalidations().is_empty() {
            self.resolve();
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
        self.revision = PackedU64::new(next_revision());
        if id.is_some() {
            self.chain.detach_nodes();
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
        self.revision = PackedU64::new(next_revision());
        let mut context = self.context(node_id);
        self.chain.attach_nodes(&mut context);
        self.keep_for_next(context);
    }

    /// Returns the modifier node chain for read-only traversal.
    pub fn chain(&self) -> &ModifierNodeChain {
        &self.chain
    }

    /// Returns mutable access to the modifier node chain.
    pub fn chain_mut(&mut self) -> &mut ModifierNodeChain {
        self.revision = PackedU64::new(next_revision());
        &mut self.chain
    }

    // Whether the chain holds `modifier`'s elements, as `rule` compares them.
    pub(crate) fn holds(&self, modifier: &Modifier, rule: ElementMatch) -> bool {
        element_match::elements_match(self.chain.held_elements(), modifier, rule)
    }

    // The node after `modifier`'s elements, when the chain holds them, as
    // they compare equal, and that one node more.
    pub(crate) fn node_after(&self, modifier: &Modifier) -> Option<&Rc<RefCell<dyn ModifierNode>>> {
        element_match::elements_match_but_last(
            self.chain.held_elements(),
            modifier,
            ElementMatch::Equal,
        )
        .then(|| self.chain.get_node_rc(modifier.element_count()))
        .flatten()
    }

    // How `modifier`'s elements compare with the chain's, for a set of it.
    pub(crate) fn match_for_set(&self, modifier: &Modifier) -> element_match::SetMatch {
        element_match::match_for_set(self.chain.held_elements(), modifier)
    }

    // A modifier of the chain's elements, equal to the one the chain was last
    // updated from. It carries the modifier's inspector metadata only where
    // inspection is built in.
    pub(crate) fn modifier(&self) -> Modifier {
        let mut held = self.chain.held_elements();
        let Some(first) = held.next() else {
            return Modifier::empty();
        };
        let elements = if held.len() == 0 {
            Elements::One(first.to_element())
        } else {
            Elements::Many(Rc::new(
                std::iter::once(first)
                    .chain(held)
                    .map(|element| element.to_element())
                    .collect(),
            ))
        };
        Modifier {
            provides_composition_locals: elements
                .iter()
                .any(|element| element.provides_composition_locals()),
            kind: ModifierKind::Single {
                elements,
                #[cfg(feature = "inspection")]
                inspector: self.inspector.clone(),
                #[cfg(not(feature = "inspection"))]
                inspector: None,
            },
        }
    }

    /// Returns the aggregated capability mask for the reconciled chain.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.chain.capabilities()
    }

    /// Returns the aggregate child capability mask rooted at the sentinel head.
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.chain.head().aggregate_child_capabilities()
    }

    pub fn has_layout_nodes(&self) -> bool {
        self.capabilities().contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_nodes(&self) -> bool {
        self.capabilities().contains(NodeCapabilities::DRAW)
    }

    /// The padding, layout properties and offset of the chain, worked out
    /// from its layout nodes.
    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        let resolution = self.resolution();
        let mut resolved = ResolvedModifiers::default();
        resolved.set_padding(resolution.padding);
        resolved.set_layout_properties(resolution.layout);
        resolved.set_offset(resolution.offset);
        resolved
    }

    /// What the chain tells the parent's measure policy about its node.
    /// The outermost node of each kind wins, as Compose folds
    /// `modifyParentData` from the innermost node out.
    pub(crate) fn parent_data(&self) -> SharedParentData {
        self.parent_data.clone()
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
            // A plain element has no node: its element type names it.
            let type_name = match node_ref.plain_words() {
                Some((fns, _)) => Some(fns.type_name()),
                None => node_ref.with_node(type_name_of_val),
            };
            let Some(type_name) = type_name else {
                return;
            };
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
                type_name,
                capabilities: node_ref.kind_set(),
                aggregate_child_capabilities: node_ref.aggregate_child_capabilities(),
                inspector,
            });
        });
        snapshot
    }

    // One walk over the chain's layout nodes, from the outermost in.
    fn resolution(&self) -> Resolution {
        let mut resolution = Resolution {
            direction: self.layout_direction,
            ..Resolution::default()
        };
        self.chain
            .for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
                match node_ref.plain_words() {
                    Some((fns, words)) => resolution.take_plain(fns, words),
                    None => {
                        node_ref.with_node(|node| resolution.take(node.as_any()));
                    }
                }
            });
        resolution
    }

    // Works out again the fill fractions of the chain's parent data, after
    // an update in place: of the elements one takes, only a fill enters the
    // parent data, so every other part of the data stands.
    fn resolve_fills(&mut self) {
        let mut fill_width = None;
        let mut fill_height = None;
        self.chain
            .for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
                if let Some(fill) = node_ref.plain::<FillElement>() {
                    let (horizontal, vertical) = fill.fills();
                    if horizontal {
                        fill_width.get_or_insert_with(|| fill.fraction());
                    }
                    if vertical {
                        fill_height.get_or_insert_with(|| fill.fraction());
                    }
                }
            });
        let width = fill_width.unwrap_or_default();
        let height = fill_height.unwrap_or_default();
        let data = self.parent_data.read(|current| {
            (current.fill_width_fraction != width || current.fill_height_fraction != height).then(
                || cranpose_ui_layout::ParentData {
                    fill_width_fraction: width,
                    fill_height_fraction: height,
                    ..current.clone()
                },
            )
        });
        if let Some(data) = data {
            self.parent_data.set(data);
        }
    }

    /// Works out the chain's parent data again.
    fn resolve(&mut self) {
        let Resolution {
            layout,
            fill_width,
            fill_height,
            layout_data,
            ..
        } = self.resolution();
        let weight = layout.weight.unwrap_or_default();
        let data = cranpose_ui_layout::ParentData {
            weight: weight.weight,
            fill: weight.fill,
            box_alignment: layout.box_alignment,
            row_alignment: layout.row_alignment,
            row_baseline: layout.row_baseline,
            column_alignment: layout.column_alignment,
            fill_width_fraction: fill_width.unwrap_or_default(),
            fill_height_fraction: fill_height.unwrap_or_default(),
            layout_data,
        };
        if !self.parent_data.read(|current| *current == data) {
            self.parent_data.set(data);
        }
    }
}

/// What a walk over a chain's layout nodes, from the outermost in, gathers
/// for its resolved modifiers and parent data. Of each kind the parent reads,
/// the outermost node wins, as Compose folds `modifyParentData` from the
/// innermost node out.
#[derive(Default)]
struct Resolution {
    layout: LayoutProperties,
    padding: EdgeInsets,
    // The direction the chain's node reads, which mirrors its offsets.
    direction: crate::LayoutDirection,
    offset: Point,
    fill_width: Option<f32>,
    fill_height: Option<f32>,
    layout_data: Option<Rc<dyn cranpose_ui_layout::LayoutData>>,
}

impl Resolution {
    /// Takes in the next plain element of the walk, of the type `fns`
    /// tells, from its `words`.
    fn take_plain(&mut self, fns: &cranpose_foundation::PlainFns, words: &[u32]) {
        // One type lookup an element: each `read` asks for it again.
        let kind = fns.element_type();
        if kind == TypeId::of::<PaddingElement>() {
            self.padding += PaddingElement::from_words(words).padding();
        } else if kind == TypeId::of::<SizeElement>() {
            apply_size_node(&mut self.layout, &SizeElement::from_words(words));
        } else if kind == TypeId::of::<FillElement>() {
            self.take_fill(&FillElement::from_words(words));
        } else if kind == TypeId::of::<WeightElement>() {
            self.layout
                .weight
                .get_or_insert_with(|| WeightElement::from_words(words).layout_weight());
        } else if kind == TypeId::of::<AlignmentElement>() {
            apply_alignment(&mut self.layout, &AlignmentElement::from_words(words));
        } else if kind == TypeId::of::<OffsetElement>() {
            let delta = OffsetElement::from_words(words).offset(self.direction);
            self.offset.x += delta.x;
            self.offset.y += delta.y;
        }
    }

    /// Takes in the next node of the walk.
    fn take(&mut self, node: &dyn Any) {
        if self.layout_data.is_none()
            && let Some(data) = node.downcast_ref::<LayoutDataNode>()
        {
            self.layout_data = Some(Rc::clone(data.data()));
        }
    }

    fn take_fill(&mut self, fill: &FillElement) {
        apply_fill_node(&mut self.layout, fill);
        let (horizontal, vertical) = fill.fills();
        if horizontal {
            self.fill_width.get_or_insert_with(|| fill.fraction());
        }
        if vertical {
            self.fill_height.get_or_insert_with(|| fill.fraction());
        }
    }
}
/// Sets the alignments a parent reads from `element` that no outer one set.
fn apply_alignment(layout: &mut LayoutProperties, element: &AlignmentElement) {
    if layout.box_alignment.is_none() {
        layout.box_alignment = element.in_box();
    }
    if layout.column_alignment.is_none() {
        layout.column_alignment = element.in_column();
    }
    // A row alignment and a baseline alignment both set where a Row child
    // sits across it: the outer one of either kind wins.
    if layout.row_alignment.is_none() && !layout.row_baseline {
        layout.row_alignment = element.in_row();
        layout.row_baseline = element.on_row_baseline();
    }
}

fn apply_size_node(layout: &mut LayoutProperties, node: &SizeElement) {
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

fn apply_fill_node(layout: &mut LayoutProperties, node: &FillElement) {
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
