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
use crate::render_state::schedule_modifier_slices_repass;
use crate::scroll::{
    scroll_motion_context_for_key, ScrollElement, ScrollMotionContext, ScrollMotionContextKey,
    ScrollState,
};
use cranpose_core::{current_runtime_handle, NodeId};
use cranpose_foundation::{
    velocity_tracker::ASSUME_STOPPED_MS, DelegatableNode, ModifierNode, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerButton, PointerButtons, VelocityTracker1D, DRAG_THRESHOLD,
    MAX_FLING_VELOCITY,
};
use std::cell::RefCell;
use std::rc::Rc;
use web_time::Instant;

#[cfg(feature = "test-helpers")]
pub fn last_fling_velocity() -> f32 {
    crate::render_state::debug_last_fling_velocity()
}

#[cfg(feature = "test-helpers")]
pub fn reset_last_fling_velocity() {
    crate::render_state::debug_reset_last_fling_velocity();
}

#[inline]
fn set_last_fling_velocity(velocity: f32) {
    crate::render_state::record_last_fling_velocity(velocity);
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

    /// Platform timestamp (ms) of the Down event, when the platform provides
    /// input timestamps. Preferred over `gesture_start_time` because batched
    /// input delivery (Android) makes delivery-time deltas meaningless.
    gesture_start_event_time_ms: Option<i64>,

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
            gesture_start_event_time_ms: None,
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

    /// Apply a wheel/trackpad event delta. Returns the consumed amount.
    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        self.apply_delta(delta)
    }

    /// Apply a scroll delta during fling. Returns consumed delta in scroll coordinates.
    fn apply_fling_delta(&self, delta: f32) -> f32;

    /// Called after scroll to trigger any necessary invalidation.
    fn invalidate(&self);

    /// Get the current scroll offset.
    fn current_offset(&self) -> f32;

    /// Whether the target can currently scroll in either direction.
    ///
    /// When this is `false` the gesture detector must not capture drags:
    /// a non-scrollable target (e.g. a lazy list realized in full inside an
    /// unbounded parent, or a scroll container whose content fits its
    /// viewport) would otherwise consume the move events that an enclosing
    /// scrollable needs to receive.
    fn can_scroll(&self) -> bool {
        true
    }
}

