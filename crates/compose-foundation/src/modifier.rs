//! Modifier node scaffolding for Compose-RS.
//!
//! This module defines the foundational pieces of the future
//! `Modifier.Node` system described in the project roadmap. It introduces
//! traits for modifier nodes and their contexts as well as a light-weight
//! chain container that can reconcile nodes across updates. The
//! implementation focuses on the core runtime plumbing so UI crates can
//! begin migrating without expanding the public API surface.

use std::any::{type_name, Any, TypeId};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{BitOr, BitOrAssign};
use std::rc::Rc;
use std::slice::{Iter, IterMut};

pub use compose_ui_graphics::DrawScope;
pub use compose_ui_graphics::Size;
pub use compose_ui_layout::{Constraints, Measurable};

use crate::nodes::input::types::PointerEvent;

/// Identifies which part of the rendering pipeline should be invalidated
/// after a modifier node changes state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidationKind {
    Layout,
    Draw,
    PointerInput,
    Semantics,
}

/// Runtime services exposed to modifier nodes while attached to a tree.
pub trait ModifierNodeContext {
    /// Requests that a particular pipeline stage be invalidated.
    fn invalidate(&mut self, _kind: InvalidationKind) {}

    /// Requests that the node's `update` method run again outside of a
    /// regular composition pass.
    fn request_update(&mut self) {}
}

/// Lightweight [`ModifierNodeContext`] implementation that records
/// invalidation requests and update signals.
///
/// The context intentionally avoids leaking runtime details so the core
/// crate can evolve independently from higher level UI crates. It simply
/// stores the sequence of requested invalidation kinds and whether an
/// explicit update was requested. Callers can inspect or drain this state
/// after driving a [`ModifierNodeChain`] reconciliation pass.
#[derive(Default, Debug, Clone)]
pub struct BasicModifierNodeContext {
    invalidations: Vec<InvalidationKind>,
    update_requested: bool,
}

impl BasicModifierNodeContext {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the ordered list of invalidation kinds that were requested
    /// since the last call to [`clear_invalidations`]. Duplicate requests for
    /// the same kind are coalesced.
    pub fn invalidations(&self) -> &[InvalidationKind] {
        &self.invalidations
    }

    /// Removes all currently recorded invalidation kinds.
    pub fn clear_invalidations(&mut self) {
        self.invalidations.clear();
    }

    /// Drains the recorded invalidations and returns them to the caller.
    pub fn take_invalidations(&mut self) -> Vec<InvalidationKind> {
        std::mem::take(&mut self.invalidations)
    }

    /// Returns whether an update was requested since the last call to
    /// [`take_update_requested`].
    pub fn update_requested(&self) -> bool {
        self.update_requested
    }

    /// Returns whether an update was requested and clears the flag.
    pub fn take_update_requested(&mut self) -> bool {
        std::mem::take(&mut self.update_requested)
    }

    fn push_invalidation(&mut self, kind: InvalidationKind) {
        if !self.invalidations.contains(&kind) {
            self.invalidations.push(kind);
        }
    }
}

impl ModifierNodeContext for BasicModifierNodeContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        self.push_invalidation(kind);
    }

    fn request_update(&mut self) {
        self.update_requested = true;
    }
}

/// Core trait implemented by modifier nodes.
///
/// Nodes receive lifecycle callbacks when they attach to or detach from a
/// composition and may optionally react to resets triggered by the runtime
/// (for example, when reusing nodes across modifier list changes).
pub trait ModifierNode: Any {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {}

    fn on_detach(&mut self) {}

    fn on_reset(&mut self) {}

    /// Returns this node as a draw modifier if it implements the trait.
    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        None
    }

    /// Returns this node as a mutable draw modifier if it implements the trait.
    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        None
    }

    /// Returns this node as a pointer-input modifier if it implements the trait.
    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        None
    }

    /// Returns this node as a mutable pointer-input modifier if it implements the trait.
    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        None
    }

    /// Returns this node as a semantics modifier if it implements the trait.
    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> {
        None
    }

    /// Returns this node as a mutable semantics modifier if it implements the trait.
    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNode> {
        None
    }
}

