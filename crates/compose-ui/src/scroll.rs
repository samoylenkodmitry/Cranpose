//! Scroll state and node implementation for rs-compose.
//!
//! This module provides the core scrolling components:
//! - `ScrollState`: Holds scroll position and provides scroll control methods
//! - `ScrollNode`: Layout modifier that applies scroll offset to content
//! - `ScrollElement`: Element for creating ScrollNode instances
//!
//! The actual `Modifier.horizontal_scroll()` and `Modifier.vertical_scroll()`
//! extension methods are defined in `modifier/scroll.rs`.

use compose_core::{mutableStateOf, MutableState, NodeId};
use compose_foundation::{
    Constraints, DelegatableNode, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState,
};
use compose_ui_graphics::Size;
use compose_ui_layout::LayoutModifierMeasureResult;
use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SCROLL_STATE_ID: AtomicU64 = AtomicU64::new(1);

/// State object for scroll position tracking.
///
/// Holds the current scroll offset and provides methods to programmatically
/// control scrolling. Can be created with `rememberScrollState()`.
///
/// This is a pure scroll model - it does NOT store ephemeral gesture/pointer state.
/// Gesture state is managed locally in the scroll modifier.
#[derive(Clone)]
pub struct ScrollState {
    inner: Rc<ScrollStateInner>,
}

pub(crate) struct ScrollStateInner {
    /// Unique ID for debugging
    id: u64,
    /// Current scroll offset in pixels.
    /// Uses MutableState<f32> for reactivity - Composables can observe this value.
    /// Layout reads use get_non_reactive() to avoid triggering recomposition.
    value: MutableState<f32>,
    /// Maximum scroll value (content_size - viewport_size)
    /// Using RefCell instead of MutableState to avoid snapshot isolation issues
    max_value: RefCell<f32>,
    /// Callbacks to invalidate layout when scroll value changes
    /// Using HashMap to allow multiple listeners (e.g. real node + clones)
    invalidate_callbacks: RefCell<std::collections::HashMap<u64, Box<dyn Fn()>>>,
    /// Tracks whether we need to invalidate once a callback is registered.
    pending_invalidation: Cell<bool>,
}

impl ScrollState {
    /// Creates a new ScrollState with the given initial scroll position.
    pub fn new(initial: f32) -> Self {
        let id = NEXT_SCROLL_STATE_ID.fetch_add(1, Ordering::Relaxed);

        Self {
            inner: Rc::new(ScrollStateInner {
                id,
                value: mutableStateOf(initial),
                max_value: RefCell::new(0.0),
                invalidate_callbacks: RefCell::new(std::collections::HashMap::new()),
                pending_invalidation: Cell::new(false),
            }),
        }
    }

    /// Get the unique ID of this ScrollState
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Gets the current scroll position in pixels (reactive - triggers recomposition).
    ///
    /// Use this in Composable functions when you want UI to update on scroll.
    /// Example: `Text("Scroll position: ${scrollState.value()}")`
    pub fn value(&self) -> f32 {
        self.inner.value.with(|v| *v)
    }

    /// Gets the current scroll position in pixels (non-reactive).
    ///
    /// Use this in layout/measure phase to avoid triggering recomposition.
    /// This is called internally by ScrollNode::measure().
    pub fn value_non_reactive(&self) -> f32 {
        self.inner.value.get_non_reactive()
    }

    /// Gets the maximum scroll value.
    pub fn max_value(&self) -> f32 {
        *self.inner.max_value.borrow()
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
            self.inner.value.set(new_value);

            // Trigger layout invalidation callbacks
            let callbacks = self.inner.invalidate_callbacks.borrow();
            if callbacks.is_empty() {
                // Defer invalidation until a node registers a callback.
                self.inner.pending_invalidation.set(true);

            } else {
                for callback in callbacks.values() {
                    callback();
                }
            }
        }

        actual_delta
    }

    /// Sets the maximum scroll value (internal use by ScrollNode).
    pub(crate) fn set_max_value(&self, max: f32) {
        *self.inner.max_value.borrow_mut() = max;
    }

    /// Scrolls to the given position immediately.
    pub fn scroll_to(&self, position: f32) {
        let max = self.max_value();
        let clamped = position.clamp(0.0, max);

        self.inner.value.set(clamped);

        // Trigger layout invalidation callbacks
        let callbacks = self.inner.invalidate_callbacks.borrow();
        if callbacks.is_empty() {
            self.inner.pending_invalidation.set(true);

        } else {
            for callback in callbacks.values() {
                callback();
            }
        }
    }

    /// Adds an invalidation callback and returns its ID
    pub(crate) fn add_invalidate_callback(&self, callback: Box<dyn Fn()>) -> u64 {
        static NEXT_CALLBACK_ID: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
        self.inner
            .invalidate_callbacks
            .borrow_mut()
            .insert(id, callback);
        if self.inner.pending_invalidation.replace(false) {
            if let Some(callback) = self.inner.invalidate_callbacks.borrow().get(&id) {
                callback();
            }
        }
        id
    }

    /// Removes an invalidation callback by ID
    pub(crate) fn remove_invalidate_callback(&self, id: u64) {
        self.inner.invalidate_callbacks.borrow_mut().remove(&id);
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
        Rc::ptr_eq(&self.state.inner, &other.state.inner)
            && self.is_vertical == other.is_vertical
            && self.reverse_scrolling == other.reverse_scrolling
    }
}

impl Eq for ScrollElement {}

impl Hash for ScrollElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.state.inner) as usize).hash(state);
        self.is_vertical.hash(state);
        self.reverse_scrolling.hash(state);
    }
}

impl ModifierNodeElement for ScrollElement {
    type Node = ScrollNode;

    fn create(&self) -> Self::Node {
        // println!("ScrollElement::create");
        ScrollNode::new(self.state.clone(), self.is_vertical, self.reverse_scrolling)
    }

    fn key(&self) -> Option<u64> {
        let mut hasher = DefaultHasher::new();
        self.state.id().hash(&mut hasher);
        self.reverse_scrolling.hash(&mut hasher);
        self.is_vertical.hash(&mut hasher);
        Some(hasher.finish())
    }

    fn update(&self, node: &mut Self::Node) {
        let needs_invalidation = !Rc::ptr_eq(&node.state.inner, &self.state.inner)
            || node.is_vertical != self.is_vertical
            || node.reverse_scrolling != self.reverse_scrolling;

        if needs_invalidation {
            node.state = self.state.clone();
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
             log::error!("ScrollNode attached without a NodeId! Layout invalidation will fail.");
        }

        // Initial invalidation
        context.invalidate(compose_foundation::InvalidationKind::Layout);
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

    fn create_measurement_proxy(&self) -> Option<Box<dyn compose_foundation::MeasurementProxy>> {
        None
    }
}

/// Creates a remembered ScrollState.
///
/// This is a convenience function for use in composable functions.
#[macro_export]
macro_rules! rememberScrollState {
    ($initial:expr) => {
        compose_core::remember(|| $crate::scroll::ScrollState::new($initial))
            .with(|state| state.clone())
    };
    () => {
        rememberScrollState!(0.0)
    };
}
