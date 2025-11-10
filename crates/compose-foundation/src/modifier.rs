//! Modifier node scaffolding for Compose-RS.
//!
//! This module defines the foundational pieces of the future
//! `Modifier.Node` system described in the project roadmap. It introduces
//! traits for modifier nodes and their contexts as well as a light-weight
//! chain container that can reconcile nodes across updates. The
//! implementation focuses on the core runtime plumbing so UI crates can
//! begin migrating without expanding the public API surface.

use std::any::{type_name, Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{BitOr, BitOrAssign};
use std::rc::Rc;

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct NodePath {
    entry: usize,
    delegates: Vec<usize>,
}

impl NodePath {
    fn root(entry: usize) -> Self {
        Self {
            entry,
            delegates: Vec::new(),
        }
    }

    fn from_slice(entry: usize, path: &[usize]) -> Self {
        Self {
            entry,
            delegates: path.to_vec(),
        }
    }

    fn entry(&self) -> usize {
        self.entry
    }

    fn delegates(&self) -> &[usize] {
        &self.delegates
    }

    fn into_parts(self) -> (usize, Vec<usize>) {
        (self.entry, self.delegates)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeLink {
    Head,
    Tail,
    Entry(NodePath),
}

/// Runtime state tracked for every [`ModifierNode`].
#[derive(Debug)]
pub struct NodeState {
    aggregate_child_capabilities: Cell<NodeCapabilities>,
    capabilities: Cell<NodeCapabilities>,
    parent: RefCell<Option<NodeLink>>,
    child: RefCell<Option<NodeLink>>,
    attached: Cell<bool>,
    is_sentinel: bool,
}

impl NodeState {
    pub const fn new() -> Self {
        Self {
            aggregate_child_capabilities: Cell::new(NodeCapabilities::empty()),
            capabilities: Cell::new(NodeCapabilities::empty()),
            parent: RefCell::new(None),
            child: RefCell::new(None),
            attached: Cell::new(false),
            is_sentinel: false,
        }
    }

    pub const fn sentinel() -> Self {
        Self {
            aggregate_child_capabilities: Cell::new(NodeCapabilities::empty()),
            capabilities: Cell::new(NodeCapabilities::empty()),
            parent: RefCell::new(None),
            child: RefCell::new(None),
            attached: Cell::new(true),
            is_sentinel: true,
        }
    }

    pub fn set_capabilities(&self, capabilities: NodeCapabilities) {
        self.capabilities.set(capabilities);
    }

    pub fn capabilities(&self) -> NodeCapabilities {
        self.capabilities.get()
    }

    pub fn set_aggregate_child_capabilities(&self, capabilities: NodeCapabilities) {
        self.aggregate_child_capabilities.set(capabilities);
    }

    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.aggregate_child_capabilities.get()
    }

    pub fn set_parent_link(&self, parent: Option<NodeLink>) {
        *self.parent.borrow_mut() = parent;
    }

    pub fn parent_link(&self) -> Option<NodeLink> {
        self.parent.borrow().clone()
    }

    pub fn set_child_link(&self, child: Option<NodeLink>) {
        *self.child.borrow_mut() = child;
    }

    pub fn child_link(&self) -> Option<NodeLink> {
        self.child.borrow().clone()
    }

    pub fn set_attached(&self, attached: bool) {
        self.attached.set(attached);
    }

    pub fn is_attached(&self) -> bool {
        self.attached.get()
    }

    pub fn is_sentinel(&self) -> bool {
        self.is_sentinel
    }
}

/// Provides traversal helpers that mirror Jetpack Compose's [`DelegatableNode`] contract.
pub trait DelegatableNode {
    fn node_state(&self) -> &NodeState;
    fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.node_state().aggregate_child_capabilities()
    }
}

/// Core trait implemented by modifier nodes.
///
/// Nodes receive lifecycle callbacks when they attach to or detach from a
/// composition and may optionally react to resets triggered by the runtime
/// (for example, when reusing nodes across modifier list changes).
pub trait ModifierNode: Any + DelegatableNode {
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

    /// Visits every delegate node owned by this modifier.
    fn for_each_delegate<'b>(&'b self, _visitor: &mut dyn FnMut(&'b dyn ModifierNode)) {}

    /// Visits every delegate node mutably.
    fn for_each_delegate_mut<'b>(&'b mut self, _visitor: &mut dyn FnMut(&'b mut dyn ModifierNode)) {
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
        let current = self.next.take()?;
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

#[derive(Debug)]
struct ModifierNodeEntry {
    element_type: TypeId,
    key: Option<u64>,
    hash_code: u64,
    element: DynModifierElement,
    node: Box<dyn ModifierNode>,
    capabilities: NodeCapabilities,
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
        let entry = Self {
            element_type,
            key,
            hash_code,
            element,
            node,
            capabilities,
        };
        entry
            .node
            .as_ref()
            .node_state()
            .set_capabilities(entry.capabilities);
        entry
    }
}

fn visit_node_tree(node: &dyn ModifierNode, visitor: &mut dyn FnMut(&dyn ModifierNode)) {
    visitor(node);
    node.for_each_delegate(&mut |child| visit_node_tree(child, visitor));
}

fn visit_node_tree_mut(
    node: &mut dyn ModifierNode,
    visitor: &mut dyn FnMut(&mut dyn ModifierNode),
) {
    visitor(node);
    node.for_each_delegate_mut(&mut |child| visit_node_tree_mut(child, visitor));
}

fn nth_delegate<'a>(node: &'a dyn ModifierNode, target: usize) -> Option<&'a dyn ModifierNode> {
    let mut current = 0usize;
    let mut result: Option<&dyn ModifierNode> = None;
    node.for_each_delegate(&mut |child| {
        if result.is_none() && current == target {
            result = Some(child);
        }
        current += 1;
    });
    result
}

fn nth_delegate_mut<'a>(
    node: &'a mut dyn ModifierNode,
    target: usize,
) -> Option<&'a mut dyn ModifierNode> {
    let mut current = 0usize;
    let mut result: Option<&mut dyn ModifierNode> = None;
    node.for_each_delegate_mut(&mut |child| {
        if result.is_none() && current == target {
            result = Some(child);
        }
        current += 1;
    });
    result
}