/// Marker trait for layout-specific modifier nodes.
///
/// Layout nodes participate in the measure and layout passes of the render
/// pipeline. They can intercept and modify the measurement and placement of
/// their wrapped content.
pub trait LayoutModifierNode: ModifierNode {
    /// Measures the wrapped content and returns the size this modifier
    /// occupies. The node receives a measurable representing the wrapped
    /// content and the incoming constraints from the parent.
    ///
    /// The default implementation delegates to the wrapped content without
    /// modification.
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> Size {
        // Default: pass through to wrapped content by measuring the child.
        let placeable = measurable.measure(constraints);
        Size {
            width: placeable.width(),
            height: placeable.height(),
        }
    }

    /// Returns the minimum intrinsic width of this modifier node.
    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        0.0
    }

    /// Returns the maximum intrinsic width of this modifier node.
    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        0.0
    }

    /// Returns the minimum intrinsic height of this modifier node.
    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        0.0
    }

    /// Returns the maximum intrinsic height of this modifier node.
    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        0.0
    }
}

/// Marker trait for draw-specific modifier nodes.
///
/// Draw nodes participate in the draw pass of the render pipeline. They can
/// intercept and modify the drawing operations of their wrapped content.
pub trait DrawModifierNode: ModifierNode {
    /// Draws this modifier node. The node can draw before and/or after
    /// calling `draw_content` to draw the wrapped content.
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // Default: draw wrapped content without modification
    }
}

/// Marker trait for pointer input modifier nodes.
///
/// Pointer input nodes participate in hit-testing and pointer event
/// dispatch. They can intercept pointer events and handle them before
/// they reach the wrapped content.
pub trait PointerInputNode: ModifierNode {
    /// Called when a pointer event occurs within the bounds of this node.
    /// Returns true if the event was consumed and should not propagate further.
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _event: &PointerEvent,
    ) -> bool {
        false
    }

    /// Returns true if this node should participate in hit-testing for the
    /// given pointer position.
    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        true
    }

    /// Returns an event handler closure if the node wants to participate in pointer dispatch.
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        None
    }
}

/// Marker trait for semantics modifier nodes.
///
/// Semantics nodes participate in the semantics tree construction. They can
/// add or modify semantic properties of their wrapped content for
/// accessibility and testing purposes.
pub trait SemanticsNode: ModifierNode {
    /// Merges semantic properties into the provided configuration.
    fn merge_semantics(&self, _config: &mut SemanticsConfiguration) {
        // Default: no semantics added
    }
}

/// Semantics configuration for accessibility.
#[derive(Clone, Debug, Default)]
pub struct SemanticsConfiguration {
    pub content_description: Option<String>,
    pub is_button: bool,
    pub is_clickable: bool,
}

impl SemanticsConfiguration {
    pub fn merge(&mut self, other: &SemanticsConfiguration) {
        if let Some(description) = &other.content_description {
            self.content_description = Some(description.clone());
        }
        self.is_button |= other.is_button;
        self.is_clickable |= other.is_clickable;
    }
}

impl fmt::Debug for dyn ModifierNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifierNode").finish_non_exhaustive()
    }
}

impl dyn ModifierNode {
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Strongly typed modifier elements that can create and update nodes while
/// exposing equality/hash/inspector contracts that mirror Jetpack Compose.
pub trait ModifierNodeElement: fmt::Debug + Hash + PartialEq + 'static {
    type Node: ModifierNode;

    /// Creates a new modifier node instance for this element.
    fn create(&self) -> Self::Node;

    /// Brings an existing modifier node up to date with the element's data.
    fn update(&self, node: &mut Self::Node);

    /// Optional key used to disambiguate multiple instances of the same element type.
    fn key(&self) -> Option<u64> {
        None
    }

    /// Human readable name surfaced to inspector tooling.
    fn inspector_name(&self) -> &'static str {
        type_name::<Self>()
    }

    /// Records inspector properties for tooling.
    fn inspector_properties(&self, _inspector: &mut dyn FnMut(&'static str, String)) {}

    /// Returns the capabilities of nodes created by this element.
    /// Override this to indicate which specialized traits the node implements.
    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }
}

/// Transitional alias so existing call sites that refer to `ModifierElement`
/// keep compiling while the ecosystem migrates to `ModifierNodeElement`.
pub trait ModifierElement: ModifierNodeElement {}

impl<T> ModifierElement for T where T: ModifierNodeElement {}

/// Capability flags indicating which specialized traits a modifier node implements.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeCapabilities(u32);

