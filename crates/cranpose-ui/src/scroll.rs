//! Scroll state and node implementation for cranpose.
//!
//! This module provides the core scrolling components:
//! - `ScrollState`: Holds scroll position and provides scroll control methods
//! - `ScrollNode`: Layout modifier that applies scroll offset to content
//! - `ScrollElement`: Element for creating ScrollNode instances
//!
//! The actual `Modifier.horizontal_scroll()` and `Modifier.vertical_scroll()`
//! extension methods are defined in `modifier/scroll.rs`.

use cranpose_core::{MutableState, NodeId};
use cranpose_foundation::{
    Constraints, DelegatableNode, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui_graphics::Size;
use cranpose_ui_layout::LayoutModifierMeasureResult;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::{Rc, Weak};

/// State object for scroll position tracking.
///
/// Holds the current scroll offset and provides methods to programmatically
/// control scrolling. Can be created with `rememberScrollState()`.
///
/// This is a pure scroll model - it does NOT store ephemeral gesture/pointer state.
/// Gesture state is managed locally in the scroll modifier.
#[derive(Clone, Copy)]
pub struct ScrollState {
    value: MutableState<f32>,
    max_value: MutableState<f32>,
    callbacks: MutableState<ScrollCallbacks>,
    settle_policy: MutableState<Option<ScrollSettlePolicy>>,
}

#[derive(Clone)]
struct ScrollCallbacks {
    listeners: Vec<(u64, Rc<dyn Fn()>)>,
    next_id: u64,
    pending_invalidation: bool,
}

impl Default for ScrollCallbacks {
    fn default() -> Self {
        Self {
            listeners: Vec::new(),
            next_id: 1,
            pending_invalidation: false,
        }
    }
}

/// Remaps where a scroll comes to rest once the user's interaction ends — the
/// `UIScrollView targetContentOffset` analog. Receives the naturally proposed
/// rest offset (the fling's predicted end, or the current offset for a plain
/// release/wheel idle) and the release velocity in offset units/sec; returns
/// the offset the scroll should settle at. Used e.g. by the liquid nav bar to
/// snap out of the large-title collapse band so the title never rests
/// half-faded.
pub type ScrollSettlePolicy = Rc<dyn Fn(f32, f32) -> f32>;

impl PartialEq for ScrollState {
    /// Two handles are equal when they share the same underlying state
    /// (identity, not value — composable-skip semantics).
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl ScrollState {
    /// Creates a new ScrollState with the given initial scroll position.
    pub fn new(initial: f32) -> Self {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("ScrollState::new requires an active runtime");
        Self {
            value: MutableState::with_runtime(initial, runtime.clone()),
            max_value: MutableState::with_runtime(0.0, runtime.clone()),
            callbacks: MutableState::with_runtime(ScrollCallbacks::default(), runtime.clone()),
            settle_policy: MutableState::with_runtime(None, runtime),
        }
    }

    /// Installs (or clears) the settle policy consulted when interactions end.
    pub fn set_settle_policy(&self, policy: Option<ScrollSettlePolicy>) {
        self.settle_policy.set(policy);
    }

    /// The currently installed settle policy, if any.
    pub fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        self.settle_policy.get_non_reactive()
    }

    /// Get the unique ID of this ScrollState
    pub fn id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.value.runtime_state_id().hash(&mut hasher);
        hasher.finish()
    }

    /// Gets the current scroll position in pixels (reactive - triggers recomposition).
    ///
    /// Use this in Composable functions when you want UI to update on scroll.
    /// Example: `Text("Scroll position: ${scrollState.value()}")`
    pub fn value(&self) -> f32 {
        self.value.value()
    }

    /// Gets the current scroll position in pixels (non-reactive).
    ///
    /// Use this in layout/measure phase to avoid triggering recomposition.
    /// This is called internally by ScrollNode::measure().
    pub fn value_non_reactive(&self) -> f32 {
        self.value.get_non_reactive()
    }

    /// Gets the maximum scroll value.
    pub fn max_value(&self) -> f32 {
        self.max_value.get_non_reactive()
    }

    /// Scrolls by the given delta, clamping to valid range [0, max_value].
    /// Returns the actual amount scrolled.
    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        let current = self.value();
        let max = self.max_value();
        let new_value = (current + delta).clamp(0.0, max);
        let actual_delta = new_value - current;

        if actual_delta.abs() > 0.001 {
            // Use MutableState::set which triggers snapshot observers for reactive updates
            self.value.set(new_value);

            self.invalidate();
        }

        actual_delta
    }

    /// Sets the maximum scroll value (internal use by ScrollNode).
    pub(crate) fn set_max_value(&self, max: f32) {
        self.max_value.set(max);
    }

    /// Scrolls to the given position immediately.
    pub fn scroll_to(&self, position: f32) {
        let max = self.max_value();
        let clamped = position.clamp(0.0, max);

        self.value.set(clamped);

        self.invalidate();
    }

    /// Adds an invalidation callback and returns its ID
    pub(crate) fn add_invalidate_callback(&self, callback: Box<dyn Fn()>) -> u64 {
        let callback: Rc<dyn Fn()> = Rc::from(callback);
        let mut pending = false;
        let id = self.callbacks.update(|callbacks| {
            let id = callbacks.next_id;
            callbacks.next_id = id.saturating_add(1);
            callbacks.listeners.push((id, Rc::clone(&callback)));
            pending = std::mem::replace(&mut callbacks.pending_invalidation, false);
            id
        });
        if pending {
            callback();
        }
        id
    }

    /// Removes an invalidation callback by ID
    pub(crate) fn remove_invalidate_callback(&self, id: u64) {
        self.callbacks.update(|callbacks| {
            callbacks
                .listeners
                .retain(|(listener_id, _)| *listener_id != id)
        });
    }

    fn invalidate(&self) {
        let callbacks = self.callbacks.update(|callbacks| {
            if callbacks.listeners.is_empty() {
                callbacks.pending_invalidation = true;
                Vec::new()
            } else {
                callbacks
                    .listeners
                    .iter()
                    .map(|(_, callback)| Rc::clone(callback))
                    .collect()
            }
        });
        for callback in callbacks {
            callback();
        }
    }
}

