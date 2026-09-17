//! Modifier node scaffolding for Cranpose.
//!
//! This module defines the foundational pieces of the Cranpose
//! `Modifier.Node` system. It introduces traits for modifier nodes and their
//! contexts as well as a lightweight chain container that reconciles nodes
//! across updates.

use std::{
    any::{Any, TypeId, type_name},
    cell::{Cell, RefCell},
    fmt,
    hash::{Hash, Hasher},
    ops::{BitOr, BitOrAssign},
    rc::Rc,
};

use cranpose_core::{collections::map::HashMap, hash::default};
pub use cranpose_ui_graphics::{DrawScope, Size};
pub use cranpose_ui_layout::{Constraints, Measurable};

use crate::nodes::input::types::PointerEvent;

/// Identifies which part of the rendering pipeline should be invalidated
/// after a modifier node changes state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidationKind {
    Layout,
    Draw,
    PointerInput,
    Semantics,
    Focus,
}

/// Runtime services exposed to modifier nodes while attached to a tree.
pub trait ModifierNodeContext {
    /// Requests that a particular pipeline stage be invalidated.
    fn invalidate(&mut self, _kind: InvalidationKind) {}

    /// Requests that the node's `update` method run again outside of a
    /// regular composition pass.
    fn request_update(&mut self) {}

    /// Returns the ID of the layout node this modifier is attached to, if known.
    /// This is used by modifiers that need to register callbacks for invalidation (e.g. Scroll).
    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        None
    }

    /// Signals that a node with `capabilities` is about to interact with this context.
    fn push_active_capabilities(&mut self, _capabilities: NodeCapabilities) {}

    /// Signals that the most recent node interaction has completed.
    fn pop_active_capabilities(&mut self) {}
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
    invalidations: Vec<ModifierInvalidation>,
    update_requested: bool,
    active_capabilities: Vec<NodeCapabilities>,
    node_id: Option<cranpose_core::NodeId>,
}

impl BasicModifierNodeContext {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the ordered list of invalidation kinds that were requested
    /// since the last call to `clear_invalidations`. Duplicate requests for
    /// the same kind are coalesced.
    pub fn invalidations(&self) -> &[ModifierInvalidation] {
        &self.invalidations
    }

    /// Removes all currently recorded invalidation kinds.
    pub fn clear_invalidations(&mut self) {
        self.invalidations.clear();
    }

    /// Drains the recorded invalidations and returns them to the caller.
    pub fn take_invalidations(&mut self) -> Vec<ModifierInvalidation> {
        std::mem::take(&mut self.invalidations)
    }

    /// Returns whether an update was requested since the last call to
    /// `take_update_requested`.
    pub fn update_requested(&self) -> bool {
        self.update_requested
    }

    /// Returns whether an update was requested and clears the flag.
    pub fn take_update_requested(&mut self) -> bool {
        std::mem::take(&mut self.update_requested)
    }

    /// Sets the node ID associated with this context.
    pub fn set_node_id(&mut self, id: Option<cranpose_core::NodeId>) {
        self.node_id = id;
    }

    fn push_invalidation(&mut self, kind: InvalidationKind) {
        let mut capabilities = self.current_capabilities();
        capabilities.insert(NodeCapabilities::for_invalidation(kind));
        if let Some(existing) = self
            .invalidations
            .iter_mut()
            .find(|entry| entry.kind() == kind)
        {
            let updated = existing.capabilities() | capabilities;
            *existing = ModifierInvalidation::new(kind, updated);
        } else {
            self.invalidations
                .push(ModifierInvalidation::new(kind, capabilities));
        }
    }

    fn current_capabilities(&self) -> NodeCapabilities {
        self.active_capabilities
            .last()
            .copied()
            .unwrap_or_else(NodeCapabilities::empty)
    }
}

impl ModifierNodeContext for BasicModifierNodeContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        self.push_invalidation(kind);
    }

    fn request_update(&mut self) {
        self.update_requested = true;
    }

    fn push_active_capabilities(&mut self, capabilities: NodeCapabilities) {
        self.active_capabilities.push(capabilities);
    }

    fn pop_active_capabilities(&mut self) {
        self.active_capabilities.pop();
    }

    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        self.node_id
    }
}

/// Path to a node within a modifier chain, supporting delegate navigation.
/// Fixed-size Copy type — delegate depth is bounded at 3 in practice
/// (modifier delegation rarely exceeds 2–3 levels).
const MAX_DELEGATE_DEPTH: usize = 3;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodePath {
    entry: usize,
    delegate_buf: [u8; MAX_DELEGATE_DEPTH],
    delegate_len: u8,
}

impl NodePath {
    #[inline]
    fn root(entry: usize) -> Self {
        Self {
            entry,
            delegate_buf: [0; MAX_DELEGATE_DEPTH],
            delegate_len: 0,
        }
    }

    #[inline]
    fn from_slice(entry: usize, path: &[usize]) -> Self {
        debug_assert!(
            path.len() <= MAX_DELEGATE_DEPTH,
            "delegate depth {} exceeds MAX_DELEGATE_DEPTH {}",
            path.len(),
            MAX_DELEGATE_DEPTH
        );
        debug_assert!(
            path.iter().all(|&i| i <= u8::MAX as usize),
            "delegate index exceeds u8 range"
        );
        let mut delegate_buf = [0u8; MAX_DELEGATE_DEPTH];
        for (i, &v) in path.iter().enumerate().take(MAX_DELEGATE_DEPTH) {
            delegate_buf[i] = v as u8;
        }
        Self {
            entry,
            delegate_buf,
            delegate_len: path.len().min(MAX_DELEGATE_DEPTH) as u8,
        }
    }

    #[inline]
    fn entry(&self) -> usize {
        self.entry
    }

    #[inline]
    fn delegates(&self) -> &[u8] {
        &self.delegate_buf[..self.delegate_len as usize]
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum NodeLink {
    Head,
    Tail,
    Entry(NodePath),
}

/// Runtime state tracked for every [`ModifierNode`].
///
/// This type is part of the internal node system API and should not be directly
/// constructed or manipulated by external code. Modifier nodes automatically receive
/// and manage their NodeState through the modifier chain infrastructure.
#[derive(Debug)]
pub struct NodeState {
    aggregate_child_capabilities: Cell<NodeCapabilities>,
    capabilities: Cell<NodeCapabilities>,
    parent: RefCell<Option<NodeLink>>,
    child: RefCell<Option<NodeLink>>,
    attached: Cell<bool>,
    is_sentinel: bool,
}

impl Default for NodeState {
    fn default() -> Self {
        Self::new()
    }
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

    #[inline]
    pub fn capabilities(&self) -> NodeCapabilities {
        self.capabilities.get()
    }

    pub fn set_aggregate_child_capabilities(&self, capabilities: NodeCapabilities) {
        self.aggregate_child_capabilities.set(capabilities);
    }

    #[inline]
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        self.aggregate_child_capabilities.get()
    }

    pub(crate) fn set_parent_link(&self, parent: Option<NodeLink>) {
        *self.parent.borrow_mut() = parent;
    }

    #[inline]
    pub(crate) fn parent_link(&self) -> Option<NodeLink> {
        *self.parent.borrow()
    }

    pub(crate) fn set_child_link(&self, child: Option<NodeLink>) {
        *self.child.borrow_mut() = child;
    }

    #[inline]
    pub(crate) fn child_link(&self) -> Option<NodeLink> {
        *self.child.borrow()
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
/// # Capability-Driven Architecture
///
/// This trait follows Jetpack Compose's `Modifier.Node` pattern where nodes declare
/// their capabilities via [`NodeCapabilities`] and implement specialized traits
/// ([`DrawModifierNode`], [`PointerInputNode`], [`SemanticsNode`], [`FocusNode`], etc.)
/// to participate in specific pipeline stages.
///
/// ## How to Implement a Modifier Node
///
/// 1. **Declare capabilities** in your [`ModifierNodeElement::capabilities()`] implementation
/// 2. **Implement specialized traits** for the capabilities you declared
/// 3. **Use helper macros** to reduce boilerplate (recommended)
///
/// ### Example: Draw Node
///
/// ```text
/// use cranpose_foundation::*;
///
/// struct MyDrawNode {
///     state: NodeState,
///     color: Color,
/// }
///
/// impl DelegatableNode for MyDrawNode {
///     fn node_state(&self) -> &NodeState {
///         &self.state
///     }
/// }
///
/// impl ModifierNode for MyDrawNode {
///     // Use the helper macro instead of manual as_* implementations
///     impl_modifier_node!(draw);
/// }
///
/// impl DrawModifierNode for MyDrawNode {
///     fn draw(&mut self, _context: &mut dyn ModifierNodeContext, draw_scope: &mut dyn DrawScope) {
///         // Drawing logic here
///     }
/// }
/// ```
///
/// ### Example: Multi-Capability Node
///
/// ```text
/// impl ModifierNode for MyComplexNode {
///     // This node participates in draw, pointer input, and semantics
///     impl_modifier_node!(draw, pointer_input, semantics);
/// }
/// ```
///
/// ## Lifecycle Callbacks
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

    /// Returns this node as a focus modifier if it implements the trait.
    fn as_focus_node(&self) -> Option<&dyn FocusNode> {
        None
    }

    /// Returns this node as a mutable focus modifier if it implements the trait.
    fn as_focus_node_mut(&mut self) -> Option<&mut dyn FocusNode> {
        None
    }

    /// Returns this node as a layout modifier if it implements the trait.
    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        None
    }

    /// Returns this node as a mutable layout modifier if it implements the trait.
    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
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
    /// Measures the wrapped content and returns both the size this modifier
    /// occupies and where the wrapped content should be placed.
    ///
    /// The node receives a measurable representing the wrapped content and
    /// the incoming constraints from the parent.
    ///
    /// Returns a `LayoutModifierMeasureResult` containing:
    /// - `size`: The final size this modifier will occupy
    /// - `placement_offset_x/y`: Where to place the wrapped content relative
    ///   to this modifier's top-left corner
    ///
    /// For example, a padding modifier would:
    /// - Measure child with deflated constraints
    /// - Return size = child size + padding
    /// - Return placement offset = (padding.left, padding.top)
    ///
    /// The default implementation delegates to the wrapped content without
    /// modification (size = child size, offset = 0).
    ///
    /// NOTE: This takes `&self` not `&mut self` to match Jetpack Compose semantics.
    /// Nodes that need mutable state should use interior mutability (Cell/RefCell).
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let placeable = measurable.measure(constraints);
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: placeable.width(),
            height: placeable.height(),
        })
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
///
/// Following Jetpack Compose's design, `draw()` is called during the actual
/// render pass with a live DrawScope, not during layout/slice collection.
pub trait DrawModifierNode: ModifierNode {
    /// Draws this modifier node into the provided DrawScope.
    ///
    /// This is called during the render pass for each node with DRAW capability.
    /// The node should draw directly into the scope using methods like
    /// `draw_scope.draw_rect_at()`.
    ///
    /// Takes `&self` to work with immutable chain iteration - use interior
    /// mutability (RefCell) for any state that needs mutation during draw.
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}

    /// Creates a closure for deferred drawing that will be evaluated at render time.
    ///
    /// This is the preferred method for nodes with dynamic content like:
    /// - Blinking cursors (visibility changes over time)
    /// - Live selection during drag (selection changes during mouse move)
    ///
    /// The returned closure captures the node's internal state (via Rc) and
    /// evaluates at render time, not at slice collection time.
    ///
    /// Returns None by default. Override for nodes needing deferred draw.
    fn create_draw_closure(&self) -> Option<NodeDrawClosure> {
        None
    }

    /// Like [`create_draw_closure`](Self::create_draw_closure), but the
    /// primitives render BEHIND the node's content — e.g. a text field's
    /// selection highlight, which must sit under the glyphs (a highlight
    /// drawn over them tints the text with its translucent fill).
    fn create_behind_draw_closure(&self) -> Option<NodeDrawClosure> {
        None
    }
}

