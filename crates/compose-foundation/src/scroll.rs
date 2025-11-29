//! High-level ScrollState implementation.
//!
//! This module provides ScrollState which wraps ScrollableState and manages
//! scroll position using integer pixels with fractional accumulation,
//! matching Jetpack Compose's Scroll.kt implementation.

use crate::scrollable::ScrollableState;
use compose_core::MutableState;
use std::cell::RefCell;
use std::rc::Rc;

/// Internal state for ScrollState.
#[derive(Debug)]
pub struct ScrollStateData {
    /// Maximum scroll value (contentSize - viewportSize)
    max_value: i32,
    /// Size of the viewport (visible area)
    viewport_size: i32,
    /// Fractional pixel accumulator to avoid rounding errors
    accumulator: f32,
    /// Whether currently scrolling
    is_scrolling: bool,
}

/// State of scroll position for use with horizontal_scroll/vertical_scroll modifiers.
///
/// This matches Jetpack Compose's ScrollState class (Scroll.kt:89-216).
/// Create using `rememberScrollState()` in a composable context.
#[derive(Clone, Debug)]
pub struct ScrollState {
    /// Reactive scroll value - triggers recomposition when changed
    value: MutableState<i32>,
    /// Other scroll state data
    pub data: Rc<RefCell<ScrollStateData>>,
}

impl ScrollState {
    /// Create a new ScrollState with an initial scroll position.
    ///
    /// **Note**: This must be called within a composable context (uses current composer).
    /// Prefer using `rememberScrollState()` for the standard pattern.
    ///
    /// # Arguments
    /// * `initial` - Initial scroll position in pixels
    pub fn new(initial: i32) -> Self {
        let runtime = compose_core::with_current_composer(|c| c.runtime_handle());
        let result = Self {
            value: MutableState::with_runtime(initial, runtime),
            data: Rc::new(RefCell::new(ScrollStateData {
                max_value: i32::MAX,
                viewport_size: 0,
                accumulator: 0.0,
                is_scrolling: false,
            })),
        };
        result
    }

    /// Get the current scroll position in pixels (reactive read).
    pub fn value(&self) -> i32 {
        self.value.get()
    }

    /// Get the maximum scroll value.
    pub fn max_value(&self) -> i32 {
        self.data.borrow().max_value
    }

    /// Get the viewport size.
    pub fn viewport_size(&self) -> i32 {
        self.data.borrow().viewport_size
    }

    /// Set the maximum scroll value (called by ScrollNode during measurement).
    pub fn set_max_value(&self, max: i32) {
        let mut data = self.data.borrow_mut();
        data.max_value = max;
        // Coerce current value if it exceeds new max
        let current = self.value.get();
        if current > max {
            self.value.set(max);
        }
    }

    /// Set the viewport size (called by ScrollNode during measurement).
    pub fn set_viewport_size(&self, size: i32) {
        self.data.borrow_mut().viewport_size = size;
    }

    /// Programmatically scroll to a specific position.
    ///
    /// # Arguments
    /// * `target` - Target scroll position in pixels
    pub fn scroll_to(&self, target: i32) {
        let data = self.data.borrow();
        let clamped = target.clamp(0, data.max_value);
        drop(data);
        self.value.set(clamped);
    }
}

impl ScrollableState for ScrollState {
    /// Consume scroll delta following Jetpack Compose's algorithm (Scroll.kt:139-150).
    ///
    /// This implementation:
    /// 1. Adds delta to current value + accumulator
    /// 2. Clamps to [0, max_value]
    /// 3. Updates integer value and fractional accumulator separately
    /// 4. Returns consumed delta
    fn consume_scroll_delta(&self, delta: f32) -> f32 {
        let data = self.data.borrow();
        
        let current = self.value.get() as f32;
        let absolute = current + delta + data.accumulator;
        
        // Clamp to bounds [0, max_value]
        let new_value = absolute.clamp(0.0, data.max_value as f32);
        
        // Calculate how much we actually consumed
        let consumed = new_value - current;
        let consumed_int = consumed.round() as i32;
        
        let accumulator_update = consumed - consumed_int as f32;
        drop(data);
        
        // Update value (triggers recomposition!) and accumulator
        self.value.set(self.value.get() + consumed_int);
        
        self.data.borrow_mut().accumulator = accumulator_update;
        
        consumed
    }

    fn is_scroll_in_progress(&self) -> bool {
        self.data.borrow().is_scrolling
    }
}