#[derive(Clone)]
pub(crate) struct ScrollMotionContext {
    inner: Rc<ScrollMotionContextInner>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum ScrollMotionContextKey {
    ScrollState {
        state_id: u64,
        is_vertical: bool,
        reverse_scrolling: bool,
    },
    LazyList {
        state_identity: usize,
        is_vertical: bool,
        reverse_scrolling: bool,
    },
}

struct ScrollMotionContextInner {
    active: Cell<bool>,
    transient_active: Cell<bool>,
    generation: Cell<u64>,
    invalidate_callbacks: RefCell<HashMap<u64, Rc<dyn Fn()>>>,
    next_invalidate_callback_id: Cell<u64>,
    pending_invalidation: Cell<bool>,
}

pub(crate) struct ScrollMotionContextStore {
    contexts: RefCell<HashMap<ScrollMotionContextKey, Weak<ScrollMotionContextInner>>>,
}

impl ScrollMotionContextStore {
    pub(crate) fn new() -> Self {
        Self {
            contexts: RefCell::new(HashMap::new()),
        }
    }

    fn context_for_key(&self, key: ScrollMotionContextKey) -> ScrollMotionContext {
        let mut contexts = self.contexts.borrow_mut();
        if let Some(inner) = contexts.get(&key).and_then(Weak::upgrade) {
            return ScrollMotionContext { inner };
        }

        let context = ScrollMotionContext::new();
        contexts.insert(key, Rc::downgrade(&context.inner));
        contexts.retain(|_, weak| weak.strong_count() > 0);
        context
    }

    pub(crate) fn clear_transient_after_frame(&self) {
        let contexts = {
            let mut contexts = self.contexts.borrow_mut();
            let live = contexts
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            contexts.retain(|_, weak| weak.strong_count() > 0);
            live
        };
        for inner in contexts {
            ScrollMotionContext { inner }.clear_transient_after_frame();
        }
    }
}

pub(crate) fn scroll_motion_context_for_key(key: ScrollMotionContextKey) -> ScrollMotionContext {
    crate::render_state::with_scroll_motion_context_store(|store| store.context_for_key(key))
}

impl ScrollMotionContext {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(ScrollMotionContextInner {
                active: Cell::new(false),
                transient_active: Cell::new(false),
                generation: Cell::new(0),
                invalidate_callbacks: RefCell::new(HashMap::new()),
                next_invalidate_callback_id: Cell::new(1),
                pending_invalidation: Cell::new(false),
            }),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.inner.active.get() || self.inner.transient_active.get()
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn stable_key(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }

    pub(crate) fn set_active(&self, active: bool) {
        let was_active = self.is_active();
        self.inner.active.set(active);
        if !active {
            self.inner.transient_active.set(false);
        }
        if was_active != self.is_active() {
            self.bump_generation();
            self.invalidate();
        }
    }

    pub(crate) fn activate_for_current_frame(&self) {
        let was_active = self.is_active();
        self.inner.transient_active.set(true);
        self.bump_generation();
        if !was_active {
            self.invalidate();
        }
    }

    pub(crate) fn add_invalidate_callback(&self, callback: Box<dyn Fn()>) -> u64 {
        let id = self.inner.next_invalidate_callback_id.get();
        self.inner
            .next_invalidate_callback_id
            .set(id.saturating_add(1));
        let callback: Rc<dyn Fn()> = Rc::from(callback);
        self.inner
            .invalidate_callbacks
            .borrow_mut()
            .insert(id, Rc::clone(&callback));
        if self.inner.pending_invalidation.replace(false) {
            callback();
        }
        id
    }

    pub(crate) fn remove_invalidate_callback(&self, id: u64) {
        self.inner.invalidate_callbacks.borrow_mut().remove(&id);
    }

    fn bump_generation(&self) -> u64 {
        let next = self.inner.generation.get().wrapping_add(1);
        self.inner.generation.set(next);
        next
    }

    fn clear_transient_after_frame(&self) {
        let was_active = self.is_active();
        if self.inner.transient_active.replace(false) {
            self.bump_generation();
            if was_active != self.is_active() {
                self.invalidate();
            }
        }
    }

    fn invalidate(&self) {
        let callbacks: Vec<Rc<dyn Fn()>> = {
            let callbacks = self.inner.invalidate_callbacks.borrow();
            if callbacks.is_empty() {
                self.inner.pending_invalidation.set(true);
                return;
            }
            callbacks.values().cloned().collect()
        };
        for callback in callbacks {
            callback();
        }
    }
}

/// Element for creating a ScrollNode.
#[derive(Clone)]
pub struct ScrollElement {
    state: ScrollState,
    is_vertical: bool,
    reverse_scrolling: bool,
}

impl ScrollElement {
    pub fn new(state: ScrollState, is_vertical: bool, reverse_scrolling: bool) -> Self {
        Self {
            state,
            is_vertical,
            reverse_scrolling,
        }
    }
}

impl std::fmt::Debug for ScrollElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollElement")
            .field("is_vertical", &self.is_vertical)
            .field("reverse_scrolling", &self.reverse_scrolling)
            .finish()
    }
}

impl PartialEq for ScrollElement {
    fn eq(&self, other: &Self) -> bool {
        // ScrollStates are equal if they point to the same underlying state
        self.state == other.state
            && self.is_vertical == other.is_vertical
            && self.reverse_scrolling == other.reverse_scrolling
    }
}

impl Eq for ScrollElement {}

impl Hash for ScrollElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.state.value.runtime_state_id().hash(state);
        self.is_vertical.hash(state);
        self.reverse_scrolling.hash(state);
    }
}

impl ModifierNodeElement for ScrollElement {
    type Node = ScrollNode;

    fn create(&self) -> Self::Node {
        // println!("ScrollElement::create");
        ScrollNode::new(self.state, self.is_vertical, self.reverse_scrolling)
    }

    fn key(&self) -> Option<u64> {
        let mut hasher = DefaultHasher::new();
        self.state.id().hash(&mut hasher);
        self.reverse_scrolling.hash(&mut hasher);
        self.is_vertical.hash(&mut hasher);
        Some(hasher.finish())
    }