/// A deferred draw closure returned by
/// [`DrawModifierNode::create_draw_closure`]: it records into a scope the
/// renderer provides at render time, so the recording's identity stays with
/// the consumer rather than with a closure-owned vector.
pub type NodeDrawClosure = Rc<dyn Fn(&mut cranpose_ui_graphics::DrawScopeDefault)>;

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

    /// Returns the cell this node reads its owning layout node's resolved size
    /// from, if it exposes a size to its handler (Compose's
    /// `PointerInputScope.size`).
    ///
    /// The cell is shared with the node, so the layout pass publishes the size
    /// into it once per pass and every read — including reads that happen
    /// before any pointer event arrives — observes the current size. The size
    /// is the node's layout box in the same local coordinate space the
    /// dispatched [`PointerEvent`] positions use, so
    /// `event.local_position / size` is a well-defined fraction of the node.
    ///
    /// Returns `None` for pointer nodes with no size-bearing scope.
    fn layout_size_sink(&self) -> Option<Rc<Cell<Size>>> {
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
    fn merge_semantics(&self, _config: &mut SemanticsConfiguration) {}
}

/// Focus state of a focus target node.
///
/// This mirrors Jetpack Compose's FocusState enum which tracks whether
/// a node is focused, has a focused child, or is inactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FocusState {
    /// The focusable component is currently active (i.e. it receives key events).
    Active,
    /// One of the descendants of the focusable component is Active.
    ActiveParent,
    /// The focusable component is currently active (has focus), and is in a state
    /// where it does not want to give up focus. (Eg. a text field with an invalid
    /// phone number).
    Captured,
    /// The focusable component does not receive any key events. (ie it is not active,
    /// nor are any of its descendants active).
    #[default]
    Inactive,
}

impl FocusState {
    /// Returns whether the component is focused (Active or Captured).
    pub fn is_focused(self) -> bool {
        matches!(self, FocusState::Active | FocusState::Captured)
    }

    /// Returns whether this node or any descendant has focus.
    pub fn has_focus(self) -> bool {
        matches!(
            self,
            FocusState::Active | FocusState::ActiveParent | FocusState::Captured
        )
    }

    /// Returns whether focus is captured.
    pub fn is_captured(self) -> bool {
        matches!(self, FocusState::Captured)
    }
}

/// Marker trait for focus modifier nodes.
///
/// Focus nodes participate in focus management. They can request focus,
/// track focus state, and participate in focus traversal.
pub trait FocusNode: ModifierNode {
    /// Returns the current focus state of this node.
    fn focus_state(&self) -> FocusState;

    /// Called when focus state changes for this node.
    fn on_focus_changed(&mut self, _context: &mut dyn ModifierNodeContext, _state: FocusState) {}
}

/// What kind of control a node is, as screen readers announce it.
///
/// This is Compose's `SemanticsProperties.Role` (`Modifier.semantics { role =
/// Role.RadioButton }`), not a description of where the node sits in the tree.
/// A screen reader turns it into the trailing noun it speaks after the label —
/// TalkBack says "CAMPAIGN, radio button, selected" — and into the on/off
/// wording it uses for a switch. Compose keeps this separate from the node's
/// structural kind for the same reason Cranpose does: a `Row` that happens to
/// be clickable is still a row, and a drawn ring segment that is a radio button
/// has no layout node of its own at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticsWidgetRole {
    Button,
    Checkbox,
    Switch,
    RadioButton,
    Tab,
    Image,
    /// A control that opens a list of choices and holds the one that is
    /// picked. Compose's `Role.DropdownList`.
    DropdownList,
    /// A control that holds one value out of an ordered set and steps through
    /// them. Compose's `Role.ValuePicker`.
    ValuePicker,
    /// Compose's `heading()`, which is a property rather than a `Role`, but
    /// reaches the platform through the same field on every backend Cranpose
    /// targets (`AccessibilityNodeInfo.setHeading`, `Role::Heading`,
    /// `<h*>`/`UIAccessibilityTraitHeader`).
    Header,
    /// A modal surface that takes over the screen until it is dismissed.
    /// Screen readers announce it and confine their traversal to it, which is
    /// the accessible half of what makes a dialog modal.
    Dialog,
    /// Text that takes the user somewhere else when pressed. Compose has no
    /// such `Role`; SwiftUI's `.isLink`, ARIA's `link`.
    Link,
    /// A field that narrows what is on the screen as the user types. ARIA's
    /// `searchbox`, VoiceOver's search field trait.
    SearchField,
    /// A control that shows how far work has come and takes no input. ARIA's
    /// `progressbar`.
    ProgressBar,
    /// A button that stays pressed or released. ARIA's `button` with
    /// `aria-pressed`.
    ToggleButton,
    /// A message a reader speaks as soon as it shows, with no move to it.
    /// ARIA's `alert`.
    Alert,
    /// A row of controls that act on the content beside them. ARIA's
    /// `toolbar`.
    Toolbar,
    /// A list of commands that opens on a press. ARIA's `menu`.
    Menu,
    /// One command inside a menu. ARIA's `menuitem`.
    MenuItem,
    /// The row that holds the tabs of a screen. ARIA's `tablist`, VoiceOver's
    /// tab bar trait.
    TabBar,
    /// A container whose rows a reader counts and walks into. ARIA's `list`.
    List,
    /// One row of a list. ARIA's `listitem`.
    ListItem,
}

/// The value a control holds inside a range, for a slider, a dial or a
/// progress bar.
///
/// This is Compose's `ProgressBarRangeInfo` (`Modifier.progressSemantics(value,
/// range, steps)`). A control without it reads as plain text: a screen reader
/// user hears "47 percent" and has no way to change it, because nothing tells
/// the platform the control is adjustable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressBarRangeInfo {
    pub current: f32,
    pub start: f32,
    pub end: f32,
    /// How many stops sit between `start` and `end`, as Compose counts them.
    /// Zero means the value moves without stops.
    pub steps: u32,
}

impl ProgressBarRangeInfo {
    pub fn new(current: f32, start: f32, end: f32, steps: u32) -> Self {
        Self {
            current,
            start,
            end,
            steps,
        }
    }

    /// Where the value sits between the two ends, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        let span = self.end - self.start;
        if span.abs() < f32::EPSILON {
            return 0.0;
        }
        ((self.current - self.start) / span).clamp(0.0, 1.0)
    }

    /// How far one stop moves the value. With no stops, one tenth of the
    /// range, which is what a screen reader's swipe up and down expects.
    pub fn step(&self) -> f32 {
        let span = self.end - self.start;
        if self.steps == 0 {
            span / 10.0
        } else {
            span / (self.steps as f32 + 1.0)
        }
    }
}

/// How far a container has scrolled along one axis and how far it can go.
///
/// How many rows and columns a list holds, so a screen reader can say
/// "list, 12 items" as its cursor enters. Compose's `CollectionInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionInfo {
    pub rows: usize,
    pub columns: usize,
}

/// This is Compose's `ScrollAxisRange` (`verticalScrollAxisRange`,
/// `horizontalScrollAxisRange`). A lazy list has no whole extent to give, so
/// it reports the first visible item as the value and one more than that as
/// the end while it can still scroll; a reader only needs to know whether it
/// can page on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollAxisRange {
    pub value: f32,
    pub max_value: f32,
    pub reverse: bool,
}

impl ScrollAxisRange {
    pub fn new(value: f32, max_value: f32, reverse: bool) -> Self {
        Self {
            value,
            max_value,
            reverse,
        }
    }

    pub fn can_scroll_forward(&self) -> bool {
        self.value < self.max_value
    }

    pub fn can_scroll_backward(&self) -> bool {
        self.value > 0.0
    }
}

/// What a container does when a screen reader pages it, e.g. TalkBack's
/// scroll forward action or an accesskit scroll down. The two deltas are in
/// layout pixels; the answer says whether anything moved.
///
/// This is Compose's `SemanticsActions.ScrollBy`.
#[derive(Clone)]
pub struct SemanticsScrollBy {
    handler: Rc<dyn Fn(f32, f32) -> bool>,
}

impl SemanticsScrollBy {
    pub fn new(handler: impl Fn(f32, f32) -> bool + 'static) -> Self {
        Self {
            handler: Rc::new(handler),
        }
    }

