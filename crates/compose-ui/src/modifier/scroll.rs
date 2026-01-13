//! Scroll modifier extensions for Modifier.
//!
//! # Overview
//! This module implements scrollable containers with gesture-based interaction.
//! It follows the pattern of separating:
//! - **State management** (`ScrollGestureState`) - tracks pointer/drag state
//! - **Event handling** (`ScrollGestureDetector`) - processes events and updates state
//! - **Layout** (`ScrollElement`/`ScrollNode` in `scroll.rs`) - applies scroll offset
//!
//! # Gesture Flow
//! 1. **Down**: Record initial position, reset drag state
//! 2. **Move**: Check if total movement exceeds `DRAG_THRESHOLD` (8px)
//!    - If threshold crossed: start consuming events, apply scroll delta
//!    - This prevents child click handlers from firing during scrolls
//! 3. **Up/Cancel**: Clean up state, consume if was dragging

use super::{inspector_metadata, Modifier, Point, PointerEventKind};
use crate::current_density;
use crate::fling_animation::FlingAnimation;
use crate::fling_animation::MIN_FLING_VELOCITY;
use crate::scroll::{ScrollElement, ScrollState};
use compose_core::current_runtime_handle;
use compose_foundation::{
    velocity_tracker::ASSUME_STOPPED_MS, PointerButton, PointerButtons, VelocityTracker1D,
    DRAG_THRESHOLD, MAX_FLING_VELOCITY,
};
use std::cell::RefCell;
use std::rc::Rc;
use web_time::Instant;

// ============================================================================
// Test Accessibility: Last Fling Velocity
// ============================================================================

use std::sync::atomic::{AtomicU32, Ordering};

/// Stores the last fling velocity calculated for test verification.
/// Uses atomic storage so the test driver thread can read values set by the UI thread.
static LAST_FLING_VELOCITY: AtomicU32 = AtomicU32::new(0);

/// Returns the last fling velocity calculated (in px/sec).
///
/// This is primarily for testing - allows robot tests to verify that
/// velocity detection is working correctly instead of relying on log output.
pub fn last_fling_velocity() -> f32 {
    f32::from_bits(LAST_FLING_VELOCITY.load(Ordering::SeqCst))
}

/// Resets the last fling velocity to 0.0.
///
/// Call this at the start of a test to ensure clean state.
pub fn reset_last_fling_velocity() {
    LAST_FLING_VELOCITY.store(0.0f32.to_bits(), Ordering::SeqCst);
}

/// Internal: Set the last fling velocity (called from gesture detection).
fn set_last_fling_velocity(velocity: f32) {
    LAST_FLING_VELOCITY.store(velocity.to_bits(), Ordering::SeqCst);
}

/// Local gesture state for scroll drag handling.
///
/// This is NOT part of `ScrollState` to keep the scroll model pure.
/// Each scroll modifier instance has its own gesture state, which enables
/// multiple independent scroll regions without state interference.
struct ScrollGestureState {
    /// Position where pointer was pressed down.
    /// Used to calculate total drag distance for threshold detection.
    drag_down_position: Option<Point>,

    /// Last known pointer position during drag.
    /// Used to calculate incremental delta for each move event.
    last_position: Option<Point>,

    /// Whether we've crossed the drag threshold and are actively scrolling.
    /// Once true, we consume all events until Up/Cancel to prevent child
    /// handlers from receiving drag events.
    is_dragging: bool,

    /// Velocity tracker for fling gesture detection.
    velocity_tracker: VelocityTracker1D,

    /// Time when gesture down started (for velocity calculation).
    gesture_start_time: Option<Instant>,

    /// Last time a velocity sample was recorded (milliseconds since gesture start).
    last_velocity_sample_ms: Option<i64>,

    /// Current fling animation (if any).
    fling_animation: Option<FlingAnimation>,
}