fn attach_node_tree(node: &mut dyn ModifierNode, context: &mut dyn ModifierNodeContext) {
    visit_node_tree_mut(node, &mut |n| {
        if !n.node_state().is_attached() {
            n.node_state().set_attached(true);
            n.on_attach(context);
        }
    });
}

fn reset_node_tree(node: &mut dyn ModifierNode) {
    visit_node_tree_mut(node, &mut |n| n.on_reset());
}

fn detach_node_tree(node: &mut dyn ModifierNode) {
    visit_node_tree_mut(node, &mut |n| {
        if n.node_state().is_attached() {
            n.on_detach();
            n.node_state().set_attached(false);
        }
        n.node_state().set_parent_link(None);
        n.node_state().set_child_link(None);
        n.node_state()
            .set_aggregate_child_capabilities(NodeCapabilities::empty());
    });
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
    head_sentinel: Box<SentinelNode>,
    tail_sentinel: Box<SentinelNode>,
    ordered_nodes: Vec<NodeLink>,
}

struct SentinelNode {
    state: NodeState,
}

impl SentinelNode {
    fn new() -> Self {
        Self {
            state: NodeState::sentinel(),
        }
    }
}

impl DelegatableNode for SentinelNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SentinelNode {}

#[derive(Clone)]
pub struct ModifierChainNodeRef<'a> {
    chain: &'a ModifierNodeChain,
    link: NodeLink,
}

impl Default for ModifierNodeChain {
    fn default() -> Self {
        Self::new()
    }
}