    pub fn invoke(&self, dx: f32, dy: f32) -> bool {
        (self.handler)(dx, dy)
    }
}

impl fmt::Debug for SemanticsScrollBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticsScrollBy").finish_non_exhaustive()
    }
}

/// Two scroll actions always read as the same action, for the reason
/// [`SemanticsCustomAction`]'s own comparison gives.
impl PartialEq for SemanticsScrollBy {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SemanticsScrollBy {}

/// What a list does when a screen reader asks for the row at an index, e.g. a
/// TalkBack scroll-to-position action.
///
/// This is Compose's `SemanticsActions.ScrollToIndex`. The index counts rows
/// from zero, and the answer says whether the list moved.
#[derive(Clone)]
pub struct SemanticsScrollToIndex {
    handler: Rc<dyn Fn(usize) -> bool>,
}

impl SemanticsScrollToIndex {
    pub fn new(handler: impl Fn(usize) -> bool + 'static) -> Self {
        Self {
            handler: Rc::new(handler),
        }
    }

    pub fn invoke(&self, index: usize) -> bool {
        (self.handler)(index)
    }
}

impl fmt::Debug for SemanticsScrollToIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticsScrollToIndex")
            .finish_non_exhaustive()
    }
}

/// Two jump actions always read as the same action, for the reason
/// [`SemanticsCustomAction`]'s own comparison gives.
impl PartialEq for SemanticsScrollToIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SemanticsScrollToIndex {}

/// What a control does when a screen reader moves its value, e.g. a VoiceOver
/// swipe up or a TalkBack set-progress action.
///
/// This is Compose's `SemanticsActions.SetProgress`. The value comes in the
/// control's own range, and the answer says whether the control took it.
#[derive(Clone)]
pub struct SemanticsSetProgress {
    handler: Rc<dyn Fn(f32) -> bool>,
}

impl SemanticsSetProgress {
    pub fn new(handler: impl Fn(f32) -> bool + 'static) -> Self {
        Self {
            handler: Rc::new(handler),
        }
    }

    pub fn invoke(&self, value: f32) -> bool {
        (self.handler)(value)
    }
}

impl fmt::Debug for SemanticsSetProgress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticsSetProgress")
            .finish_non_exhaustive()
    }
}

/// What a text field does when a screen reader or a voice tool hands it new
/// text. This is Compose's `SemanticsActions.SetText`; the answer says
/// whether the field took the text.
#[derive(Clone)]
pub struct SemanticsSetText(Rc<dyn Fn(&str) -> bool>);

impl SemanticsSetText {
    pub fn new(handler: impl Fn(&str) -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self, text: &str) -> bool {
        (self.0)(text)
    }
}

impl fmt::Debug for SemanticsSetText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsSetText")
    }
}

/// What a text field does when a screen reader moves its caret or picks a
/// stretch of its text: the two ends of the new selection, as byte offsets
/// into the field's text, the anchor first and the end that moves second. Equal
/// ends are a caret. This is Compose's `SemanticsActions.SetSelection`; the
/// answer says whether the field took the selection.
#[derive(Clone)]
pub struct SemanticsSetSelection(Rc<dyn Fn(usize, usize) -> bool>);

impl SemanticsSetSelection {
    pub fn new(handler: impl Fn(usize, usize) -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self, anchor: usize, focus: usize) -> bool {
        (self.0)(anchor, focus)
    }
}

impl fmt::Debug for SemanticsSetSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsSetSelection")
    }
}

/// What a control does when a screen reader asks it to open or to close.
/// Compose's `expand` and `collapse` actions.
#[derive(Clone)]
pub struct SemanticsExpand(Rc<dyn Fn() -> bool>);

impl SemanticsExpand {
    pub fn new(handler: impl Fn() -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self) -> bool {
        (self.0)()
    }
}

impl fmt::Debug for SemanticsExpand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsExpand")
    }
}

/// What a control does when a screen reader asks for its long press. This is
/// Compose's `SemanticsActions.OnLongClick`; the answer says whether the
/// control took the ask.
#[derive(Clone)]
pub struct SemanticsLongClick(Rc<dyn Fn() -> bool>);

impl SemanticsLongClick {
    pub fn new(handler: impl Fn() -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self) -> bool {
        (self.0)()
    }
}

impl fmt::Debug for SemanticsLongClick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsLongClick")
    }
}

impl PartialEq for SemanticsLongClick {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl PartialEq for SemanticsExpand {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// What a control does when a screen reader asks to send it away: a row a
/// sighted person swipes off, a sheet a sighted person taps outside of.
/// Compose's `dismiss` action.
#[derive(Clone)]
pub struct SemanticsDismiss(Rc<dyn Fn() -> bool>);

impl SemanticsDismiss {
    pub fn new(handler: impl Fn() -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self) -> bool {
        (self.0)()
    }
}

impl fmt::Debug for SemanticsDismiss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsDismiss")
    }
}

impl PartialEq for SemanticsDismiss {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl PartialEq for SemanticsSetText {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl PartialEq for SemanticsSetSelection {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// What a control does when a VoiceOver user makes the magic tap, the two
/// finger double tap that starts or stops the main action of a screen. On
/// the other platforms the action is listed by its label among the control's
/// actions. SwiftUI's `accessibilityAction(.magicTap)`; the answer says
/// whether the control took the tap.
#[derive(Clone)]
pub struct SemanticsMagicTap(Rc<dyn Fn() -> bool>);

impl SemanticsMagicTap {
    pub fn new(handler: impl Fn() -> bool + 'static) -> Self {
        Self(Rc::new(handler))
    }

    pub fn invoke(&self) -> bool {
        (self.0)()
    }
}

impl fmt::Debug for SemanticsMagicTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsMagicTap")
    }
}

impl PartialEq for SemanticsMagicTap {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Two set-progress actions always read as the same action, for the reason
/// [`SemanticsCustomAction`]'s own comparison gives: the closure is rebuilt on
/// every semantics collection, so comparing handler identity would report a
/// changed tree on every frame.
impl PartialEq for SemanticsSetProgress {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SemanticsSetProgress {}

/// How urgently a screen reader reads a node whose text changed on its own.
///
/// This is Compose's `LiveRegionMode` (`Modifier.semantics { liveRegion =
/// LiveRegionMode.Polite }`). A node without it stays silent until the reader
/// lands on it, which is wrong for a timer, a countdown, a status line, or an
/// error that appears next to a text field: a blind user hears nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LiveRegionMode {
    /// Read the new text once the reader finishes what it says now.
    Polite,
    /// Cut off what the reader says now and read the new text at once. For
    /// text a user must hear immediately, such as an error that stops them.
    Assertive,
}

/// A screen-reader action that is not a click, e.g. Compose's
/// `customActions = listOf(CustomAccessibilityAction("Pause") { … })`.
///
/// TalkBack surfaces these through its actions menu rather than by activating
/// the node, which is the only way to reach a command that has no on-screen
/// control — pausing a game whose whole surface is one tap-to-launch target.
#[derive(Clone)]
pub struct SemanticsCustomAction {
    /// What the screen reader reads out in its actions menu.
    pub label: String,
    handler: Rc<dyn Fn()>,
}

impl SemanticsCustomAction {
    pub fn new(label: impl Into<String>, handler: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            handler: Rc::new(handler),
        }
    }

    pub fn invoke(&self) {
        (self.handler)();
    }
}

impl fmt::Debug for SemanticsCustomAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticsCustomAction")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// Two custom actions are the same action when they read the same.
///
/// The handler is deliberately excluded. A semantics recorder runs on every
/// collection, so the closure is a fresh `Rc` each time and comparing handler
/// identity would report "the tree changed" on every frame — which on Android
/// means re-serialising and re-publishing the whole virtual-view tree across
/// JNI 60 times a second. Handlers are looked up in the live semantics tree at
/// the moment the action fires (see `perform_custom_action`), so a handler that
/// is newer than the last published snapshot is still the one that runs.
impl PartialEq for SemanticsCustomAction {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
    }
}

impl Eq for SemanticsCustomAction {}

/// A semantics node for content that is *drawn* rather than laid out.
///
/// An immediate-mode surface — one `Canvas` that paints a whole screen — has
/// exactly one layout node, so the semantics tree built from layout has exactly
/// one node to offer a screen reader. This is the escape hatch: the drawing
/// code already knows where it put every control, so it publishes those
/// rectangles as semantics directly. Android's own answer for a canvas-drawn
/// `View` is the same shape (`ExploreByTouchHelper` feeding virtual view ids
/// into an `AccessibilityNodeProvider`), and Cranpose's Android bridge is
/// already an `AccessibilityNodeProvider`, so these land as first-class
/// virtual views next to the ones layout produces.
///
/// `bounds` is in the publishing node's own coordinates (logical px, origin at
/// that node's top-left), because that is what a draw scope works in.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSemanticsNode {
    /// Identity that must survive a redraw.
    ///
    /// A screen reader parks its cursor on a virtual view id; if the id for
    /// "the Haptics switch" changes when the list scrolls, the cursor jumps.
    /// Derive this from what the control *is* (a row index, an enum
    /// discriminant), never from where it currently sits.
    pub key: u64,
    /// Where the control was drawn, relative to the publishing node.
    pub bounds: cranpose_ui_graphics::Rect,
    pub label: String,
    pub role: Option<SemanticsWidgetRole>,
    /// Compose's `stateDescription` — what the control currently reads as
    /// ("CAMPAIGN", "3 of 18 gold"), spoken after the label and re-spoken on
    /// its own when only the state changed.
    pub state_description: Option<String>,
    /// Compose's `onClick(label = …)`. TalkBack reads it as "double tap to
    /// `<label>`", so it is a verb phrase, not a repeat of the label.
    pub on_click_label: Option<String>,
    pub clickable: bool,
    /// Compose's `selected`, for `Role.RadioButton`/`Role.Tab`.
    pub selected: Option<bool>,
    /// Compose's `toggleableState`, for `Role.Switch`/`Role.Checkbox`.
    pub toggled: Option<bool>,
    pub enabled: bool,
    pub custom_actions: Vec<SemanticsCustomAction>,
}