impl Default for ScrollGestureState {
    fn default() -> Self {
        Self {
            drag_down_position: None,
            last_position: None,
            is_dragging: false,
            velocity_tracker: VelocityTracker1D::new(),
            gesture_start_time: None,
            last_velocity_sample_ms: None,
            fling_animation: None,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculates the total movement distance from the original down position.
///
/// This is used to determine if we've crossed the drag threshold.
/// Returns the distance in the scroll axis direction (Y for vertical, X for horizontal).
#[inline]
fn calculate_total_delta(from: Point, to: Point, is_vertical: bool) -> f32 {
    if is_vertical {
        to.y - from.y
    } else {
        to.x - from.x
    }
}

/// Calculates the incremental movement delta from the previous position.
///
/// This is used to update the scroll offset incrementally during drag.
/// Returns the distance in the scroll axis direction (Y for vertical, X for horizontal).
#[inline]
fn calculate_incremental_delta(from: Point, to: Point, is_vertical: bool) -> f32 {
    if is_vertical {
        to.y - from.y
    } else {
        to.x - from.x
    }
}

// ============================================================================
// Scroll Gesture Detector (Generic Implementation)
// ============================================================================

/// Trait for scroll targets that can receive scroll deltas.
///
/// Implemented by both `ScrollState` (regular scroll) and `LazyListState` (lazy lists).
trait ScrollTarget: Clone {
    /// Apply a gesture delta. Returns the consumed amount in gesture coordinates.
    fn apply_delta(&self, delta: f32) -> f32;

    /// Apply a scroll delta during fling. Returns consumed delta in scroll coordinates.
    fn apply_fling_delta(&self, delta: f32) -> f32;

    /// Called after scroll to trigger any necessary invalidation.
    fn invalidate(&self);

    /// Get the current scroll offset.
    fn current_offset(&self) -> f32;
}

impl ScrollTarget for ScrollState {
    fn apply_delta(&self, delta: f32) -> f32 {
        // Regular scroll uses negative delta (natural scrolling)
        let consumed = self.dispatch_raw_delta(-delta);
        #[cfg(debug_assertions)]
        if delta.abs() > 0.1 {
            eprintln!("[ScrollState] apply_delta({:.1}) -> dispatch_raw_delta({:.1}) -> consumed={:.1}, new_value={:.1}", 
                delta, -delta, consumed, self.value());
        }
        consumed
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        let consumed = self.dispatch_raw_delta(delta);
        #[cfg(debug_assertions)]
        if delta.abs() > 0.1 {
            eprintln!(
                "[ScrollState] apply_fling_delta({:.1}) -> consumed={:.1}, new_value={:.1}",
                delta,
                consumed,
                self.value()
            );
        }
        consumed
    }

    fn invalidate(&self) {
        // ScrollState triggers invalidation internally
    }

    fn current_offset(&self) -> f32 {
        self.value()
    }
}

impl ScrollTarget for LazyListState {
    fn apply_delta(&self, delta: f32) -> f32 {
        // LazyListState uses positive delta directly
        // dispatch_scroll_delta already calls self.invalidate() which triggers the
        // layout invalidation callback registered in lazy_scroll_impl
        self.dispatch_scroll_delta(delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self.dispatch_scroll_delta(-delta)
    }

    fn invalidate(&self) {
        // dispatch_scroll_delta already handles invalidation internally via callback.
        // We do NOT call request_layout_invalidation() here - that's the global
        // nuclear option that invalidates ALL layout caches app-wide.
        // The registered callback uses schedule_layout_repass for scoped invalidation.
    }

    fn current_offset(&self) -> f32 {
        // LazyListState doesn't have a simple offset - use first visible item offset
        self.first_visible_item_scroll_offset()
    }
}

/// Generic scroll gesture detector that works with any ScrollTarget.
///
/// This struct provides a clean interface for processing pointer events
/// and managing scroll interactions. The generic parameter S determines
/// how scroll deltas are applied.
struct ScrollGestureDetector<S: ScrollTarget> {
    /// Shared gesture state (position tracking, drag status).
    gesture_state: Rc<RefCell<ScrollGestureState>>,

    /// The scroll target to update when drag is detected.
    scroll_target: S,

    /// Whether this is vertical or horizontal scroll.
    is_vertical: bool,

    /// Whether to reverse the scroll direction (flip delta).
    reverse_scrolling: bool,
}

impl<S: ScrollTarget + 'static> ScrollGestureDetector<S> {
    /// Creates a new detector for the given scroll configuration.
    fn new(
        gesture_state: Rc<RefCell<ScrollGestureState>>,
        scroll_target: S,
        is_vertical: bool,
        reverse_scrolling: bool,
    ) -> Self {
        Self {
            gesture_state,
            scroll_target,
            is_vertical,
            reverse_scrolling,
        }
    }

    /// Handles pointer down event.
    ///
    /// Records the initial position for threshold calculation and
    /// resets drag state. We don't consume Down events because we
    /// don't know yet if this will become a drag or a click.
    ///
    /// Returns `false` - Down events are never consumed to allow
    /// potential child click handlers to receive the initial press.
    fn on_down(&self, position: Point) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        #[cfg(debug_assertions)]
        let had_fling = gs.fling_animation.is_some();

        // Cancel any running fling animation
        if let Some(fling) = gs.fling_animation.take() {
            #[cfg(debug_assertions)]
            eprintln!("[Scroll] on_down: cancelling existing fling");
            fling.cancel();
        }

        #[cfg(debug_assertions)]
        eprintln!(
            "[Scroll] on_down: pos=({:.1}, {:.1}), had_fling={}",
            position.x, position.y, had_fling
        );

        gs.drag_down_position = Some(position);
        gs.last_position = Some(position);
        gs.is_dragging = false;
        gs.velocity_tracker.reset();
        gs.gesture_start_time = Some(Instant::now());

        // Add initial position to velocity tracker
        let pos = if self.is_vertical {
            position.y
        } else {
            position.x
        };
        gs.velocity_tracker.add_data_point(0, pos);
        gs.last_velocity_sample_ms = Some(0);

        // Never consume Down - we don't know if this is a drag yet
        false
    }

    /// Handles pointer move event.
    ///
    /// This is the core gesture detection logic:
    /// 1. Safety check: if no primary button is pressed but we think we're
    ///    tracking, we missed an Up event - reset state.
    /// 2. Calculate total movement from down position.
    /// 3. If total movement exceeds `DRAG_THRESHOLD` (8px), start dragging.
    /// 4. While dragging, apply scroll delta and consume events.
    ///
    /// Returns `true` if event should be consumed (we're actively dragging).
    fn on_move(&self, position: Point, buttons: PointerButtons) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        // Safety: detect missed Up events (hit test delivered to wrong target)
        if !buttons.contains(PointerButton::Primary) && gs.drag_down_position.is_some() {
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.gesture_start_time = None;
            gs.last_velocity_sample_ms = None;
            gs.velocity_tracker.reset();
            return false;
        }

        let Some(down_pos) = gs.drag_down_position else {
            return false;
        };

        let Some(last_pos) = gs.last_position else {
            gs.last_position = Some(position);
            return false;
        };

        let total_delta = calculate_total_delta(down_pos, position, self.is_vertical);
        let incremental_delta = calculate_incremental_delta(last_pos, position, self.is_vertical);

        // Threshold check: start dragging only after moving 8px from down position.
        if !gs.is_dragging && total_delta.abs() > DRAG_THRESHOLD {
            gs.is_dragging = true;
        }

        gs.last_position = Some(position);

        // Track velocity for fling
        if let Some(start_time) = gs.gesture_start_time {
            let elapsed_ms = start_time.elapsed().as_millis() as i64;
            let pos = if self.is_vertical {
                position.y
            } else {
                position.x
            };
            // Keep sample times strictly increasing so velocity stays stable when
            // multiple move events land in the same millisecond.
            let sample_ms = match gs.last_velocity_sample_ms {
                Some(last_sample_ms) => {
                    let mut sample_ms = if elapsed_ms <= last_sample_ms {
                        last_sample_ms + 1
                    } else {
                        elapsed_ms
                    };
                    // Clamp large processing gaps so frame stalls don't erase fling velocity.
                    if sample_ms - last_sample_ms > ASSUME_STOPPED_MS {
                        sample_ms = last_sample_ms + ASSUME_STOPPED_MS;
                    }
                    sample_ms
                }
                None => elapsed_ms,
            };
            gs.velocity_tracker.add_data_point(sample_ms, pos);
            gs.last_velocity_sample_ms = Some(sample_ms);
        }

        if gs.is_dragging {
            drop(gs); // Release borrow before calling scroll target
            let delta = if self.reverse_scrolling {
                -incremental_delta
            } else {
                incremental_delta
            };
            let _ = self.scroll_target.apply_delta(delta);
            self.scroll_target.invalidate();
            true // Consume event while dragging
        } else {
            false
        }
    }

    /// Handles pointer up event.
    ///
    /// Cleans up drag state. If we were actively dragging, calculates fling
    /// velocity and starts fling animation if velocity is above threshold.
    ///
    /// Returns `true` if we were dragging (event should be consumed).
    fn finish_gesture(&self, allow_fling: bool) -> bool {
        let (was_dragging, velocity, start_fling, existing_fling) = {
            let mut gs = self.gesture_state.borrow_mut();
            let was_dragging = gs.is_dragging;
            let mut velocity = 0.0;

            if allow_fling && was_dragging && gs.gesture_start_time.is_some() {
                velocity = gs
                    .velocity_tracker
                    .calculate_velocity_with_max(MAX_FLING_VELOCITY);
            }

            #[cfg(debug_assertions)]
            if allow_fling {
                eprintln!(
                    "[Fling] on_up: was_dragging={}, velocity={:.0}",
                    was_dragging, velocity
                );
            } else {
                eprintln!("[Scroll] on_cancel: was_dragging={}", was_dragging);
            }

            let start_fling = allow_fling && was_dragging && velocity.abs() > MIN_FLING_VELOCITY;
            let existing_fling = if start_fling {
                gs.fling_animation.take()
            } else {
                None
            };

            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.gesture_start_time = None;
            gs.last_velocity_sample_ms = None;

            (was_dragging, velocity, start_fling, existing_fling)
        };

        // Always record velocity for test accessibility (even if below fling threshold)
        if allow_fling && was_dragging {
            set_last_fling_velocity(velocity);
        }

        // Start fling animation if velocity is significant
        if start_fling {
            #[cfg(debug_assertions)]
            eprintln!(
                "[Fling] Triggering fling with velocity: {:.1} px/sec",
                velocity
            );

            if let Some(old_fling) = existing_fling {
                old_fling.cancel();
            }

            // Get runtime handle for frame callbacks
            if let Some(runtime) = current_runtime_handle() {
                let scroll_target = self.scroll_target.clone();
                let reverse = self.reverse_scrolling;
                let fling = FlingAnimation::new(runtime);

                // Get current scroll position for fling start
                let initial_value = scroll_target.current_offset();

                #[cfg(debug_assertions)]
                eprintln!(
                    "[Fling] current_offset()={:.1} at fling start",
                    initial_value
                );

                // Convert gesture velocity to scroll velocity.
                let adjusted_velocity = if reverse { -velocity } else { velocity };
                let fling_velocity = -adjusted_velocity;

                let scroll_target_for_fling = scroll_target.clone();
                let scroll_target_for_end = scroll_target.clone();

                fling.start_fling(
                    initial_value,
                    fling_velocity,
                    current_density(),
                    move |delta| {
                        // Apply scroll delta during fling, return consumed amount
                        let consumed = scroll_target_for_fling.apply_fling_delta(delta);
                        scroll_target_for_fling.invalidate();
                        consumed
                    },
                    move || {
                        // Animation complete - invalidate to ensure final render
                        scroll_target_for_end.invalidate();
                    },
                );

                let mut gs = self.gesture_state.borrow_mut();
                gs.fling_animation = Some(fling);
            }
        }

        was_dragging
    }

    /// Handles pointer up event.
    ///
    /// Cleans up drag state. If we were actively dragging, calculates fling
    /// velocity and starts fling animation if velocity is above threshold.
    ///
    /// Returns `true` if we were dragging (event should be consumed).
    fn on_up(&self) -> bool {
        self.finish_gesture(true)
    }

    /// Handles pointer cancel event.
    ///
    /// Cleans up state without starting a fling. Returns `true` if we were dragging.
    fn on_cancel(&self) -> bool {
        self.finish_gesture(false)
    }
}

// ============================================================================
// Modifier Extensions
// ============================================================================

impl Modifier {
    /// Creates a horizontally scrollable modifier.
    ///
    /// # Arguments
    /// * `state` - The ScrollState to control scroll position
    /// * `reverse_scrolling` - If true, reverses the scroll direction in layout.
    ///   Note: This affects how scroll offset is applied to content (via `ScrollNode`),
    ///   NOT the drag direction. Drag gestures always follow natural touch semantics:
    ///   drag right = scroll left (content moves right under finger).
    ///
    /// # Example
    /// ```text
    /// let scroll_state = ScrollState::new(0.0);
    /// Row(
    ///     Modifier::empty().horizontal_scroll(scroll_state, false),
    ///     // ... content
    /// );
    /// ```
    pub fn horizontal_scroll(self, state: ScrollState, reverse_scrolling: bool) -> Self {
        self.then(scroll_impl(state, false, reverse_scrolling))
    }