impl ModifierNodeChain {
    pub fn new() -> Self {
        let mut chain = Self {
            entries: Vec::new(),
            aggregated_capabilities: NodeCapabilities::empty(),
            head_aggregate_child_capabilities: NodeCapabilities::empty(),
            head_sentinel: Box::new(SentinelNode::new()),
            tail_sentinel: Box::new(SentinelNode::new()),
            ordered_nodes: Vec::new(),
        };
        chain.sync_chain_links();
        chain
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
                    if !entry_mut.node.node_state().is_attached() {
                        attach_node_tree(entry_mut.node.as_mut(), context);
                    }

                    if !same_element {
                        element.update_node(entry_mut.node.as_mut());
                    }

                    entry_mut.key = key;
                    entry_mut.element = element.clone();
                    entry_mut.element_type = element_type;
                    entry_mut.hash_code = hash_code;
                    entry_mut.capabilities = capabilities;
                    entry_mut
                        .node
                        .as_ref()
                        .node_state()
                        .set_capabilities(entry_mut.capabilities);
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
                    attach_node_tree(entry_mut.node.as_mut(), context);
                    element.update_node(entry_mut.node.as_mut());
                }
                new_entries.push(entry);
            }
        }

        for mut entry in old_entries {
            detach_node_tree(entry.node.as_mut());
        }

        self.entries = new_entries;
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
            reset_node_tree(entry.node.as_mut());
        }
    }

    /// Detaches every node in the chain and clears internal storage.
    pub fn detach_all(&mut self) {
        for mut entry in std::mem::take(&mut self.entries) {
            detach_node_tree(entry.node.as_mut());
            let state = entry.node.as_ref().node_state();
            state.set_capabilities(NodeCapabilities::empty());
        }
        self.aggregated_capabilities = NodeCapabilities::empty();
        self.head_aggregate_child_capabilities = NodeCapabilities::empty();
        self.ordered_nodes.clear();
        self.sync_chain_links();
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
        self.make_node_ref(NodeLink::Head)
    }

    /// Returns the sentinel tail reference for traversal.
    pub fn tail(&self) -> ModifierChainNodeRef<'_> {
        self.make_node_ref(NodeLink::Tail)
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
            Some(self.make_node_ref(NodeLink::Entry(NodePath::root(index))))
        }
    }

    /// Returns the node reference that owns `node`.
    pub fn find_node_ref(&self, node: &dyn ModifierNode) -> Option<ModifierChainNodeRef<'_>> {
        fn node_data_ptr(node: &dyn ModifierNode) -> *const () {
            node as *const dyn ModifierNode as *const ()
        }

        let target = node_data_ptr(node);
        for (index, entry) in self.entries.iter().enumerate() {
            if node_data_ptr(entry.node.as_ref()) == target {
                return Some(self.make_node_ref(NodeLink::Entry(NodePath::root(index))));
            }
        }

        self.ordered_nodes.iter().find_map(|link| {
            if matches!(link, NodeLink::Entry(path) if path.delegates().is_empty()) {
                return None;
            }
            let candidate = self.resolve_node(link);
            if node_data_ptr(candidate) == target {
                Some(self.make_node_ref(link.clone()))
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

    /// Visits every node in insertion order together with its capability mask.
    pub fn visit_nodes<F>(&self, mut f: F)
    where
        F: FnMut(&dyn ModifierNode, NodeCapabilities),
    {
        for link in &self.ordered_nodes {
            let node = self.resolve_node(link);
            f(node, node.node_state().capabilities());
        }
    }

    /// Visits every node mutably in insertion order together with its capability mask.
    pub fn visit_nodes_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut dyn ModifierNode, NodeCapabilities),
    {
        for index in 0..self.ordered_nodes.len() {
            let link = self.ordered_nodes[index].clone();
            let node = self.resolve_node_mut_from_link(link);
            let capabilities = node.node_state().capabilities();
            f(node, capabilities);
        }
    }

    fn make_node_ref(&self, link: NodeLink) -> ModifierChainNodeRef<'_> {
        ModifierChainNodeRef { chain: self, link }
    }

    fn sync_chain_links(&mut self) {
        self.rebuild_ordered_nodes();

        self.head_sentinel.node_state().set_parent_link(None);
        self.tail_sentinel.node_state().set_child_link(None);

        if self.ordered_nodes.is_empty() {
            self.head_sentinel
                .node_state()
                .set_child_link(Some(NodeLink::Tail));
            self.tail_sentinel
                .node_state()
                .set_parent_link(Some(NodeLink::Head));
            self.aggregated_capabilities = NodeCapabilities::empty();
            self.head_aggregate_child_capabilities = NodeCapabilities::empty();
            self.head_sentinel
                .node_state()
                .set_aggregate_child_capabilities(NodeCapabilities::empty());
            self.tail_sentinel
                .node_state()
                .set_aggregate_child_capabilities(NodeCapabilities::empty());
            return;
        }

        let mut previous = NodeLink::Head;
        for link in self.ordered_nodes.iter().cloned() {
            self.node_state_for_link(&previous)
                .set_child_link(Some(link.clone()));
            self.node_state_for_link(&link)
                .set_parent_link(Some(previous.clone()));
            previous = link;
        }

        self.node_state_for_link(&previous)
            .set_child_link(Some(NodeLink::Tail));
        self.tail_sentinel
            .node_state()
            .set_parent_link(Some(previous.clone()));
        self.tail_sentinel.node_state().set_child_link(None);

        let mut aggregate = NodeCapabilities::empty();
        for link in self.ordered_nodes.iter().rev() {
            let node = self.resolve_node(link);
            aggregate |= node.node_state().capabilities();
            node.node_state()
                .set_aggregate_child_capabilities(aggregate);
        }

        self.aggregated_capabilities = aggregate;
        self.head_aggregate_child_capabilities = aggregate;
        self.head_sentinel
            .node_state()
            .set_aggregate_child_capabilities(aggregate);
        self.tail_sentinel
            .node_state()
            .set_aggregate_child_capabilities(NodeCapabilities::empty());
    }

    fn rebuild_ordered_nodes(&mut self) {
        self.ordered_nodes.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            let mut path = Vec::new();
            Self::enumerate_link_order(
                entry.node.as_ref(),
                index,
                &mut path,
                &mut self.ordered_nodes,
            );
        }
    }

    fn enumerate_link_order(
        node: &dyn ModifierNode,
        entry: usize,
        path: &mut Vec<usize>,
        out: &mut Vec<NodeLink>,
    ) {
        out.push(NodeLink::Entry(NodePath::from_slice(entry, path)));
        let mut delegate_index = 0usize;
        node.for_each_delegate(&mut |child| {
            path.push(delegate_index);
            Self::enumerate_link_order(child, entry, path, out);
            path.pop();
            delegate_index += 1;
        });
    }

    fn node_state_for_link(&self, link: &NodeLink) -> &NodeState {
        match link {
            NodeLink::Head => self.head_sentinel.node_state(),
            NodeLink::Tail => self.tail_sentinel.node_state(),
            NodeLink::Entry(path) => self.resolve_entry_node(path).node_state(),
        }
    }

    fn resolve_node(&self, link: &NodeLink) -> &dyn ModifierNode {
        match link {
            NodeLink::Head => self.head_sentinel.as_ref(),
            NodeLink::Tail => self.tail_sentinel.as_ref(),
            NodeLink::Entry(path) => self.resolve_entry_node(path),
        }
    }

    fn resolve_node_mut_from_link(&mut self, link: NodeLink) -> &mut dyn ModifierNode {
        match link {
            NodeLink::Head => self.head_sentinel.as_mut(),
            NodeLink::Tail => self.tail_sentinel.as_mut(),
            NodeLink::Entry(path) => self.resolve_entry_node_mut(path),
        }
    }

    fn resolve_entry_node(&self, path: &NodePath) -> &dyn ModifierNode {
        let mut node: &dyn ModifierNode = self.entries[path.entry()].node.as_ref();
        for &delegate_index in path.delegates() {
            node = nth_delegate(node, delegate_index).expect("invalid delegate path");
        }
        node
    }

    fn resolve_entry_node_mut(&mut self, path: NodePath) -> &mut dyn ModifierNode {
        let (entry, delegates) = path.into_parts();
        let mut node: &mut dyn ModifierNode = self.entries[entry].node.as_mut();
        for delegate_index in delegates {
            node = nth_delegate_mut(node, delegate_index).expect("invalid delegate path");
        }
        node
    }
}