impl ScrollTarget for ScrollState {
    fn apply_delta(&self, delta: f32) -> f32 {
        // Regular scroll uses negative delta (natural scrolling)
        self.dispatch_raw_delta(-delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn invalidate(&self) {
        // ScrollState triggers invalidation internally
    }

    fn current_offset(&self) -> f32 {
        self.value()
    }

    fn can_scroll(&self) -> bool {
        self.max_value() > 0.0
    }
}

impl ScrollTarget for LazyListState {
    fn apply_delta(&self, delta: f32) -> f32 {
        // LazyListState uses positive delta directly
        // dispatch_scroll_delta already calls self.invalidate() which triggers the
        // layout invalidation callback registered in lazy_scroll_impl
        self.dispatch_scroll_delta(delta)
    }

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        if delta.abs() <= 0.001 {
            0.0
        } else {
            self.dispatch_scroll_delta(delta)
        }
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self.dispatch_scroll_delta(-delta)
    }

    fn invalidate(&self) {
        // dispatch_scroll_delta already handles invalidation internally via callback.
        // The registered callback uses schedule_layout_repass for scoped layout work.
    }

    fn current_offset(&self) -> f32 {
        // LazyListState doesn't have a simple offset - use first visible item offset
        self.first_visible_item_scroll_offset()
    }

    fn can_scroll(&self) -> bool {
        // Before the first measure pass no bounds are known; stay permissive
        // so gestures that race the first layout are not dropped.
        self.layout_info().total_items_count == 0
            || self.can_scroll_forward_non_reactive()
            || self.can_scroll_backward_non_reactive()
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

    /// Active motion state for renderer policy selection.
    motion_context: ScrollMotionContext,
}

impl<S: ScrollTarget + 'static> ScrollGestureDetector<S> {
    /// Creates a new detector for the given scroll configuration.
    fn new(
        gesture_state: Rc<RefCell<ScrollGestureState>>,
        scroll_target: S,
        is_vertical: bool,
        reverse_scrolling: bool,
        motion_context: ScrollMotionContext,
    ) -> Self {
        Self {
            gesture_state,
            scroll_target,
            is_vertical,
            reverse_scrolling,
            motion_context,
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
    fn on_down(&self, position: Point, time_ms: Option<i64>) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        // Cancel any running fling animation
        if let Some(fling) = gs.fling_animation.take() {
            fling.cancel();
        }
        self.motion_context.set_active(false);

        gs.drag_down_position = Some(position);
        gs.last_position = Some(position);
        gs.is_dragging = false;
        gs.velocity_tracker.reset();
        gs.gesture_start_time = Some(Instant::now());
        gs.gesture_start_event_time_ms = time_ms;

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
    fn on_move(&self, position: Point, buttons: PointerButtons, time_ms: Option<i64>) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        // Safety: detect missed Up events (hit test delivered to wrong target)
        if !buttons.contains(PointerButton::Primary) && gs.drag_down_position.is_some() {
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.gesture_start_time = None;
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;
            gs.velocity_tracker.reset();
            self.motion_context.set_active(false);
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

        // Threshold check: start dragging only after moving 8px from down
        // position. Targets that cannot scroll in either direction never
        // capture the gesture, so enclosing scrollables receive it instead.
        if !gs.is_dragging && total_delta.abs() > DRAG_THRESHOLD && self.scroll_target.can_scroll()
        {
            gs.is_dragging = true;
            self.motion_context.set_active(true);
        }

        gs.last_position = Some(position);

        // Track velocity for fling
        let pos = if self.is_vertical {
            position.y
        } else {
            position.x
        };
        let event_sample_ms = gs
            .gesture_start_event_time_ms
            .zip(time_ms)
            .map(|(start_ms, now_ms)| now_ms - start_ms);
        let sample_ms = if let Some(event_sample_ms) = event_sample_ms {
            // The platform supplied real input timestamps: trust them.
            // Android delivers touch samples batched/frame-aligned, so several
            // moves are processed back-to-back here; only the event's own
            // timestamp yields the real dt between finger positions. Real
            // pauses must also stay real so a stop-then-release does not fling
            // (the tracker treats gaps > ASSUME_STOPPED_MS as stopped).
            Some(match gs.last_velocity_sample_ms {
                Some(last_sample_ms) => event_sample_ms.max(last_sample_ms),
                None => event_sample_ms.max(0),
            })
        } else if let Some(start_time) = gs.gesture_start_time {
            // Fallback: delivery-time stamping for platforms without input
            // timestamps (desktop mouse, web).
            let elapsed_ms = start_time.elapsed().as_millis() as i64;
            // Keep sample times strictly increasing so velocity stays stable when
            // multiple move events land in the same millisecond.
            Some(match gs.last_velocity_sample_ms {
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
            })
        } else {
            None
        };
        if let Some(sample_ms) = sample_ms {
            log::trace!(
                target: "cranpose::velocity",
                "sample t={sample_ms}ms pos={pos:.2} event_time={time_ms:?}"
            );
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
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;

            (was_dragging, velocity, start_fling, existing_fling)
        };

        // Always record velocity for test accessibility (even if below fling threshold)
        if allow_fling && was_dragging {
            log::debug!(
                target: "cranpose::velocity",
                "gesture finished: fling velocity={velocity:.2} dp/s start_fling={start_fling}"
            );
            set_last_fling_velocity(velocity);
        }

        // Start fling animation if velocity is significant
        if start_fling {
            if let Some(old_fling) = existing_fling {
                old_fling.cancel();
            }

            // Get runtime handle for frame callbacks
            if let Some(runtime) = current_runtime_handle() {
                self.motion_context.set_active(true);
                let scroll_target = self.scroll_target.clone();
                let reverse = self.reverse_scrolling;
                let fling = FlingAnimation::new(runtime);
                let motion_context = self.motion_context.clone();

                // Get current scroll position for fling start
                let initial_value = scroll_target.current_offset();

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
                        motion_context.set_active(false);
                    },
                );

                let mut gs = self.gesture_state.borrow_mut();
                gs.fling_animation = Some(fling);
            }
        } else {
            self.motion_context.set_active(false);
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

    /// Handles mouse wheel / trackpad scroll event.
    ///
    /// Returns `true` when the target consumed any delta.
    fn on_scroll(&self, axis_delta: f32) -> bool {
        if axis_delta.abs() <= f32::EPSILON {
            return false;
        }

        {
            // Wheel scroll should take over immediately and stop any active drag/fling state.
            let mut gs = self.gesture_state.borrow_mut();
            if let Some(fling) = gs.fling_animation.take() {
                fling.cancel();
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.gesture_start_time = None;
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;
            gs.velocity_tracker.reset();
        }

        self.motion_context.activate_for_current_frame();

        let delta = if self.reverse_scrolling {
            -axis_delta
        } else {
            axis_delta
        };
        let consumed = self.scroll_target.apply_wheel_delta(delta);
        if consumed.abs() > 0.001 {
            self.scroll_target.invalidate();
            true
        } else {
            false
        }
    }
}

pub(crate) struct MotionContextAnimatedNode {
    state: NodeState,
    motion_context: ScrollMotionContext,
    invalidation_callback_id: Option<u64>,
    node_id: Option<NodeId>,
}

impl MotionContextAnimatedNode {
    fn new(motion_context: ScrollMotionContext) -> Self {
        Self {
            state: NodeState::new(),
            motion_context,
            invalidation_callback_id: None,
            node_id: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.motion_context.is_active()
    }
}

pub(crate) struct TranslatedContentContextNode {
    state: NodeState,
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
}

impl TranslatedContentContextNode {
    fn new(identity: usize, offset_source: TranslatedContentOffsetSource) -> Self {
        Self {
            state: NodeState::new(),
            identity,
            offset_source,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        true
    }

    pub(crate) fn identity(&self) -> usize {
        self.identity
    }

    pub(crate) fn content_offset_reader(&self) -> Option<Rc<dyn Fn() -> Point>> {
        self.offset_source.content_offset_reader()
    }
}

impl DelegatableNode for TranslatedContentContextNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TranslatedContentContextNode {}

impl DelegatableNode for MotionContextAnimatedNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for MotionContextAnimatedNode {
    fn on_attach(&mut self, context: &mut dyn cranpose_foundation::ModifierNodeContext) {
        let node_id = context.node_id();
        self.node_id = node_id;
        if let Some(node_id) = node_id {
            let callback_id = self
                .motion_context
                .add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id);
                }));
            self.invalidation_callback_id = Some(callback_id);
        }
    }

    fn on_detach(&mut self) {
        if let Some(id) = self.invalidation_callback_id.take() {
            self.motion_context.remove_invalidate_callback(id);
        }
        self.node_id = None;
    }
}

#[derive(Clone)]
struct MotionContextAnimatedElement {
    motion_context: ScrollMotionContext,
}

impl MotionContextAnimatedElement {
    fn new(motion_context: ScrollMotionContext) -> Self {
        Self { motion_context }
    }
}

impl std::fmt::Debug for MotionContextAnimatedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MotionContextAnimatedElement").finish()
    }
}