impl NodeCapabilities {
    /// No capabilities.
    pub const NONE: Self = Self(0);
    /// Modifier participates in measure/layout.
    pub const LAYOUT: Self = Self(1 << 0);
    /// Modifier participates in draw.
    pub const DRAW: Self = Self(1 << 1);
    /// Modifier participates in pointer input.
    pub const POINTER_INPUT: Self = Self(1 << 2);
    /// Modifier participates in semantics tree construction.
    pub const SEMANTICS: Self = Self(1 << 3);
    /// Modifier participates in modifier locals.
    pub const MODIFIER_LOCALS: Self = Self(1 << 4);

    /// Returns an empty capability set.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Returns whether all bits in `other` are present in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns whether any bit in `other` is present in `self`.
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Inserts the requested capability bits.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Returns the raw bit representation.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Returns true when no capabilities are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the capability bit mask required for the given invalidation.
    pub const fn for_invalidation(kind: InvalidationKind) -> Self {
        match kind {
            InvalidationKind::Layout => Self::LAYOUT,
            InvalidationKind::Draw => Self::DRAW,
            InvalidationKind::PointerInput => Self::POINTER_INPUT,
            InvalidationKind::Semantics => Self::SEMANTICS,
        }
    }
}

impl Default for NodeCapabilities {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Debug for NodeCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeCapabilities")
            .field("layout", &self.contains(Self::LAYOUT))
            .field("draw", &self.contains(Self::DRAW))
            .field("pointer_input", &self.contains(Self::POINTER_INPUT))
            .field("semantics", &self.contains(Self::SEMANTICS))
            .field("modifier_locals", &self.contains(Self::MODIFIER_LOCALS))
            .finish()
    }
}

impl BitOr for NodeCapabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for NodeCapabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Type-erased modifier element used by the runtime to reconcile chains.
pub trait AnyModifierElement: fmt::Debug {
    fn node_type(&self) -> TypeId;

    fn element_type(&self) -> TypeId;

    fn create_node(&self) -> Box<dyn ModifierNode>;

    fn update_node(&self, node: &mut dyn ModifierNode);

    fn key(&self) -> Option<u64>;

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }

    fn hash_code(&self) -> u64;

    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool;

    fn inspector_name(&self) -> &'static str;

    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String));

    fn as_any(&self) -> &dyn Any;
}

struct TypedModifierElement<E: ModifierNodeElement> {
    element: E,
}

impl<E: ModifierNodeElement> TypedModifierElement<E> {
    fn new(element: E) -> Self {
        Self { element }
    }
}

impl<E> fmt::Debug for TypedModifierElement<E>
where
    E: ModifierNodeElement,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedModifierElement")
            .field("type", &type_name::<E>())
            .finish()
    }
}

impl<E> AnyModifierElement for TypedModifierElement<E>
where
    E: ModifierNodeElement,
{
    fn node_type(&self) -> TypeId {
        TypeId::of::<E::Node>()
    }

    fn element_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn create_node(&self) -> Box<dyn ModifierNode> {
        Box::new(self.element.create())
    }

    fn update_node(&self, node: &mut dyn ModifierNode) {
        let typed = node
            .as_any_mut()
            .downcast_mut::<E::Node>()
            .expect("modifier node type mismatch");
        self.element.update(typed);
    }

    fn key(&self) -> Option<u64> {
        self.element.key()
    }

    fn capabilities(&self) -> NodeCapabilities {
        self.element.capabilities()
    }

    fn hash_code(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.element.hash(&mut hasher);
        hasher.finish()
    }

    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .map(|typed| typed.element == self.element)
            .unwrap_or(false)
    }

    fn inspector_name(&self) -> &'static str {
        self.element.inspector_name()
    }

    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String)) {
        self.element.inspector_properties(visitor);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Convenience helper for callers to construct a type-erased modifier
/// element without having to mention the internal wrapper type.
pub fn modifier_element<E: ModifierNodeElement>(element: E) -> DynModifierElement {
    Rc::new(TypedModifierElement::new(element))
}

/// Boxed type-erased modifier element.
pub type DynModifierElement = Rc<dyn AnyModifierElement>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainPosition {
    Head,
    Tail,
    Entry(usize),
}

#[derive(Clone, Copy)]
enum TraversalDirection {
    Forward,
    Backward,
}

/// Iterator walking a modifier chain either from head-to-tail or tail-to-head.
pub struct ModifierChainIter<'a> {
    next: Option<ModifierChainNodeRef<'a>>,
    direction: TraversalDirection,
}