impl<'a> ModifierChainNodeRef<'a> {
    fn raw_node(&self) -> &'a dyn ModifierNode {
        self.chain.resolve_node(&self.link)
    }

    fn state(&self) -> &'a NodeState {
        self.chain.node_state_for_link(&self.link)
    }

    /// Returns the underlying modifier node when this reference targets a real entry.
    pub fn node(&self) -> Option<&'a dyn ModifierNode> {
        if self.state().is_sentinel() {
            None
        } else {
            Some(self.raw_node())
        }
    }

    /// Returns the parent reference, including sentinel head when applicable.
    pub fn parent(&self) -> Option<Self> {
        self.state()
            .parent_link()
            .map(|link| self.chain.make_node_ref(link))
    }

    /// Returns the child reference, including sentinel tail for the last entry.
    pub fn child(&self) -> Option<Self> {
        self.state()
            .child_link()
            .map(|link| self.chain.make_node_ref(link))
    }

    /// Returns the capability mask for this specific node.
    pub fn kind_set(&self) -> NodeCapabilities {
        if self.state().is_sentinel() {
            NodeCapabilities::empty()
        } else {
            self.state().capabilities()
        }
    }

    /// Returns the aggregated capability mask for the subtree rooted at this node.
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        if self.is_tail() {
            NodeCapabilities::empty()
        } else {
            self.state().aggregate_child_capabilities()
        }
    }

    /// Returns true if this reference targets the sentinel head.
    pub fn is_head(&self) -> bool {
        matches!(self.link, NodeLink::Head)
    }

    /// Returns true if this reference targets the sentinel tail.
    pub fn is_tail(&self) -> bool {
        matches!(self.link, NodeLink::Tail)
    }

    /// Returns true if this reference targets either sentinel.
    pub fn is_sentinel(&self) -> bool {
        self.state().is_sentinel()
    }

    /// Returns true if this node has any capability bits present in `mask`.
    pub fn has_capability(&self, mask: NodeCapabilities) -> bool {
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
            if !node.is_sentinel() {
                f(node.clone());
            }
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
            f(node.clone());
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