impl PartialEq for MotionContextAnimatedElement {
    fn eq(&self, other: &Self) -> bool {
        self.motion_context.ptr_eq(&other.motion_context)
    }
}

impl Eq for MotionContextAnimatedElement {}

impl std::hash::Hash for MotionContextAnimatedElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.motion_context.stable_key().hash(state);
    }
}

impl ModifierNodeElement for MotionContextAnimatedElement {
    type Node = MotionContextAnimatedNode;

    fn create(&self) -> Self::Node {
        MotionContextAnimatedNode::new(self.motion_context.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        if node.motion_context.ptr_eq(&self.motion_context) {
            return;
        }
        if let Some(id) = node.invalidation_callback_id.take() {
            node.motion_context.remove_invalidate_callback(id);
        }
        node.motion_context = self.motion_context.clone();
        if let Some(node_id) = node.node_id {
            let callback_id = node
                .motion_context
                .add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id);
                }));
            node.invalidation_callback_id = Some(callback_id);
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[derive(Clone)]
enum TranslatedContentOffsetSource {
    LayoutContentOffset,
    LazyList {
        state: LazyListState,
        is_vertical: bool,
        reverse_scrolling: bool,
    },
}

impl TranslatedContentOffsetSource {
    fn content_offset_reader(&self) -> Option<Rc<dyn Fn() -> Point>> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList {
                state, is_vertical, ..
            } => Some(Rc::new(lazy_list_content_offset_reader(
                *state,
                *is_vertical,
            ))),
        }
    }

    fn is_vertical(&self) -> Option<bool> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList { is_vertical, .. } => Some(*is_vertical),
        }
    }

    fn reverse_scrolling(&self) -> Option<bool> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList {
                reverse_scrolling, ..
            } => Some(*reverse_scrolling),
        }
    }
}