impl<'a> ModifierChainIter<'a> {
    fn new(start: Option<ModifierChainNodeRef<'a>>, direction: TraversalDirection) -> Self {
        Self {
            next: start,
            direction,
        }
    }
}

impl<'a> Iterator for ModifierChainIter<'a> {
    type Item = ModifierChainNodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;
        if current.is_sentinel() {
            self.next = None;
            return None;
        }
        self.next = match self.direction {
            TraversalDirection::Forward => current.child(),
            TraversalDirection::Backward => current.parent(),
        };
        Some(current)
    }
}

impl<'a> std::iter::FusedIterator for ModifierChainIter<'a> {}

#[derive(Clone, Copy, Debug)]
struct NodeLinks {
    parent: ChainPosition,
    child: ChainPosition,
}

impl Default for NodeLinks {
    fn default() -> Self {
        Self {
            parent: ChainPosition::Head,
            child: ChainPosition::Tail,
        }
    }
}

struct ModifierNodeEntry {
    element_type: TypeId,
    key: Option<u64>,
    hash_code: u64,
    element: DynModifierElement,
    node: Box<dyn ModifierNode>,
    attached: bool,
    capabilities: NodeCapabilities,
    links: NodeLinks,
    aggregate_child_capabilities: NodeCapabilities,
}

impl ModifierNodeEntry {
    fn new(
        element_type: TypeId,
        key: Option<u64>,
        element: DynModifierElement,
        node: Box<dyn ModifierNode>,
        hash_code: u64,
        capabilities: NodeCapabilities,
    ) -> Self {
        Self {
            element_type,
            key,
            hash_code,
            element,
            node,
            attached: false,
            capabilities,
            links: NodeLinks::default(),
            aggregate_child_capabilities: NodeCapabilities::empty(),
        }
    }

    fn matches_invalidation(&self, kind: InvalidationKind) -> bool {
        self.capabilities
            .contains(NodeCapabilities::for_invalidation(kind))
    }

    fn draw_node(&self) -> Option<&dyn DrawModifierNode> {
        self.node.as_ref().as_draw_node()
    }

    fn draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        self.node.as_mut().as_draw_node_mut()
    }

    fn pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        self.node.as_ref().as_pointer_input_node()
    }

    fn pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        self.node.as_mut().as_pointer_input_node_mut()
    }
}

/// Chain of modifier nodes attached to a layout node.
///
/// The chain tracks ownership of modifier nodes and reuses them across
/// updates when the incoming element list still contains a node of the
/// same type. Removed nodes detach automatically so callers do not need
/// to manually manage their lifetimes.
pub struct ModifierNodeChain {
    entries: Vec<Box<ModifierNodeEntry>>,
    aggregated_capabilities: NodeCapabilities,
    head_aggregate_child_capabilities: NodeCapabilities,
}

#[derive(Clone, Copy)]
pub struct ModifierChainNodeRef<'a> {
    chain: &'a ModifierNodeChain,
    position: ChainPosition,
}

impl Default for ModifierNodeChain {
    fn default() -> Self {
        Self::new()
    }
}