impl Default for CanvasSemanticsNode {
    fn default() -> Self {
        Self {
            key: 0,
            bounds: cranpose_ui_graphics::Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            label: String::new(),
            role: None,
            state_description: None,
            on_click_label: None,
            clickable: false,
            selected: None,
            toggled: None,
            enabled: true,
            custom_actions: Vec::new(),
        }
    }
}

impl CanvasSemanticsNode {
    /// A clickable control drawn at `bounds`.
    pub fn control(key: u64, bounds: cranpose_ui_graphics::Rect, label: impl Into<String>) -> Self {
        Self {
            key,
            bounds,
            label: label.into(),
            clickable: true,
            ..Self::default()
        }
    }

    /// A drawn label that is read but not activated.
    pub fn text(key: u64, bounds: cranpose_ui_graphics::Rect, label: impl Into<String>) -> Self {
        Self {
            key,
            bounds,
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn with_role(mut self, role: SemanticsWidgetRole) -> Self {
        self.role = Some(role);
        self
    }

    pub fn with_state_description(mut self, state: impl Into<String>) -> Self {
        self.state_description = Some(state.into());
        self
    }

    pub fn with_click_label(mut self, label: impl Into<String>) -> Self {
        self.on_click_label = Some(label.into());
        self.clickable = true;
        self
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn with_toggled(mut self, toggled: bool) -> Self {
        self.toggled = Some(toggled);
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_custom_action(mut self, action: SemanticsCustomAction) -> Self {
        self.custom_actions.push(action);
        self
    }
}

/// Semantics configuration for accessibility.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsConfiguration {
    pub content_description: Option<String>,
    /// Compose's `stateDescription`.
    pub state_description: Option<String>,
    /// Compose's `onClick(label = …)`; implies clickable.
    pub on_click_label: Option<String>,
    /// What this control does when a screen reader asks for its long press.
    /// Compose's `onLongClick`.
    pub on_long_click: Option<SemanticsLongClick>,
    /// What the long press does, as a verb phrase a reader reads out:
    /// "Remove receipt". Compose's `onLongClick(label = …)`.
    pub on_long_click_label: Option<String>,
    /// What this control does on VoiceOver's magic tap, and on the other
    /// platforms as an action listed by [`Self::on_magic_tap_label`].
    /// SwiftUI's `accessibilityAction(.magicTap)`.
    pub on_magic_tap: Option<SemanticsMagicTap>,
    /// What the magic tap does, as a verb phrase a reader reads out: "Take
    /// the photo".
    pub on_magic_tap_label: Option<String>,
    /// The short names a person says to Voice Control to reach this control,
    /// when the name a reader hears is too long to say. SwiftUI's
    /// `accessibilityInputLabels`; iOS only.
    pub input_labels: Vec<String>,
    /// The language of this control's text as a BCP 47 tag, "de" or "pt-BR",
    /// so a reader picks the right voice for it. SwiftUI's
    /// `accessibilityLanguage`, ARIA's `lang`.
    pub language: Option<String>,
    /// Compose's `Role`.
    pub role: Option<SemanticsWidgetRole>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub enabled: bool,
    pub is_clickable: bool,
    pub is_editable_text: bool,
    /// The text an editable field holds, read as its value. Compose's
    /// `editableText`.
    pub text: Option<String>,
    pub text_selection: Option<crate::text::TextRange>,
    pub custom_actions: Vec<SemanticsCustomAction>,
    /// Controls this node drew itself instead of laying out. See
    /// [`CanvasSemanticsNode`].
    pub canvas_children: Vec<CanvasSemanticsNode>,
    /// Whether this node takes over the screen: everything outside it is
    /// inert, and a screen reader keeps its traversal inside.
    pub is_modal: bool,
    /// Whether a screen reader skips this node and everything under it: a
    /// decorative image, or a placeholder drawn under a named field. Compose's
    /// `hideFromAccessibility`.
    pub hidden: bool,
    /// Whether a screen reader takes this node and the text under it as one
    /// stop, the way it does for a button: a row whose name, count and price
    /// belong together. Compose's `mergeDescendants`.
    pub merge_descendants: bool,
    /// Whether the selectable controls under this node form one group, so a
    /// screen reader says which of how many a tab or a radio button is.
    /// Compose's `selectableGroup`.
    pub selectable_group: bool,
    /// The title of the screen or pane this node is the root of, read out when
    /// the app moves to it. Compose's `paneTitle`.
    pub pane_title: Option<String>,
    /// Why the control's content is wrong, read after its state: "invalid,
    /// the amount needs a number". Compose's `error`.
    pub error: Option<String>,
    /// Whether this field holds a secret, so no screen reader reads its text
    /// out and no platform mirror carries it. Compose's `password`.
    pub password: bool,
    /// Where a screen reader visits this node among the ones beside it: a
    /// smaller number comes first, and nodes left at zero keep the order the
    /// app laid them out in. Compose's `traversalIndex`.
    pub traversal_index: f32,
    /// Compose's `liveRegion`. When set, a screen reader reads this node again
    /// whenever its text changes, without the user moving to it.
    pub live_region: Option<LiveRegionMode>,
    /// Compose's `progressBarRangeInfo`. A control with it is adjustable: a
    /// screen reader offers its own way to move the value.
    pub progress: Option<ProgressBarRangeInfo>,
    /// What the control does when a screen reader moves its value. Compose's
    /// `setProgress`.
    pub set_progress: Option<SemanticsSetProgress>,
    /// What this field does when a screen reader or a voice tool hands it
    /// text. Compose's `setText`.
    pub set_text: Option<SemanticsSetText>,
    /// What this field does when a screen reader moves its caret or picks a
    /// stretch of its text. Compose's `setSelection`.
    pub set_selection: Option<SemanticsSetSelection>,
    /// What this control does when a screen reader asks it to open. A control
    /// that says so reads as closed. Compose's `expand`.
    pub expand: Option<SemanticsExpand>,
    /// What this control does when a screen reader asks to send it away: a row
    /// a sighted person swipes off, a sheet a sighted person taps outside of.
    /// Compose's `dismiss`.
    pub dismiss: Option<SemanticsDismiss>,
    /// What this control does when a screen reader asks it to close. A control
    /// that says so reads as open. Compose's `collapse`.
    pub collapse: Option<SemanticsExpand>,
    /// How far this container scrolled up and down. Compose's
    /// `verticalScrollAxisRange`.
    pub vertical_scroll: Option<ScrollAxisRange>,
    /// How far this container scrolled left and right. Compose's
    /// `horizontalScrollAxisRange`.
    pub horizontal_scroll: Option<ScrollAxisRange>,
    /// What this container does when a screen reader pages it. Compose's
    /// `scrollBy`.
    pub scroll_by: Option<SemanticsScrollBy>,
    /// What this list does when a screen reader asks for the row at an index,
    /// so a reader reaches row 300 without paging to it. Compose's
    /// `scrollToIndex`.
    pub scroll_to_index: Option<SemanticsScrollToIndex>,
    /// How many rows and columns this list holds. Compose's `collectionInfo`.
    pub collection: Option<CollectionInfo>,
}

impl Default for SemanticsConfiguration {
    fn default() -> Self {
        Self {
            content_description: None,
            state_description: None,
            on_click_label: None,
            on_long_click: None,
            on_long_click_label: None,
            on_magic_tap: None,
            on_magic_tap_label: None,
            input_labels: Vec::new(),
            language: None,
            role: None,
            selected: None,
            toggled: None,
            enabled: true,
            is_clickable: false,
            is_editable_text: false,
            text: None,
            text_selection: None,
            custom_actions: Vec::new(),
            canvas_children: Vec::new(),
            is_modal: false,
            hidden: false,
            merge_descendants: false,
            selectable_group: false,
            pane_title: None,
            error: None,
            password: false,
            traversal_index: 0.0,
            live_region: None,
            progress: None,
            set_progress: None,
            set_text: None,
            set_selection: None,
            expand: None,
            dismiss: None,
            collapse: None,
            vertical_scroll: None,
            horizontal_scroll: None,
            scroll_by: None,
            scroll_to_index: None,
            collection: None,
        }
    }
}

/// One value that says what a screen reader reads for a node, built up with
/// the methods below and handed to `Modifier::semantics_spec`.
///
/// Compose has no such value: it takes a receiver lambda, which Kotlin makes
/// read well and Rust has no match for. `SemanticsSpec::new().content_description("Save")`
/// reads better than `|config| config.content_description = Some("Save".into())`,
/// it is one chain element rather than one per property, and two specs can be
/// compared. The closure form stays as `Modifier::semantics` for parity.
pub type SemanticsSpec = SemanticsConfiguration;

impl SemanticsConfiguration {
    /// An empty spec to build on. Every field is what it is with no semantics
    /// declared at all.
    pub fn new() -> Self {
        Self::default()
    }

    /// The name a screen reader reads for the control. Compose's
    /// `contentDescription`.
    pub fn content_description(mut self, name: impl Into<String>) -> Self {
        self.content_description = Some(name.into());
        self
    }

    /// What the control says about itself after its name. Compose's
    /// `stateDescription`.
    pub fn state_description(mut self, state: impl Into<String>) -> Self {
        self.state_description = Some(state.into());
        self
    }

    /// A screen reader offers to activate the control. Compose's `onClick`.
    pub fn clickable(mut self) -> Self {
        self.is_clickable = true;
        self
    }

    /// What the control does when a screen reader asks for its long press,
    /// and the verb phrase a reader reads out for it. Compose's
    /// `onLongClick(label) { … }`.
    pub fn on_long_click(
        mut self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        self.on_long_click_label = Some(label.into());
        self.on_long_click = Some(SemanticsLongClick::new(action));
        self
    }

    /// What the control does on VoiceOver's magic tap, and the verb phrase
    /// the other platforms list it under. SwiftUI's
    /// `accessibilityAction(.magicTap)`.
    pub fn on_magic_tap(
        mut self,
        label: impl Into<String>,
        action: impl Fn() -> bool + 'static,
    ) -> Self {
        self.on_magic_tap_label = Some(label.into());
        self.on_magic_tap = Some(SemanticsMagicTap::new(action));
        self
    }

    /// The short names a person says to Voice Control to reach the control.
    /// SwiftUI's `accessibilityInputLabels`.
    pub fn input_labels<S: Into<String>>(mut self, labels: impl IntoIterator<Item = S>) -> Self {
        self.input_labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// The language of the control's text, as a BCP 47 tag. SwiftUI's
    /// `accessibilityLanguage`, ARIA's `lang`.
    pub fn language(mut self, tag: impl Into<String>) -> Self {
        self.language = Some(tag.into());
        self
    }

    /// Whether the control is on or off. Compose's `toggleableState`.
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.toggled = Some(toggled);
        self
    }

    /// Whether the control is the one picked out of a group. Compose's
    /// `selected`.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// What kind of control a screen reader reads this as. Compose's `Role`.
    pub fn role(mut self, role: SemanticsWidgetRole) -> Self {
        self.role = Some(role);
        self
    }

    /// Reads as a heading, so a reader can jump between the headings of a
    /// screen. Compose's `heading()`.
    pub fn heading(self) -> Self {
        self.role(SemanticsWidgetRole::Header)
    }

    /// Why the control's content is wrong. Compose's `error`.
    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.error = Some(message.into());
        self
    }

    /// Holds a secret, so no reader reads the text out. Compose's `password`.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// Names the screen or pane this node is the root of. Compose's
    /// `paneTitle`.
    pub fn pane_title(mut self, title: impl Into<String>) -> Self {
        self.pane_title = Some(title.into());
        self
    }

    /// Where a reader visits this node among the ones beside it. Compose's
    /// `traversalIndex`.
    pub fn traversal_index(mut self, index: f32) -> Self {
        self.traversal_index = index;
        self
    }

    /// Skips this node and everything under it. Compose's
    /// `hideFromAccessibility`.
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Takes this node and the text under it as one stop. Compose's
    /// `mergeDescendants`.
    pub fn merge_descendants(mut self) -> Self {
        self.merge_descendants = true;
        self
    }

    /// Makes the selectable controls under this node one group. Compose's
    /// `selectableGroup`.
    pub fn selectable_group(mut self) -> Self {
        self.selectable_group = true;
        self
    }

    /// Reads this node again whenever its text changes. Compose's
    /// `liveRegion`.
    pub fn live_region(mut self, mode: LiveRegionMode) -> Self {
        self.live_region = Some(mode);
        self
    }
    pub fn merge(&mut self, other: &SemanticsConfiguration) {
        if let Some(description) = &other.content_description {
            self.content_description = Some(description.clone());
        }
        if let Some(state) = &other.state_description {
            self.state_description = Some(state.clone());
        }
        if let Some(label) = &other.on_click_label {
            self.on_click_label = Some(label.clone());
        }
        if let Some(label) = &other.on_long_click_label {
            self.on_long_click_label = Some(label.clone());
        }
        if let Some(label) = &other.on_magic_tap_label {
            self.on_magic_tap_label = Some(label.clone());
        }
        if !other.input_labels.is_empty() {
            self.input_labels.clone_from(&other.input_labels);
        }
        if let Some(language) = &other.language {
            self.language = Some(language.clone());
        }
        if let Some(role) = other.role {
            self.role = Some(role);
        }
        if let Some(selected) = other.selected {
            self.selected = Some(selected);
        }
        if let Some(toggled) = other.toggled {
            self.toggled = Some(toggled);
        }
        self.enabled &= other.enabled;
        self.is_clickable |= other.is_clickable;
        self.is_editable_text |= other.is_editable_text;
        if let Some(text) = &other.text {
            self.text = Some(text.clone());
        }
        self.is_modal |= other.is_modal;
        self.hidden |= other.hidden;
        self.merge_descendants |= other.merge_descendants;
        self.selectable_group |= other.selectable_group;
        self.password |= other.password;
        if other.traversal_index != 0.0 {
            self.traversal_index = other.traversal_index;
        }
        if let Some(live_region) = other.live_region {
            self.live_region = Some(live_region);
        }
        self.merge_words(other);
        self.merge_actions(other);
        self.merge_ranges(other);
    }

    /// The lines a screen reader reads out that stand on their own: the title
    /// of a pane, and the reason a control's content is wrong.
    fn merge_words(&mut self, other: &SemanticsConfiguration) {
        if let Some(title) = &other.pane_title {
            self.pane_title = Some(title.clone());
        }
        if let Some(error) = &other.error {
            self.error = Some(error.clone());
        }
    }

    /// What a screen reader can ask the node to do, and the controls the node
    /// drew rather than laid out.
    fn merge_actions(&mut self, other: &SemanticsConfiguration) {
        self.custom_actions
            .extend(other.custom_actions.iter().cloned());
        self.canvas_children
            .extend(other.canvas_children.iter().cloned());
        if let Some(set_progress) = &other.set_progress {
            self.set_progress = Some(set_progress.clone());
        }
        if let Some(set_text) = &other.set_text {
            self.set_text = Some(set_text.clone());
        }
        if let Some(set_selection) = &other.set_selection {
            self.set_selection = Some(set_selection.clone());
        }
        if let Some(expand) = &other.expand {
            self.expand = Some(expand.clone());
        }
        if let Some(collapse) = &other.collapse {
            self.collapse = Some(collapse.clone());
        }
        if let Some(dismiss) = &other.dismiss {
            self.dismiss = Some(dismiss.clone());
        }
        if let Some(long_click) = &other.on_long_click {
            self.on_long_click = Some(long_click.clone());
        }
        if let Some(magic_tap) = &other.on_magic_tap {
            self.on_magic_tap = Some(magic_tap.clone());
        }
        if let Some(scroll_by) = &other.scroll_by {
            self.scroll_by = Some(scroll_by.clone());
        }
        if let Some(scroll_to_index) = &other.scroll_to_index {
            self.scroll_to_index = Some(scroll_to_index.clone());
        }
    }

    /// The numbers behind a control: where a value sits in its range, how far
    /// a container scrolled, how much a list holds, and what text is picked.
    fn merge_ranges(&mut self, other: &SemanticsConfiguration) {
        if let Some(selection) = other.text_selection {
            self.text_selection = Some(selection);
        }
        if let Some(progress) = other.progress {
            self.progress = Some(progress);
        }
        if let Some(range) = other.vertical_scroll {
            self.vertical_scroll = Some(range);
        }
        if let Some(range) = other.horizontal_scroll {
            self.horizontal_scroll = Some(range);
        }
        if let Some(collection) = other.collection {
            self.collection = Some(collection);
        }
    }

    /// Whether a screen reader should offer activation. A named click label is
    /// how Compose declares `onClick`, so it implies the action the same way.
    pub fn is_activatable(&self) -> bool {
        self.is_clickable || self.on_click_label.is_some()
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

    /// Whether this element requires `update` to be called even if `eq` returns true.
    ///
    /// This is useful for elements that ignore certain fields in `eq` (e.g. closures)
    /// to allow node reuse, but still need those fields updated in the existing node.
    /// Defaults to `false`.
    fn always_update(&self) -> bool {
        false
    }

    /// Whether modifier reconciliation should request capability-wide invalidations
    /// after updating an existing node.
    fn auto_invalidate_on_update(&self) -> bool {
        true
    }

    /// Optional targeted invalidation requested after updating an existing node.
    ///
    /// This is for nodes whose attach/remove capability is broader than the
    /// work needed for a value-only update. For example, an offset node
    /// participates in layout on attach but an x/y change only needs placement
    /// data and draw output refreshed.
    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        None
    }
}

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
    /// Modifier participates in focus management.
    pub const FOCUS: Self = Self(1 << 5);

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
            InvalidationKind::Focus => Self::FOCUS,
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
            .field("focus", &self.contains(Self::FOCUS))
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

/// Records an invalidation request together with the capability mask that triggered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModifierInvalidation {
    kind: InvalidationKind,
    capabilities: NodeCapabilities,
}

impl ModifierInvalidation {
    /// Creates a new modifier invalidation entry.
    pub const fn new(kind: InvalidationKind, capabilities: NodeCapabilities) -> Self {
        Self { kind, capabilities }
    }