fn lazy_list_content_offset_reader(state: LazyListState, is_vertical: bool) -> impl Fn() -> Point {
    move || {
        let info = state.layout_info();
        if info.visible_items_info.is_empty() {
            return Point::default();
        };
        let main_offset = info.snap_anchor_offset;
        if is_vertical {
            Point::new(0.0, main_offset)
        } else {
            Point::new(main_offset, 0.0)
        }
    }
}

#[derive(Clone)]
struct TranslatedContentContextElement {
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
}

impl TranslatedContentContextElement {
    fn new(identity: usize, offset_source: TranslatedContentOffsetSource) -> Self {
        Self {
            identity,
            offset_source,
        }
    }
}

impl std::fmt::Debug for TranslatedContentContextElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let offset_source = match &self.offset_source {
            TranslatedContentOffsetSource::LayoutContentOffset => "layout",
            TranslatedContentOffsetSource::LazyList { .. } => "lazy_list",
        };
        f.debug_struct("TranslatedContentContextElement")
            .field("identity", &self.identity)
            .field("offset_source", &offset_source)
            .finish()
    }
}

impl PartialEq for TranslatedContentContextElement {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.offset_source.is_vertical() == other.offset_source.is_vertical()
            && self.offset_source.reverse_scrolling() == other.offset_source.reverse_scrolling()
    }
}

impl Eq for TranslatedContentContextElement {}

impl std::hash::Hash for TranslatedContentContextElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
        self.offset_source.is_vertical().hash(state);
        self.offset_source.reverse_scrolling().hash(state);
    }
}

impl ModifierNodeElement for TranslatedContentContextElement {
    type Node = TranslatedContentContextNode;

