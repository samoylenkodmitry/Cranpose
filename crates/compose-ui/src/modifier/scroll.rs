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
use crate::scroll::{ScrollElement, ScrollState};
use compose_foundation::{PointerButton, PointerButtons, DRAG_THRESHOLD};
use std::cell::RefCell;
use std::rc::Rc;

/// Local gesture state for scroll drag handling.
///
/// This is NOT part of `ScrollState` to keep the scroll model pure.
/// Each scroll modifier instance has its own gesture state, which enables
/// multiple independent scroll regions without state interference.
#[derive(Default)]
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
// Scroll Gesture Detector
// ============================================================================

/// Encapsulates scroll gesture detection and handling logic.
///
/// This struct provides a clean interface for processing pointer events
/// and managing scroll interactions. It separates the gesture detection
/// logic from the async event loop plumbing.
struct ScrollGestureDetector {
    /// Shared gesture state (position tracking, drag status).
    gesture_state: Rc<RefCell<ScrollGestureState>>,

    /// The scroll model to update when drag is detected.
    scroll_state: ScrollState,

    /// Whether this is vertical or horizontal scroll.
    is_vertical: bool,
}

impl ScrollGestureDetector {
    /// Creates a new detector for the given scroll configuration.
    fn new(
        gesture_state: Rc<RefCell<ScrollGestureState>>,
        scroll_state: ScrollState,
        is_vertical: bool,
    ) -> Self {
        Self {
            gesture_state,
            scroll_state,
            is_vertical,
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
        gs.drag_down_position = Some(position);
        gs.last_position = Some(position);
        gs.is_dragging = false;

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
    /// # Why negative delta?
    /// Natural scrolling: dragging finger down moves content down,
    /// which means scroll offset increases (content shifts up under viewport).
    /// The scroll model uses positive offset = content scrolled up,
    /// so drag down = negative delta to scroll state.
    ///
    /// Returns `true` if event should be consumed (we're actively dragging).
    fn on_move(&self, position: Point, buttons: PointerButtons) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        // Safety: detect missed Up events (hit test delivered to wrong target)
        if !buttons.contains(PointerButton::Primary) && gs.drag_down_position.is_some() {
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            return false;
        }

        let (Some(down_pos), Some(last_pos)) = (gs.drag_down_position, gs.last_position) else {
            return false;
        };

        let total_delta = calculate_total_delta(down_pos, position, self.is_vertical);
        let incremental_delta = calculate_incremental_delta(last_pos, position, self.is_vertical);

        // Threshold check: start dragging only after moving 8px from down position.
        // This matches the clickable modifier's threshold, ensuring consistent
        // gesture disambiguation across scroll containers with clickable children.
        if !gs.is_dragging && total_delta.abs() > DRAG_THRESHOLD {
            gs.is_dragging = true;
        }

        gs.last_position = Some(position);

        if gs.is_dragging {
            // Apply scroll: negative because natural scrolling (drag down = scroll up)
            let _ = self.scroll_state.dispatch_raw_delta(-incremental_delta);
            true // Consume event while dragging
        } else {
            false
        }
    }

    /// Handles pointer up event.
    ///
    /// Cleans up drag state. If we were actively dragging, consume the
    /// Up event to prevent child click handlers from firing.
    ///
    /// Returns `true` if we were dragging (event should be consumed).
    fn on_up(&self) -> bool {
        let mut gs = self.gesture_state.borrow_mut();
        let was_dragging = gs.is_dragging;

        gs.drag_down_position = None;
        gs.last_position = None;
        gs.is_dragging = false;

        was_dragging
    }

    /// Handles pointer cancel event.
    ///
    /// Same as Up - cleans up state and consumes if we were dragging.
    ///
    /// Returns `true` if we were dragging (event should be consumed).
    fn on_cancel(&self) -> bool {
        self.on_up()
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
    let layout_modifier = Modifier::with_element(element).with_inspector_metadata(
        inspector_metadata(
            if is_vertical {
                "verticalScroll"
            } else {
                "horizontalScroll"
            },
            move |info| {
                info.add_property("isVertical", is_vertical.to_string());
                info.add_property("reverseScrolling", reverse_scrolling.to_string());
            },
        ),
    );

    // Combine: pointer input THEN layout modifier
    pointer_input.then(layout_modifier)
}