impl ModifierNodeChain {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            aggregated_capabilities: NodeCapabilities::empty(),
            head_aggregate_child_capabilities: NodeCapabilities::empty(),
        }
    }

    /// Reconcile the chain against the provided elements, attaching newly
    /// created nodes and detaching nodes that are no longer required.
    pub fn update_from_slice(
        &mut self,
        elements: &[DynModifierElement],
        context: &mut dyn ModifierNodeContext,
    ) {
        let mut old_entries = std::mem::take(&mut self.entries);
        let mut new_entries: Vec<Box<ModifierNodeEntry>> = Vec::with_capacity(elements.len());
        let mut aggregated = NodeCapabilities::empty();

        for element in elements {
            let element_type = element.element_type();
            let key = element.key();
            let hash_code = element.hash_code();
            let capabilities = element.capabilities();
            let mut same_element = false;
            let mut reused_entry: Option<Box<ModifierNodeEntry>> = None;

            if let Some(key_value) = key {
                if let Some(index) = old_entries.iter().position(|entry| {
                    entry.element_type == element_type && entry.key == Some(key_value)
                }) {
                    let entry = old_entries.remove(index);
                    same_element = entry.element.as_ref().equals_element(element.as_ref());
                    reused_entry = Some(entry);
                }
            } else if let Some(index) = old_entries.iter().position(|entry| {
                entry.key.is_none()
                    && entry.hash_code == hash_code
                    && entry.element.as_ref().equals_element(element.as_ref())
            }) {
                let entry = old_entries.remove(index);
                same_element = true;
                reused_entry = Some(entry);
            } else if let Some(index) = old_entries
                .iter()
                .position(|entry| entry.element_type == element_type && entry.key.is_none())
            {
                let entry = old_entries.remove(index);
                same_element = entry.element.as_ref().equals_element(element.as_ref());
                reused_entry = Some(entry);
            }

            if let Some(mut entry) = reused_entry {
                {
                    let entry_mut = entry.as_mut();
                    if !entry_mut.attached {
                        entry_mut.node.on_attach(context);
                        entry_mut.attached = true;
                    }

                    if !same_element {
                        element.update_node(entry_mut.node.as_mut());
                    }

                    entry_mut.key = key;
                    entry_mut.element = element.clone();
                    entry_mut.element_type = element_type;
                    entry_mut.hash_code = hash_code;
                    entry_mut.capabilities = capabilities;
                    aggregated |= entry_mut.capabilities;
                }
                new_entries.push(entry);
            } else {
                let mut entry = Box::new(ModifierNodeEntry::new(
                    element_type,
                    key,
                    element.clone(),
                    element.create_node(),
                    hash_code,
                    capabilities,
                ));
                {
                    let entry_mut = entry.as_mut();
                    entry_mut.node.on_attach(context);
                    entry_mut.attached = true;
                    element.update_node(entry_mut.node.as_mut());
                    aggregated |= entry_mut.capabilities;
                }
                new_entries.push(entry);
            }
        }

        for mut entry in old_entries {
            if entry.attached {
                entry.node.on_detach();
                entry.attached = false;
            }
        }

        self.entries = new_entries;
        self.aggregated_capabilities = aggregated;
        self.sync_chain_links();
    }

    /// Convenience wrapper that accepts any iterator of type-erased
    /// modifier elements. Elements are collected into a temporary vector
    /// before reconciliation.
    pub fn update<I>(&mut self, elements: I, context: &mut dyn ModifierNodeContext)
    where
        I: IntoIterator<Item = DynModifierElement>,
    {
        let collected: Vec<DynModifierElement> = elements.into_iter().collect();
        self.update_from_slice(&collected, context);
    }

    /// Resets all nodes in the chain. This mirrors the behaviour of
    /// Jetpack Compose's `onReset` callback.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            entry.node.on_reset();
        }
    }

    /// Detaches every node in the chain and clears internal storage.
    pub fn detach_all(&mut self) {
        for mut entry in std::mem::take(&mut self.entries) {
            if entry.attached {
                entry.node.on_detach();
                entry.attached = false;
            }
        }
        self.aggregated_capabilities = NodeCapabilities::empty();
        self.head_aggregate_child_capabilities = NodeCapabilities::empty();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the aggregated capability mask for the entire chain.
    pub fn capabilities(&self) -> NodeCapabilities {
        self.aggregated_capabilities
    }

    /// Returns true if the chain contains at least one node with the requested capability.
    pub fn has_capability(&self, capability: NodeCapabilities) -> bool {
        self.aggregated_capabilities.contains(capability)
    }

    /// Returns the sentinel head reference for traversal.
    pub fn head(&self) -> ModifierChainNodeRef<'_> {
        self.make_node_ref(ChainPosition::Head)
    }

    /// Returns the sentinel tail reference for traversal.
    pub fn tail(&self) -> ModifierChainNodeRef<'_> {
        self.make_node_ref(ChainPosition::Tail)
    }

    /// Iterates over the chain from head to tail, skipping sentinels.
    pub fn head_to_tail(&self) -> ModifierChainIter<'_> {
        ModifierChainIter::new(self.head().child(), TraversalDirection::Forward)
    }

    /// Iterates over the chain from tail to head, skipping sentinels.
    pub fn tail_to_head(&self) -> ModifierChainIter<'_> {
        ModifierChainIter::new(self.tail().parent(), TraversalDirection::Backward)
    }

    /// Calls `f` for every node in insertion order.
    pub fn for_each_forward<F>(&self, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        for node in self.head_to_tail() {
            f(node);
        }
    }

    /// Calls `f` for every node containing any capability from `mask`.
    pub fn for_each_forward_matching<F>(&self, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        if mask.is_empty() {
            self.for_each_forward(f);
            return;
        }

        if !self.head().aggregate_child_capabilities().intersects(mask) {
            return;
        }

        for node in self.head_to_tail() {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        }
    }

    /// Calls `f` for every node in reverse insertion order.
    pub fn for_each_backward<F>(&self, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        for node in self.tail_to_head() {
            f(node);
        }
    }

    /// Calls `f` for every node in reverse order that matches `mask`.
    pub fn for_each_backward_matching<F>(&self, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>),
    {
        if mask.is_empty() {
            self.for_each_backward(f);
            return;
        }

        if !self.head().aggregate_child_capabilities().intersects(mask) {
            return;
        }

        for node in self.tail_to_head() {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        }
    }

    /// Returns a node reference for the entry at `index`.
    pub fn node_ref_at(&self, index: usize) -> Option<ModifierChainNodeRef<'_>> {
        if index >= self.entries.len() {
            None
        } else {
            Some(self.make_node_ref(ChainPosition::Entry(index)))
        }
    }

    /// Returns the node reference that owns `node`.
    pub fn find_node_ref(&self, node: &dyn ModifierNode) -> Option<ModifierChainNodeRef<'_>> {
        #[inline]
        fn data_ptr(node: &dyn ModifierNode) -> *const () {
            node as *const dyn ModifierNode as *const ()
        }

        let target = data_ptr(node);
        self.entries.iter().enumerate().find_map(|(index, entry)| {
            if data_ptr(entry.node.as_ref()) == target {
                Some(self.make_node_ref(ChainPosition::Entry(index)))
            } else {
                None
            }
        })
    }

    /// Downcasts the node at `index` to the requested type.
    pub fn node<N: ModifierNode + 'static>(&self, index: usize) -> Option<&N> {
        self.entries
            .get(index)
            .and_then(|entry| entry.node.as_ref().as_any().downcast_ref::<N>())
    }

    /// Downcasts the node at `index` to the requested mutable type.
    pub fn node_mut<N: ModifierNode + 'static>(&mut self, index: usize) -> Option<&mut N> {
        self.entries
            .get_mut(index)
            .and_then(|entry| entry.node.as_mut().as_any_mut().downcast_mut::<N>())
    }

    /// Returns true if the chain contains any nodes matching the given invalidation kind.
    pub fn has_nodes_for_invalidation(&self, kind: InvalidationKind) -> bool {
        self.aggregated_capabilities
            .contains(NodeCapabilities::for_invalidation(kind))
    }

    /// Iterates over all layout nodes in the chain.
    pub fn layout_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.capabilities.contains(NodeCapabilities::LAYOUT))
            .map(|entry| entry.node.as_ref())
    }

    /// Iterates over all draw nodes in the chain.
    pub fn draw_nodes(&self) -> DrawNodes<'_> {
        DrawNodes::new(self.entries.iter())
    }

    /// Iterates over all mutable draw nodes in the chain.
    pub fn draw_nodes_mut(&mut self) -> DrawNodesMut<'_> {
        DrawNodesMut::new(self.entries.iter_mut())
    }

    /// Iterates over all pointer input nodes in the chain.
    pub fn pointer_input_nodes(&self) -> PointerInputNodes<'_> {
        PointerInputNodes::new(self.entries.iter())
    }

    /// Iterates over all mutable pointer input nodes in the chain.
    pub fn pointer_input_nodes_mut(&mut self) -> PointerInputNodesMut<'_> {
        PointerInputNodesMut::new(self.entries.iter_mut())
    }

    /// Iterates over all semantics nodes in the chain.
    pub fn semantics_nodes(&self) -> impl Iterator<Item = &dyn ModifierNode> {
        self.entries
            .iter()
            .filter(|entry| entry.capabilities.contains(NodeCapabilities::SEMANTICS))
            .map(|entry| entry.node.as_ref())
    }

    /// Visits every node in insertion order together with its capability mask.
    pub fn visit_nodes<F>(&self, mut f: F)
    where
        F: FnMut(&dyn ModifierNode, NodeCapabilities),
    {
        for entry in &self.entries {
            f(entry.node.as_ref(), entry.capabilities);
        }
    }

    /// Visits every node mutably in insertion order together with its capability mask.
    pub fn visit_nodes_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut dyn ModifierNode, NodeCapabilities),
    {
        for entry in &mut self.entries {
            f(entry.node.as_mut(), entry.capabilities);
        }
    }

    fn make_node_ref(&self, position: ChainPosition) -> ModifierChainNodeRef<'_> {
        ModifierChainNodeRef {
            chain: self,
            position,
        }
    }

    fn head_child_position(&self) -> ChainPosition {
        if self.entries.is_empty() {
            ChainPosition::Tail
        } else {
            ChainPosition::Entry(0)
        }
    }

    fn tail_parent_position(&self) -> ChainPosition {
        if self.entries.is_empty() {
            ChainPosition::Head
        } else {
            ChainPosition::Entry(self.entries.len() - 1)
        }
    }

    fn sync_chain_links(&mut self) {
        let len = self.entries.len();
        if len == 0 {
            self.head_aggregate_child_capabilities = NodeCapabilities::empty();
            return;
        }

        for (index, entry) in self.entries.iter_mut().enumerate() {
            let parent = if index == 0 {
                ChainPosition::Head
            } else {
                ChainPosition::Entry(index - 1)
            };
            let child = if index + 1 == len {
                ChainPosition::Tail
            } else {
                ChainPosition::Entry(index + 1)
            };
            entry.links = NodeLinks { parent, child };
        }

        let mut aggregate = NodeCapabilities::empty();
        for entry in self.entries.iter_mut().rev() {
            aggregate |= entry.capabilities;
            entry.aggregate_child_capabilities = aggregate;
        }
        self.head_aggregate_child_capabilities = aggregate;
    }
}