    fn create(&self) -> Self::Node {
        TranslatedContentContextNode::new(self.identity, self.offset_source.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.identity = self.identity;
        node.offset_source = self.offset_source.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
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
        self.then(scroll_impl(state, false, reverse_scrolling, None))
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
        self.then(scroll_impl(state, true, reverse_scrolling, None))
    }

    /// Creates a horizontally scrollable modifier with a guard that can disable scrolling.
    pub fn horizontal_scroll_guarded(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
        guard: impl Fn() -> bool + 'static,
    ) -> Self {
        self.then(scroll_impl(
            state,
            false,
            reverse_scrolling,
            Some(Rc::new(guard)),
        ))
    }

    /// Creates a vertically scrollable modifier with a guard that can disable scrolling.
    pub fn vertical_scroll_guarded(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
        guard: impl Fn() -> bool + 'static,
    ) -> Self {
        self.then(scroll_impl(
            state,
            true,
            reverse_scrolling,
            Some(Rc::new(guard)),
        ))
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
fn scroll_impl(
    state: ScrollState,
    is_vertical: bool,
    reverse_scrolling: bool,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    // Create local gesture state - each scroll modifier instance is independent
    let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
        state_id: state.id(),
        is_vertical,
        reverse_scrolling,
    });

    // Set up pointer input handler
    let scroll_state = state.clone();
    let pointer_motion_context = motion_context.clone();
    let key = (state.id(), is_vertical);
    let pointer_input = Modifier::empty().pointer_input(key, move |scope| {
        // Create detector inside the async closure to capture the cloned state
        let detector = ScrollGestureDetector::new(
            gesture_state.clone(),
            scroll_state.clone(),
            is_vertical,
            false, // ScrollState handles reversing in layout, not input
            pointer_motion_context.clone(),
        );
        let guard = guard.clone();

        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    // Main event loop - processes events until scope is cancelled
                    loop {
                        let event = await_scope.await_pointer_event().await;

                        if event.is_consumed() {
                            if matches!(
                                event.kind,
                                PointerEventKind::Down
                                    | PointerEventKind::Move
                                    | PointerEventKind::Up
                                    | PointerEventKind::Cancel
                            ) {
                                detector.on_cancel();
                            }
                            continue;
                        }

                        if let Some(ref guard) = guard {
                            if !guard() {
                                if matches!(
                                    event.kind,
                                    PointerEventKind::Up | PointerEventKind::Cancel
                                ) {
                                    detector.on_cancel();
                                }
                                continue;
                            }
                        }

                        // Delegate to detector's lifecycle methods
                        let should_consume = match event.kind {
                            PointerEventKind::Down => {
                                detector.on_down(event.position, event.time_ms)
                            }
                            PointerEventKind::Move => {
                                detector.on_move(event.position, event.buttons, event.time_ms)
                            }
                            PointerEventKind::Up => detector.on_up(),
                            PointerEventKind::Cancel => detector.on_cancel(),
                            PointerEventKind::Scroll => detector.on_scroll(if is_vertical {
                                event.scroll_delta.y
                            } else {
                                event.scroll_delta.x
                            }),
                            PointerEventKind::Enter | PointerEventKind::Exit => false,
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
    let motion_modifier =
        Modifier::with_element(MotionContextAnimatedElement::new(motion_context.clone()));
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state.id() as usize,
        TranslatedContentOffsetSource::LayoutContentOffset,
    ));

    // Combine: pointer input THEN layout modifier, clip to bounds by default
    pointer_input
        .then(motion_modifier)
        .then(translated_content_modifier)
        .then(layout_modifier)
        .clip_to_bounds()
}

// ============================================================================
// Lazy Scroll Support for LazyListState
// ============================================================================

use cranpose_foundation::lazy::LazyListState;

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
    let state_id = state.inner_ptr() as usize;
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
        state_identity: state_id,
        is_vertical,
        reverse_scrolling,
    });
    let key = (state_id, is_vertical, reverse_scrolling);
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state_id,
        TranslatedContentOffsetSource::LazyList {
            state,
            is_vertical,
            reverse_scrolling,
        },
    ));

    Modifier::with_element(MotionContextAnimatedElement::new(motion_context.clone()))
        .then(translated_content_modifier)
        .pointer_input(key, move |scope| {
            // Use the same generic detector with LazyListState
            let detector = ScrollGestureDetector::new(
                gesture_state.clone(),
                list_state,
                is_vertical,
                reverse_scrolling,
                motion_context.clone(),
            );

            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;

                            if event.is_consumed() {
                                if matches!(
                                    event.kind,
                                    PointerEventKind::Down
                                        | PointerEventKind::Move
                                        | PointerEventKind::Up
                                        | PointerEventKind::Cancel
                                ) {
                                    detector.on_cancel();
                                }
                                continue;
                            }

                            // Delegate to detector's lifecycle methods
                            let should_consume = match event.kind {
                                PointerEventKind::Down => {
                                    detector.on_down(event.position, event.time_ms)
                                }
                                PointerEventKind::Move => {
                                    detector.on_move(event.position, event.buttons, event.time_ms)
                                }
                                PointerEventKind::Up => detector.on_up(),
                                PointerEventKind::Cancel => detector.on_cancel(),
                                PointerEventKind::Scroll => detector.on_scroll(if is_vertical {
                                    event.scroll_delta.y
                                } else {
                                    event.scroll_delta.x
                                }),
                                PointerEventKind::Enter | PointerEventKind::Exit => false,
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