    fn update(&self, node: &mut Self::Node) {
        let needs_invalidation = node.state != self.state
            || node.is_vertical != self.is_vertical
            || node.reverse_scrolling != self.reverse_scrolling;

        if needs_invalidation {
            node.state = self.state;
            node.is_vertical = self.is_vertical;
            node.reverse_scrolling = self.reverse_scrolling;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

/// ScrollNode layout modifier that physically moves content based on scroll position.
/// This is the component that actually reads ScrollState and applies the visual offset.
pub struct ScrollNode {
    state: ScrollState,
    is_vertical: bool,
    reverse_scrolling: bool,
    node_state: NodeState,
    /// ID of the invalidation callback registered with ScrollState
    invalidation_callback_id: Option<u64>,
    /// We capture the NodeId when attached to ensure correct invalidation scope
    node_id: Option<NodeId>,
}

impl ScrollNode {
    pub fn new(state: ScrollState, is_vertical: bool, reverse_scrolling: bool) -> Self {
        Self {
            state,
            is_vertical,
            reverse_scrolling,
            node_state: NodeState::default(),
            invalidation_callback_id: None,
            node_id: None,
        }
    }

    /// Returns a reference to the ScrollState.
    pub fn state(&self) -> &ScrollState {
        &self.state
    }
}

impl DelegatableNode for ScrollNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for ScrollNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        // Set up the invalidation callback to trigger layout when scroll state changes.
        // We capture the node_id directly from the context, avoiding any global registry.

        let node_id = context.node_id();
        self.node_id = node_id;

        if let Some(node_id) = node_id {
            let callback_id = self.state.add_invalidate_callback(Box::new(move || {
                // Schedule scoped layout repass for this node
                crate::schedule_layout_repass(node_id);
            }));
            self.invalidation_callback_id = Some(callback_id);
        } else {
            log::debug!(
                "ScrollNode attached without a NodeId; deferring invalidation registration."
            );
        }

        // Initial invalidation
        context.invalidate(cranpose_foundation::InvalidationKind::Layout);
    }

    fn on_detach(&mut self) {
        // Remove invalidation callback
        if let Some(id) = self.invalidation_callback_id.take() {
            self.state.remove_invalidate_callback(id);
        }
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for ScrollNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        // Step 1: Give child infinite space in scroll direction
        let scroll_constraints = if self.is_vertical {
            Constraints {
                min_height: 0.0,
                max_height: f32::INFINITY,
                ..constraints
            }
        } else {
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                ..constraints
            }
        };

        // Step 2: Measure child
        let placeable = measurable.measure(scroll_constraints);

        // Step 3: Calculate viewport size (constrained size)
        let width = placeable.width().min(constraints.max_width);
        let height = placeable.height().min(constraints.max_height);

        // Step 4: Calculate max scroll
        let max_scroll = if self.is_vertical {
            (placeable.height() - height).max(0.0)
        } else {
            (placeable.width() - width).max(0.0)
        };

        // Step 5: Update state with max scroll value
        // Only update if the viewport is constrained (not infinite probe)
        if (self.is_vertical && constraints.max_height.is_finite())
            || (!self.is_vertical && constraints.max_width.is_finite())
        {
            self.state.set_max_value(max_scroll);
        }

        // Step 6: Read scroll value and calculate offset
        // IMPORTANT: Use value_non_reactive() during measure to avoid triggering recomposition
        let scroll = self.state.value_non_reactive().clamp(0.0, max_scroll);

        let abs_scroll = if self.reverse_scrolling {
            scroll - max_scroll
        } else {
            -scroll
        };

        let (x_offset, y_offset) = if self.is_vertical {
            (0.0, abs_scroll)
        } else {
            (abs_scroll, 0.0)
        };

        // Step 7: Return result with viewport size and scroll offset as placement_offset
        // This makes the scroll offset part of the layout modifier's placement, which will be
        // correctly applied to children by the layout system
        LayoutModifierMeasureResult::new(Size { width, height }, x_offset, y_offset)
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.max_intrinsic_height(width)
    }
}

/// Creates a remembered ScrollState.
///
/// This is a convenience function for use in composable functions.
#[macro_export]
macro_rules! rememberScrollState {
    ($initial:expr) => {
        cranpose_core::remember(|| $crate::scroll::ScrollState::new($initial))
            .with(|state| state.clone())
    };
    () => {
        rememberScrollState!(0.0)
    };
}

#[cfg(test)]
#[path = "tests/scroll_tests.rs"]
mod tests;