/// Iterator over draw modifier nodes stored in a [`ModifierNodeChain`].
pub struct DrawNodes<'a> {
    entries: Iter<'a, Box<ModifierNodeEntry>>,
}

impl<'a> DrawNodes<'a> {
    fn new(entries: Iter<'a, Box<ModifierNodeEntry>>) -> Self {
        Self { entries }
    }
}

impl<'a> Iterator for DrawNodes<'a> {
    type Item = &'a dyn DrawModifierNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.entries.next() {
            if let Some(node) = entry.draw_node() {
                return Some(node);
            }
        }
        None
    }
}

/// Mutable iterator over draw modifier nodes.
pub struct DrawNodesMut<'a> {
    entries: IterMut<'a, Box<ModifierNodeEntry>>,
}

impl<'a> DrawNodesMut<'a> {
    fn new(entries: IterMut<'a, Box<ModifierNodeEntry>>) -> Self {
        Self { entries }
    }
}

impl<'a> Iterator for DrawNodesMut<'a> {
    type Item = &'a mut dyn DrawModifierNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.entries.next() {
            if let Some(node) = entry.draw_node_mut() {
                return Some(node);
            }
        }
        None
    }
}

/// Iterator over pointer-input modifier nodes.
pub struct PointerInputNodes<'a> {
    entries: Iter<'a, Box<ModifierNodeEntry>>,
}

