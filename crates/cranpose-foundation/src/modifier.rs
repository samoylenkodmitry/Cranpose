//! Modifier node scaffolding for Cranpose.
//!
//! This module defines the foundational pieces of the Cranpose
//! `Modifier.Node` system. It introduces traits for modifier nodes and their
//! contexts as well as a lightweight chain container that reconciles nodes
//! across updates.

use std::any::{type_name, Any, TypeId};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{BitOr, BitOrAssign};
use std::rc::Rc;

use cranpose_core::collections::map::HashMap;
use cranpose_core::hash::default;

pub use cranpose_ui_graphics::DrawScope;
pub use cranpose_ui_graphics::Size;
pub use cranpose_ui_layout::{Constraints, Measurable};

use crate::nodes::input::types::PointerEvent;
// use cranpose_core::NodeId;

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
    /// since the last call to [`clear_invalidations`]. Duplicate requests for
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
    /// [`take_update_requested`].
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
        // Default: pass through to wrapped content by measuring the child.
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
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // Default: no custom drawing
    }

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
    fn merge_semantics(&self, _config: &mut SemanticsConfiguration) {
        // Default: no semantics added
    }
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
    fn on_focus_changed(&mut self, _context: &mut dyn ModifierNodeContext, _state: FocusState) {
        // Default: no action on focus change
    }
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
    /// Compose's `heading()`, which is a property rather than a `Role`, but
    /// reaches the platform through the same field on every backend Cranpose
    /// targets (`AccessibilityNodeInfo.setHeading`, `Role::Heading`,
    /// `<h*>`/`UIAccessibilityTraitHeader`).
    Header,
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
    /// <label>", so it is a verb phrase, not a repeat of the label.
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
            // Matches Compose: a node is enabled unless `disabled()` says
            // otherwise, so an app that never thinks about it gets the right
            // answer.
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
    /// Compose's `Role`.
    pub role: Option<SemanticsWidgetRole>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub enabled: bool,
    pub is_clickable: bool,
    pub is_editable_text: bool,
    pub text_selection: Option<crate::text::TextRange>,
    pub custom_actions: Vec<SemanticsCustomAction>,
    /// Controls this node drew itself instead of laying out. See
    /// [`CanvasSemanticsNode`].
    pub canvas_children: Vec<CanvasSemanticsNode>,
}

impl Default for SemanticsConfiguration {
    fn default() -> Self {
        Self {
            content_description: None,
            state_description: None,
            on_click_label: None,
            role: None,
            selected: None,
            toggled: None,
            enabled: true,
            is_clickable: false,
            is_editable_text: false,
            text_selection: None,
            custom_actions: Vec::new(),
            canvas_children: Vec::new(),
        }
    }
}