    /// Creates a vertically scrollable modifier.
    ///
    /// # Arguments
    /// * `state` - The ScrollState to control scroll position
    /// * `reverse_scrolling` - If true, reverses the scroll direction in layout.
    ///   Note: This affects how scroll offset is applied to content (via `ScrollNode`),
    ///   NOT the drag direction. Drag gestures always follow natural touch semantics:
    ///   drag down = scroll up (content moves down under finger).
    pub fn vertical_scroll(self, state: ScrollState, reverse_scrolling: bool) -> Self {
        self.then(scroll_impl(state, true, reverse_scrolling))
    }
}

/// Internal implementation for scroll modifiers.
///
/// Creates a combined modifier consisting of:
/// 1. Pointer input handler (for gesture detection)
/// 2. Layout modifier (for applying scroll offset)
///
/// The pointer input is added FIRST so it appears earlier in the modifier
/// chain, allowing it to intercept events before layout-related handlers.
fn scroll_impl(state: ScrollState, is_vertical: bool, reverse_scrolling: bool) -> Modifier {
    // Create local gesture state - each scroll modifier instance is independent
    let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));

    // Set up pointer input handler
    let scroll_state = state.clone();
    let key = (state.id(), is_vertical);
    let pointer_input = Modifier::empty().pointer_input(key, move |scope| {
        // Create detector inside the async closure to capture the cloned state
        let detector = ScrollGestureDetector::new(
            gesture_state.clone(),
            scroll_state.clone(),
            is_vertical,
            false, // ScrollState handles reversing in layout, not input
        );

        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    // Main event loop - processes events until scope is cancelled
                    loop {
                        let event = await_scope.await_pointer_event().await;

                        // Delegate to detector's lifecycle methods
                        let should_consume = match event.kind {
                            PointerEventKind::Down => detector.on_down(event.position),
                            PointerEventKind::Move => {
                                detector.on_move(event.position, event.buttons)
                            }
                            PointerEventKind::Up => detector.on_up(),
                            PointerEventKind::Cancel => detector.on_cancel(),
                        };

                        if should_consume {
                            event.consume();
                        }
                    }
                })
                .await;
        }
    });

    // Create layout modifier for applying scroll offset to content
    let element = ScrollElement::new(state.clone(), is_vertical, reverse_scrolling);
    let layout_modifier =
        Modifier::with_element(element).with_inspector_metadata(inspector_metadata(
            if is_vertical {
                "verticalScroll"
            } else {
                "horizontalScroll"
            },
            move |info| {
                info.add_property("isVertical", is_vertical.to_string());
                info.add_property("reverseScrolling", reverse_scrolling.to_string());
            },
        ));

    // Combine: pointer input THEN layout modifier
    pointer_input.then(layout_modifier)
}