impl<'a> PointerInputNodes<'a> {
    fn new(entries: Iter<'a, Box<ModifierNodeEntry>>) -> Self {
        Self { entries }
    }
}

impl<'a> Iterator for PointerInputNodes<'a> {
    type Item = &'a dyn PointerInputNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.entries.next() {
            if let Some(node) = entry.pointer_input_node() {
                return Some(node);
            }
        }
        None
    }
}

/// Mutable iterator over pointer-input modifier nodes.
pub struct PointerInputNodesMut<'a> {
    entries: IterMut<'a, Box<ModifierNodeEntry>>,
}

impl<'a> PointerInputNodesMut<'a> {
    fn new(entries: IterMut<'a, Box<ModifierNodeEntry>>) -> Self {
        Self { entries }
    }
}

impl<'a> Iterator for PointerInputNodesMut<'a> {
    type Item = &'a mut dyn PointerInputNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.entries.next() {
            if let Some(node) = entry.pointer_input_node_mut() {
                return Some(node);
            }
        }
        None
    }
}

impl<'a> ModifierChainNodeRef<'a> {
    /// Returns the underlying modifier node when this reference targets a real entry.
    pub fn node(self) -> Option<&'a dyn ModifierNode> {
        match self.position {
            ChainPosition::Entry(index) => Some(self.chain.entries[index].node.as_ref()),
            _ => None,
        }
    }

    /// Returns the parent reference, including sentinel head when applicable.
    pub fn parent(self) -> Option<Self> {
        match self.position {
            ChainPosition::Head => None,
            ChainPosition::Tail => {
                Some(self.chain.make_node_ref(self.chain.tail_parent_position()))
            }
            ChainPosition::Entry(index) => Some(
                self.chain
                    .make_node_ref(self.chain.entries[index].links.parent),
            ),
        }
    }

    /// Returns the child reference, including sentinel tail for the last entry.
    pub fn child(self) -> Option<Self> {
        match self.position {
            ChainPosition::Tail => None,
            ChainPosition::Head => Some(self.chain.make_node_ref(self.chain.head_child_position())),
            ChainPosition::Entry(index) => Some(
                self.chain
                    .make_node_ref(self.chain.entries[index].links.child),
            ),
        }
    }

    /// Returns the capability mask for this specific node.
    pub fn kind_set(self) -> NodeCapabilities {
        match self.position {
            ChainPosition::Entry(index) => self.chain.entries[index].capabilities,
            _ => NodeCapabilities::empty(),
        }
    }

    /// Returns the aggregated capability mask for the subtree rooted at this node.
    pub fn aggregate_child_capabilities(self) -> NodeCapabilities {
        match self.position {
            ChainPosition::Head => self.chain.head_aggregate_child_capabilities,
            ChainPosition::Entry(index) => self.chain.entries[index].aggregate_child_capabilities,
            ChainPosition::Tail => NodeCapabilities::empty(),
        }
    }

    /// Returns true if this reference targets the sentinel head.
    pub fn is_head(self) -> bool {
        matches!(self.position, ChainPosition::Head)
    }

    /// Returns true if this reference targets the sentinel tail.
    pub fn is_tail(self) -> bool {
        matches!(self.position, ChainPosition::Tail)
    }

    /// Returns true if this reference targets either sentinel.
    pub fn is_sentinel(self) -> bool {
        self.is_head() || self.is_tail()
    }

    /// Returns true if this node has any capability bits present in `mask`.
    pub fn has_capability(self, mask: NodeCapabilities) -> bool {
        !mask.is_empty() && self.kind_set().intersects(mask)
    }

    /// Visits descendant nodes, optionally including `self`, in insertion order.
    pub fn visit_descendants<F>(self, include_self: bool, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'a>),
    {
        let mut current = if include_self {
            Some(self)
        } else {
            self.child()
        };
        while let Some(node) = current {
            if node.is_tail() {
                break;
            }
            f(node);
            current = node.child();
        }
    }

    /// Visits descendant nodes that match `mask`, short-circuiting when possible.
    pub fn visit_descendants_matching<F>(self, include_self: bool, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'a>),
    {
        if mask.is_empty() {
            self.visit_descendants(include_self, f);
            return;
        }

        if !self.aggregate_child_capabilities().intersects(mask) {
            return;
        }

        self.visit_descendants(include_self, |node| {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        });
    }

    /// Visits ancestor nodes up to (but excluding) the sentinel head.
    pub fn visit_ancestors<F>(self, include_self: bool, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'a>),
    {
        let mut current = if include_self {
            Some(self)
        } else {
            self.parent()
        };
        while let Some(node) = current {
            if node.is_head() {
                break;
            }
            f(node);
            current = node.parent();
        }
    }

    /// Visits ancestor nodes that match `mask`.
    pub fn visit_ancestors_matching<F>(self, include_self: bool, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'a>),
    {
        if mask.is_empty() {
            self.visit_ancestors(include_self, f);
            return;
        }

        self.visit_ancestors(include_self, |node| {
            if node.kind_set().intersects(mask) {
                f(node);
            }
        });
    }
}

#[cfg(test)]
#[path = "tests/modifier_tests.rs"]
mod tests;