    /// Returns the invalidated pipeline kind.
    pub const fn kind(self) -> InvalidationKind {
        self.kind
    }

    /// Returns the capability mask associated with the invalidation.
    pub const fn capabilities(self) -> NodeCapabilities {
        self.capabilities
    }
}

/// Type-erased modifier element used by the runtime to reconcile chains.
pub trait AnyModifierElement: fmt::Debug {
    fn node_type(&self) -> TypeId;

    fn element_type(&self) -> TypeId;

    fn create_node(&self) -> Box<dyn ModifierNode>;

    fn can_update_node(&self, node: &dyn ModifierNode) -> bool;

    fn update_node(&self, node: &mut dyn ModifierNode);

    fn key(&self) -> Option<u64>;

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::default()
    }

    fn hash_code(&self) -> u64;

    fn equals_element(&self, other: &dyn AnyModifierElement) -> bool;

    fn inspector_name(&self) -> &'static str;

    fn record_inspector_properties(&self, visitor: &mut dyn FnMut(&'static str, String));

    fn requires_update(&self) -> bool;

    fn auto_invalidates_on_update(&self) -> bool;

    fn update_invalidation_kind(&self) -> Option<InvalidationKind>;

    fn as_any(&self) -> &dyn Any;
}

struct TypedModifierElement<E: ModifierNodeElement> {
    element: E,
    cached_hash: u64,
}

impl<E: ModifierNodeElement> TypedModifierElement<E> {
    fn new(element: E) -> Self {
        let mut hasher = default::new();
        element.hash(&mut hasher);
        Self {
            element,
            cached_hash: hasher.finish(),
        }
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

    fn can_update_node(&self, node: &dyn ModifierNode) -> bool {
        node.as_any().is::<E::Node>()
    }

    fn update_node(&self, node: &mut dyn ModifierNode) {
        if let Some(typed) = node.as_any_mut().downcast_mut::<E::Node>() {
            self.element.update(typed);
        }
    }

    fn key(&self) -> Option<u64> {
        self.element.key()
    }

    fn capabilities(&self) -> NodeCapabilities {
        self.element.capabilities()
    }

    fn hash_code(&self) -> u64 {
        self.cached_hash
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

    fn requires_update(&self) -> bool {
        self.element.always_update()
    }

    fn auto_invalidates_on_update(&self) -> bool {
        self.element.auto_invalidate_on_update()
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        self.element.update_invalidation_kind()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn request_update_auto_invalidations(
    element: &dyn AnyModifierElement,
    context: &mut dyn ModifierNodeContext,
    capabilities: NodeCapabilities,
) {
    if let Some(kind) = element.update_invalidation_kind() {
        let capabilities = NodeCapabilities::for_invalidation(kind);
        context.push_active_capabilities(capabilities);
        context.invalidate(kind);
        context.pop_active_capabilities();
    } else if element.auto_invalidates_on_update() {
        request_auto_invalidations(context, capabilities);
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

/// Iterator walking a modifier chain by indexing into `ordered_nodes`.
///
/// This avoids the per-step `RefCell::borrow()` + `NodeLink::clone()` cost
/// of following the linked-list through `NodeState::child`/`parent`.
pub struct ModifierChainIter<'a> {
    chain: &'a ModifierNodeChain,
    cursor: usize,
    remaining: usize,
    direction: TraversalDirection,
}

impl<'a> ModifierChainIter<'a> {
    fn forward(chain: &'a ModifierNodeChain) -> Self {
        Self {
            chain,
            cursor: 0,
            remaining: chain.ordered_nodes.len(),
            direction: TraversalDirection::Forward,
        }
    }

    fn backward(chain: &'a ModifierNodeChain) -> Self {
        let len = chain.ordered_nodes.len();
        Self {
            chain,
            cursor: len.wrapping_sub(1),
            remaining: len,
            direction: TraversalDirection::Backward,
        }
    }
}

impl<'a> Iterator for ModifierChainIter<'a> {
    type Item = ModifierChainNodeRef<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let (link, caps, agg) = self.chain.ordered_nodes[self.cursor];
        let node_ref = self.chain.make_node_ref_with_caps(link, caps, agg);
        self.remaining -= 1;
        match self.direction {
            TraversalDirection::Forward => self.cursor += 1,
            TraversalDirection::Backward => self.cursor = self.cursor.wrapping_sub(1),
        }
        Some(node_ref)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for ModifierChainIter<'a> {}
impl<'a> std::iter::FusedIterator for ModifierChainIter<'a> {}

#[derive(Debug)]
struct ModifierNodeEntry {
    element_type: TypeId,
    node_type: TypeId,
    key: Option<u64>,
    hash_code: u64,
    element: DynModifierElement,
    node: Rc<RefCell<Box<dyn ModifierNode>>>,
    capabilities: NodeCapabilities,
}

impl ModifierNodeEntry {
    fn new(
        element_type: TypeId,
        node_type: TypeId,
        key: Option<u64>,
        element: DynModifierElement,
        node: Box<dyn ModifierNode>,
        hash_code: u64,
        capabilities: NodeCapabilities,
    ) -> Self {
        let node_rc = Rc::new(RefCell::new(node));
        let entry = Self {
            element_type,
            node_type,
            key,
            hash_code,
            element,
            node: Rc::clone(&node_rc),
            capabilities,
        };
        entry
            .node
            .borrow()
            .node_state()
            .set_capabilities(entry.capabilities);
        entry
    }
}

fn visit_node_tree_mut(
    node: &mut dyn ModifierNode,
    visitor: &mut dyn FnMut(&mut dyn ModifierNode),
) {
    visitor(node);
    node.for_each_delegate_mut(&mut |child| visit_node_tree_mut(child, visitor));
}

fn nth_delegate(node: &dyn ModifierNode, target: usize) -> Option<&dyn ModifierNode> {
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

fn nth_delegate_mut(node: &mut dyn ModifierNode, target: usize) -> Option<&mut dyn ModifierNode> {
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

fn with_node_context<F, R>(
    node: &mut dyn ModifierNode,
    context: &mut dyn ModifierNodeContext,
    f: F,
) -> R
where
    F: FnOnce(&mut dyn ModifierNode, &mut dyn ModifierNodeContext) -> R,
{
    context.push_active_capabilities(node.node_state().capabilities());
    let result = f(node, context);
    context.pop_active_capabilities();
    result
}

fn request_auto_invalidations(
    context: &mut dyn ModifierNodeContext,
    capabilities: NodeCapabilities,
) {
    if capabilities.is_empty() {
        return;
    }

    context.push_active_capabilities(capabilities);

    if capabilities.contains(NodeCapabilities::LAYOUT) {
        context.invalidate(InvalidationKind::Layout);
    }
    if capabilities.contains(NodeCapabilities::DRAW) {
        context.invalidate(InvalidationKind::Draw);
    }
    if capabilities.contains(NodeCapabilities::POINTER_INPUT) {
        context.invalidate(InvalidationKind::PointerInput);
    }
    if capabilities.contains(NodeCapabilities::SEMANTICS) {
        context.invalidate(InvalidationKind::Semantics);
    }
    if capabilities.contains(NodeCapabilities::FOCUS) {
        context.invalidate(InvalidationKind::Focus);
    }

    context.pop_active_capabilities();
}

/// Attaches a node tree by calling on_attach for all unattached nodes.
///
/// # Safety
/// Callers must ensure no immutable RefCell borrows are held on the node
/// when calling this function. The on_attach callback may trigger mutations
/// (invalidations, state updates, etc.) that require mutable access, which
/// would panic if an immutable borrow is held across the call.
fn attach_node_tree(node: &mut dyn ModifierNode, context: &mut dyn ModifierNodeContext) {
    visit_node_tree_mut(node, &mut |n| {
        if !n.node_state().is_attached() {
            n.node_state().set_attached(true);
            with_node_context(n, context, |node, ctx| node.on_attach(ctx));
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
    entries: Vec<ModifierNodeEntry>,
    aggregated_capabilities: NodeCapabilities,
    head_aggregate_child_capabilities: NodeCapabilities,
    head_sentinel: Box<SentinelNode>,
    tail_sentinel: Box<SentinelNode>,
    ordered_nodes: Vec<(NodeLink, NodeCapabilities, NodeCapabilities)>,
    scratch_old_used: Vec<bool>,
    scratch_match_order: Vec<Option<usize>>,
    scratch_final_slots: Vec<Option<ModifierNodeEntry>>,
    scratch_elements: Vec<DynModifierElement>,
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
    cached_capabilities: Option<NodeCapabilities>,
    cached_aggregate_child: Option<NodeCapabilities>,
}

impl Default for ModifierNodeChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Index structure for O(1) modifier entry lookups during update.
///
/// This avoids O(n²) complexity by pre-building hash maps that allow constant-time
/// lookups for matching entries by key, hash, or type.
struct EntryIndex {
    keyed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    hashed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    typed: HashMap<(TypeId, TypeId), Vec<usize>>,
}

struct EntryMatchQuery<'a> {
    element_type: TypeId,
    node_type: TypeId,
    key: Option<u64>,
    hash_code: u64,
    element: &'a DynModifierElement,
}

impl EntryIndex {
    fn build(entries: &[ModifierNodeEntry]) -> Self {
        let mut keyed = HashMap::default();
        let mut hashed = HashMap::default();
        let mut typed = HashMap::default();

        for (i, entry) in entries.iter().enumerate() {
            if let Some(key_value) = entry.key {
                keyed
                    .entry((entry.element_type, entry.node_type, key_value))
                    .or_insert_with(Vec::new)
                    .push(i);
            } else {
                hashed
                    .entry((entry.element_type, entry.node_type, entry.hash_code))
                    .or_insert_with(Vec::new)
                    .push(i);
                typed
                    .entry((entry.element_type, entry.node_type))
                    .or_insert_with(Vec::new)
                    .push(i);
            }
        }

        Self {
            keyed,
            hashed,
            typed,
        }
    }

    fn find_match(
        &self,
        entries: &[ModifierNodeEntry],
        used: &[bool],
        query: EntryMatchQuery<'_>,
    ) -> Option<usize> {
        if let Some(key_value) = query.key {
            if let Some(candidates) =
                self.keyed
                    .get(&(query.element_type, query.node_type, key_value))
            {
                for &i in candidates {
                    if !used[i] {
                        return Some(i);
                    }
                }
            }
        } else {
            if let Some(candidates) =
                self.hashed
                    .get(&(query.element_type, query.node_type, query.hash_code))
            {
                for &i in candidates {
                    if !used[i]
                        && entries[i]
                            .element
                            .as_ref()
                            .equals_element(query.element.as_ref())
                    {
                        return Some(i);
                    }
                }
            }

            if let Some(candidates) = self.typed.get(&(query.element_type, query.node_type)) {
                for &i in candidates {
                    if !used[i] {
                        return Some(i);
                    }
                }
            }
        }

        None
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
            scratch_old_used: Vec::new(),
            scratch_match_order: Vec::new(),
            scratch_final_slots: Vec::new(),
            scratch_elements: Vec::new(),
        };
        chain.sync_chain_links();
        chain
    }

    /// Detaches all nodes in the chain.
    pub fn detach_nodes(&mut self) {
        for entry in &self.entries {
            detach_node_tree(&mut **entry.node.borrow_mut());
        }
    }

    /// Attaches all nodes in the chain.
    pub fn attach_nodes(&mut self, context: &mut dyn ModifierNodeContext) {
        for entry in &self.entries {
            attach_node_tree(&mut **entry.node.borrow_mut(), context);
        }
    }

    /// Rebuilds the internal chain links (parent/child relationships).
    /// This should be called if nodes have been detached but are intended to be reused.
    pub fn repair_chain(&mut self) {
        self.sync_chain_links();
    }

    /// Reconcile the chain against the provided elements, attaching newly
    /// created nodes and detaching nodes that are no longer required.
    ///
    /// This method delegates to `update_from_ref_iter` which handles the
    /// actual reconciliation logic.
    pub fn update_from_slice(
        &mut self,
        elements: &[DynModifierElement],
        context: &mut dyn ModifierNodeContext,
    ) {
        self.update_from_ref_iter(elements.iter(), context);
    }

    /// Reconcile the chain against the provided iterator of element references.
    ///
    /// This is the preferred method as it avoids requiring a collected slice,
    /// enabling zero-allocation traversal of modifier trees.
    pub fn update_from_ref_iter<'a, I>(
        &mut self,
        elements: I,
        context: &mut dyn ModifierNodeContext,
    ) where
        I: Iterator<Item = &'a DynModifierElement>,
    {
        let old_len = self.entries.len();
        let mut fast_path_failed_at: Option<usize> = None;
        let mut elements_count = 0;

        self.scratch_elements.clear();

        for (idx, element) in elements.enumerate() {
            elements_count = idx + 1;

            if fast_path_failed_at.is_none() && idx < old_len {
                let entry = &mut self.entries[idx];
                let same_type = entry.element_type == element.element_type();
                let same_node_type = entry.node_type == element.node_type();
                let same_key = entry.key == element.key();
                let same_hash = entry.hash_code == element.hash_code();

                let positional_update = element.requires_update();
                if same_type && same_node_type && same_key && (same_hash || positional_update) {
                    let can_update_node = {
                        let node_borrow = entry.node.borrow();
                        element.can_update_node(&**node_borrow)
                    };
                    if !can_update_node {
                        fast_path_failed_at = Some(idx);
                        self.scratch_elements.push(element.clone());
                        continue;
                    }

                    let same_element = entry.element.as_ref().equals_element(element.as_ref());
                    let capabilities = element.capabilities();

                    {
                        let node_borrow = entry.node.borrow();
                        if !node_borrow.node_state().is_attached() {
                            drop(node_borrow);
                            attach_node_tree(&mut **entry.node.borrow_mut(), context);
                        }
                    }

                    let needs_update = !same_element || element.requires_update();
                    if needs_update {
                        element.update_node(&mut **entry.node.borrow_mut());
                        entry.element = element.clone();
                        entry.hash_code = element.hash_code();
                        request_update_auto_invalidations(element.as_ref(), context, capabilities);
                    }

                    entry.capabilities = capabilities;
                    entry
                        .node
                        .borrow()
                        .node_state()
                        .set_capabilities(capabilities);
                    continue;
                }
                fast_path_failed_at = Some(idx);
            }

            self.scratch_elements.push(element.clone());
        }

        if fast_path_failed_at.is_none() && self.scratch_elements.is_empty() {
            if elements_count < self.entries.len() {
                for entry in self.entries.drain(elements_count..) {
                    request_auto_invalidations(context, entry.capabilities);
                    detach_node_tree(&mut **entry.node.borrow_mut());
                }
            }
            self.sync_chain_links();
            return;
        }

        let fail_idx = fast_path_failed_at.unwrap_or(old_len);

        let mut old_entries: Vec<ModifierNodeEntry> = self.entries.drain(fail_idx..).collect();
        let processed_entries_len = self.entries.len();
        let old_len = old_entries.len();

        self.scratch_old_used.clear();
        self.scratch_old_used.resize(old_len, false);

        self.scratch_match_order.clear();
        self.scratch_match_order.resize(old_len, None);

        let index = EntryIndex::build(&old_entries);

        let new_elements_count = self.scratch_elements.len();
        self.scratch_final_slots.clear();
        self.scratch_final_slots.reserve(new_elements_count);

        for (new_pos, element) in self.scratch_elements.drain(..).enumerate() {
            self.scratch_final_slots.push(None);
            let element_type = element.element_type();
            let node_type = element.node_type();
            let key = element.key();
            let hash_code = element.hash_code();
            let capabilities = element.capabilities();

            let matched_idx = index.find_match(
                &old_entries,
                &self.scratch_old_used,
                EntryMatchQuery {
                    element_type,
                    node_type,
                    key,
                    hash_code,
                    element: &element,
                },
            );

            if let Some(idx) = matched_idx {
                let entry = &mut old_entries[idx];
                let can_update_node = {
                    let node_borrow = entry.node.borrow();
                    element.can_update_node(&**node_borrow)
                };
                if !can_update_node {
                    let replacement = ModifierNodeEntry::new(
                        element_type,
                        node_type,
                        key,
                        element.clone(),
                        element.create_node(),
                        hash_code,
                        capabilities,
                    );
                    attach_node_tree(&mut **replacement.node.borrow_mut(), context);
                    element.update_node(&mut **replacement.node.borrow_mut());
                    request_auto_invalidations(context, capabilities);
                    self.scratch_final_slots[new_pos] = Some(replacement);
                    continue;
                }

                self.scratch_old_used[idx] = true;
                self.scratch_match_order[idx] = Some(new_pos);
                let moved = idx != new_pos;

                let same_element = entry.element.as_ref().equals_element(element.as_ref());

                {
                    let node_borrow = entry.node.borrow();
                    if !node_borrow.node_state().is_attached() {
                        drop(node_borrow);
                        attach_node_tree(&mut **entry.node.borrow_mut(), context);
                    }
                }

                let needs_update = !same_element || element.requires_update();
                if needs_update {
                    element.update_node(&mut **entry.node.borrow_mut());
                    entry.element = element;
                    entry.hash_code = hash_code;
                    request_update_auto_invalidations(
                        entry.element.as_ref(),
                        context,
                        capabilities,
                    );
                }
                if moved {
                    request_auto_invalidations(context, capabilities);
                }

                entry.key = key;
                entry.element_type = element_type;
                entry.node_type = node_type;
                entry.capabilities = capabilities;
                entry
                    .node
                    .borrow()
                    .node_state()
                    .set_capabilities(capabilities);
            } else {
                let entry = ModifierNodeEntry::new(
                    element_type,
                    node_type,
                    key,
                    element.clone(),
                    element.create_node(),
                    hash_code,
                    capabilities,
                );
                attach_node_tree(&mut **entry.node.borrow_mut(), context);
                element.update_node(&mut **entry.node.borrow_mut());
                request_auto_invalidations(context, capabilities);
                self.scratch_final_slots[new_pos] = Some(entry);
            }
        }

        for (i, entry) in old_entries.into_iter().enumerate() {
            if self.scratch_old_used[i] {
                if let Some(pos) = self.scratch_match_order[i] {
                    self.scratch_final_slots[pos] = Some(entry);
                } else {
                    request_auto_invalidations(context, entry.capabilities);
                    detach_node_tree(&mut **entry.node.borrow_mut());
                }
            } else {
                request_auto_invalidations(context, entry.capabilities);
                detach_node_tree(&mut **entry.node.borrow_mut());
            }
        }

        self.entries.reserve(self.scratch_final_slots.len());
        for slot in self.scratch_final_slots.drain(..) {
            if let Some(entry) = slot {
                self.entries.push(entry);
            } else {
                log::error!("modifier reconciliation produced an empty final slot");
            }
        }

        debug_assert_eq!(
            self.entries.len(),
            processed_entries_len + new_elements_count
        );
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
            reset_node_tree(&mut **entry.node.borrow_mut());
        }
    }

    /// Detaches every node in the chain and clears internal storage.
    pub fn detach_all(&mut self) {
        for entry in std::mem::take(&mut self.entries) {
            detach_node_tree(&mut **entry.node.borrow_mut());
            {
                let node_borrow = entry.node.borrow();
                let state = node_borrow.node_state();
                state.set_capabilities(NodeCapabilities::empty());
            }
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
        ModifierChainIter::forward(self)
    }

    /// Iterates over the chain from tail to head, skipping sentinels.
    pub fn tail_to_head(&self) -> ModifierChainIter<'_> {
        ModifierChainIter::backward(self)
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

    /// Calls `f` for every node containing any capability from `mask`, providing the node ref.
    pub fn for_each_node_with_capability<F>(&self, mask: NodeCapabilities, mut f: F)
    where
        F: FnMut(ModifierChainNodeRef<'_>, &dyn ModifierNode),
    {
        self.for_each_forward_matching(mask, |node_ref| {
            node_ref.with_node(|node| f(node_ref.clone(), node));
        });
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

    /// Returns the node reference that owns `node`.
    pub fn find_node_ref(&self, node: &dyn ModifierNode) -> Option<ModifierChainNodeRef<'_>> {
        fn node_data_ptr(node: &dyn ModifierNode) -> *const () {
            node as *const dyn ModifierNode as *const ()
        }

        let target = node_data_ptr(node);
        for (index, entry) in self.entries.iter().enumerate() {
            if node_data_ptr(&**entry.node.borrow()) == target {
                return Some(self.make_node_ref(NodeLink::Entry(NodePath::root(index))));
            }
        }

        self.ordered_nodes.iter().find_map(|(link, _caps, _agg)| {
            if matches!(link, NodeLink::Entry(path) if path.delegates().is_empty()) {
                return None;
            }
            let matches_target = match link {
                NodeLink::Head => node_data_ptr(self.head_sentinel.as_ref()) == target,
                NodeLink::Tail => node_data_ptr(self.tail_sentinel.as_ref()) == target,
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    node_data_ptr(&**node_borrow) == target
                }
            };
            if matches_target {
                Some(self.make_node_ref(*link))
            } else {
                None
            }
        })
    }

    /// Downcasts the node at `index` to the requested type.
    /// Returns a `Ref` guard that dereferences to the node type.
    pub fn node<N: ModifierNode + 'static>(&self, index: usize) -> Option<std::cell::Ref<'_, N>> {
        self.entries.get(index).and_then(|entry| {
            std::cell::Ref::filter_map(entry.node.borrow(), |boxed_node| {
                boxed_node.as_any().downcast_ref::<N>()
            })
            .ok()
        })
    }

    /// Downcasts the node at `index` to the requested mutable type.
    /// Returns a `RefMut` guard that dereferences to the node type.
    pub fn node_mut<N: ModifierNode + 'static>(
        &self,
        index: usize,
    ) -> Option<std::cell::RefMut<'_, N>> {
        self.entries.get(index).and_then(|entry| {
            std::cell::RefMut::filter_map(entry.node.borrow_mut(), |boxed_node| {
                boxed_node.as_any_mut().downcast_mut::<N>()
            })
            .ok()
        })
    }

    /// Returns an Rc clone of the node at the given index for shared ownership.
    /// This is used by coordinators to hold direct references to nodes.
    pub fn get_node_rc(&self, index: usize) -> Option<Rc<RefCell<Box<dyn ModifierNode>>>> {
        self.entries.get(index).map(|entry| Rc::clone(&entry.node))
    }

    /// Returns true if the chain contains any nodes matching the given invalidation kind.
    pub fn has_nodes_for_invalidation(&self, kind: InvalidationKind) -> bool {
        self.aggregated_capabilities
            .contains(NodeCapabilities::for_invalidation(kind))
    }

    /// Visits every node mutably in insertion order together with its capability mask.
    pub fn visit_nodes_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut dyn ModifierNode, NodeCapabilities),
    {
        for index in 0..self.ordered_nodes.len() {
            let (link, cached_caps, _agg) = self.ordered_nodes[index];
            match link {
                NodeLink::Head => {
                    f(self.head_sentinel.as_mut(), cached_caps);
                }
                NodeLink::Tail => {
                    f(self.tail_sentinel.as_mut(), cached_caps);
                }
                NodeLink::Entry(path) => {
                    let mut node_borrow = self.entries[path.entry()].node.borrow_mut();
                    if path.delegates().is_empty() {
                        f(&mut **node_borrow, cached_caps);
                    } else {
                        let mut current: &mut dyn ModifierNode = &mut **node_borrow;
                        for &delegate_index in path.delegates() {
                            if let Some(delegate) =
                                nth_delegate_mut(current, delegate_index as usize)
                            {
                                current = delegate;
                            } else {
                                return;
                            }
                        }
                        f(current, cached_caps);
                    }
                }
            }
        }
    }

    fn make_node_ref(&self, link: NodeLink) -> ModifierChainNodeRef<'_> {
        ModifierChainNodeRef {
            chain: self,
            link,
            cached_capabilities: None,
            cached_aggregate_child: None,
        }
    }

    fn make_node_ref_with_caps(
        &self,
        link: NodeLink,
        caps: NodeCapabilities,
        aggregate_child: NodeCapabilities,
    ) -> ModifierChainNodeRef<'_> {
        ModifierChainNodeRef {
            chain: self,
            link,
            cached_capabilities: Some(caps),
            cached_aggregate_child: Some(aggregate_child),
        }
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
        for (link, _caps, _agg) in self.ordered_nodes.iter().copied() {
            match &previous {
                NodeLink::Head => self.head_sentinel.node_state().set_child_link(Some(link)),
                NodeLink::Tail => self.tail_sentinel.node_state().set_child_link(Some(link)),
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    if path.delegates().is_empty() {
                        node_borrow.node_state().set_child_link(Some(link));
                    } else {
                        let mut current: &dyn ModifierNode = &**node_borrow;
                        for &delegate_index in path.delegates() {
                            if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                                current = delegate;
                            }
                        }
                        current.node_state().set_child_link(Some(link));
                    }
                }
            }
            match &link {
                NodeLink::Head => self
                    .head_sentinel
                    .node_state()
                    .set_parent_link(Some(previous)),
                NodeLink::Tail => self
                    .tail_sentinel
                    .node_state()
                    .set_parent_link(Some(previous)),
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    if path.delegates().is_empty() {
                        node_borrow.node_state().set_parent_link(Some(previous));
                    } else {
                        let mut current: &dyn ModifierNode = &**node_borrow;
                        for &delegate_index in path.delegates() {
                            if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                                current = delegate;
                            }
                        }
                        current.node_state().set_parent_link(Some(previous));
                    }
                }
            }
            previous = link;
        }

        match &previous {
            NodeLink::Head => self
                .head_sentinel
                .node_state()
                .set_child_link(Some(NodeLink::Tail)),
            NodeLink::Tail => self
                .tail_sentinel
                .node_state()
                .set_child_link(Some(NodeLink::Tail)),
            NodeLink::Entry(path) => {
                let node_borrow = self.entries[path.entry()].node.borrow();
                if path.delegates().is_empty() {
                    node_borrow
                        .node_state()
                        .set_child_link(Some(NodeLink::Tail));
                } else {
                    let mut current: &dyn ModifierNode = &**node_borrow;
                    for &delegate_index in path.delegates() {
                        if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                            current = delegate;
                        }
                    }
                    current.node_state().set_child_link(Some(NodeLink::Tail));
                }
            }
        }
        self.tail_sentinel
            .node_state()
            .set_parent_link(Some(previous));
        self.tail_sentinel.node_state().set_child_link(None);

        let mut aggregate = NodeCapabilities::empty();
        for (link, cached_caps, cached_aggregate) in self.ordered_nodes.iter_mut().rev() {
            aggregate |= *cached_caps;
            *cached_aggregate = aggregate;
            match link {
                NodeLink::Head => {
                    self.head_sentinel
                        .node_state()
                        .set_aggregate_child_capabilities(aggregate);
                }
                NodeLink::Tail => {
                    self.tail_sentinel
                        .node_state()
                        .set_aggregate_child_capabilities(aggregate);
                }
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    let state = if path.delegates().is_empty() {
                        node_borrow.node_state()
                    } else {
                        let mut current: &dyn ModifierNode = &**node_borrow;
                        for &delegate_index in path.delegates() {
                            if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                                current = delegate;
                            }
                        }
                        current.node_state()
                    };
                    state.set_aggregate_child_capabilities(aggregate);
                }
            }
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
        let mut path_buf = [0usize; MAX_DELEGATE_DEPTH];
        for (index, entry) in self.entries.iter().enumerate() {
            let node_borrow = entry.node.borrow();
            Self::enumerate_link_order(
                &**node_borrow,
                index,
                &mut path_buf,
                0,
                &mut self.ordered_nodes,
            );
        }
    }

    fn enumerate_link_order(
        node: &dyn ModifierNode,
        entry: usize,
        path_buf: &mut [usize; MAX_DELEGATE_DEPTH],
        path_len: usize,
        out: &mut Vec<(NodeLink, NodeCapabilities, NodeCapabilities)>,
    ) {
        let caps = node.node_state().capabilities();
        out.push((
            NodeLink::Entry(NodePath::from_slice(entry, &path_buf[..path_len])),
            caps,
            NodeCapabilities::empty(),
        ));
        let mut delegate_index = 0usize;
        node.for_each_delegate(&mut |child| {
            if path_len < MAX_DELEGATE_DEPTH {
                path_buf[path_len] = delegate_index;
                Self::enumerate_link_order(child, entry, path_buf, path_len + 1, out);
            }
            delegate_index += 1;
        });
    }
}