// ============================================================================
// Lazy Scroll Support for LazyListState
// ============================================================================

use compose_foundation::lazy::LazyListState;

impl Modifier {
    /// Creates a vertically scrollable modifier for lazy lists.
    ///
    /// This connects pointer gestures to LazyListState for scroll handling.
    /// Unlike regular vertical_scroll, no layout offset is applied here
    /// since LazyListState manages item positioning internally.
    /// Creates a vertically scrollable modifier for lazy lists.
    ///
    /// This connects pointer gestures to LazyListState for scroll handling.
    /// Unlike regular vertical_scroll, no layout offset is applied here
    /// since LazyListState manages item positioning internally.
    pub fn lazy_vertical_scroll(self, state: LazyListState, reverse_scrolling: bool) -> Self {
        self.then(lazy_scroll_impl(state, true, reverse_scrolling))
    }

    /// Creates a horizontally scrollable modifier for lazy lists.
    pub fn lazy_horizontal_scroll(self, state: LazyListState, reverse_scrolling: bool) -> Self {
        self.then(lazy_scroll_impl(state, false, reverse_scrolling))
    }
}

/// Internal implementation for lazy scroll modifiers.
fn lazy_scroll_impl(state: LazyListState, is_vertical: bool, reverse_scrolling: bool) -> Modifier {
    let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));
    let list_state = state;

    // Note: Layout invalidation callback is registered in LazyColumnImpl/LazyRowImpl
    // after the node is created, using schedule_layout_repass(node_id) for O(subtree)
    // performance instead of request_layout_invalidation() which is O(entire app).

    // Use a unique key per LazyListState
    let state_id = std::ptr::addr_of!(*state.inner_ptr()) as usize;
    let key = (state_id, is_vertical, reverse_scrolling);

    Modifier::empty().pointer_input(key, move |scope| {
        // Use the same generic detector with LazyListState
        let detector = ScrollGestureDetector::new(
            gesture_state.clone(),
            list_state,
            is_vertical,
            reverse_scrolling,
        );

        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;

                        // Delegate to detector's lifecycle methods
                        let should_consume = match event.kind {
                            PointerEventKind::Down => detector.on_down(event.position),
                            PointerEventKind::Move => {
                                detector.on_move(event.position, event.buttons)
                            }
                            PointerEventKind::Up => detector.on_up(),
                            PointerEventKind::Cancel => detector.on_cancel(),
                        };

                        if should_consume {
                            event.consume();
                        }
                    }
                })
                .await;
        }
    })
}