impl SemanticsConfiguration {
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
        if let Some(role) = other.role {
            self.role = Some(role);
        }
        if let Some(selected) = other.selected {
            self.selected = Some(selected);
        }
        if let Some(toggled) = other.toggled {
            self.toggled = Some(toggled);
        }
        // Disabling wins: a chain that disables the node anywhere disables it,
        // matching how Compose's `disabled()` is not undone by an inner node.
        self.enabled &= other.enabled;
        self.is_clickable |= other.is_clickable;
        self.is_editable_text |= other.is_editable_text;
        if let Some(selection) = other.text_selection {
            self.text_selection = Some(selection);
        }
        self.custom_actions
            .extend(other.custom_actions.iter().cloned());
        self.canvas_children
            .extend(other.canvas_children.iter().cloned());
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
    /// Current position in `ordered_nodes`. For forward iteration, starts at 0
    /// and increments; for backward, starts at len-1 and decrements.
    cursor: usize,
    /// Number of elements remaining (avoids underflow on backward iteration).
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
        // Wrap the boxed node in Rc<RefCell<>> for shared ownership
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
    /// (link, own_capabilities, aggregate_child_capabilities)
    ordered_nodes: Vec<(NodeLink, NodeCapabilities, NodeCapabilities)>,
    // Scratch buffers reused during update to avoid repeated allocations
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
    /// Capabilities cached from `ordered_nodes` build time — avoids RefCell borrow in kind_set().
    cached_capabilities: Option<NodeCapabilities>,
    /// Aggregate child capabilities cached from `ordered_nodes` — avoids RefCell borrow.
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
    /// Map (element type, node type, key) to keyed entries.
    keyed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    /// Map (element type, node type, hash) to unkeyed entries with specific hash.
    hashed: HashMap<(TypeId, TypeId, u64), Vec<usize>>,
    /// Map (element type, node type) to unkeyed entries.
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
                // Keyed entry
                keyed
                    .entry((entry.element_type, entry.node_type, key_value))
                    .or_insert_with(Vec::new)
                    .push(i);
            } else {
                // Unkeyed entry - add to both hash and type indices
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

    /// Find the best matching entry for reuse.
    ///
    /// Matching priority (from highest to lowest):
    /// 1. Keyed match: same element type, node type, and key.
    /// 2. Exact match: same retained identity, no key, same hash, and equal element.
    /// 3. Retained identity match without equality, which requires update.
    fn find_match(
        &self,
        entries: &[ModifierNodeEntry],
        used: &[bool],
        query: EntryMatchQuery<'_>,
    ) -> Option<usize> {
        if let Some(key_value) = query.key {
            // Priority 1: Keyed lookup - O(1)
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
            // Priority 2: Exact match (hash + equality) - O(1) lookup + O(k) equality checks
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

            // Priority 3: Type match only - O(1) lookup + O(k) scan
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
        // Fast path: try to match elements sequentially without building index.
        // If all elements match in order (same type and key at same position),
        // we skip the expensive EntryIndex building. This is O(n) instead of O(n + m).
        let old_len = self.entries.len();
        let mut fast_path_failed_at: Option<usize> = None;
        let mut elements_count = 0;

        // Collect elements we need to process in slow path
        self.scratch_elements.clear();

        for (idx, element) in elements.enumerate() {
            elements_count = idx + 1;

            if fast_path_failed_at.is_none() && idx < old_len {
                let entry = &mut self.entries[idx];
                let same_type = entry.element_type == element.element_type();
                let same_node_type = entry.node_type == element.node_type();
                let same_key = entry.key == element.key();
                let same_hash = entry.hash_code == element.hash_code();

                // Fast path requires same type, key, AND hash to ensure we're not
                // breaking reordering semantics (where elements can move positions)
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

                    // Fast path: element matches at same position
                    let same_element = entry.element.as_ref().equals_element(element.as_ref());
                    let capabilities = element.capabilities();

                    // Re-attach node if it was detached during a previous update
                    {
                        let node_borrow = entry.node.borrow();
                        if !node_borrow.node_state().is_attached() {
                            drop(node_borrow);
                            attach_node_tree(&mut **entry.node.borrow_mut(), context);
                        }
                    }

                    // Optimize updates: only call update_node if element changed
                    let needs_update = !same_element || element.requires_update();
                    if needs_update {
                        element.update_node(&mut **entry.node.borrow_mut());
                        entry.element = element.clone();
                        entry.hash_code = element.hash_code();
                        request_update_auto_invalidations(element.as_ref(), context, capabilities);
                    }

                    // Always update metadata
                    entry.capabilities = capabilities;
                    entry
                        .node
                        .borrow()
                        .node_state()
                        .set_capabilities(capabilities);
                    continue;
                }
                // Fast path failed - mark position and fall through to collect
                fast_path_failed_at = Some(idx);
            }

            // Collect element for slow path processing
            self.scratch_elements.push(element.clone());
        }

        // Fast path succeeded if:
        // 1. No mismatch was found (fast_path_failed_at is None)
        // 2. All elements were processed via fast path (scratch_elements is empty)
        // Note: If old_len=0 and we have new elements, scratch_elements won't be empty
        if fast_path_failed_at.is_none() && self.scratch_elements.is_empty() {
            // Detach any removed entries (elements_count <= old_len guaranteed here)
            if elements_count < self.entries.len() {
                for entry in self.entries.drain(elements_count..) {
                    request_auto_invalidations(context, entry.capabilities);
                    detach_node_tree(&mut **entry.node.borrow_mut());
                }
            }
            self.sync_chain_links();
            return;
        }

        // Slow path: need full reconciliation starting from failure point
        // If no mismatch but we have extra elements, fail_idx is the old length
        let fail_idx = fast_path_failed_at.unwrap_or(old_len);

        // Move entries that were already processed to a safe place
        let mut old_entries: Vec<ModifierNodeEntry> = self.entries.drain(fail_idx..).collect();
        let processed_entries_len = self.entries.len();
        let old_len = old_entries.len();

        // Reuse scratch buffers for the remaining entries only
        self.scratch_old_used.clear();
        self.scratch_old_used.resize(old_len, false);

        self.scratch_match_order.clear();
        self.scratch_match_order.resize(old_len, None);

        // Build index only for unprocessed entries
        let index = EntryIndex::build(&old_entries);

        let new_elements_count = self.scratch_elements.len();
        self.scratch_final_slots.clear();
        self.scratch_final_slots.reserve(new_elements_count);

        // Process each remaining element
        for (new_pos, element) in self.scratch_elements.drain(..).enumerate() {
            self.scratch_final_slots.push(None);
            let element_type = element.element_type();
            let node_type = element.node_type();
            let key = element.key();
            let hash_code = element.hash_code();
            let capabilities = element.capabilities();

            // Find best matching old entry via index
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
                // Reuse existing entry
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

                // Check if element actually changed
                let same_element = entry.element.as_ref().equals_element(element.as_ref());

                // Re-attach node if it was detached
                {
                    let node_borrow = entry.node.borrow();
                    if !node_borrow.node_state().is_attached() {
                        drop(node_borrow);
                        attach_node_tree(&mut **entry.node.borrow_mut(), context);
                    }
                }

                // Optimize updates: only call update_node if element changed
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

                // Always update metadata
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
                // Create new entry
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

        // Place matched entries in their new positions
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

        // Append processed entries to self.entries
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

    /// Visits every node in insertion order together with its capability mask.
    pub fn visit_nodes<F>(&self, mut f: F)
    where
        F: FnMut(&dyn ModifierNode, NodeCapabilities),
    {
        for (link, cached_caps, _agg) in &self.ordered_nodes {
            match link {
                NodeLink::Head => {
                    f(self.head_sentinel.as_ref(), *cached_caps);
                }
                NodeLink::Tail => {
                    f(self.tail_sentinel.as_ref(), *cached_caps);
                }
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    if path.delegates().is_empty() {
                        f(&**node_borrow, *cached_caps);
                    } else {
                        let mut current: &dyn ModifierNode = &**node_borrow;
                        for &delegate_index in path.delegates() {
                            if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                                current = delegate;
                            } else {
                                return; // Invalid delegate path
                            }
                        }
                        f(current, *cached_caps);
                    }
                }
            }
        }
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
                                return; // Invalid delegate path
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
            // Set child link on previous
            match &previous {
                NodeLink::Head => self.head_sentinel.node_state().set_child_link(Some(link)),
                NodeLink::Tail => self.tail_sentinel.node_state().set_child_link(Some(link)),
                NodeLink::Entry(path) => {
                    let node_borrow = self.entries[path.entry()].node.borrow();
                    // Navigate to delegate if needed
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
            // Set parent link on current
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
                    // Navigate to delegate if needed
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

        // Set child link on last node to Tail
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
                // Navigate to delegate if needed
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
            // Also update NodeState for code that reads through DelegatableNode
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
    /// Helper to get NodeState, properly handling RefCell for entries.
    /// Returns NodeState values by calling a closure with the state.
    fn with_state<R>(&self, f: impl FnOnce(&NodeState) -> R) -> R {
        match &self.link {
            NodeLink::Head => f(self.chain.head_sentinel.node_state()),
            NodeLink::Tail => f(self.chain.tail_sentinel.node_state()),
            NodeLink::Entry(path) => {
                let node_borrow = self.chain.entries[path.entry()].node.borrow();
                // Navigate through delegates if path has them
                if path.delegates().is_empty() {
                    f(node_borrow.node_state())
                } else {
                    // Navigate to the delegate node
                    let mut current: &dyn ModifierNode = &**node_borrow;
                    for &delegate_index in path.delegates() {
                        if let Some(delegate) = nth_delegate(current, delegate_index as usize) {
                            current = delegate;
                        } else {
                            // Fallback to root node state if delegate path is invalid
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
            NodeLink::Head => None, // Head sentinel
            NodeLink::Tail => None, // Tail sentinel
            NodeLink::Entry(path) => {
                let node_borrow = self.chain.entries[path.entry()].node.borrow();
                // Navigate through delegates if path has them
                if path.delegates().is_empty() {
                    Some(f(&**node_borrow))
                } else {
                    // Navigate to the delegate node
                    let mut current: &dyn ModifierNode = &**node_borrow;
                    for &delegate_index in path.delegates() {
                        // `?`: bail out with None if the delegate path is invalid.
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

    /// Finds the nearest ancestor focus target node.
    ///
    /// This is useful for focus navigation to find the parent focusable
    /// component in the tree.
    pub fn find_parent_focus_target(&self) -> Option<ModifierChainNodeRef<'a>> {
        let mut result = None;
        self.clone()
            .visit_ancestors_matching(false, NodeCapabilities::FOCUS, |node| {
                if result.is_none() {
                    result = Some(node);
                }
            });
        result
    }

    /// Finds the first descendant focus target node.
    ///
    /// This is useful for focus navigation to find the first focusable
    /// child component in the tree.
    pub fn find_first_focus_target(&self) -> Option<ModifierChainNodeRef<'a>> {
        let mut result = None;
        self.clone()
            .visit_descendants_matching(false, NodeCapabilities::FOCUS, |node| {
                if result.is_none() {
                    result = Some(node);
                }
            });
        result
    }

    /// Returns true if this node or any ancestor has focus capability.
    pub fn has_focus_capability_in_ancestors(&self) -> bool {
        let mut found = false;
        self.clone()
            .visit_ancestors_matching(true, NodeCapabilities::FOCUS, |_| {
                found = true;
            });
        found
    }
}

#[cfg(test)]
#[path = "tests/modifier_tests.rs"]
mod tests;