impl<'a> ModifierChainNodeRef<'a> {
    fn with_state<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        match &self.link {
            NodeLink::Head => f(self.chain.head_sentinel.node_state()),
            NodeLink::Tail => f(self.chain.tail_sentinel.node_state()),
            NodeLink::Entry(path) => {
                let node_borrow = self.chain.entries[path.entry()].node.borrow();
                if path.delegates().is_empty() {
                    f(node_borrow.node_state())
                } else {
                    let mut current: &dyn ModifierNode = &**node_borrow;
                    for &delegate_index in path.delegates() {
                        if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                            current = delegate;
                        } else {
                            return f(node_borrow.node_state());
                        }
                    }
                    f(current.node_state())
                }
            }
        }
    }

    /// Provides access to the node via a closure, properly handling RefCell borrows.
    /// Returns None for sentinel nodes.
    pub fn with_node<R>(&self, f: impl FnOnce(&dyn ModifierNode) -> R) -> Option<R> {
        match &self.link {
            NodeLink::Head => None,
            NodeLink::Tail => None,
            NodeLink::Entry(path) => {
                let node_borrow = self.chain.entries[path.entry()].node.borrow();
                if path.delegates().is_empty() {
                    Some(f(&**node_borrow))
                } else {
                    let mut current: &dyn ModifierNode = &**node_borrow;
                    for &delegate_index in path.delegates() {
                        current = nth_delegate(current, delegate_index as usize)?;
                    }
                    Some(f(current))
                }
            }
        }
    }

    /// Returns the parent reference, including sentinel head when applicable.
    #[inline]
    pub fn parent(&self) -> Option<Self> {
        self.with_state(|state| state.parent_link())
            .map(|link| self.chain.make_node_ref(link))
    }

    /// Returns the child reference, including sentinel tail for the last entry.
    #[inline]
    pub fn child(&self) -> Option<Self> {
        self.with_state(|state| state.child_link())
            .map(|link| self.chain.make_node_ref(link))
    }

    /// Returns the capability mask for this specific node.
    #[inline]
    pub fn kind_set(&self) -> NodeCapabilities {
        if let Some(caps) = self.cached_capabilities {
            return caps;
        }
        match &self.link {
            NodeLink::Head | NodeLink::Tail => NodeCapabilities::empty(),
            NodeLink::Entry(_) => self.with_state(|state| state.capabilities()),
        }
    }

    /// Returns the entry index backing this node when it is part of the chain.
    pub fn entry_index(&self) -> Option<usize> {
        match &self.link {
            NodeLink::Entry(path) => Some(path.entry()),
            _ => None,
        }
    }

    /// Returns how many delegate hops separate this node from its root element.
    pub fn delegate_depth(&self) -> usize {
        match &self.link {
            NodeLink::Entry(path) => path.delegates().len(),
            _ => 0,
        }
    }

    /// Returns the aggregated capability mask for the subtree rooted at this node.
    #[inline]
    pub fn aggregate_child_capabilities(&self) -> NodeCapabilities {
        if let Some(agg) = self.cached_aggregate_child {
            return agg;
        }
        if self.is_tail() {
            NodeCapabilities::empty()
        } else {
            self.with_state(|state| state.aggregate_child_capabilities())
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
        matches!(self.link, NodeLink::Head | NodeLink::Tail)
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
